//! LLM Gateway - unified interface for multiple LLM providers.

use std::path::PathBuf;
use std::sync::Arc;

use tracing::{info, warn};

use crate::error::{AgentError, AgentResult};
use crate::llm::history::{ConversationHistory, LlmChatMessage};
use crate::llm::providers::{
    build_anthropic_agent, build_anthropic_agent_streaming, build_ollama_agent,
    build_ollama_agent_streaming, build_openai_agent, build_openai_agent_streaming,
};
use crate::llm::{AgentEvent, LlmConfig, LlmProvider, TokenUsage};

pub struct LlmGateway {
    config: LlmConfig,
    /// If set, file tools are restricted to this directory tree.
    /// If None, defaults to CWD at the time the gateway was created.
    jail_root: Option<PathBuf>,
    /// Shared HTTP client for connection pooling and TLS session reuse.
    /// Passed to all rig provider clients so a single connection pool
    /// is shared across all LLM requests.
    http_client: Arc<reqwest::Client>,
}

impl LlmGateway {
    /// Create a gateway with jail_root set to the current working directory.
    /// File writes are restricted to CWD and its subdirectories.
    pub fn new(config: LlmConfig) -> Self {
        let jail_root = std::env::current_dir()
            .ok()
            .map(|p| Self::normalize_path(&p));

        info!(
            jail_root = ?jail_root.as_ref().map(|p| p.as_os_str()),
            "Created LlmGateway with jail_root from CWD"
        );

        Self {
            jail_root,
            http_client: crate::http_client::HTTP_CLIENT.clone(),
            config,
        }
    }

    pub fn with_jail(config: LlmConfig, jail_root: PathBuf) -> Self {
        let normalized = Self::normalize_path(&jail_root);

        info!(
            jail_root = ?normalized.as_os_str(),
            "Created LlmGateway with explicit jail_root"
        );

        Self {
            config,
            jail_root: Some(normalized),
            http_client: crate::http_client::HTTP_CLIENT.clone(),
        }
    }

    /// Create a gateway with a pre-built shared HTTP client.
    pub fn with_http_client(
        config: LlmConfig,
        jail_root: Option<PathBuf>,
        http_client: Arc<reqwest::Client>,
    ) -> Self {
        let jail_root = jail_root.map(|p| Self::normalize_path(&p));

        info!(
            jail_root = ?jail_root.as_ref().map(|p| p.as_os_str()),
            "Created LlmGateway with custom HTTP client"
        );

        Self {
            config,
            jail_root,
            http_client,
        }
    }

    /// Normalize a path to an absolute, canonical path.
    /// This prevents path traversal attacks by resolving all symlinks and
    /// converting to an absolute path.
    fn normalize_path(path: &std::path::Path) -> PathBuf {
        match path.canonicalize() {
            Ok(canonical) => canonical,
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "Failed to canonicalize path, falling back to absolute path"
                );
                // Fall back to absolute path if canonicalization fails
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    match std::env::current_dir() {
                        Ok(cwd) => cwd.join(path),
                        Err(_) => path.to_path_buf(),
                    }
                }
            }
        }
    }

    /// Validate that a path is within the jail_root.
    /// Returns Ok(path) if valid, Err if path traversal is detected.
    pub fn validate_path(&self, path: &std::path::Path) -> AgentResult<PathBuf> {
        let normalized = Self::normalize_path(path);

        if let Some(ref jail) = self.jail_root {
            // Check if the normalized path starts with the jail_root
            if !normalized.starts_with(jail) {
                return Err(AgentError::PathTraversal {
                    path: path.display().to_string(),
                    jail_root: jail.display().to_string(),
                });
            }
        }

        Ok(normalized)
    }

    /// Get the jail_root as an absolute path.
    pub fn jail_root_absolute(&self) -> Option<&PathBuf> {
        self.jail_root.as_ref()
    }

    pub fn config(&self) -> &LlmConfig {
        &self.config
    }

    /// Send messages to the LLM and return the response text and token usage.
    pub async fn chat(&self, messages: &[LlmChatMessage]) -> AgentResult<(String, TokenUsage)> {
        use rig::completion::Prompt;

        let (system, rest): (Vec<_>, Vec<_>) = messages
            .iter()
            .partition(|m| m.role == crate::llm::LlmChatRole::System);

        let preamble = system
            .first()
            .map(|m| m.content.as_str())
            .unwrap_or("You are a helpful AI assistant.");

        let prompt = if rest.is_empty() {
            "Continue.".to_string()
        } else {
            rest.iter()
                .map(|m| {
                    format!(
                        "{}: {}",
                        crate::llm::providers::role_label(&m.role),
                        m.content
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let jail_root = self.jail_root.clone();
        let (text, prompt_tokens, completion_tokens): (String, u64, u64) =
            match &self.config.provider {
                LlmProvider::Anthropic => {
                    let agent = build_anthropic_agent(
                        &self.config,
                        preamble,
                        jail_root.as_deref(),
                        &self.http_client,
                    )?;
                    let response = agent
                        .prompt(&prompt)
                        .extended_details()
                        .await
                        .map_err(|e| AgentError::Llm(format!("LLM request failed: {e}")))?;
                    (
                        response.output,
                        response.total_usage.input_tokens,
                        response.total_usage.output_tokens,
                    )
                }
                LlmProvider::OpenAI => {
                    let agent = build_openai_agent(
                        &self.config,
                        preamble,
                        jail_root.as_deref(),
                        &self.http_client,
                    )?;
                    let response = agent
                        .prompt(&prompt)
                        .extended_details()
                        .await
                        .map_err(|e| AgentError::Llm(format!("LLM request failed: {e}")))?;
                    (
                        response.output,
                        response.total_usage.input_tokens,
                        response.total_usage.output_tokens,
                    )
                }
                LlmProvider::Ollama => {
                    let agent = build_ollama_agent(
                        &self.config,
                        preamble,
                        jail_root.as_deref(),
                        &self.http_client,
                    )?;
                    let response = agent
                        .prompt(&prompt)
                        .extended_details()
                        .await
                        .map_err(|e| AgentError::Llm(format!("LLM request failed: {e}")))?;
                    (
                        response.output,
                        response.total_usage.input_tokens,
                        response.total_usage.output_tokens,
                    )
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
    #[allow(clippy::too_many_arguments)]
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
            let compressed =
                crate::signal::compress_history_with_intent(llm_messages, intent_state);
            compressed
                .iter()
                .map(|m| {
                    use rig::message::{AssistantContent, Message, Text, UserContent};
                    use rig::OneOrMany;
                    match m.role {
                        crate::llm::LlmChatRole::System | crate::llm::LlmChatRole::User => {
                            Message::User {
                                content: OneOrMany::one(UserContent::Text(Text {
                                    text: m.content.clone(),
                                })),
                            }
                        }
                        crate::llm::LlmChatRole::Assistant => Message::Assistant {
                            id: None,
                            content: OneOrMany::one(AssistantContent::Text(Text {
                                text: m.content.clone(),
                            })),
                        },
                    }
                })
                .collect::<Vec<_>>()
        } else {
            raw_messages
        };
        use rig::message::{Message, Text, UserContent};
        use rig::OneOrMany;
        let mut messages = messages;

        // Inject dynamic context as the first system message in the message list.
        // This keeps the preamble (static) separate from per-turn context (dynamic),
        // enabling prompt caching on the static prefix.
        if let Some(ctx) = dynamic_context {
            messages.insert(
                0,
                Message::User {
                    content: OneOrMany::one(UserContent::Text(Text {
                        text: format!("[System Context — updated this turn]\n{}", ctx),
                    })),
                },
            );
        }

        messages.push(Message::User {
            content: OneOrMany::one(UserContent::Text(Text {
                text: user_message.to_string(),
            })),
        });

        match &self.config.provider {
            LlmProvider::Anthropic => {
                let agent = build_anthropic_agent_streaming(
                    &self.config,
                    &preamble,
                    self.jail_root.as_deref(),
                    safe_mode,
                    &self.http_client,
                )?;
                self.chat_stream_generic("Anthropic", agent, messages, max_turns, &mut on_event)
                    .await
            }
            LlmProvider::OpenAI => {
                let agent = build_openai_agent_streaming(
                    &self.config,
                    &preamble,
                    self.jail_root.as_deref(),
                    safe_mode,
                    &self.http_client,
                )?;
                self.chat_stream_generic("OpenAI", agent, messages, max_turns, &mut on_event)
                    .await
            }
            LlmProvider::Ollama => {
                let agent = build_ollama_agent_streaming(
                    &self.config,
                    &preamble,
                    self.jail_root.as_deref(),
                    safe_mode,
                    &self.http_client,
                )?;
                self.chat_stream_generic("Ollama", agent, messages, max_turns, &mut on_event)
                    .await
            }
        }
    }

    /// Extract text from ToolResultContent.
    fn extract_tool_result_text(
        content: &rig::OneOrMany<rig::message::ToolResultContent>,
    ) -> String {
        content
            .iter()
            .map(|item| match item {
                rig::message::ToolResultContent::Text(text) => text.text.clone(),
                rig::message::ToolResultContent::Image(_) => "[Image]".to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Extract tool name and args from StreamedAssistantContent.
    /// Only emits from complete ToolCall; ToolCallDelta is partial (Name or Delta only).
    fn extract_tool_info<R>(
        content: &rig::streaming::StreamedAssistantContent<R>,
    ) -> Option<(String, String)> {
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
        use rig::streaming::StreamingPrompt;

        let mut stream = agent
            .stream_prompt("")
            .with_history(messages)
            .multi_turn(max_turns)
            .await;

        let mut full_text = String::new();
        let mut token_usage = TokenUsage::default();

        while let Some(result) = stream.next().await {
            let chunk = match result {
                Ok(c) => c,
                Err(e) => {
                    return Err(AgentError::Llm(format!(
                        "{provider_name} stream error: {e}"
                    )))
                }
            };
            match chunk {
                rig::agent::MultiTurnStreamItem::StreamAssistantItem(content) => match content {
                    rig::streaming::StreamedAssistantContent::Text(text) => {
                        let t = text.text.clone();
                        full_text.push_str(&t);
                        on_event(AgentEvent::TextDelta(t));
                    }
                    _ => {
                        if let Some((tool_name, args)) = Self::extract_tool_info(&content) {
                            on_event(AgentEvent::ToolCall { tool_name, args });
                        }
                    }
                },
                rig::agent::MultiTurnStreamItem::StreamUserItem(user_item) => {
                    let rig::streaming::StreamedUserContent::ToolResult { tool_result, .. } =
                        user_item;
                    let tool_name = tool_result
                        .call_id
                        .clone()
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
    pub async fn generate_plan(&self, task: &str) -> AgentResult<Vec<crate::llm::StepSpec>> {
        let preamble = r#"You are a task planning assistant. Given a user task, break it down into a sequence of steps.

For each step, specify:
- type: "think", "exec", "file_read", "file_write", "list_dir", "http_request", "wait_for_input", "finish"
- instruction: What to do in this step
- tool_name: The tool to use (if applicable)
- params: Tool parameters as JSON (if applicable)
- prompt: For think steps, the actual prompt to send to the LLM
- summary: Brief description of what this step accomplishes

Respond with a JSON array of steps."#;

        use rig::message::{Message, Text, UserContent};
        use rig::OneOrMany;

        let messages = vec![
            Message::User {
                content: OneOrMany::one(UserContent::Text(Text {
                    text: preamble.to_string(),
                })),
            },
            Message::User {
                content: OneOrMany::one(UserContent::Text(Text {
                    text: format!("Task: {}", task),
                })),
            },
        ];

        let (response, _usage) = self.chat_with_messages(&messages).await?;

        // Strip markdown code fences if present (common with some models)
        let cleaned = extract_json_from_llm_response(&response);

        // Parse JSON response
        let steps: Vec<crate::llm::StepSpec> = serde_json::from_str(cleaned).map_err(|e| {
            AgentError::Llm(format!("Failed to parse plan: {e}. Response: {}", response))
        })?;

        Ok(steps)
    }
}

/// Extract JSON from an LLM response that may contain markdown fences and
/// preamble text (e.g. "Ok, let me analyze... ```json [...] ```").
pub fn extract_json_from_llm_response(response: &str) -> &str {
    let trimmed = response.trim();

    // If the whole response is already valid-looking JSON, return as-is
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        return trimmed;
    }

    // Find the first ```json or ``` code block
    if let Some(start) = trimmed.find("```json") {
        let after_fence = &trimmed[start + 7..]; // skip past ```json
        if let Some(end) = after_fence.find("```") {
            return after_fence[..end].trim();
        }
    }
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..]; // skip past ```
        if let Some(end) = after_fence.find("```") {
            return after_fence[..end].trim();
        }
    }

    trimmed
}

impl LlmGateway {
    async fn chat_with_messages(
        &self,
        messages: &[rig::message::Message],
    ) -> AgentResult<(String, TokenUsage)> {
        use rig::completion::Prompt;

        let jail_root = self.jail_root.clone();
        let prompt_text = messages
            .iter()
            .map(|m| match m {
                rig::message::Message::User { content } => {
                    let text = crate::llm::providers::extract_text_from_user_content(content);
                    format!("User: {}", text)
                }
                rig::message::Message::Assistant { content, .. } => {
                    let text = crate::llm::providers::extract_text_from_assistant_content(content);
                    format!("Assistant: {}", text)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let (text, prompt_tokens, completion_tokens): (String, u64, u64) = match &self
            .config
            .provider
        {
            LlmProvider::Anthropic => {
                let agent = build_anthropic_agent(
                    &self.config,
                    "",
                    jail_root.as_deref(),
                    &self.http_client,
                )?;
                let response = agent
                    .prompt(&prompt_text)
                    .extended_details()
                    .await
                    .map_err(|e| AgentError::Llm(format!("LLM request failed: {e}")))?;
                (
                    response.output,
                    response.total_usage.input_tokens,
                    response.total_usage.output_tokens,
                )
            }
            LlmProvider::OpenAI => {
                let agent =
                    build_openai_agent(&self.config, "", jail_root.as_deref(), &self.http_client)?;
                let response = agent
                    .prompt(&prompt_text)
                    .extended_details()
                    .await
                    .map_err(|e| AgentError::Llm(format!("LLM request failed: {e}")))?;
                (
                    response.output,
                    response.total_usage.input_tokens,
                    response.total_usage.output_tokens,
                )
            }
            LlmProvider::Ollama => {
                let agent =
                    build_ollama_agent(&self.config, "", jail_root.as_deref(), &self.http_client)?;
                let response = agent
                    .prompt(&prompt_text)
                    .extended_details()
                    .await
                    .map_err(|e| AgentError::Llm(format!("LLM request failed: {e}")))?;
                (
                    response.output,
                    response.total_usage.input_tokens,
                    response.total_usage.output_tokens,
                )
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

## Demand Clarification Flow
When the user provides a vague or ambiguous demand, follow this structured flow:
1. Start at clarification_state=core_confirm - ask 2-3 core questions with A/B choices
2. Then move to clarification_state=detail_drill - ask follow-up questions based on choices
3. Then move to clarification_state=summary - generate a need summary and ask for confirmation
4. After user confirms, move to clarification_state=completed - generate and execute development plan

Clarification Principles:
- Keep questions focused and concise
- Provide multiple-choice answers (A/B/C) whenever possible
- Limit to 2-3 questions per turn to avoid overwhelming user
- Goal is to get to actionable clarity in 3-5 turns max
- After each user response, store their choices in collected_choices

## Development Execution Mode (When clarification_state=completed)
When clarification is completed and you start executing, you MUST:

### Step 1: Show Development Plan
Before writing any code, present a structured development plan:

```
## 🚀 Development Plan

**Phase 1: 项目初始化**
- [ ] 创建项目结构
- [ ] 配置开发环境

**Phase 2: 核心功能实现**
- [ ] 实现功能 A
- [ ] 实现功能 B

**Phase 3: 测试验证**
- [ ] 编写测试用例
- [ ] 运行测试

**Phase 4: 交付完成**
- [ ] 代码审查
- [ ] 交付文档
```

### Step 2: Execute with Progress Updates
When executing each step:
- Say what you're doing: "【Phase 1 - 1/4】正在创建项目结构..."
- Explain why: "因为 [reason]"
- Show the result: "✅ 完成！创建了 index.html"

### Step 3: Show Progress After Each Step
After completing each task, show:
```
【进度】Phase 1 - 1/4 ✅
【总体进度】25% ████░░░░░░
```

## Transparent Execution
When executing tasks, ALWAYS be transparent about your process:

### Before Tool Calls - MANDATORY
Before EVERY tool call, you MUST output a clear explanation in this format:
```
## 🔧 工具调用准备
**即将执行**: [tool_name]
**执行原因**: [详细说明为什么要调用这个工具]
**输入参数**: 
- [参数名1]: [参数值1]
- [参数名2]: [参数值2]
**预期结果**: [这个调用应该返回什么]
```
Example:
```
## 🔧 工具调用准备
**即将执行**: file_write
**执行原因**: 需要创建扫雷游戏的HTML骨架文件
**输入参数**: 
- path: "minesweeper/index.html"
- content: "<!DOCTYPE html>..."
**预期结果**: 文件创建成功，返回文件大小确认
```

### During Execution
- Show your thinking process clearly: explain what you're about to do and why
- When modifying code, show before/after context with explanations
- Provide step-by-step explanations of what you are doing
- Be open about your decision-making process and trade-offs you considered
- If you make mistakes, acknowledge them and explain how you're fixing them

## Communication Style
- When the user's request is ambiguous or unclear, start the demand clarification flow
- Before making irreversible changes (file writes, command execution), briefly confirm your plan
- Show your reasoning: explain what you are about to do and why, especially for multi-step tasks
- If a task requires multiple tool calls, describe the overall plan first, then execute step by step
- Be transparent about every action you take - the user should understand exactly what you're doing

## Output Format
Be concise and structured.

### Development Progress Format:
```
## 📋 当前状态

**阶段**: Phase X/Y
**任务**: 具体任务名称
**进度**: ████░░░░░░ 40%

---

## 🔍 正在执行

[具体执行内容]

**原因**: [为什么这么做]
**预期**: [预期结果]
```

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

### Making changes:
Show before/after context and explain the changes.

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
