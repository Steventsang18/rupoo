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

// ---------------------------------------------------------------------------
// File edit (str_replace)
// ---------------------------------------------------------------------------

pub fn file_edit() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to the file to edit"
            },
            "old_string": {
                "type": "string",
                "description": "Exact text to replace. Must match the file content exactly (including whitespace). Must be unique unless replace_all is true."
            },
            "new_string": {
                "type": "string",
                "description": "Replacement text"
            },
            "replace_all": {
                "type": "boolean",
                "description": "Replace all occurrences of old_string (default false). Required when old_string appears more than once."
            }
        },
        "required": ["path", "old_string", "new_string"]
    })
}

// ---------------------------------------------------------------------------
// Code search (local grep / ripgrep-like)
// ---------------------------------------------------------------------------

pub fn code_search() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "Substring to search for in file contents (case-sensitive by default)"
            },
            "path": {
                "type": "string",
                "description": "Directory or file to search (defaults to current directory)"
            },
            "file_glob": {
                "type": "string",
                "description": "Only search files whose name matches this glob, e.g. '*.rs' (optional)"
            },
            "ignore_case": {
                "type": "boolean",
                "description": "Case-insensitive matching (default false)"
            },
            "max_results": {
                "type": "integer",
                "description": "Maximum number of matches to return (default 200)"
            }
        },
        "required": ["pattern"]
    })
}
