//! Feishu (飞书) channel adapter.
//!
//! Implements a WebSocket-based bot that receives messages via Feishu's
//! event subscription protocol (pbbp2) and sends replies through the
//! Feishu Open API.

use anyhow::{bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMsg;
use tracing::{info, warn};

use crate::channel::base::ChannelRuntime;
use crate::config::FeishuConfig;

// ── Constants ────────────────────────────────────────────────────────

const FEISHU_API_BASE: &str = "https://open.feishu.cn/open-apis";
const FEISHU_WS_BASE: &str = "https://open.feishu.cn";
const LARK_API_BASE: &str = "https://open.larksuite.com/open-apis";
const LARK_WS_BASE: &str = "https://open.larksuite.com";

const WS_HEARTBEAT_TIMEOUT_SECS: u64 = 60;
const DEDUP_MAX_EVENTS: usize = 1000;

/// 飞书 tenant_access_token 有效期（秒），实际约 2 小时。
const FEISHU_TOKEN_TTL_SECS: u64 = 7200;
/// token 刷新安全余量（秒），避免临界过期。
const FEISHU_TOKEN_REFRESH_BUFFER_SECS: u64 = 300;

// ── Minimal protobuf parser for pbbp2 frames ─────────────────────────

#[derive(Debug, Default, Clone)]
struct PbFrame {
    seq_id: u64,
    log_id: u64,
    service: i32,
    method: i32,
    headers: Vec<PbHeader>,
    payload: Option<Vec<u8>>,
}

#[derive(Debug, Default, Clone)]
struct PbHeader {
    key: String,
    value: String,
}

impl PbFrame {
    fn header_value(&self, key: &str) -> &str {
        self.headers
            .iter()
            .find(|h| h.key == key)
            .map(|h| h.value.as_str())
            .unwrap_or("")
    }

    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(64);
        encode_varint(1, self.seq_id, &mut buf);
        encode_varint(2, self.log_id, &mut buf);
        encode_varint(3, self.service as u64, &mut buf);
        encode_varint(4, self.method as u64, &mut buf);
        for h in &self.headers {
            let hdr_bytes = encode_header(h);
            encode_varint(5, hdr_bytes.len() as u64, &mut buf);
            buf.extend_from_slice(&hdr_bytes);
        }
        if let Some(ref p) = self.payload {
            encode_varint(8, p.len() as u64, &mut buf);
            buf.extend_from_slice(p);
        }
        buf
    }

    fn parse(data: &[u8]) -> Result<Self> {
        let mut frame = PbFrame::default();
        let mut pos = 0;
        let len = data.len();

        while pos < len {
            let tag = data[pos];
            pos += 1;
            let field_number = tag >> 3;
            let wire_type = tag & 0x07;

            match wire_type {
                0 => {
                    let (value, consumed) = decode_varint(data, pos)?;
                    pos = consumed;
                    match field_number {
                        1 => frame.seq_id = value,
                        2 => frame.log_id = value,
                        3 => frame.service = value as i32,
                        4 => frame.method = value as i32,
                        _ => {}
                    }
                }
                2 => {
                    let (len_val, consumed) = decode_varint(data, pos)?;
                    pos = consumed;
                    let end = (pos + len_val as usize).min(len);
                    let raw = &data[pos..end];
                    pos = end;

                    match field_number {
                        5 => {
                            if let Ok(hdr) = parse_header(raw) {
                                frame.headers.push(hdr);
                            }
                        }
                        8 => {
                            frame.payload = Some(raw.to_vec());
                        }
                        _ => {}
                    }
                }
                5 => pos = (pos + 4).min(len),
                1 => pos = (pos + 8).min(len),
                _ => break,
            }
        }

        Ok(frame)
    }
}

fn encode_header(h: &PbHeader) -> Vec<u8> {
    let mut buf = Vec::with_capacity(32);
    let key_bytes = h.key.as_bytes();
    buf.push(0x0A);
    encode_varint_raw(key_bytes.len() as u64, &mut buf);
    buf.extend_from_slice(key_bytes);
    let val_bytes = h.value.as_bytes();
    buf.push(0x12);
    encode_varint_raw(val_bytes.len() as u64, &mut buf);
    buf.extend_from_slice(val_bytes);
    buf
}

fn decode_varint(data: &[u8], pos: usize) -> Result<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift = 0;
    let mut p = pos;

    loop {
        if p >= data.len() {
            bail!("truncated varint at position {pos}");
        }
        let byte = data[p] as u64;
        value |= (byte & 0x7F) << shift;
        shift += 7;
        p += 1;
        if byte & 0x80 == 0 {
            break;
        }
        if shift > 63 {
            bail!("varint too long at position {pos}");
        }
    }

    Ok((value, p))
}

fn parse_header(data: &[u8]) -> Result<PbHeader> {
    let mut header = PbHeader::default();
    let mut pos = 0;
    let len = data.len();

    while pos < len {
        let tag = data[pos];
        pos += 1;
        let field_number = tag >> 3;
        let wire_type = tag & 0x07;

        if wire_type != 2 {
            if wire_type == 0 {
                let (_val, new_pos) = decode_varint(data, pos)?;
                pos = new_pos;
            }
            continue;
        }

        let (len_val, consumed) = decode_varint(data, pos)?;
        pos = consumed;
        let end = (pos + len_val as usize).min(len);
        let s = String::from_utf8_lossy(&data[pos..end]).to_string();
        pos = end;

        match field_number {
            1 => header.key = s,
            2 => header.value = s,
            _ => {}
        }
    }

    Ok(header)
}

fn encode_varint(field_number: u32, value: u64, buf: &mut Vec<u8>) {
    let tag = (field_number << 3) as u8;
    buf.push(tag);
    let mut v = value;
    loop {
        if v < 0x80 {
            buf.push(v as u8);
            break;
        }
        buf.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
}

fn encode_varint_raw(value: u64, buf: &mut Vec<u8>) {
    let mut v = value;
    loop {
        if v < 0x80 {
            buf.push(v as u8);
            break;
        }
        buf.push((v as u8 & 0x7F) | 0x80);
        v >>= 7;
    }
}

// ── Feishu WS endpoint types ──────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
struct WsEndpointResp {
    code: i32,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    data: Option<WsEndpoint>,
}

#[derive(Debug, serde::Deserialize)]
struct WsEndpoint {
    #[serde(rename = "URL")]
    url: String,
    #[serde(rename = "ClientConfig")]
    client_config: Option<WsClientConfig>,
}

#[derive(Debug, serde::Deserialize, Default, Clone)]
struct WsClientConfig {
    #[serde(rename = "PingInterval")]
    ping_interval: Option<u64>,
}

#[derive(Debug, serde::Deserialize)]
struct LarkEvent {
    header: LarkEventHeader,
    event: serde_json::Value,
}

#[derive(Debug, serde::Deserialize)]
struct LarkEventHeader {
    event_type: String,
    event_id: String,
}

// ── Session Manager ──────────────────────────────────────────────────

// ── FeishuChannel ────────────────────────────────────────────────────

pub struct FeishuChannel {
    config: FeishuConfig,
    api_base: String,
    ws_base: String,
    locale: &'static str,
    /// 共享 HTTP 客户端（与 Agent / LLM 复用连接池，避免重复 TLS 握手）。
    http_client: Arc<reqwest::Client>,
    /// 共享运行时（会话管理 + agent + slash 命令）。运行期由 `with_runtime` 注入。
    runtime: Option<Arc<ChannelRuntime>>,
    /// 飞书 tenant_access_token 缓存（带 TTL 与安全余量）。
    token_cache: std::sync::Mutex<Option<(String, Instant)>>,
}

impl FeishuChannel {
    /// 构建轻量飞书通道用于配置校验（无 agent / runtime）。
    pub fn new(config: FeishuConfig) -> Result<Self> {
        Self::build(config, None)
    }

    /// 构建带共享运行时的飞书通道（运行期使用）。
    pub fn with_runtime(config: FeishuConfig, runtime: Arc<ChannelRuntime>) -> Result<Self> {
        Self::build(config, Some(runtime))
    }

    fn build(config: FeishuConfig, runtime: Option<Arc<ChannelRuntime>>) -> Result<Self> {
        let (api_base, ws_base, locale) = if config.lark_mode {
            (LARK_API_BASE.to_string(), LARK_WS_BASE.to_string(), "en")
        } else {
            (
                FEISHU_API_BASE.to_string(),
                FEISHU_WS_BASE.to_string(),
                "zh",
            )
        };

        Ok(Self {
            http_client: crate::http_client::HTTP_CLIENT.clone(),
            config,
            api_base,
            ws_base,
            locale,
            runtime,
            token_cache: std::sync::Mutex::new(None),
        })
    }

    async fn get_ws_endpoint(&self) -> Result<(String, WsClientConfig)> {
        let url = format!("{}/callback/ws/endpoint", self.ws_base);
        let resp = self
            .http_client
            .post(&url)
            .header("locale", self.locale)
            .json(&serde_json::json!({
                "AppID": self.config.app_id,
                "AppSecret": self.config.app_secret,
            }))
            .send()
            .await
            .context("request feishu ws endpoint")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("feishu ws endpoint HTTP {status}: {body}");
        }

        let ep: WsEndpointResp = resp.json().await.context("parse ws endpoint response")?;
        if ep.code != 0 {
            bail!(
                "feishu ws endpoint error: code={} msg={}",
                ep.code,
                ep.msg.as_deref().unwrap_or("(none)")
            );
        }
        let data = ep.data.context("feishu ws endpoint: empty data")?;
        Ok((data.url, data.client_config.unwrap_or_default()))
    }

    async fn get_tenant_token(&self) -> Result<String> {
        // 1) 命中缓存且未过期（飞书 token TTL=2h，留 5min 余量）直接返回。
        if let Some((tok, expiry)) = self.token_cache.lock().unwrap().as_ref() {
            if expiry.elapsed()
                < Duration::from_secs(FEISHU_TOKEN_TTL_SECS - FEISHU_TOKEN_REFRESH_BUFFER_SECS)
            {
                return Ok(tok.clone());
            }
        }
        // 2) 重新获取并写回缓存。
        let token = self.fetch_tenant_token().await?;
        let expiry = Instant::now()
            + Duration::from_secs(FEISHU_TOKEN_TTL_SECS - FEISHU_TOKEN_REFRESH_BUFFER_SECS);
        *self.token_cache.lock().unwrap() = Some((token.clone(), expiry));
        Ok(token)
    }

    async fn fetch_tenant_token(&self) -> Result<String> {
        let url = format!("{}/auth/v3/tenant_access_token/internal", self.api_base);
        let resp = self
            .http_client
            .post(&url)
            .json(&serde_json::json!({
                "app_id": self.config.app_id,
                "app_secret": self.config.app_secret,
            }))
            .send()
            .await
            .context("request tenant_access_token")?;

        let data: serde_json::Value = resp.json().await.context("parse token response")?;
        if data.get("code").and_then(|c| c.as_i64()) != Some(0) {
            let msg = data
                .get("msg")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");
            bail!("feishu token error: {msg}");
        }
        data.get("tenant_access_token")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .context("missing tenant_access_token in response")
    }

    /// Validate Feishu configuration by testing the WS endpoint.
    /// Returns Ok with a success message if credentials are valid.
    pub async fn validate_config(&self) -> Result<String> {
        let (url, _) = self.get_ws_endpoint().await?;
        Ok(format!("验证通过！WebSocket 端点: {}", url))
    }

    async fn add_reaction(&self, message_id: &str, emoji: &str) {
        let token = match self.get_tenant_token().await {
            Ok(t) => t,
            Err(e) => {
                warn!(error = %e, "failed to get token for reaction");
                return;
            }
        };
        let url = format!("{}/im/v1/messages/{}/reactions", self.api_base, message_id);

        match self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&serde_json::json!({ "reaction_type": { "emoji_type": emoji } }))
            .send()
            .await
        {
            Ok(resp) => {
                if !resp.status().is_success() {
                    warn!(status = %resp.status(), emoji = %emoji, "add reaction failed (non-fatal)");
                }
            }
            Err(e) => warn!(error = %e, "add reaction request failed (non-fatal)"),
        }
    }

    pub async fn send_message(&self, open_id: &str, text: &str) -> Result<()> {
        let token = self.get_tenant_token().await?;
        let url = format!("{}/im/v1/messages?receive_id_type=open_id", self.api_base);

        let resp = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&serde_json::json!({
                "receive_id": open_id,
                "msg_type": "text",
                "content": serde_json::json!({ "text": text }).to_string(),
            }))
            .send()
            .await
            .context("send feishu message")?;

        let status = resp.status();
        let data: serde_json::Value = resp.json().await.context("parse send response")?;

        if !status.is_success() || data.get("code").and_then(|c| c.as_i64()) != Some(0) {
            let msg = data
                .get("msg")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");
            warn!(status = %status, error = %msg, "feishu send message failed");
        }

        Ok(())
    }

    pub async fn run_listener(&self) -> Result<()> {
        let (wss_url, client_config) = self.get_ws_endpoint().await?;
        info!("connecting to feishu ws");

        let (ws_stream, _response) = connect_async(wss_url.as_str())
            .await
            .map_err(|e| anyhow::anyhow!("feishu ws connect failed: {e} (url: {wss_url})"))?;
        info!("feishu ws connected");

        let (mut write, mut read) = ws_stream.split();

        let seen_events = Arc::new(Mutex::new(LruCache::<String, ()>::new(
            NonZeroUsize::new(DEDUP_MAX_EVENTS).unwrap(),
        )));

        let ping_secs = client_config.ping_interval.unwrap_or(120).max(10);
        let mut hb_interval = tokio::time::interval(Duration::from_secs(ping_secs));
        let mut timeout_check = tokio::time::interval(Duration::from_secs(10));
        hb_interval.tick().await;

        let mut seq: u64 = 0;
        let mut last_recv = Instant::now();
        seq = seq.wrapping_add(1);

        loop {
            tokio::select! {
                biased;

                _ = hb_interval.tick() => {
                    seq = seq.wrapping_add(1);
                    let ping = PbFrame {
                        seq_id: seq, log_id: 0, service: 0, method: 0,
                        headers: vec![PbHeader { key: "type".into(), value: "ping".into() }],
                        payload: None,
                    };
                    if write.send(WsMsg::Binary(ping.encode())).await.is_err() {
                        info!("feishu ws ping failed, reconnecting"); break;
                    }
                }

                _ = timeout_check.tick() => {
                    if last_recv.elapsed() > Duration::from_secs(WS_HEARTBEAT_TIMEOUT_SECS) {
                        info!("feishu ws heartbeat timeout, reconnecting"); break;
                    }
                }

                msg = read.next() => {
                    let raw = match msg {
                        Some(Ok(WsMsg::Binary(b))) => b,
                        Some(Ok(WsMsg::Ping(d))) => { let _ = write.send(WsMsg::Pong(d)).await; continue; }
                        Some(Ok(WsMsg::Close(_))) | None => { info!("feishu ws closed, reconnecting"); break; }
                        Some(Ok(_)) => continue,
                        Some(Err(e)) => { warn!(error = %e, "feishu ws read error, reconnecting"); break; }
                    };

                    last_recv = Instant::now();
                    let frame = match PbFrame::parse(&raw) {
                        Ok(f) => f,
                        Err(e) => { warn!(error = %e, "failed to parse pb frame"); continue; }
                    };

                    if frame.method == 0 {
                        if frame.header_value("type") == "pong" {
                            if let Some(p) = &frame.payload {
                                if let Ok(cfg) = serde_json::from_slice::<WsClientConfig>(p) {
                                    if let Some(secs) = cfg.ping_interval { let _ = secs.max(10); }
                                }
                            }
                        }
                        continue;
                    }

                    if frame.header_value("type") != "event" { continue; }

                    // ACK within 3s
                    {
                        let mut ack = frame.clone();
                        ack.payload = Some(br#"{"code":200,"headers":{},"data":[]}"#.to_vec());
                        ack.headers.push(PbHeader { key: "biz_rt".into(), value: "0".into() });
                        let _ = write.send(WsMsg::Binary(ack.encode())).await;
                    }

                    let payload = frame.payload.unwrap_or_default();
                    let event: LarkEvent = match serde_json::from_slice(&payload) {
                        Ok(e) => e,
                        Err(e) => { warn!(error = %e, "failed to parse event json"); continue; }
                    };

                    if event.header.event_type != "im.message.receive_v1" { continue; }

                    // Dedup by event_id（有界 LRU，超过容量时淘汰最旧而非整集清空）
                    {
                        let mut seen = seen_events.lock().unwrap();
                        if seen.contains(&event.header.event_id) {
                            info!(event_id = %event.header.event_id, "duplicate event, skipping");
                            continue;
                        }
                        seen.put(event.header.event_id.clone(), ());
                    }

                    // Extract user text for content-type detection
                    let msg_id = event.event
                        .get("message")
                        .and_then(|m| m.get("message_id"))
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string());

                    let raw_content = event.event
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("");

                    let user_text = extract_text_content(raw_content);
                    let is_code = is_code_related(&user_text);

                    // Add initial reaction based on content type
                    if let Some(ref mid) = msg_id {
                        self.add_reaction(mid, if is_code { "HAMMER" } else { "GLANCE" }).await;
                    }

                    let result = self.handle_event(&event.event).await;

                    // Update reaction on completion
                    if let Some(ref mid) = msg_id {
                        match &result {
                            Ok(()) => self.add_reaction(mid, "DONE").await,
                            Err(_) => self.add_reaction(mid, "Alarm").await,
                        }
                    }

                    if let Err(e) = result {
                        warn!(error = %e, "failed to handle im.message.receive_v1");
                    }
                }
            }
        }

        info!("feishu ws listener exited");
        Ok(())
    }

    async fn handle_event(&self, event: &serde_json::Value) -> Result<()> {
        let sender = event.get("sender").context("missing sender")?;
        let sender_type = sender
            .get("sender_type")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        if sender_type == "app" || sender_type == "bot" {
            return Ok(());
        }

        let sender_open_id = sender
            .get("sender_id")
            .and_then(|s| s.get("open_id"))
            .and_then(|s| s.as_str())
            .context("missing sender.open_id")?;

        let message = event.get("message").context("missing message")?;
        let message_type = message
            .get("message_type")
            .and_then(|m| m.as_str())
            .unwrap_or("");
        let chat_type = message
            .get("chat_type")
            .and_then(|c| c.as_str())
            .unwrap_or("p2p");

        if message_type != "text" {
            return Ok(());
        }

        let content_str = message
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("");
        let raw_text = extract_text_content(content_str);
        let is_code = is_code_related(&raw_text);

        if self.config.mention_only && chat_type == "group" {
            // Check if the bot was @mentioned in the group message
            let is_mentioned = event
                .get("mentions")
                .and_then(|m| m.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            if !is_mentioned {
                return Ok(()); // Not @bot, skip
            }
        }

        let text = raw_text.trim();
        if text.is_empty() {
            return Ok(());
        }

        // Strip @mention from group messages
        let text = if chat_type == "group" {
            strip_mention(text).to_string()
        } else {
            text.to_string()
        };

        let runtime = self
            .runtime
            .as_ref()
            .context("feishu runtime not initialized")?;

        info!(sender = %sender_open_id, chat = %chat_type, "feishu message received");

        // 收到消息时先打初始反应（基于内容类型）。
        if let Some(mid) = extract_message_id(event) {
            self.add_reaction(&mid, if is_code { "HAMMER" } else { "GLANCE" })
                .await;
        }

        // 统一走 ChannelRuntime 流水线：slash 命令 / 会话 / agent_chat。
        let result = runtime.process_text_message(sender_open_id, &text).await;

        // 完成时根据成败更新反应。
        match &result {
            Ok(_) => {
                if let Some(mid) = extract_message_id(event) {
                    self.add_reaction(&mid, "DONE").await;
                }
            }
            Err(_) => {
                if let Some(mid) = extract_message_id(event) {
                    self.add_reaction(&mid, "Alarm").await;
                }
            }
        }

        match result {
            Ok(reply) => self.send_message(sender_open_id, &reply).await?,
            Err(e) => self.send_message(sender_open_id, &e).await?,
        }

        Ok(())
    }
}

/// Extract the message_id from a Lark event (for reactions).
fn extract_message_id(event: &serde_json::Value) -> Option<String> {
    event
        .get("message")
        .and_then(|m| m.get("message_id"))
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
}

/// Extract user text from Feishu's content JSON field.
fn extract_text_content(content_str: &str) -> String {
    serde_json::from_str::<serde_json::Value>(content_str)
        .ok()
        .and_then(|v| {
            v.get("text")
                .and_then(|t| t.as_str().map(|s| s.to_string()))
        })
        .unwrap_or_else(|| content_str.to_string())
}

/// Determine if user text is code/development related.
fn is_code_related(text: &str) -> bool {
    let keywords = [
        "code",
        "代码",
        "function",
        "class",
        "impl",
        "fn ",
        "rust",
        "python",
        "javascript",
        "typescript",
        "golang",
        "implement",
        "fix",
        "bug",
        "debug",
        "compile",
        "error",
        "test",
        "refactor",
        "commit",
        "pr ",
        "api",
        "lib",
        "crate",
        "mod ",
        "struct",
        "enum",
        "trait",
        "async",
        "await",
        "let ",
        "mut ",
        "pub ",
        "写一个",
        "实现",
        "修复",
        "重构",
        "代码",
        "terminal",
        "command",
        "shell",
        "git ",
    ];
    let lower = text.to_lowercase();
    keywords.iter().any(|kw| lower.contains(kw))
}

/// Strip @mention prefix from group chat messages (e.g. "@bot 你好" → "你好").
fn strip_mention(text: &str) -> &str {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix('@') {
        // Try to find the end of the mention (space or start of text)
        if let Some(pos) = rest.find(|c: char| c.is_whitespace() || c == '\u{200b}') {
            let after_mention = rest[pos..].trim();
            if !after_mention.is_empty() {
                return after_mention;
            }
        }
        // If the whole thing starts with @ and has no space, it's just a mention
        // Check if there's text after the @name by looking for common patterns
        return text;
    }
    text
}
