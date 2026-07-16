//! 渠道共享运行时 — SessionManager + 消息处理流水线。
//!
//! 各渠道（飞书、钉钉、企微）共享此模块的会话管理、记忆标签、
//! slash command 处理等逻辑，只需实现协议收发层即可。

use lru::LruCache;
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use tracing::{error, info};

/// 每个会话保留的最大"轮数"（每轮 = 用户 + 助手各一条，故实际最多 2×N 条消息）。
const MAX_HISTORY_TURNS: usize = 50;
/// 会话历史的最大 token 预算（约 2 字符/token）。超出时自动裁剪旧消息，
/// 防止长对话把整段历史反复发给 LLM 导致 token 成本线性膨胀。
const MAX_HISTORY_TOKENS: usize = 8000;

use crate::agent::{Agent, ToolExecutor};
use crate::config::RupooConfig;
use crate::error::AgentResult;
use crate::task::McpToolResult;

// ── Tool Filter ──────────────────────────────────────────────────

/// 透明工具过滤器 — 包装 ToolExecutor，在 execute 前检查 allow/deny 列表。
/// 不修改 agent_chat 或 agent.rs，对上层完全透明。
pub struct FilteredToolExecutor {
    inner: Arc<dyn ToolExecutor>,
    allowed: Option<HashSet<String>>,
    excluded: Option<HashSet<String>>,
}

impl FilteredToolExecutor {
    pub fn new(
        inner: Arc<dyn ToolExecutor>,
        allowed: Option<Vec<String>>,
        excluded: Option<Vec<String>>,
    ) -> Self {
        Self {
            inner,
            allowed: allowed.map(|v| v.into_iter().collect()),
            excluded: excluded.map(|v| v.into_iter().collect()),
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor for FilteredToolExecutor {
    async fn execute_tool(
        &self,
        tool_name: &str,
        params: serde_json::Value,
    ) -> AgentResult<McpToolResult> {
        if let Some(ref allowed) = self.allowed {
            if !allowed.contains(tool_name) {
                return Err(crate::error::AgentError::Tool(format!(
                    "工具 '{tool_name}' 不在允许列表中"
                )));
            }
        }
        if let Some(ref excluded) = self.excluded {
            if excluded.contains(tool_name) {
                return Err(crate::error::AgentError::Tool(format!(
                    "工具 '{tool_name}' 已被禁用"
                )));
            }
        }
        self.inner.execute_tool(tool_name, params).await
    }
}

// ── Session Manager ──────────────────────────────────────────────────

/// 管理每个发送者的对话历史，LRU 淘汰。
/// 每个发送者首次发消息时注入系统提示词，后续复用上下文。
pub struct SessionManager {
    sessions: Mutex<LruCache<String, crate::llm::ConversationHistory>>,
    repo: Option<std::sync::Arc<crate::db::TaskRepo>>,
}

impl SessionManager {
    pub fn new(max_sessions: usize) -> Self {
        Self {
            sessions: Mutex::new(LruCache::new(
                NonZeroUsize::new(max_sessions.max(1)).unwrap(),
            )),
            repo: None,
        }
    }

    /// Create SessionManager with SQLite persistence support.
    pub fn with_repo(max_sessions: usize, repo: std::sync::Arc<crate::db::TaskRepo>) -> Self {
        let mut mgr = Self::new(max_sessions);
        mgr.repo = Some(repo);
        mgr
    }

    fn session_key(channel: &str, sender: &str) -> String {
        format!("{channel}.{sender}")
    }

    /// 推送用户消息并克隆完整历史。
    /// 首次创建时尝试从 SQLite 加载历史。
    pub async fn push_and_clone(
        &self,
        sender: &str,
        text: &str,
        system_prompt: &str,
        channel: &str,
    ) -> crate::llm::ConversationHistory {
        // Fast path: sender 已缓存 — 单次加锁内 clone + push，O(n) 深拷贝。
        {
            let mut cache = self.sessions.lock().unwrap();
            if let Some(session) = cache.get_mut(sender) {
                let mut fresh = session.clone();
                fresh.push_user(text);
                session.push_user(text);
                return fresh;
            }
        }

        // 未缓存：尝试从 SQLite 加载，否则新建。
        let loaded = if let Some(ref repo) = self.repo {
            let key = Self::session_key(channel, sender);
            repo.load_conversation_history(&key).await.ok().flatten()
        } else {
            None
        };

        let mut cache = self.sessions.lock().unwrap();
        let history = match loaded {
            Some(mut h) => {
                h = h.with_token_budget(MAX_HISTORY_TOKENS);
                h.push_user(text);
                h
            }
            None => {
                let mut h = crate::llm::ConversationHistory::new(MAX_HISTORY_TURNS)
                    .with_token_budget(MAX_HISTORY_TOKENS);
                h.push_system(system_prompt);
                h.push_user(text);
                h
            }
        };
        cache.put(sender.into(), history.clone());
        history
    }

    /// 保存助手回复到会话历史并持久化。
    pub async fn push_response(&self, sender: &str, response: &str, channel: &str) {
        // 作用域内加锁：push 后直接 clone（ConversationHistory 已实现 Clone，O(n)）。
        let clone: Option<crate::llm::ConversationHistory> = {
            let mut cache = self.sessions.lock().unwrap();
            if let Some(session) = cache.get_mut(sender) {
                session.push_assistant(response);
                Some(session.clone())
            } else {
                None
            }
        }; // 锁在此释放

        // 持久化到 SQLite（异步，不持锁）
        if let Some(ref repo) = self.repo {
            if let Some(clone) = clone {
                let key = Self::session_key(channel, sender);
                let _ = repo.save_conversation_history(&key, &clone).await;
            }
        }
    }

    /// 清除某个发送者的会话并删除持久化数据。
    pub async fn clear_session(&self, sender: &str, channel: &str) {
        {
            let mut cache = self.sessions.lock().unwrap();
            cache.pop(sender);
        } // lock released here
        if let Some(ref repo) = self.repo {
            let key = Self::session_key(channel, sender);
            let empty = crate::llm::ConversationHistory::new(MAX_HISTORY_TURNS)
                .with_token_budget(MAX_HISTORY_TOKENS);
            let _ = repo.save_conversation_history(&key, &empty).await;
        }
    }
}

// ── Channel Runtime ─────────────────────────────────────────────────

/// 渠道运行时 — 统一的消息处理流水线，各渠道共享。
pub struct ChannelRuntime {
    pub sessions: SessionManager,
    pub agent: Arc<Agent>,
    pub config: RupooConfig,
    /// 当前渠道的 agent 角色名 (feishu / dingtalk / wecom)
    pub agent_role: &'static str,
}

impl ChannelRuntime {
    pub fn new(
        agent: Arc<Agent>,
        config: RupooConfig,
        agent_role: &'static str,
        repo: Option<std::sync::Arc<crate::db::TaskRepo>>,
    ) -> Self {
        let sessions = match repo {
            Some(r) => SessionManager::with_repo(1000, r),
            None => SessionManager::new(1000),
        };
        Self {
            sessions,
            agent,
            config,
            agent_role,
        }
    }

    /// 获取当前渠道的 agent system prompt（从 config.agents[role] 读取）。
    pub fn get_system_prompt(&self) -> String {
        self.config
            .agents
            .get(self.agent_role)
            .and_then(|a| a.system_prompt.clone())
            .unwrap_or_else(|| {
                format!(
                    "你正在 {} 上与用户对话。用自然的中文回复，简洁口语化。",
                    self.agent_role
                )
            })
    }

    /// 处理文本消息 — slash command / agent chat 的统一入口。
    /// 返回需要发送给用户的回复文本。
    pub async fn process_text_message(&self, sender: &str, text: &str) -> Result<String, String> {
        let text = text.trim();

        // Slash commands
        if let Some(cmd) = text.strip_prefix('/') {
            return self.handle_slash_command(cmd, sender).await;
        }

        if text.is_empty() {
            return Err("空消息".into());
        }

        info!(sender = %sender, "channel message received");

        // Session + agent chat
        let prompt = self.get_system_prompt();
        let history = self
            .sessions
            .push_and_clone(sender, text, &prompt, self.agent_role)
            .await;

        match self
            .agent
            .agent_chat("", &history, 8, false, |_| {}, None, Some(prompt))
            .await
        {
            Ok((response, _usage)) => {
                self.sessions
                    .push_response(sender, &response, self.agent_role)
                    .await;
                Ok(format!("{}\n\n[OK]", response))
            }
            Err(e) => {
                error!(error = %e, "agent chat failed");
                Err(format!("抱歉，处理消息时出错: {e}"))
            }
        }
    }

    /// 处理 slash command。
    #[allow(clippy::unused_async)]
    async fn handle_slash_command(&self, cmd: &str, sender: &str) -> Result<String, String> {
        match cmd.trim().to_lowercase().as_str() {
            "new" | "clear" => {
                self.sessions.clear_session(sender, self.agent_role).await;
                Ok("✅ 对话已重置，开始新的会话吧！".into())
            }
            "help" => Ok("🤖 **Rupoo 快捷指令**\n\n\
                    /new 或 /clear — 重置对话\n\
                    /help — 显示本帮助\n\
                    /status — 查看机器人状态\n\n\
                    其他消息直接发送即可。"
                .into()),
            "status" => Ok(format!("✅ Rupoo is running on {}", self.agent_role)),
            _ => Ok(format!("未知指令 `/{cmd}`，发送 /help 查看可用指令")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, DummyToolExecutor};
    use crate::config::RupooConfig;
    use crate::db::TaskRepo;
    use std::sync::Arc;

    fn test_agent() -> Arc<Agent> {
        let repo = Arc::new(TaskRepo::new(":memory:").unwrap());
        Arc::new(Agent::new(Arc::clone(&repo), Arc::new(DummyToolExecutor)))
    }

    #[tokio::test]
    async fn session_clone_accumulates_turns_and_reinjects_system_once() {
        let sm = SessionManager::new(10);
        let sys = "SYS";

        // First message: new session → [system, user("hi")]
        let h1 = sm.push_and_clone("s1", "hi", sys, "feishu").await;
        assert_eq!(h1.message_count(), 2, "first turn should be system+user");

        // Second message: clone existing + append → [system, hi, bye]
        let h2 = sm.push_and_clone("s1", "bye", sys, "feishu").await;
        assert_eq!(h2.message_count(), 3);

        // Assistant reply then another user turn → 5 messages total
        sm.push_response("s1", "reply", "feishu").await;
        let h3 = sm.push_and_clone("s1", "again", sys, "feishu").await;
        assert_eq!(h3.message_count(), 5);
    }

    #[tokio::test]
    async fn clear_session_resets_history_and_reinjects_system() {
        let sm = SessionManager::new(10);
        let sys = "SYS";
        sm.push_and_clone("s1", "hi", sys, "feishu").await;
        sm.clear_session("s1", "feishu").await;

        // After clear, a new message starts a fresh session with system re-injected.
        let h = sm.push_and_clone("s1", "fresh", sys, "feishu").await;
        assert_eq!(h.message_count(), 2);
    }

    #[tokio::test]
    async fn runtime_slash_commands_are_unified() {
        let agent = test_agent();
        let runtime = ChannelRuntime::new(agent, RupooConfig::default(), "feishu", None);

        let help = runtime.process_text_message("u1", "/help").await.unwrap();
        assert!(help.contains("快捷指令"));

        let status = runtime.process_text_message("u1", "/status").await.unwrap();
        assert!(status.contains("feishu"));

        let reset = runtime.process_text_message("u1", "/new").await.unwrap();
        assert!(reset.contains("重置"));

        let unknown = runtime.process_text_message("u1", "/nope").await.unwrap();
        assert!(unknown.contains("未知指令"));
    }

    #[tokio::test]
    async fn session_history_respects_token_budget() {
        let sm = SessionManager::new(10);
        // 一条超大消息（~10000 tokens）应被裁剪到预算以内，避免上下文无限膨胀。
        let big = "x".repeat(20000);
        let h = sm.push_and_clone("s1", &big, "SYS", "feishu").await;
        assert!(
            h.estimated_tokens() <= MAX_HISTORY_TOKENS + 50,
            "history should be trimmed to the token budget, got {}",
            h.estimated_tokens()
        );
    }
}
