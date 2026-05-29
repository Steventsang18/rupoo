use serde::{Deserialize, Serialize};
use tracing::info;

use std::path::PathBuf;

use crate::error::{AgentError, AgentResult};
use rig::completion::Prompt;
use rig::streaming::StreamingPrompt;

// ---------------------------------------------------------------------------
// Token usage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

impl TokenUsage {
    pub fn total(&self) -> u32 {
        self.prompt_tokens + self.completion_tokens
    }
}

// ---------------------------------------------------------------------------
// LLM-internal ChatMessage types (separate from shared::ChatMessage)
// ---------------------------------------------------------------------------

/// Chat message for LLM communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmChatMessage {
    pub role: LlmChatRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LlmChatRole {
    System,
    User,
    Assistant,
}

impl LlmChatMessage {
    pub fn system(content: &str) -> Self {
        Self { role: LlmChatRole::System, content: content.to_string() }
    }
    pub fn user(content: &str) -> Self {
        Self { role: LlmChatRole::User, content: content.to_string() }
    }
    pub fn assistant(content: &str) -> Self {
        Self { role: LlmChatRole::Assistant, content: content.to_string() }
    }
}

// ---------------------------------------------------------------------------
// AgentEvent for streaming callbacks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum AgentEvent {
    TextDelta(String),
    ToolCall { tool_name: String, args: String },
    ToolResult { tool_name: String, result: String },
}

// ---------------------------------------------------------------------------
// ConversationHistory for multi-turn chat
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConversationHistory {
    messages: Vec<LlmChatMessage>,
    max_turns: usize,
    /// Maximum estimated token budget for history (0 = no limit)
    max_tokens: usize,
}

impl ConversationHistory {
    pub fn new(max_turns: usize) -> Self {
        Self { messages: Vec::new(), max_turns, max_tokens: 0 }
    }

    /// Set a token budget for conversation history. When exceeded, older messages are trimmed.
    /// Uses a rough estimate of ~2 chars per token.
    pub fn with_token_budget(mut self, max_tokens: usize) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn push_user(&mut self, content: &str) {
        self.messages.push(LlmChatMessage::user(content));
        self.trim_to_limits();
    }

    pub fn push_assistant(&mut self, content: &str) {
        self.messages.push(LlmChatMessage::assistant(content));
        self.trim_to_limits();
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Convert to rig-core Message format for LLM consumption.
    pub fn to_rig_messages(&self) -> Vec<rig::message::Message> {
        self.messages
            .iter()
            .map(|m| {
                use rig::message::{Message, UserContent, AssistantContent, Text};
                use rig::OneOrMany;
                match m.role {
                    LlmChatRole::System | LlmChatRole::User => {
                        Message::User {
                            content: OneOrMany::one(UserContent::Text(Text { text: m.content.clone() }))
                        }
                    }
                    LlmChatRole::Assistant => {
                        Message::Assistant {
                            id: None,
                            content: OneOrMany::one(AssistantContent::Text(Text { text: m.content.clone() }))
                        }
                    }
                }
            })
            .collect()
    }

    fn trim_to_limits(&mut self) {
        // First trim by turn count
        self.trim_by_turns();
        // Then trim by token budget if set
        if self.max_tokens > 0 {
            self.trim_by_token_budget();
        }
    }

    fn trim_by_turns(&mut self) {
        // Keep system messages, trim user/assistant pairs from the front
        let systems: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.role == LlmChatRole::System)
            .cloned()
            .collect();

        let non_system: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.role != LlmChatRole::System)
            .cloned()
            .collect();

        let to_remove = non_system.len().saturating_sub(self.max_turns * 2);
        let trimmed: Vec<_> = non_system.into_iter().skip(to_remove).collect();

        self.messages.clear();
        self.messages.extend(systems);
        self.messages.extend(trimmed);
    }

    /// Trim oldest non-system messages until estimated token count is within budget.
    /// Rough estimate: ~2 chars per token.
    fn trim_by_token_budget(&mut self) {
        let budget = self.max_tokens;
        // Calculate total estimated tokens
        let total_chars: usize = self.messages.iter().map(|m| m.content.len()).sum();
        let estimated_tokens = total_chars / 2;

        if estimated_tokens <= budget {
            return;
        }

        // Remove oldest non-system messages until within budget
        let systems: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.role == LlmChatRole::System)
            .cloned()
            .collect();

        let mut non_system: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.role != LlmChatRole::System)
            .cloned()
            .collect();

        // Remove from front until budget is met
        let system_chars: usize = systems.iter().map(|m| m.content.len()).sum();
        let budget_chars = budget.saturating_mul(2).saturating_sub(system_chars);

        let mut current_chars: usize = non_system.iter().map(|m| m.content.len()).sum();
        while current_chars > budget_chars && non_system.len() > 2 {
            current_chars -= non_system.remove(0).content.len();
        }

        self.messages.clear();
        self.messages.extend(systems);
        self.messages.extend(non_system);
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Access the raw message list (for history compression).
    pub fn messages(&self) -> &[LlmChatMessage] {
        &self.messages
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Get the current token budget (0 = no limit)
    pub fn token_budget(&self) -> usize {
        self.max_tokens
    }

    /// Get estimated token count for current history
    pub fn estimated_tokens(&self) -> usize {
        self.messages.iter().map(|m| m.content.len()).sum::<usize>() / 2
    }
}

// ---------------------------------------------------------------------------
// StepSpec for plan generation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSpec {
    #[serde(rename = "type")]
    pub step_type: String,
    pub instruction: String,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub params: serde_json::Value,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub summary: String,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: LlmProvider,
    pub model: String,
    pub api_key: Option<String>,
    /// Base URL override (for proxies or local servers).
    pub base_url: Option<String>,
    pub max_tokens: u32,
    pub temperature: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LlmProvider {
    Anthropic,
    OpenAI,
    DeepSeek,
    Ollama,
}

impl std::fmt::Display for LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmProvider::Anthropic => write!(f, "anthropic"),
            LlmProvider::OpenAI => write!(f, "openai"),
            LlmProvider::DeepSeek => write!(f, "deepseek"),
            LlmProvider::Ollama => write!(f, "ollama"),
        }
    }
}

impl LlmConfig {
    /// Create a default config for a given provider.
    pub fn new(provider: LlmProvider, api_key: Option<String>) -> Self {
        let (model, base_url) = match &provider {
            LlmProvider::Anthropic => ("claude-sonnet-4-20250514".into(), None),
            LlmProvider::OpenAI => ("gpt-4o".into(), None),
            LlmProvider::DeepSeek => ("deepseek-chat".into(), None),
            LlmProvider::Ollama => {
                ("llama3.2".into(), Some("http://localhost:11434".into()))
            }
        };
        Self {
            provider,
            model,
            api_key,
            base_url,
            max_tokens: 2048,
            temperature: 0.7,
        }
    }
}

// ---------------------------------------------------------------------------
// Gateway — wraps rig-core providers behind a unified interface
// ---------------------------------------------------------------------------

/// Unified gateway for multiple LLM providers.
pub struct LlmGateway {
    config: LlmConfig,
    jail_root: Option<PathBuf>,
}

impl LlmGateway {
    pub fn new(config: LlmConfig) -> Self {
        Self { config, jail_root: None }
    }

    pub fn with_jail(config: LlmConfig, jail_root: PathBuf) -> Self {
        Self { config, jail_root: Some(jail_root) }
    }

    pub fn config(&self) -> &LlmConfig {
        &self.config
    }

    /// Send messages to the LLM and return the response text and token usage.
    pub async fn chat(&self, messages: &[LlmChatMessage]) -> AgentResult<(String, TokenUsage)> {
        let (system, rest): (Vec<_>, Vec<_>) =
            messages.iter().partition(|m| m.role == LlmChatRole::System);

        let preamble = system
            .first()
            .map(|m| m.content.as_str())
            .unwrap_or("You are a helpful AI assistant.");

        let prompt = if rest.is_empty() {
            "Continue.".to_string()
        } else {
            rest.iter()
                .map(|m| format!("{}: {}", role_label(&m.role), m.content))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let jail_root = self.jail_root.clone();
        let (text, prompt_tokens, completion_tokens): (String, u64, u64) = match &self.config.provider {
            LlmProvider::Anthropic => {
                let agent = build_anthropic_agent(&self.config, preamble, jail_root.as_deref())?;
                let response = agent.prompt(&prompt)
                    .extended_details()
                    .await
                    .map_err(|e| AgentError::Llm(format!("LLM request failed: {e}")))?;
                (response.output, response.total_usage.input_tokens, response.total_usage.output_tokens)
            }
            LlmProvider::OpenAI => {
                let agent = build_openai_agent(&self.config, preamble, jail_root.as_deref())?;
                let response = agent.prompt(&prompt)
                    .extended_details()
                    .await
                    .map_err(|e| AgentError::Llm(format!("LLM request failed: {e}")))?;
                (response.output, response.total_usage.input_tokens, response.total_usage.output_tokens)
            }
            LlmProvider::DeepSeek => {
                let agent = build_deepseek_agent(&self.config, preamble, jail_root.as_deref())?;
                let response = agent.prompt(&prompt)
                    .extended_details()
                    .await
                    .map_err(|e| AgentError::Llm(format!("LLM request failed: {e}")))?;
                (response.output, response.total_usage.input_tokens, response.total_usage.output_tokens)
            }
            LlmProvider::Ollama => {
                let agent = build_ollama_agent(&self.config, preamble, jail_root.as_deref())?;
                let response = agent.prompt(&prompt)
                    .extended_details()
                    .await
                    .map_err(|e| AgentError::Llm(format!("LLM request failed: {e}")))?;
                (response.output, response.total_usage.input_tokens, response.total_usage.output_tokens)
            }
        };

        let usage = TokenUsage {
            prompt_tokens: prompt_tokens as u32,
            completion_tokens: completion_tokens as u32,
        };

        info!(
            provider = %self.config.provider,
            model = %self.config.model,
            prompt_tokens = usage.prompt_tokens,
            completion_tokens = usage.completion_tokens,
            "LLM response received"
        );

        Ok((text, usage))
    }

    /// Multi-turn agent chat loop with memory context and streaming.
    pub async fn chat_agent_loop<F>(
        &self,
        user_message: &str,
        history: &ConversationHistory,
        max_turns: usize,
        safe_mode: bool,
        memory_context: Option<&str>,
        mut on_event: F,
        custom_preamble: Option<&str>,
        intent: Option<&crate::signal::IntentState>,
    ) -> AgentResult<(String, TokenUsage)>
    where
        F: FnMut(AgentEvent) + Send,
    {
        // Build preamble — STATIC ONLY for prompt caching.
        // Dynamic context (env signals, intent state, memory) goes into
        // the message list so the preamble stays identical across turns,
        // allowing Anthropic/OpenAI prompt caching to hit.
        let mut preamble = if let Some(custom) = custom_preamble {
            if !custom.is_empty() {
                custom.to_string()
            } else {
                self.build_preamble()
            }
        } else {
            self.build_preamble()
        };

        // Intent tracking instruction is static — it never changes across turns.
        // It belongs in the preamble so it gets cached.
        preamble.push_str("\n\n");
        preamble.push_str(&crate::signal::IntentState::system_instruction());

        // Build dynamic context — env signals, intent state, memory.
        // This goes into the message list, not the preamble, so the
        // cached preamble prefix stays valid across turns.
        let dynamic_context = Self::build_dynamic_context(memory_context, intent);

        // Build message history — compress old turns with intent if available
        let raw_messages = history.to_rig_messages();
        let messages = if let Some(intent_state) = intent {
            // Convert rig messages back to LlmChatMessage for compression,
            // then back to rig messages. This is a bridge step — ideally
            // compress_history_with_intent works on rig::Message directly,
            // but for now we use the LlmChatMessage path.
            let llm_messages = history.messages();
            let compressed = crate::signal::compress_history_with_intent(llm_messages, intent_state);
            compressed.iter().map(|m| {
                use rig::message::{Message, UserContent, AssistantContent, Text};
                use rig::OneOrMany;
                match m.role {
                    crate::llm::LlmChatRole::System | crate::llm::LlmChatRole::User => {
                        Message::User {
                            content: OneOrMany::one(UserContent::Text(Text { text: m.content.clone() }))
                        }
                    }
                    crate::llm::LlmChatRole::Assistant => {
                        Message::Assistant {
                            id: None,
                            content: OneOrMany::one(AssistantContent::Text(Text { text: m.content.clone() }))
                        }
                    }
                }
            }).collect::<Vec<_>>()
        } else {
            raw_messages
        };
        use rig::message::{Message, UserContent, Text};
        use rig::OneOrMany;
        let mut messages = messages;

        // Inject dynamic context as the first system message in the message list.
        // This keeps the preamble (static) separate from per-turn context (dynamic),
        // enabling prompt caching on the static prefix.
        if let Some(ctx) = dynamic_context {
            messages.insert(0, Message::User {
                content: OneOrMany::one(UserContent::Text(Text {
                    text: format!("[System Context — updated this turn]\n{}", ctx),
                })),
            });
        }

        messages.push(Message::User {
            content: OneOrMany::one(UserContent::Text(Text { text: user_message.to_string() }))
        });

        match &self.config.provider {
            LlmProvider::Anthropic => {
                let agent = build_anthropic_agent_streaming(&self.config, &preamble, self.jail_root.as_deref(), safe_mode)?;
                self.chat_stream_generic("Anthropic", agent, messages, max_turns, &mut on_event).await
            }
            LlmProvider::OpenAI => {
                let agent = build_openai_agent_streaming(&self.config, &preamble, self.jail_root.as_deref(), safe_mode)?;
                self.chat_stream_generic("OpenAI", agent, messages, max_turns, &mut on_event).await
            }
            LlmProvider::DeepSeek => {
                let agent = build_deepseek_agent_streaming(&self.config, &preamble, self.jail_root.as_deref(), safe_mode)?;
                self.chat_stream_generic("DeepSeek", agent, messages, max_turns, &mut on_event).await
            }
            LlmProvider::Ollama => {
                let agent = build_ollama_agent_streaming(&self.config, &preamble, self.jail_root.as_deref(), safe_mode)?;
                self.chat_stream_generic("Ollama", agent, messages, max_turns, &mut on_event).await
            }
        }
    }

    /// Extract text from ToolResultContent.
    fn extract_tool_result_text(content: &rig::OneOrMany<rig::message::ToolResultContent>) -> String {
        content.iter().map(|item| {
            match item {
                rig::message::ToolResultContent::Text(text) => text.text.clone(),
                rig::message::ToolResultContent::Image(_) => "[Image]".to_string(),
            }
        }).collect::<Vec<_>>().join("\n")
    }

    /// Extract tool name and args from StreamedAssistantContent.
    /// Only emits from complete ToolCall; ToolCallDelta is partial (Name or Delta only).
    fn extract_tool_info<R>(content: &rig::streaming::StreamedAssistantContent<R>) -> Option<(String, String)> {
        use rig::streaming::StreamedAssistantContent;
        match content {
            StreamedAssistantContent::ToolCall { tool_call, .. } => {
                let args_str = match &tool_call.function.arguments {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                Some((tool_call.function.name.clone(), args_str))
            }
            StreamedAssistantContent::ToolCallDelta { content: delta, .. } => {
                // ToolCallDeltaContent is partial: Name(String) or Delta(String)
                match delta {
                    rig::streaming::ToolCallDeltaContent::Name(name) => {
                        Some((name.clone(), String::new()))
                    }
                    rig::streaming::ToolCallDeltaContent::Delta(delta_text) => {
                        Some((String::new(), delta_text.clone()))
                    }
                }
            }
            _ => None,
        }
    }

    /// Generic streaming chat handler — shared by all providers.
    /// Eliminates the previous three near-identical methods.
    async fn chat_stream_generic<M, F>(
        &self,
        provider_name: &str,
        agent: rig::agent::Agent<M>,
        messages: Vec<rig::message::Message>,
        max_turns: usize,
        on_event: &mut F,
    ) -> AgentResult<(String, TokenUsage)>
    where
        M: rig::completion::CompletionModel + 'static,
        M::StreamingResponse: rig::completion::GetTokenUsage,
        F: FnMut(AgentEvent) + Send,
    {
        use futures::StreamExt;

        let mut stream = agent.stream_prompt("")
            .with_history(messages)
            .multi_turn(max_turns)
            .await;

        let mut full_text = String::new();
        let mut token_usage = TokenUsage::default();

        while let Some(result) = stream.next().await {
            let chunk = match result {
                Ok(c) => c,
                Err(e) => return Err(AgentError::Llm(format!("{provider_name} stream error: {e}"))),
            };
            match chunk {
                rig::agent::MultiTurnStreamItem::StreamAssistantItem(content) => {
                    match content {
                        rig::streaming::StreamedAssistantContent::Text(text) => {
                            let t = text.text.clone();
                            full_text.push_str(&t);
                            on_event(AgentEvent::TextDelta(t));
                        }
                        _ => {
                            if let Some((tool_name, args)) = Self::extract_tool_info(&content) {
                                on_event(AgentEvent::ToolCall {
                                    tool_name,
                                    args,
                                });
                            }
                        }
                    }
                }
                rig::agent::MultiTurnStreamItem::StreamUserItem(user_item) => {
                    let rig::streaming::StreamedUserContent::ToolResult { tool_result, .. } = user_item;
                    let tool_name = tool_result.call_id.clone()
                        .unwrap_or_else(|| tool_result.id.clone());
                    let result_text = Self::extract_tool_result_text(&tool_result.content);
                    on_event(AgentEvent::ToolResult {
                        tool_name,
                        result: result_text,
                    });
                }
                rig::agent::MultiTurnStreamItem::FinalResponse(response) => {
                    let usage = response.usage();
                    token_usage.prompt_tokens = usage.input_tokens as u32;
                    token_usage.completion_tokens = usage.output_tokens as u32;
                }
                _ => {}
            }
        }

        Ok((full_text, token_usage))
    }

    /// Generate a plan from a user task description using the LLM.
    pub async fn generate_plan(&self, task: &str) -> AgentResult<Vec<StepSpec>> {
        let preamble = r#"You are a task planning assistant. Given a user task, break it down into a sequence of steps.

For each step, specify:
- type: "think", "exec", "file_read", "file_write", "list_dir", "http_request", "wait_for_input", "finish"
- instruction: What to do in this step
- tool_name: The tool to use (if applicable)
- params: Tool parameters as JSON (if applicable)
- prompt: For think steps, the actual prompt to send to the LLM
- summary: Brief description of what this step accomplishes

Respond with a JSON array of steps."#;

        use rig::message::{Message, UserContent, Text};
        use rig::OneOrMany;

        let messages = vec![
            Message::User {
                content: OneOrMany::one(UserContent::Text(Text { text: preamble.to_string() }))
            },
            Message::User {
                content: OneOrMany::one(UserContent::Text(Text { text: format!("Task: {}", task) }))
            },
        ];

        let (response, _usage) = self.chat_with_messages(&messages).await?;

        // Parse JSON response
        let steps: Vec<StepSpec> = serde_json::from_str(&response)
            .map_err(|e| AgentError::Llm(format!("Failed to parse plan: {e}. Response: {}", response)))?;

        Ok(steps)
    }

    /// Internal helper: chat with pre-built message list.
    async fn chat_with_messages(
        &self,
        messages: &[rig::message::Message],
    ) -> AgentResult<(String, TokenUsage)> {
        let jail_root = self.jail_root.clone();
        let prompt_text = messages.iter().map(|m| {
            match m {
                rig::message::Message::User { content } => {
                    let text = extract_text_from_user_content(content);
                    format!("User: {}", text)
                }
                rig::message::Message::Assistant { content, .. } => {
                    let text = extract_text_from_assistant_content(content);
                    format!("Assistant: {}", text)
                }
            }
        }).collect::<Vec<_>>().join("\n\n");

        let (text, prompt_tokens, completion_tokens): (String, u64, u64) = match &self.config.provider {
            LlmProvider::Anthropic => {
                let agent = build_anthropic_agent(&self.config, "", jail_root.as_deref())?;
                let response = agent.prompt(&prompt_text)
                    .extended_details()
                    .await
                    .map_err(|e| AgentError::Llm(format!("LLM request failed: {e}")))?;
                (response.output, response.total_usage.input_tokens, response.total_usage.output_tokens)
            }
            LlmProvider::OpenAI => {
                let agent = build_openai_agent(&self.config, "", jail_root.as_deref())?;
                let response = agent.prompt(&prompt_text)
                    .extended_details()
                    .await
                    .map_err(|e| AgentError::Llm(format!("LLM request failed: {e}")))?;
                (response.output, response.total_usage.input_tokens, response.total_usage.output_tokens)
            }
            LlmProvider::DeepSeek => {
                let agent = build_deepseek_agent(&self.config, "", jail_root.as_deref())?;
                let response = agent.prompt(&prompt_text)
                    .extended_details()
                    .await
                    .map_err(|e| AgentError::Llm(format!("LLM request failed: {e}")))?;
                (response.output, response.total_usage.input_tokens, response.total_usage.output_tokens)
            }
            LlmProvider::Ollama => {
                let agent = build_ollama_agent(&self.config, "", jail_root.as_deref())?;
                let response = agent.prompt(&prompt_text)
                    .extended_details()
                    .await
                    .map_err(|e| AgentError::Llm(format!("LLM request failed: {e}")))?;
                (response.output, response.total_usage.input_tokens, response.total_usage.output_tokens)
            }
        };

        let usage = TokenUsage {
            prompt_tokens: prompt_tokens as u32,
            completion_tokens: completion_tokens as u32,
        };

        Ok((text, usage))
    }

    /// Build the static preamble — content that never changes across turns.
    /// This is the part that prompt caching can cache: identity, capabilities,
    /// communication style, output format, and intent tracking instructions.
    /// By keeping this pure static, Anthropic/OpenAI prompt caching can hit
    /// on the prefix across every turn in a session.
    fn build_preamble(&self) -> String {
        r#"You are Rupoo, an AI-powered terminal assistant running inside the user's terminal.
You help with software development, file operations, and system tasks.

## Your Capabilities
- File Operations: file_read, file_write, list_directory
- Web Search: search the internet for information (DuckDuckGo)
- Terminal Commands: execute shell commands (dangerous commands blocked)
- HTTP Requests: GET/POST to public URLs (localhost blocked for security)
- Browser Automation: headless navigation, screenshots, and page text extraction
- Memory: stores and retrieves context across sessions (FTS5 search)
- Skills: reusable workflows as JSON files, with auto-trigger on keywords
- Git: status, commit, create PR
- MCP Server: exposes tools via JSON-RPC over stdio

## Communication Style
- When the user's request is ambiguous or unclear, ask clarifying questions before proceeding.
- Before making irreversible changes (file writes, command execution), briefly confirm your plan.
- Show your reasoning: explain what you are about to do and why, especially for multi-step tasks.
- If a task requires multiple tool calls, describe the overall plan first, then execute step by step.

## Output Format
Be concise and structured.

### Reading files:
Show the file path, then the relevant content or summary.

### Listing directories:
Show the structure clearly.

### Running commands:
Show the command, then the output.

### Analyzing code:
Be specific about what you find. Show relevant snippets.

### Errors:
Be specific about the problem and the fix.

Keep responses tight. Use Markdown naturally for structure.
"#.to_string()
    }

    /// Build the dynamic context block — content that changes every turn.
    /// This is injected as a system message at the start of the message list,
    /// keeping it separate from the static preamble so that the preamble
    /// can be cached by the LLM provider's prompt caching mechanism.
    ///
    /// Contains: environment signals, intent state, memory context.
    fn build_dynamic_context(
        memory_context: Option<&str>,
        intent: Option<&crate::signal::IntentState>,
    ) -> Option<String> {
        let mut parts = Vec::new();

        // Environment signals (PWD, git, project type, etc.)
        let env_signals = crate::signal::EnvironmentSignals::collect();
        let env_block = env_signals.to_prompt_block();
        if !env_block.is_empty() {
            parts.push(env_block);
        }

        // Intent state
        if let Some(intent_state) = intent {
            let intent_block = intent_state.to_prompt_block();
            if !intent_block.is_empty() {
                parts.push(intent_block);
            }
        }

        // Memory context
        if let Some(context) = memory_context {
            if !context.is_empty() {
                parts.push(format!("## Relevant Memory\n{}", context));
            }
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
    }
}

// ---------------------------------------------------------------------------
// Per-provider agent builders
// ---------------------------------------------------------------------------

/// Register tools on the builder based on safe_mode setting.
/// Returns AgentBuilderSimple because .tool() transitions from AgentBuilder to AgentBuilderSimple.
fn register_tools<M: rig::completion::CompletionModel>(
    builder: rig::agent::AgentBuilderSimple<M>,
    jail_root: Option<&std::path::Path>,
    safe_mode: bool,
) -> rig::agent::AgentBuilderSimple<M> {
    // Already have EchoTool from the initial builder
    let mut builder = builder;

    // Web search is read-only and safe — always register
    builder = builder.tool(crate::rig_tools::WebSearchTool::new());

    // FileReadTool is safe
    if let Some(root) = jail_root {
        builder = builder.tool(crate::rig_tools::FileReadTool::with_jail(root.to_path_buf()));
    } else {
        builder = builder.tool(crate::rig_tools::FileReadTool::new());
    }

    // ListDirTool is safe
    if let Some(root) = jail_root {
        builder = builder.tool(crate::rig_tools::ListDirTool::with_jail(root.to_path_buf()));
    } else {
        builder = builder.tool(crate::rig_tools::ListDirTool::new());
    }

    // FileWriteTool is write operations - only register in unsafe mode
    if !safe_mode {
        if let Some(root) = jail_root {
            builder = builder.tool(crate::rig_tools::FileWriteTool::with_jail(root.to_path_buf()));
        } else {
            builder = builder.tool(crate::rig_tools::FileWriteTool::new());
        }
    }

    builder
}

/// Register tools (all tools, no safe_mode filtering) for legacy non-streaming agents.
fn register_tools_legacy<M: rig::completion::CompletionModel>(
    builder: rig::agent::AgentBuilderSimple<M>,
    jail_root: Option<&std::path::Path>,
) -> rig::agent::AgentBuilderSimple<M> {
    let builder = builder.tool(crate::rig_tools::WebSearchTool::new());
    if let Some(root) = jail_root {
        builder
            .tool(crate::rig_tools::FileReadTool::with_jail(root.to_path_buf()))
            .tool(crate::rig_tools::FileWriteTool::with_jail(root.to_path_buf()))
            .tool(crate::rig_tools::ListDirTool::with_jail(root.to_path_buf()))
    } else {
        builder
            .tool(crate::rig_tools::FileReadTool::new())
            .tool(crate::rig_tools::FileWriteTool::new())
            .tool(crate::rig_tools::ListDirTool::new())
    }
}

fn build_anthropic_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
) -> AgentResult<rig::agent::Agent<rig::providers::anthropic::completion::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Config("Anthropic requires an API key. Set it via: rupoo config set api_key.anthropic <key>".into()))?;
    let client = rig::providers::anthropic::client::Client::new(api_key)
        .map_err(|e| AgentError::Llm(format!("Anthropic client init failed: {e}")))?;
    let model = rig::providers::anthropic::completion::CompletionModel::new(client, &config.model)
        .with_prompt_caching();

    let builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .default_max_turns(25)
        .tool(crate::rig_tools::EchoTool::new());

    let builder = register_tools_legacy(builder, jail_root);

    Ok(builder.build())
}

fn build_openai_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
) -> AgentResult<rig::agent::Agent<rig::providers::openai::completion::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Config("OpenAI requires an API key. Set it via: rupoo config set api_key.openai <key>".into()))?;
    let client = match &config.base_url {
        Some(custom_url) => {
            rig::providers::openai::client::Client::builder()
                .api_key(api_key)
                .base_url(custom_url)
                .build()
                .map_err(|e| AgentError::Llm(format!("OpenAI client init failed: {e}")))?
        }
        None => {
            rig::providers::openai::client::Client::new(api_key)
                .map_err(|e| AgentError::Llm(format!("OpenAI client init failed: {e}")))?
        }
    };
    let model = rig::providers::openai::completion::CompletionModel::new(
        client.completions_api(),
        &config.model,
    );

    let builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .default_max_turns(25)
        .tool(crate::rig_tools::EchoTool::new());

    let mut builder = register_tools_legacy(builder, jail_root);

    if config.base_url.is_some() {
        builder = builder.additional_params(serde_json::json!({
            "thinking": {"type": "disabled"}
        }));
    }

    Ok(builder.build())
}

fn build_ollama_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
) -> AgentResult<rig::agent::Agent<rig::providers::ollama::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let base_url = config.base_url.as_deref().unwrap_or("http://localhost:11434");
    let client = rig::providers::ollama::Client::builder()
        .api_key(rig::client::Nothing)
        .base_url(base_url)
        .build()
        .map_err(|e| AgentError::Llm(format!("Ollama client init failed: {e}")))?;
    let model = rig::providers::ollama::CompletionModel::new(client, &config.model);

    let builder = AgentBuilder::new(model)
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .default_max_turns(25)
        .tool(crate::rig_tools::EchoTool::new());

    let builder = register_tools_legacy(builder, jail_root);

    Ok(builder.build())
}

/// DeepSeek agent — native provider with reasoning_content support.
/// Using the dedicated DeepSeek provider instead of the OpenAI-compatible
/// path ensures that `reasoning_content` is correctly handled in multi-turn
/// tool-call conversations (DeepSeek API requires reasoning_content to be
/// passed back on subsequent turns).
fn build_deepseek_agent(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
) -> AgentResult<rig::agent::Agent<rig::providers::deepseek::CompletionModel>> {
    use rig::agent::AgentBuilder;
    use rig::client::CompletionClient;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Config("DeepSeek requires an API key. Set it via: rupoo config set api_key.deepseek <key>".into()))?;

    let mut client_builder = rig::providers::deepseek::Client::builder();
    if let Some(base_url) = &config.base_url {
        client_builder = client_builder.base_url(base_url);
    }
    let client = client_builder
        .api_key(api_key)
        .build()
        .map_err(|e| AgentError::Llm(format!("DeepSeek client init failed: {e}")))?;
    let model = client.completion_model(&config.model);

    let builder = AgentBuilder::new(model)
        .additional_params(serde_json::json!({
            "thinking": {"type": "disabled"}
        }))
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .default_max_turns(25)
        .tool(crate::rig_tools::EchoTool::new());

    let builder = register_tools_legacy(builder, jail_root);

    Ok(builder.build())
}

/// Helper to finish building a streaming agent: apply common settings, register tools, build.
fn finish_streaming_agent<M: rig::completion::CompletionModel>(
    builder: rig::agent::AgentBuilder<M>,
    preamble: &str,
    config: &LlmConfig,
    jail_root: Option<&std::path::Path>,
    safe_mode: bool,
) -> AgentResult<rig::agent::Agent<M>> {
    let builder = builder
        .preamble(preamble)
        .temperature(config.temperature)
        .max_tokens(config.max_tokens as u64)
        .default_max_turns(25)
        .tool(crate::rig_tools::EchoTool::new());

    let builder = register_tools(builder, jail_root, safe_mode);

    Ok(builder.build())
}

/// Streaming agent for Anthropic with safe_mode + prompt caching.
fn build_anthropic_agent_streaming(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    safe_mode: bool,
) -> AgentResult<rig::agent::Agent<rig::providers::anthropic::completion::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Config("Anthropic requires an API key. Set it via: rupoo config set api_key.anthropic <key>".into()))?;
    let client = rig::providers::anthropic::client::Client::new(api_key)
        .map_err(|e| AgentError::Llm(format!("Anthropic client init failed: {e}")))?;
    // Enable prompt caching — Anthropic caches the system prompt prefix,
    // saving ~90% on input tokens for cached turns. The preamble is kept
    // pure static (no dynamic context) specifically to maximize cache hits.
    let model = rig::providers::anthropic::completion::CompletionModel::new(client, &config.model)
        .with_prompt_caching();

    finish_streaming_agent(AgentBuilder::new(model), preamble, config, jail_root, safe_mode)
}

/// Streaming agent for OpenAI with safe_mode.
fn build_openai_agent_streaming(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    safe_mode: bool,
) -> AgentResult<rig::agent::Agent<rig::providers::openai::completion::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Config("OpenAI requires an API key. Set it via: rupoo config set api_key.openai <key>".into()))?;
    let client = match &config.base_url {
        Some(custom_url) => {
            rig::providers::openai::client::Client::builder()
                .api_key(api_key)
                .base_url(custom_url)
                .build()
                .map_err(|e| AgentError::Llm(format!("OpenAI client init failed: {e}")))?
        }
        None => {
            rig::providers::openai::client::Client::new(api_key)
                .map_err(|e| AgentError::Llm(format!("OpenAI client init failed: {e}")))?
        }
    };
    let model = rig::providers::openai::completion::CompletionModel::new(
        client.completions_api(),
        &config.model,
    );

    finish_streaming_agent(AgentBuilder::new(model), preamble, config, jail_root, safe_mode)
}

/// Streaming agent for Ollama with safe_mode.
fn build_ollama_agent_streaming(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    safe_mode: bool,
) -> AgentResult<rig::agent::Agent<rig::providers::ollama::CompletionModel>> {
    use rig::agent::AgentBuilder;

    let base_url = config.base_url.as_deref().unwrap_or("http://localhost:11434");
    let client = rig::providers::ollama::Client::builder()
        .api_key(rig::client::Nothing)
        .base_url(base_url)
        .build()
        .map_err(|e| AgentError::Llm(format!("Ollama client init failed: {e}")))?;
    let model = rig::providers::ollama::CompletionModel::new(client, &config.model);

    finish_streaming_agent(AgentBuilder::new(model), preamble, config, jail_root, safe_mode)
}

/// Streaming DeepSeek agent with reasoning_content support.
fn build_deepseek_agent_streaming(
    config: &LlmConfig,
    preamble: &str,
    jail_root: Option<&std::path::Path>,
    safe_mode: bool,
) -> AgentResult<rig::agent::Agent<rig::providers::deepseek::CompletionModel>> {
    use rig::agent::AgentBuilder;
    use rig::client::CompletionClient;

    let api_key = config.api_key.as_deref()
        .ok_or_else(|| AgentError::Config("DeepSeek requires an API key. Set it via: rupoo config set api_key.deepseek <key>".into()))?;

    let mut client_builder = rig::providers::deepseek::Client::builder();
    if let Some(base_url) = &config.base_url {
        client_builder = client_builder.base_url(base_url);
    }
    let client = client_builder
        .api_key(api_key)
        .build()
        .map_err(|e| AgentError::Llm(format!("DeepSeek client init failed: {e}")))?;
    let model = client.completion_model(&config.model);

    // Disable DeepSeek thinking mode — rig's DeepSeek provider has a bug in
    // reasoning_content handling during multi-turn tool calls (it drops
    // assistant messages that contain reasoning_content from chat history).
    // By disabling thinking mode, the API won't return reasoning_content,
    // avoiding the 400 error "reasoning_content must be passed back".
    let builder = AgentBuilder::new(model)
        .additional_params(serde_json::json!({
            "thinking": {"type": "disabled"}
        }));

    finish_streaming_agent(builder, preamble, config, jail_root, safe_mode)
}

fn role_label(role: &LlmChatRole) -> &'static str {
    match role {
        LlmChatRole::System => "System",
        LlmChatRole::User => "User",
        LlmChatRole::Assistant => "Assistant",
    }
}

/// Extract text content from UserContent.
fn extract_text_from_user_content(content: &rig::OneOrMany<rig::message::UserContent>) -> String {
    content.iter().filter_map(|item| {
        if let rig::message::UserContent::Text(text) = item {
            Some(text.text.clone())
        } else {
            None
        }
    }).collect::<Vec<_>>().join("\n")
}

/// Extract text content from AssistantContent.
fn extract_text_from_assistant_content(content: &rig::OneOrMany<rig::message::AssistantContent>) -> String {
    content.iter().filter_map(|item| {
        if let rig::message::AssistantContent::Text(text) = item {
            Some(text.text.clone())
        } else {
            None
        }
    }).collect::<Vec<_>>().join("\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_config_defaults() {
        let cfg = LlmConfig::new(LlmProvider::Anthropic, Some("sk-ant-xxx".into()));
        assert_eq!(cfg.provider, LlmProvider::Anthropic);
        assert!(cfg.model.contains("claude"));
        assert!((cfg.max_tokens as f64 - 2048.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ollama_default_base_url() {
        let cfg = LlmConfig::new(LlmProvider::Ollama, None);
        assert_eq!(cfg.base_url.as_deref(), Some("http://localhost:11434"));
    }

    #[test]
    fn test_llm_chat_message_serde() {
        let msg = LlmChatMessage::user("Hello");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("User"));
        assert!(json.contains("Hello"));

        let deserialized: LlmChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.role, LlmChatRole::User);
        assert_eq!(deserialized.content, "Hello");
    }

    #[test]
    fn test_conversation_history() {
        let mut history = ConversationHistory::new(5);
        history.push_user("Hello");
        history.push_assistant("Hi there!");
        history.push_user("How are you?");
        history.push_assistant("I'm good!");

        assert_eq!(history.message_count(), 4);
        assert!(!history.is_empty());

        let messages = history.to_rig_messages();
        assert_eq!(messages.len(), 4);
    }

    #[test]
    fn test_conversation_history_trim() {
        let mut history = ConversationHistory::new(2);
        history.push_user("Turn 1");
        history.push_assistant("Response 1");
        history.push_user("Turn 2");
        history.push_assistant("Response 2");
        history.push_user("Turn 3");
        history.push_assistant("Response 3");

        assert!(history.message_count() <= 4);
    }

    #[test]
    fn test_step_spec_serde() {
        let spec = StepSpec {
            step_type: "think".to_string(),
            instruction: "Analyze this".to_string(),
            tool_name: "".to_string(),
            params: serde_json::json!({}),
            prompt: "What is 2+2?".to_string(),
            summary: "Math analysis".to_string(),
        };

        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("\"type\":\"think\""));

        let back: StepSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.step_type, "think");
    }

    #[test]
    fn test_llm_config_f64_types() {
        let cfg = LlmConfig::new(LlmProvider::Anthropic, None);
        let _: f64 = cfg.temperature;
    }
}
