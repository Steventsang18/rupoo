use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::task;
use tauri::{Emitter, State};

use rupoo::{
    agent::Agent,
    db::TaskRepo,
    llm::{AgentEvent, ConversationHistory},
    mcp::McpToolExecutor,
};

// ============================================================
// Global Agent State with tokio::sync::Mutex for thread safety
// ============================================================

#[derive(Clone)]
pub struct AgentState {
    pub repo: Arc<TaskRepo>,
    pub agent: Arc<tokio::sync::Mutex<Option<Arc<Agent>>>>,
    #[allow(dead_code)]
    pub request_queue: Arc<Mutex<VecDeque<ChatRequest>>>,
}

use std::collections::VecDeque;

#[allow(dead_code)]
pub struct ChatRequest {
    pub prompt: String,
    pub history: Vec<ChatMessage>,
    pub max_turns: usize,
    pub safe_mode: bool,
    pub app_handle: tauri::AppHandle,
}

impl AgentState {
    pub fn new(repo: Arc<TaskRepo>) -> Self {
        let tool_executor: Arc<dyn rupoo::agent::ToolExecutor> = Arc::new(McpToolExecutor::new());
        let agent = Arc::new(Agent::new(Arc::clone(&repo), tool_executor));
        Self { 
            repo,
            agent: Arc::new(tokio::sync::Mutex::new(Some(agent))),
            request_queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

// ============================================================
// Request/Response Types
// ============================================================

#[derive(Deserialize)]
pub struct AgentChatRequest {
    pub prompt: String,
    pub history: Vec<ChatMessage>,
    pub max_turns: Option<usize>,
    pub safe_mode: Option<bool>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct AgentChatResponse {
    pub content: String,
    pub token_usage: Option<TokenUsage>,
}

#[derive(Serialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Deserialize)]
pub struct SaveSessionRequest {
    pub session_id: String,
    pub name: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub metadata: serde_json::Value,
}

#[derive(Deserialize)]
pub struct LoadSessionRequest {
    pub session_id: String,
}

#[derive(Serialize)]
pub struct LoadSessionResponse {
    pub session_id: String,
    pub name: String,
    pub messages: Vec<ChatMessage>,
    pub metadata: serde_json::Value,
}

#[derive(Deserialize)]
pub struct RenameSessionRequest {
    pub session_id: String,
    pub new_name: String,
}

#[derive(Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub name: String,
    pub message_count: usize,
    pub updated_at: String,
}

// ============================================================
// Helper: Convert frontend messages to ConversationHistory
// ============================================================

fn history_from_messages(messages: &[ChatMessage]) -> ConversationHistory {
    let mut history = ConversationHistory::new(10);
    
    for msg in messages {
        let role = match msg.role.as_str() {
            "system" => rupoo::llm::history::LlmChatRole::System,
            "user" => rupoo::llm::history::LlmChatRole::User,
            _ => rupoo::llm::history::LlmChatRole::Assistant,
        };
        if role == rupoo::llm::history::LlmChatRole::User {
            history.push_user(&msg.content);
        } else {
            history.push_assistant(&msg.content);
        }
    }
    
    history
}

// ============================================================
// Helper: Emit log event
// ============================================================

fn emit_log(app: &tauri::AppHandle, level: &str, category: &str, message: &str, detail: &str) {
    let _ = app.emit(
        "log-event",
        serde_json::json!({
            "level": level,
            "category": category,
            "message": message,
            "detail": detail,
            "timestamp": chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()
        }),
    );
}

/// Public convenience for agent log (used by lib.rs setup)
pub fn emit_agent_log(app: &tauri::AppHandle, level: &str, message: &str) {
    emit_log(app, level, "agent", message, "");
}

// ============================================================
// IPC Commands
// ============================================================

#[tauri::command]
pub async fn run_agent_chat(
    app: tauri::AppHandle,
    state: State<'_, AgentState>,
    req: AgentChatRequest,
) -> Result<AgentChatResponse, String> {
    emit_log(&app, "info", "llm", "Starting agent chat", &format!(
        "prompt_len={} history_len={}",
        req.prompt.len(),
        req.history.len()
    ));

    let max_turns = req.max_turns.unwrap_or(3);
    let safe_mode = req.safe_mode.unwrap_or(false);

    let history = history_from_messages(&req.history);
    let app_clone = app.clone();
    let app_clone2 = app.clone();

    // Clone the agent Arc to move into the async task
    let agent_arc = {
        let guard = state.agent.lock().await;
        guard.clone().ok_or_else(|| "Agent not initialized".to_string())?
    };

    let result = task::spawn(async move {
        let mut full_response = String::new();

        let on_event = |event: AgentEvent| {
            match event {
                AgentEvent::TextDelta(delta) => {
                    full_response.push_str(&delta);
                    let _ = app_clone.emit(
                        "agent_stream",
                        serde_json::json!({ "content": full_response.clone() })
                    );
                }
                AgentEvent::ToolCall { tool_name, args } => {
                    emit_log(&app_clone, "info", "tool", &format!("Tool call: {}", tool_name), &args);
                    let _ = app_clone.emit(
                        "agent_stream",
                        serde_json::json!({
                            "content": full_response.clone(),
                            "tool_call": serde_json::json!({
                                "tool_name": tool_name,
                                "args": serde_json::from_str::<serde_json::Value>(&args).unwrap_or_default()
                            })
                        })
                    );
                }
                AgentEvent::ToolResult { tool_name, result } => {
                    emit_log(&app_clone, "info", "tool", &format!("Tool result: {}", tool_name), &result);
                }
            }
        };

        let (response, usage) = match agent_arc
            .agent_chat(
                &req.prompt,
                &history,
                max_turns,
                safe_mode,
                on_event,
                None,
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let err_str = format!("Agent chat failed: {}", e);
                emit_log(&app_clone, "error", "llm", &err_str, "");
                let _ = app_clone.emit("agent_error", serde_json::json!({ "error": err_str.clone() }));
                return Err(err_str);
            }
        };

        full_response = response.clone();
        emit_log(&app_clone, "info", "llm", &format!("Agent chat completed: {} chars", full_response.len()), "");
        
        let _ = app_clone.emit(
            "agent_done",
            serde_json::json!({ "message": full_response.clone() })
        );

        Ok(AgentChatResponse {
            content: full_response,
            token_usage: Some(TokenUsage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total(),
            }),
        })
    }).await.unwrap_or_else(|e| {
        let err_str = format!("Task spawn error: {}", e);
        emit_log(&app_clone2, "error", "llm", &err_str, "");
        let _ = app_clone2.emit("agent_error", serde_json::json!({ "error": err_str.clone() }));
        Err(err_str)
    });

    result
}

#[tauri::command]
pub async fn save_session(
    app: tauri::AppHandle,
    state: State<'_, AgentState>,
    req: SaveSessionRequest,
) -> Result<bool, String> {
    emit_log(&app, "info", "session", &format!("Saving session: {}", req.session_id), "");

    let session_name = req.name.clone().unwrap_or_else(|| {
        format!("Chat {}", chrono::Local::now().format("%m-%d %H:%M"))
    });

    let data = serde_json::json!({
        "session_id": req.session_id,
        "name": session_name,
        "messages": req.messages,
        "metadata": req.metadata,
        "saved_at": chrono::Utc::now().to_rfc3339(),
        "updated_at": chrono::Utc::now().to_rfc3339()
    });

    state.repo
        .set_setting(&format!("session:{}", req.session_id), &data.to_string())
        .await
        .map_err(|e| format!("Failed to save session: {}", e))?;

    emit_log(&app, "info", "session", &format!("Session saved: {}", req.session_id), "");
    Ok(true)
}

#[tauri::command]
pub async fn load_session(
    app: tauri::AppHandle,
    state: State<'_, AgentState>,
    req: LoadSessionRequest,
) -> Result<LoadSessionResponse, String> {
    emit_log(&app, "info", "session", &format!("Loading session: {}", req.session_id), "");

    let data_str = state.repo
        .get_setting(&format!("session:{}", req.session_id))
        .await
        .map_err(|e| format!("Failed to load session: {}", e))?
        .ok_or_else(|| format!("Session not found: {}", req.session_id))?;

    let data: serde_json::Value = serde_json::from_str(&data_str)
        .map_err(|e| format!("Invalid session data: {}", e))?;

    let session_id = data["session_id"].as_str().unwrap_or(&req.session_id).to_string();
    let name = data["name"].as_str().unwrap_or(&format!("Chat {}", req.session_id)).to_string();
    let messages: Vec<ChatMessage> = serde_json::from_value(data["messages"].clone())
        .map_err(|e| format!("Failed to parse messages: {}", e))?;

    let metadata = data["metadata"].clone();

    emit_log(&app, "info", "session", &format!("Session loaded: {} messages", messages.len()), "");
    
    Ok(LoadSessionResponse { 
        session_id,
        name,
        messages, 
        metadata 
    })
}

#[tauri::command]
pub async fn delete_session(
    app: tauri::AppHandle,
    state: State<'_, AgentState>,
    session_id: String,
) -> Result<bool, String> {
    emit_log(&app, "info", "session", &format!("Deleting session: {}", session_id), "");

    state.repo
        .delete_setting(&format!("session:{}", session_id))
        .await
        .map_err(|e| format!("Failed to delete session: {}", e))?;

    emit_log(&app, "info", "session", &format!("Session deleted: {}", session_id), "");
    Ok(true)
}

#[tauri::command]
pub async fn rename_session(
    app: tauri::AppHandle,
    state: State<'_, AgentState>,
    req: RenameSessionRequest,
) -> Result<bool, String> {
    emit_log(&app, "info", "session", &format!("Renaming session: {} -> {}", req.session_id, req.new_name), "");

    let data_str = state.repo
        .get_setting(&format!("session:{}", req.session_id))
        .await
        .map_err(|e| format!("Failed to load session: {}", e))?
        .ok_or_else(|| format!("Session not found: {}", req.session_id))?;

    let mut data: serde_json::Value = serde_json::from_str(&data_str)
        .map_err(|e| format!("Invalid session data: {}", e))?;

    data["name"] = serde_json::Value::String(req.new_name.clone());
    data["updated_at"] = serde_json::Value::String(chrono::Utc::now().to_rfc3339());

    state.repo
        .set_setting(&format!("session:{}", req.session_id), &data.to_string())
        .await
        .map_err(|e| format!("Failed to save session: {}", e))?;

    emit_log(&app, "info", "session", &format!("Session renamed: {}", req.session_id), "");
    Ok(true)
}

#[tauri::command]
pub async fn list_sessions(
    app: tauri::AppHandle,
    state: State<'_, AgentState>,
) -> Result<Vec<SessionInfo>, String> {
    emit_log(&app, "info", "session", "Listing sessions", "");

    let settings = state.repo
        .list_settings()
        .await
        .map_err(|e| format!("Failed to list settings: {}", e))?;

    let mut sessions: Vec<SessionInfo> = settings
        .into_iter()
        .filter(|(key, _)| key.starts_with("session:"))
        .filter_map(|(key, value)| {
            let session_id = key.replace("session:", "");
            match serde_json::from_str::<serde_json::Value>(&value) {
                Ok(data) => {
                    let name = data["name"].as_str().unwrap_or(&session_id).to_string();
                    let messages = data["messages"].as_array().map(|a| a.len()).unwrap_or(0);
                    let updated_at = data["updated_at"].as_str().unwrap_or("").to_string();
                    Some(SessionInfo {
                        session_id,
                        name,
                        message_count: messages,
                        updated_at,
                    })
                }
                Err(_) => None,
            }
        })
        .collect();

    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    emit_log(&app, "info", "session", &format!("Found {} sessions", sessions.len()), "");
    Ok(sessions)
}