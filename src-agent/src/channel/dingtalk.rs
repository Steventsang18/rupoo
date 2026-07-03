//! 钉钉（DingTalk）渠道适配器。
//!
//! 通过钉钉 Stream 模式 WebSocket 接收消息，调用 Rupoo 内核处理，通过
//! 回调 WebHook 发送回复。复用了 base.rs 的会话管理、slash command 等能力。

use anyhow::{bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMsg;
use tracing::{info, warn};

use crate::channel::base::ChannelRuntime;

// ── 常量 ──────────────────────────────────────────────────────────────

const DINGTALK_API_BASE: &str = "https://api.dingtalk.com";

/// 钉钉 Stream 网关注册端点
const GATEWAY_REGISTER_PATH: &str = "/v1.0/gateway/connections/open";

/// 从 Stream 消息中提取业务数据的字段名
const DINGTALK_BOT_CALLBACK_TOPIC: &str = "/v1.0/im/bot/messages/get";

// ── 钉钉渠道 ──────────────────────────────────────────────────────────

pub struct DingTalkChannel {
    client_id: String,
    client_secret: String,
    /// 发送回复时的 session webhook（chat_id → webhook URL）
    session_webhooks: Arc<RwLock<HashMap<String, String>>>,
    /// 共享运行时
    runtime: Arc<ChannelRuntime>,
    /// HTTP 客户端
    http_client: reqwest::Client,
}

/// 钉钉网关注册响应
#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct GatewayResponse {
    endpoint: String,
    ticket: String,
}

/// Stream 消息帧
#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct StreamFrame {
    #[serde(default)]
    data: Option<serde_json::Value>,
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
    #[serde(default)]
    topic: Option<String>,
}

impl DingTalkChannel {
    pub fn new(
        client_id: String,
        client_secret: String,
        runtime: Arc<ChannelRuntime>,
    ) -> Result<Self> {
        Ok(Self {
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .context("build dingtalk http client")?,
            client_id,
            client_secret,
            session_webhooks: Arc::new(RwLock::new(HashMap::new())),
            runtime,
        })
    }

    /// 获取钉钉 access token。
    async fn get_token(&self) -> Result<String> {
        let url = format!("{}/v1.0/oauth2/accessToken", DINGTALK_API_BASE);
        let resp = self
            .http_client
            .post(&url)
            .json(&serde_json::json!({
                "appKey": self.client_id,
                "appSecret": self.client_secret,
            }))
            .send()
            .await
            .context("request dingtalk token")?;

        let data: serde_json::Value = resp.json().await.context("parse token response")?;
        if let Some(token) = data.get("accessToken").and_then(|t| t.as_str()) {
            return Ok(token.to_string());
        }
        if let Some(code) = data.get("code").and_then(|c| c.as_str()) {
            let msg = data
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");
            bail!("dingtalk token error: code={code} msg={msg}");
        }
        bail!("dingtalk token response missing accessToken")
    }

    /// 注册 Stream 网关，获取 WebSocket 端点。
    async fn register_gateway(&self) -> Result<(String, String)> {
        let token = self.get_token().await?;
        let url = format!("{}{}", DINGTALK_API_BASE, GATEWAY_REGISTER_PATH);

        let resp = self
            .http_client
            .post(&url)
            .header("x-acs-dingtalk-access-token", &token)
            .json(&serde_json::json!({
                "clientId": self.client_id,
                "clientSecret": self.client_secret,
                "subscriptions": [{
                    "topic": DINGTALK_BOT_CALLBACK_TOPIC,
                    "type": "EVENT"
                }]
            }))
            .send()
            .await
            .context("register dingtalk gateway")?;

        let data: serde_json::Value = resp.json().await.context("parse gateway response")?;

        let endpoint = data
            .get("endpoint")
            .and_then(|e| e.as_str())
            .context("missing endpoint")?
            .to_string();
        let ticket = data
            .get("ticket")
            .and_then(|t| t.as_str())
            .context("missing ticket")?
            .to_string();

        Ok((endpoint, ticket))
    }

    /// 通过 session webhook 发送回复消息。
    async fn send_reply(&self, chat_id: &str, text: &str) -> Result<()> {
        let webhooks = self.session_webhooks.read().await;
        let webhook_url = webhooks
            .get(chat_id)
            .context("no webhook url for this chat")?;

        self.http_client
            .post(webhook_url)
            .json(&serde_json::json!({
                "msgtype": "text",
                "text": { "content": text }
            }))
            .send()
            .await
            .context("send dingtalk message")?;

        Ok(())
    }

    /// 运行 WebSocket 监听循环。
    pub async fn run_listener(&self) -> Result<()> {
        let (endpoint, ticket) = self.register_gateway().await?;
        let ws_url = format!("wss://{}/connect?ticket={}", endpoint, ticket);
        info!("connecting to dingtalk ws");

        let (ws_stream, _response) = connect_async(ws_url.as_str())
            .await
            .context("dingtalk ws connect")?;
        info!("dingtalk ws connected");

        let (mut write, mut read) = ws_stream.split();
        let mut seq: u64 = 0;

        loop {
            tokio::select! {
                msg = read.next() => {
                    let raw = match msg {
                        Some(Ok(WsMsg::Text(t))) => t,
                        Some(Ok(WsMsg::Ping(d))) => { let _ = write.send(WsMsg::Pong(d)).await; continue; }
                        Some(Ok(WsMsg::Close(_))) | None => { info!("dingtalk ws closed, reconnecting"); break; }
                        Some(Ok(_)) => continue,
                        Some(Err(e)) => { warn!(error = %e, "dingtalk ws read error"); break; }
                    };

                    // 解析 Stream 帧
                    let frame: StreamFrame = match serde_json::from_str(&raw) {
                        Ok(f) => f,
                        Err(e) => {
                            warn!(error = %e, "failed to parse dingtalk frame");
                            continue;
                        }
                    };

                    // 处理业务数据
                    if let Some(data) = frame.data {
                        if let Err(e) = self.handle_message(data).await {
                            warn!(error = %e, "handle dingtalk message failed");
                        }
                    }

                    // ACK
                    seq += 1;
                    let ack = serde_json::json!({
                        "code": 200,
                        "message": "success",
                        "seqId": seq,
                    });
                    let _ = write.send(WsMsg::Text(ack.to_string())).await;
                }
            }
        }

        info!("dingtalk ws listener exited");
        Ok(())
    }

    /// 处理一条钉钉消息。
    async fn handle_message(&self, data: serde_json::Value) -> Result<()> {
        // 提取消息内容
        let body = match data.get("data") {
            Some(serde_json::Value::String(s)) => {
                serde_json::from_str::<serde_json::Value>(s).unwrap_or(data.clone())
            }
            Some(v) => v.clone(),
            None => data,
        };

        let msg_type = body.get("msgtype").and_then(|m| m.as_str()).unwrap_or("");

        // 只处理文本消息
        if msg_type != "text" && body.get("text").is_none() {
            return Ok(());
        }

        let text = body
            .get("text")
            .and_then(|t| t.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("");

        let sender_id = body
            .get("senderId")
            .and_then(|s| s.as_str())
            .or_else(|| body.get("sender").and_then(|s| s.as_str()))
            .unwrap_or("unknown");

        let chat_id = body
            .get("conversationId")
            .and_then(|c| c.as_str())
            .unwrap_or(sender_id);

        // 存 webhook 用于回复
        if let Some(webhook) = body
            .get("sessionWebhook")
            .and_then(|w| w.as_str())
            .map(|s| s.to_string())
        {
            let mut wh = self.session_webhooks.write().await;
            wh.insert(chat_id.to_string(), webhook);
        }

        info!(sender = %sender_id, chat = %chat_id, "dingtalk message received");

        // 使用共享运行时处理
        match self.runtime.process_text_message(sender_id, text).await {
            Ok(reply) => {
                self.send_reply(chat_id, &reply).await?;
            }
            Err(e) => {
                self.send_reply(chat_id, &e).await?;
            }
        }

        Ok(())
    }
}
