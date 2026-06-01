//! Single source of truth for tool parameter JSON Schemas.
//!
//! Every tool's parameter schema is defined here once and consumed by
//! `rig_tools.rs` (via `definition()`), `mcp.rs` (via `ToolKind::parameters_schema()`),
//! and `verify.rs` (via `definition()`).  Descriptions use the most detailed
//! wording available so LLMs get the richest context.

// ---------------------------------------------------------------------------
// Echo
// ---------------------------------------------------------------------------

pub fn echo() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "message": {
                "type": "string",
                "description": "The message to echo back"
            }
        },
        "required": ["message"]
    })
}

// ---------------------------------------------------------------------------
// File read
// ---------------------------------------------------------------------------

pub fn file_read() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Absolute or relative path to the file"
            }
        },
        "required": ["path"]
    })
}

// ---------------------------------------------------------------------------
// File write
// ---------------------------------------------------------------------------

pub fn file_write() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to the file to write"
            },
            "content": {
                "type": "string",
                "description": "Content to write to the file"
            }
        },
        "required": ["path", "content"]
    })
}

// ---------------------------------------------------------------------------
// List directory
// ---------------------------------------------------------------------------

pub fn list_directory() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to the directory to list"
            }
        },
        "required": ["path"]
    })
}

// ---------------------------------------------------------------------------
// Web search
// ---------------------------------------------------------------------------

pub fn web_search() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "The search query string"
            }
        },
        "required": ["query"]
    })
}

// ---------------------------------------------------------------------------
// Shell exec
// ---------------------------------------------------------------------------

pub fn shell_exec() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "description": "The shell command to execute (e.g. 'ls -la', 'cargo build', 'python script.py')"
            },
            "timeout": {
                "type": "integer",
                "description": "Optional timeout in seconds (default: 30)"
            }
        },
        "required": ["command"]
    })
}

// ---------------------------------------------------------------------------
// Run tests
// ---------------------------------------------------------------------------

pub fn run_tests() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Optional path to the project directory (defaults to current directory)"
            }
        },
        "required": []
    })
}

// ---------------------------------------------------------------------------
// Check output
// ---------------------------------------------------------------------------

pub fn check_output() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "description": "The command to run (e.g. 'cargo run', 'python main.py')"
            },
            "args": {
                "type": "string",
                "description": "Command-line arguments as a single string (optional)"
            },
            "cwd": {
                "type": "string",
                "description": "Working directory for the command (defaults to current directory)"
            },
            "timeout": {
                "type": "integer",
                "description": "Timeout in seconds (default 30, max 120)"
            }
        },
        "required": ["command"]
    })
}

// ---------------------------------------------------------------------------
// Diff check
// ---------------------------------------------------------------------------

pub fn diff_check() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "scope": {
                "type": "string",
                "description": "What to diff: 'staged' (git diff --cached), 'unstaged' (git diff), or 'all' (default)"
            },
            "path": {
                "type": "string",
                "description": "Path to the project directory (defaults to current directory)"
            }
        },
        "required": []
    })
}
