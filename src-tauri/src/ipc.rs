use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, RwLock};
use std::time::SystemTime;
use tauri::{Emitter, Manager, State};

// ============================================================
// AppState - Enhanced with caching and debouncing
// ============================================================

pub struct AppState {
    pub initialized: Mutex<bool>,
    pub workspace_root: Mutex<String>,
    pub file_tree_cache: RwLock<HashMap<String, (u64, Vec<FileTreeNode>)>>,
    pub open_files: RwLock<HashMap<String, (u64, String)>>,
    pub recent_logs: RwLock<VecDeque<LogEntry>>,
    pub pending_writes: Mutex<HashMap<String, String>>,
    pub last_write_time: Mutex<HashMap<String, u64>>,
    pub layout_config: RwLock<LayoutConfig>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct LayoutConfig {
    pub explorer_width: f64,
    pub chat_width: f64,
    pub console_height: f64,
    pub theme: String,
    pub window_x: i32,
    pub window_y: i32,
    pub window_width: i32,
    pub window_height: i32,
}

#[derive(Serialize, Clone)]
pub struct LogEntry {
    pub level: String,
    pub category: String,
    pub message: String,
    pub detail: String,
    pub timestamp: String,
}

impl AppState {
    pub fn new() -> Self {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".into());
        
        let layout_config = LayoutConfig {
            explorer_width: 240.0,
            chat_width: 360.0,
            console_height: 180.0,
            theme: "dark".into(),
            window_x: 0,
            window_y: 0,
            window_width: 1200,
            window_height: 800,
        };
        
        Self {
            initialized: Mutex::new(false),
            workspace_root: Mutex::new(cwd),
            file_tree_cache: RwLock::new(HashMap::new()),
            open_files: RwLock::new(HashMap::new()),
            recent_logs: RwLock::new(VecDeque::with_capacity(1000)),
            pending_writes: Mutex::new(HashMap::new()),
            last_write_time: Mutex::new(HashMap::new()),
            layout_config: RwLock::new(layout_config),
        }
    }
}

// ============================================================
// Response types
// ============================================================

#[derive(Serialize, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub role: String,
}

#[derive(Serialize, Clone)]
pub struct PlanSummary {
    pub id: String,
    pub name: String,
    pub status: String,
    pub steps_count: usize,
    pub current_step: usize,
    pub created_at: String,
}

#[derive(Serialize, Clone)]
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub tags: Vec<String>,
    pub created_at: String,
}

#[derive(Serialize, Clone)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub version: String,
    pub installed: bool,
}

#[derive(Serialize, Clone)]
pub struct FileTreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub children: Option<Vec<FileTreeNode>>,
}

#[derive(Deserialize)]
pub struct CreatePlanRequest {
    pub name: String,
}

#[derive(Deserialize)]
pub struct FileWriteRequest {
    pub path: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct FileCreateRequest {
    pub parent_dir: String,
    pub name: String,
    pub is_dir: bool,
}

#[derive(Deserialize)]
pub struct FileDeleteRequest {
    pub path: String,
    pub is_dir: bool,
}

#[derive(Deserialize)]
pub struct FileRenameRequest {
    pub old_path: String,
    pub new_name: String,
}

#[derive(Deserialize)]
pub struct ChatParams {
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
}

#[derive(Deserialize)]
pub struct LogFilterRequest {
    pub level: Option<String>,
    pub category: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct LayoutSaveRequest {
    pub explorer_width: f64,
    pub chat_width: f64,
    pub console_height: f64,
}

#[derive(Deserialize)]
pub struct WindowStateRequest {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

// ============================================================
// Constants
// ============================================================

const MAX_LOG_ENTRIES: usize = 1000;
const WRITE_DEBOUNCE_MS: u64 = 1000;
const CACHE_TTL_SECONDS: u64 = 300;

// ============================================================
// Helper functions
// ============================================================

fn now_iso() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

// ============================================================
// Unified log emitter with caching
// ============================================================

fn emit_log(app: &tauri::AppHandle, level: &str, category: &str, message: &str, detail: &str) {
    let entry = LogEntry {
        level: level.to_string(),
        category: category.to_string(),
        message: message.to_string(),
        detail: detail.to_string(),
        timestamp: now_iso(),
    };
    
    let state = app.state::<AppState>();
    let mut logs = state.recent_logs.write().unwrap();
    logs.push_back(entry.clone());
    if logs.len() > MAX_LOG_ENTRIES {
        logs.pop_front();
    }
    
    let _ = app.emit(
        "log-event",
        serde_json::json!({
            "level": level,
            "category": category,
            "message": message,
            "detail": detail,
            "timestamp": now_iso()
        }),
    );
}

fn emit_tool_log(app: &tauri::AppHandle, tool_name: &str, params: &str, result: &str, elapsed_ms: u64) {
    let detail = serde_json::json!({
        "tool": tool_name,
        "params": params,
        "result": result.trim(),
        "elapsed_ms": elapsed_ms
    }).to_string();
    emit_log(app, "info", "tool", &format!("Tool call: {}", tool_name), &detail);
}

fn emit_file_log(app: &tauri::AppHandle, action: &str, path: &str, success: bool, detail: &str) {
    let level = if success { "info" } else { "error" };
    emit_log(app, level, "file", &format!("[file] {} {}", action, path), detail);
}

// ============================================================
// Ignore patterns for file tree
// ============================================================

const IGNORE_PATTERNS: &[&str] = &["target", "node_modules", ".git", "dist", "build", ".idea", "*.log"];

fn is_ignored(name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }
    IGNORE_PATTERNS.contains(&name) || IGNORE_PATTERNS.iter().any(|p| {
        if let Some(suffix) = p.strip_prefix('*') {
            name.ends_with(suffix)
        } else {
            name == *p
        }
    })
}

// ============================================================
// File tree caching helpers
// ============================================================

fn get_cache_key(path: &str) -> String {
    path.to_string()
}

fn should_use_cache<'a>(cache: &'a HashMap<String, (u64, Vec<FileTreeNode>)>, path: &str) -> Option<&'a Vec<FileTreeNode>> {
    let key = get_cache_key(path);
    cache.get(&key).and_then(|(timestamp, nodes)| {
        if now_ms() - *timestamp < CACHE_TTL_SECONDS * 1000 {
            Some(nodes)
        } else {
            None
        }
    })
}

fn update_cache(cache: &mut HashMap<String, (u64, Vec<FileTreeNode>)>, path: &str, nodes: Vec<FileTreeNode>) {
    cache.insert(get_cache_key(path), (now_ms(), nodes));
}

// ============================================================
// Recursive directory tree builder returning absolute paths.
// ============================================================

fn build_tree(path: &std::path::Path) -> Vec<FileTreeNode> {
    let mut nodes = vec![];

    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Failed to read dir {}: {}", path.display(), e);
            return nodes;
        }
    };

    for entry in entries.flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        if is_ignored(&fname) {
            continue;
        }

        let fpath = entry.path();
        let abs_path = fpath.to_string_lossy().to_string();
        let is_dir = fpath.is_dir();

        nodes.push(FileTreeNode {
            name: fname,
            path: abs_path,
            is_dir,
            children: if is_dir {
                Some(build_tree(&fpath))
            } else {
                None
            },
        });
    }

    nodes.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    nodes
}

// ============================================================
// Commands
// ============================================================

#[tauri::command]
pub async fn chat_send(
    app: tauri::AppHandle,
    message: String,
    params: Option<ChatParams>,
) -> Result<ChatResponse, String> {
    let temp = params.as_ref().and_then(|p| p.temperature).unwrap_or(0.7);
    let max_tok = params.as_ref().and_then(|p| p.max_tokens).unwrap_or(4096);

    emit_log(&app, "info", "llm", "LLM request", &format!(
        "model=default temperature={} max_tokens={} input_len={}",
        temp, max_tok, message.len()
    ));

    let tool_name = "read_file";
    let tool_params = r#"{"path": "src/main.rs"}"#;
    emit_tool_log(&app, tool_name, tool_params, "// tool result placeholder", 12);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let tool_name2 = "search_codebase";
    emit_tool_log(&app, tool_name2, r#"{"query": "main function"}"#, "found 3 matches", 45);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let full_response = format!(
        "Echo (temp={}, max_tok={}): {}",
        temp, max_tok, message
    );

    for (i, _) in message.split_whitespace().enumerate() {
        let end = std::cmp::min((i + 1) * 6, full_response.len());
        let chunk = &full_response[..end];
        let _ = app.emit(
            "agent_stream",
            serde_json::json!({ "content": chunk.to_string() }),
        );
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }

    emit_log(&app, "info", "llm", &format!("LLM response complete — {} tokens", full_response.split_whitespace().count()), "");

    let _ = app.emit(
        "agent_done",
        serde_json::json!({ "message": full_response }),
    );

    Ok(ChatResponse {
        content: full_response,
        role: "assistant".into(),
    })
}

#[tauri::command]
pub async fn plan_list(
    _state: State<'_, AppState>,
    agent_state: State<'_, crate::commands::AgentState>,
) -> Result<Vec<PlanSummary>, String> {
    let plans = agent_state.repo.list_plans(20, 0).await.map_err(|e| format!("Failed to list plans: {}", e))?;

    Ok(plans
        .into_iter()
        .map(|p| PlanSummary {
            id: p.id,
            name: p.name,
            status: p.status,
            steps_count: p.total_steps,
            current_step: p.current_step_index,
            created_at: p.created_at,
        })
        .collect())
}

#[tauri::command]
pub async fn plan_create(
    _state: State<'_, AppState>,
    agent_state: State<'_, crate::commands::AgentState>,
    req: CreatePlanRequest,
) -> Result<PlanSummary, String> {
    let plan = rupoo::task::Plan::new(&req.name, vec![]);
    agent_state.repo.save_plan(&plan).await.map_err(|e| format!("Failed to save plan: {}", e))?;

    Ok(PlanSummary {
        id: plan.id,
        name: plan.name,
        status: "Pending".into(),
        steps_count: 0,
        current_step: 0,
        created_at: plan.created_at.format("%Y-%m-%d %H:%M").to_string(),
    })
}

#[tauri::command]
pub async fn plan_execute(app: tauri::AppHandle, plan_id: String) -> Result<String, String> {
    let _ = app.emit(
        "agent-event",
        serde_json::json!({
            "event": "plan_execution_started",
            "plan_id": plan_id
        }),
    );

    emit_log(&app, "info", "plan", &format!("Plan execution started: {}", plan_id), "");
    Ok(format!("Plan {} execution started", plan_id))
}

#[tauri::command]
pub async fn plan_delete(
    _state: State<'_, AppState>,
    agent_state: State<'_, crate::commands::AgentState>,
    plan_id: String,
) -> Result<bool, String> {
    agent_state.repo.delete_plan(&plan_id).await.map_err(|e| format!("Failed to delete plan: {}", e))?;
    Ok(true)
}

#[tauri::command]
pub async fn memory_list(_state: State<'_, AppState>) -> Result<Vec<MemoryEntry>, String> {
    Ok(vec![])
}

#[tauri::command]
pub async fn memory_search(
    _state: State<'_, AppState>,
    _query: String,
) -> Result<Vec<MemoryEntry>, String> {
    Ok(vec![])
}

#[tauri::command]
pub async fn skill_list(_state: State<'_, AppState>) -> Result<Vec<SkillEntry>, String> {
    Ok(vec![])
}

#[tauri::command]
pub async fn skill_install(
    _state: State<'_, AppState>,
    _skill_name: String,
) -> Result<bool, String> {
    Ok(true)
}

#[tauri::command]
pub async fn config_get(
    _state: State<'_, AppState>,
    agent_state: State<'_, crate::commands::AgentState>,
    key: String,
) -> Result<Option<String>, String> {
    agent_state.repo.get_setting(&key).await.map_err(|e| format!("Failed to get config: {}", e))
}

#[tauri::command]
pub async fn config_set(
    _state: State<'_, AppState>,
    agent_state: State<'_, crate::commands::AgentState>,
    key: String,
    value: String,
) -> Result<bool, String> {
    agent_state.repo.set_setting(&key, &value).await.map_err(|e| format!("Failed to set config: {}", e))?;
    Ok(true)
}

// ============================================================
// File system commands — all via absolute paths
// ============================================================

fn resolve_workspace_path(state: &AppState, relative: &str) -> String {
    let root = state.workspace_root.lock().unwrap();
    if relative.starts_with('/') {
        relative.to_string()
    } else {
        format!("{}/{}", root.trim_end_matches('/'), relative.trim_start_matches('/'))
    }
}

#[tauri::command]
pub async fn file_read_tree(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    dir: String,
) -> Result<Vec<FileTreeNode>, String> {
    let abs = resolve_workspace_path(&state, &dir);
    let path = std::path::Path::new(&abs);
    
    if !path.exists() {
        return Err(format!("Directory not found: {}", abs));
    }

    let cache = state.file_tree_cache.read().unwrap();
    if let Some(nodes) = should_use_cache(&cache, &abs) {
        emit_log(&app, "info", "cache", &format!("Using cached tree for: {}", abs), "");
        return Ok(nodes.clone());
    }
    drop(cache);

    let nodes = build_tree(path);
    
    let mut cache = state.file_tree_cache.write().unwrap();
    update_cache(&mut cache, &abs, nodes.clone());

    Ok(nodes)
}

#[tauri::command]
pub async fn file_read_content(state: State<'_, AppState>, file_path: String) -> Result<String, String> {
    let open_files = state.open_files.read().unwrap();
    
    if let Some((timestamp, content)) = open_files.get(&file_path) {
        let file_meta = match std::fs::metadata(&file_path) {
            Ok(m) => m,
            Err(e) => return Err(format!("Failed to read file metadata: {}", e)),
        };
        
        let file_modified = match file_meta.modified() {
            Ok(t) => t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
            Err(_) => 0,
        };
        
        if *timestamp >= file_modified {
            return Ok(content.clone());
        }
    }
    drop(open_files);

    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read {}: {}", file_path, e))?;

    let mut open_files = state.open_files.write().unwrap();
    open_files.insert(file_path, (now_ms(), content.clone()));

    Ok(content)
}

#[tauri::command]
pub async fn file_write(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    req: FileWriteRequest,
) -> Result<bool, String> {
    let now = now_ms();
    let mut last_write = state.last_write_time.lock().unwrap();
    
    if let Some(&last) = last_write.get(&req.path) {
        if now - last < WRITE_DEBOUNCE_MS {
            let mut pending = state.pending_writes.lock().unwrap();
            pending.insert(req.path.clone(), req.content.clone());
            emit_log(&app, "info", "file", "Write debounced", &format!("path={}", req.path));
            return Ok(true);
        }
    }
    
    last_write.insert(req.path.clone(), now);
    
    std::fs::write(&req.path, &req.content)
        .map_err(|e| format!("Failed to write {}: {}", req.path, e))?;

    let mut open_files = state.open_files.write().unwrap();
    open_files.insert(req.path.clone(), (now_ms(), req.content.clone()));

    let mut pending = state.pending_writes.lock().unwrap();
    pending.remove(&req.path);

    emit_file_log(&app, "write", &req.path, true, &format!("{} bytes", req.content.len()));
    Ok(true)
}

#[tauri::command]
pub async fn flush_pending_writes(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<usize, String> {
    let pending = state.pending_writes.lock().unwrap().clone();
    let mut count = 0;

    for (path, content) in pending {
        match std::fs::write(&path, &content) {
            Ok(_) => {
                let mut open_files = state.open_files.write().unwrap();
                open_files.insert(path.clone(), (now_ms(), content.clone()));
                
                let mut pending = state.pending_writes.lock().unwrap();
                pending.remove(&path);
                
                let mut last_write = state.last_write_time.lock().unwrap();
                last_write.insert(path.clone(), now_ms());
                
                emit_file_log(&app, "write", &path, true, &format!("{} bytes (flushed)", content.len()));
                count += 1;
            }
            Err(e) => {
                emit_log(&app, "error", "file", &format!("Failed to flush {}", path), &e.to_string());
            }
        }
    }

    Ok(count)
}

#[tauri::command]
pub async fn file_create(
    app: tauri::AppHandle,
    req: FileCreateRequest,
) -> Result<FileTreeNode, String> {
    let parent = std::path::Path::new(&req.parent_dir);
    if !parent.is_dir() {
        return Err(format!("Not a directory: {}", req.parent_dir));
    }

    let target = parent.join(&req.name);
    let target_str = target.to_string_lossy().to_string();

    if target.exists() {
        return Err(format!("Already exists: {}", target_str));
    }

    if req.is_dir {
        std::fs::create_dir_all(&target)
            .map_err(|e| format!("Failed to create dir: {}", e))?;
        emit_file_log(&app, "create_dir", &target_str, true, "");
    } else {
        std::fs::write(&target, "")
            .map_err(|e| format!("Failed to create file: {}", e))?;
        emit_file_log(&app, "create_file", &target_str, true, "");
    }

    Ok(FileTreeNode {
        name: req.name,
        path: target_str,
        is_dir: req.is_dir,
        children: if req.is_dir { Some(vec![]) } else { None },
    })
}

#[tauri::command]
pub async fn file_delete(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    req: FileDeleteRequest,
) -> Result<bool, String> {
    let path = std::path::Path::new(&req.path);
    if !path.exists() {
        return Err(format!("Not found: {}", req.path));
    }

    if req.is_dir {
        std::fs::remove_dir_all(path)
            .map_err(|e| format!("Failed to delete dir: {}", e))?;
    } else {
        std::fs::remove_file(path)
            .map_err(|e| format!("Failed to delete file: {}", e))?;
        
        let mut open_files = state.open_files.write().unwrap();
        open_files.remove(&req.path);
    }

    let mut cache = state.file_tree_cache.write().unwrap();
    cache.clear();

    emit_file_log(&app, "delete", &req.path, true, "");
    Ok(true)
}

#[tauri::command]
pub async fn file_rename(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    req: FileRenameRequest,
) -> Result<String, String> {
    let old = std::path::Path::new(&req.old_path);
    if !old.exists() {
        return Err(format!("Not found: {}", req.old_path));
    }

    let new_path = old.parent().unwrap_or(std::path::Path::new(".")).join(&req.new_name);
    let new_str = new_path.to_string_lossy().to_string();

    if new_path.exists() {
        return Err(format!("Already exists: {}", new_str));
    }

    std::fs::rename(old, &new_path)
        .map_err(|e| format!("Failed to rename: {}", e))?;

    let mut open_files = state.open_files.write().unwrap();
    if let Some((_, content)) = open_files.remove(&req.old_path) {
        open_files.insert(new_str.clone(), (now_ms(), content));
    }

    let mut cache = state.file_tree_cache.write().unwrap();
    cache.clear();

    emit_file_log(&app, "rename", &req.old_path, true, &format!("→ {}", new_str));
    Ok(new_str)
}

#[tauri::command]
pub async fn file_read_large(
    file_path: String,
    offset: usize,
    limit: usize,
) -> Result<String, String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(&file_path)
        .map_err(|e| format!("Failed to open {}: {}", file_path, e))?;

    let size = f.metadata().map(|m| m.len() as usize).unwrap_or(0);

    if size <= 1_048_576 && offset == 0 && limit == 0 {
        let mut buf = String::new();
        f.read_to_string(&mut buf).map_err(|e| format!("Read error: {}", e))?;
        return Ok(buf);
    }

    if offset > 0 {
        f.seek(SeekFrom::Start(offset as u64))
            .map_err(|e| format!("Seek error: {}", e))?;
    }

    let cap = if limit > 0 { limit } else { 1_048_576 };
    let mut buf = vec![0u8; cap];
    let n = f.read(&mut buf).map_err(|e| format!("Read error: {}", e))?;
    buf.truncate(n);
    String::from_utf8(buf).map_err(|e| format!("UTF-8 error: {}", e))
}

#[tauri::command]
pub async fn file_import_external(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    source_path: String,
    target_dir: String,
) -> Result<String, String> {
    let source = std::path::Path::new(&source_path);
    if !source.exists() {
        return Err(format!("Source file not found: {}", source_path));
    }

    let target = std::path::Path::new(&target_dir);
    if !target.is_dir() {
        return Err(format!("Target is not a directory: {}", target_dir));
    }

    let file_name = source.file_name().unwrap_or_default().to_string_lossy().to_string();
    let dest_path = target.join(&file_name);
    
    if dest_path.exists() {
        return Err(format!("File already exists: {}", dest_path.to_string_lossy()));
    }

    std::fs::copy(source, &dest_path)
        .map_err(|e| format!("Failed to copy file: {}", e))?;

    let mut cache = state.file_tree_cache.write().unwrap();
    cache.clear();

    let dest_str = dest_path.to_string_lossy().to_string();
    emit_file_log(&app, "import", &dest_str, true, &format!("from: {}", source_path));
    Ok(dest_str)
}

#[tauri::command]
pub async fn agent_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let initialized = state.initialized.lock().map(|g| *g).unwrap_or(false);

    Ok(serde_json::json!({
        "initialized": initialized,
        "version": "0.4.1",
        "status": if initialized { "ready" } else { "initializing" }
    }))
}

// ============================================================
// Log filtering commands
// ============================================================

#[tauri::command]
pub async fn log_get_recent(
    state: State<'_, AppState>,
    filter: Option<LogFilterRequest>,
) -> Result<Vec<LogEntry>, String> {
    let logs = state.recent_logs.read().unwrap();
    let limit = filter.as_ref().and_then(|f| f.limit).unwrap_or(100);
    
    let filtered: Vec<LogEntry> = logs
        .iter()
        .rev()
        .filter(|entry| {
            if let Some(f) = &filter {
                if let Some(level) = &f.level {
                    if entry.level != *level {
                        return false;
                    }
                }
                if let Some(category) = &f.category {
                    if entry.category != *category {
                        return false;
                    }
                }
            }
            true
        })
        .take(limit)
        .cloned()
        .collect();

    Ok(filtered.into_iter().rev().collect())
}

#[tauri::command]
pub async fn log_clear(state: State<'_, AppState>) -> Result<bool, String> {
    let mut logs = state.recent_logs.write().unwrap();
    logs.clear();
    Ok(true)
}

// ============================================================
// Layout configuration commands
// ============================================================

#[tauri::command]
pub async fn layout_get(state: State<'_, AppState>) -> Result<LayoutConfig, String> {
    let config = state.layout_config.read().unwrap();
    Ok(config.clone())
}

#[tauri::command]
pub async fn layout_save(
    agent_state: State<'_, crate::commands::AgentState>,
    req: LayoutSaveRequest,
) -> Result<bool, String> {
    let config = serde_json::json!({
        "explorer_width": req.explorer_width,
        "chat_width": req.chat_width,
        "console_height": req.console_height,
    });

    agent_state.repo
        .set_setting("layout:panel_sizes", &config.to_string())
        .await
        .map_err(|e| format!("Failed to save layout: {}", e))?;

    Ok(true)
}

#[tauri::command]
pub async fn theme_set(
    agent_state: State<'_, crate::commands::AgentState>,
    theme: String,
) -> Result<bool, String> {
    agent_state.repo
        .set_setting("ui:theme", &theme)
        .await
        .map_err(|e| format!("Failed to save theme: {}", e))?;

    Ok(true)
}

#[tauri::command]
pub async fn theme_get(agent_state: State<'_, crate::commands::AgentState>) -> Result<String, String> {
    agent_state.repo
        .get_setting("ui:theme")
        .await
        .map_err(|e| format!("Failed to get theme: {}", e))?
        .ok_or_else(|| "Theme not set".to_string())
}

#[tauri::command]
pub async fn window_state_save(
    agent_state: State<'_, crate::commands::AgentState>,
    req: WindowStateRequest,
) -> Result<bool, String> {
    let state = serde_json::json!({
        "x": req.x,
        "y": req.y,
        "width": req.width,
        "height": req.height,
    });

    agent_state.repo
        .set_setting("window:state", &state.to_string())
        .await
        .map_err(|e| format!("Failed to save window state: {}", e))?;

    Ok(true)
}

#[tauri::command]
pub async fn window_state_get(agent_state: State<'_, crate::commands::AgentState>) -> Result<serde_json::Value, String> {
    agent_state.repo
        .get_setting("window:state")
        .await
        .map_err(|e| format!("Failed to get window state: {}", e))?
        .map(|s| serde_json::from_str(&s).unwrap_or_default())
        .ok_or_else(|| "Window state not set".to_string())
}