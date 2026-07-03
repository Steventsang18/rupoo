//! 渠道共享运行时 — SessionManager + 消息处理流水线。
//!
//! 各渠道（飞书、钉钉、企微）共享此模块的会话管理、记忆标签、
//! slash command 处理等逻辑，只需实现协议收发层即可。

use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use tracing::{error, info};

use crate::agent::Agent;
use crate::config::RupooConfig;

// ── Session Manager ──────────────────────────────────────────────────

/// 管理每个发送者的对话历史，LRU 淘汰。
/// 每个发送者首次发消息时注入系统提示词，后续复用上下文。
pub struct SessionManager {
    sessions: Mutex<LruCache<String, crate::llm::ConversationHistory>>,
}

impl SessionManager {
    pub fn new(max_sessions: usize) -> Self {
        Self {
            sessions: Mutex::new(LruCache::new(
                NonZeroUsize::new(max_sessions.max(1)).unwrap(),
            )),
        }
    }

    /// 推送用户消息并克隆完整历史（释放锁后供 agent 调用）。
    pub fn push_and_clone(
        &self,
        sender: &str,
        text: &str,
        system_prompt: &str,
    ) -> crate::llm::ConversationHistory {
        let mut cache = self.sessions.lock().unwrap();
        if !cache.contains(sender) {
            let mut history = crate::llm::ConversationHistory::new(50);
            history.push_system(system_prompt);
            cache.put(sender.into(), history);
        }
        let session = cache.get_mut(sender).unwrap();
        session.push_user(text);
        // Clone history by copying messages
        let mut fresh = crate::llm::ConversationHistory::new(50);
        for msg in session.messages() {
            use crate::llm::LlmChatRole;
            match msg.role {
                LlmChatRole::System => fresh.push_system(&msg.content),
                LlmChatRole::User => fresh.push_user(&msg.content),
                LlmChatRole::Assistant => fresh.push_assistant(&msg.content),
            }
        }
        fresh
    }

    /// 保存助手回复到会话历史。
    pub fn push_response(&self, sender: &str, response: &str) {
        let mut cache = self.sessions.lock().unwrap();
        if let Some(session) = cache.get_mut(sender) {
            session.push_assistant(response);
        }
    }

    /// 清除某个发送者的会话（/new 或 /clear）。
    pub fn clear_session(&self, sender: &str) {
        let mut cache = self.sessions.lock().unwrap();
        cache.pop(sender);
    }
}

// ── Channel Runtime ─────────────────────────────────────────────────

/// 渠道运行时 — 统一的消息处理流水线，各渠道共享。
pub struct ChannelRuntime {
    pub sessions: SessionManager,
    pub agent: Arc<Agent>,
    pub config: RupooConfig,
    /// 当前渠道的记忆 source 标签 (feishu / dingtalk / wecom)
    pub memory_source: &'static str,
    /// 当前渠道的 agent 角色名 (feishu / dingtalk / wecom)
    pub agent_role: &'static str,
}

impl ChannelRuntime {
    pub fn new(
        agent: Arc<Agent>,
        config: RupooConfig,
        memory_source: &'static str,
        agent_role: &'static str,
    ) -> Self {
        Self {
            sessions: SessionManager::new(1000),
            agent,
            config,
            memory_source,
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
        let history = self.sessions.push_and_clone(sender, text, &prompt);

        match self
            .agent
            .agent_chat("", &history, 8, false, |_| {}, None, Some(prompt))
            .await
        {
            Ok((response, _usage)) => {
                self.sessions.push_response(sender, &response);
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
                self.sessions.clear_session(sender);
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
