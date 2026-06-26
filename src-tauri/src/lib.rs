mod commands;
mod ipc;

use std::sync::Arc;
use tauri::{Emitter, Manager};

pub fn run() {
    let db_path = dirs::data_dir()
        .map(|p| p.join("rupoo").join("agent.db"))
        .expect("Failed to get data directory");

    std::fs::create_dir_all(db_path.parent().unwrap()).ok();

    let db_path_str = db_path.to_string_lossy();
    let agent_state = match rupoo::db::TaskRepo::new(&db_path_str) {
        Ok(repo) => Some(commands::AgentState::new(Arc::new(repo))),
        Err(e) => {
            eprintln!("Failed to initialize agent engine: {}", e);
            None
        }
    };

    let mut app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(ipc::AppState::new());

    if let Some(state) = &agent_state {
        app = app.manage(state.clone());
    }

    let app = app.setup(move |app| {
        let handle = app.handle().clone();

        if agent_state.is_some() {
            commands::emit_agent_log(&handle, "info", "Agent engine initialized successfully");
            let _ = handle.emit(
                "agent-event",
                serde_json::json!({
                    "event": "agent_initialized",
                    "status": "ready"
                }),
            );

            if let Ok(mut initialized) = app.state::<ipc::AppState>().initialized.lock() {
                *initialized = true;
            }
        } else {
            commands::emit_agent_log(
                &handle,
                "error",
                "Agent init failed: database connection error",
            );
            let _ = handle.emit(
                "agent-event",
                serde_json::json!({
                    "event": "agent_init_failed",
                    "error": "Failed to initialize database"
                }),
            );
        }

        Ok(())
    });

    app.invoke_handler(tauri::generate_handler![
        // Original IPC commands
        ipc::chat_send,
        ipc::plan_list,
        ipc::plan_create,
        ipc::plan_execute,
        ipc::plan_delete,
        ipc::memory_list,
        ipc::memory_search,
        ipc::skill_list,
        ipc::skill_install,
        ipc::config_get,
        ipc::config_set,
        // File system commands with caching
        ipc::file_read_tree,
        ipc::file_read_content,
        ipc::file_read_large,
        ipc::file_write,
        ipc::flush_pending_writes,
        ipc::file_create,
        ipc::file_delete,
        ipc::file_rename,
        ipc::file_import_external,
        ipc::agent_status,
        // Log commands
        ipc::log_get_recent,
        ipc::log_clear,
        // Layout & theme commands
        ipc::layout_get,
        ipc::layout_save,
        ipc::theme_set,
        ipc::theme_get,
        ipc::window_state_save,
        ipc::window_state_get,
        // New agent chat commands
        commands::run_agent_chat,
        commands::save_session,
        commands::load_session,
        commands::delete_session,
        commands::rename_session,
        commands::list_sessions,
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
