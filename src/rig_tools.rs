//! Rig-compatible tool definitions that bridge our MCP tools into
//! rig-core's agent tool-calling system.
//!
//! Each tool implements `rig::tool::Tool` with type-safe Args/Output.
//! Note: rig-core's Tool trait uses `impl Future` return types (not
//! #[async_trait]), so all methods use `async move { }` blocks.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Echo tool
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct EchoArgs {
    pub message: String,
}

#[derive(Serialize)]
pub struct EchoOutput {
    pub result: String,
}

pub struct EchoTool;

#[allow(clippy::manual_async_fn)]
impl rig::tool::Tool for EchoTool {
    const NAME: &'static str = "echo";
    type Error = ToolCallError;
    type Args = EchoArgs;
    type Output = EchoOutput;

    fn name(&self) -> String {
        "echo".into()
    }

    fn definition(
        &self,
        _prompt: String,
    ) -> impl std::future::Future<Output = rig::completion::ToolDefinition>
           + rig::wasm_compat::WasmCompatSend
           + rig::wasm_compat::WasmCompatSync {
        async move {
            rig::completion::ToolDefinition {
                name: "echo".into(),
                description: "Echo back a message. Useful for testing.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                            "description": "The message to echo back"
                        }
                    },
                    "required": ["message"]
                }),
            }
        }
    }

    fn call(
        &self,
        args: EchoArgs,
    ) -> impl std::future::Future<Output = Result<EchoOutput, Self::Error>>
           + rig::wasm_compat::WasmCompatSend {
        async move {
            Ok(EchoOutput {
                result: format!("echo: {}", args.message),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// File read tool
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct FileReadArgs {
    pub path: String,
}

#[derive(Serialize)]
pub struct FileReadOutput {
    pub content: String,
    pub success: bool,
    pub error: Option<String>,
}

pub struct FileReadTool;

#[allow(clippy::manual_async_fn)]
impl rig::tool::Tool for FileReadTool {
    const NAME: &'static str = "file_read";
    type Error = ToolCallError;
    type Args = FileReadArgs;
    type Output = FileReadOutput;

    fn name(&self) -> String {
        "file_read".into()
    }

    fn definition(
        &self,
        _prompt: String,
    ) -> impl std::future::Future<Output = rig::completion::ToolDefinition>
           + rig::wasm_compat::WasmCompatSend
           + rig::wasm_compat::WasmCompatSync {
        async move {
            rig::completion::ToolDefinition {
                name: "file_read".into(),
                description: "Read the contents of a file at the given path.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute or relative path to the file"
                        }
                    },
                    "required": ["path"]
                }),
            }
        }
    }

    fn call(
        &self,
        args: FileReadArgs,
    ) -> impl std::future::Future<Output = Result<FileReadOutput, Self::Error>>
           + rig::wasm_compat::WasmCompatSend {
        async move {
            match tokio::fs::read_to_string(&args.path).await {
                Ok(content) => {
                    let truncated = if content.len() > 4096 {
                        // Use floor_char_boundary to avoid splitting a multi-byte UTF-8 char
                        let end = content.floor_char_boundary(4096);
                        format!("{}...(truncated {end} bytes)", &content[..end])
                    } else {
                        content
                    };
                    Ok(FileReadOutput {
                        content: truncated,
                        success: true,
                        error: None,
                    })
                }
                Err(e) => Ok(FileReadOutput {
                    content: String::new(),
                    success: false,
                    error: Some(format!("cannot read file '{}': {e}", args.path)),
                }),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// File write tool
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct FileWriteArgs {
    pub path: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct FileWriteOutput {
    pub bytes_written: usize,
    pub success: bool,
    pub error: Option<String>,
}

pub struct FileWriteTool;

#[allow(clippy::manual_async_fn)]
impl rig::tool::Tool for FileWriteTool {
    const NAME: &'static str = "file_write";
    type Error = ToolCallError;
    type Args = FileWriteArgs;
    type Output = FileWriteOutput;

    fn name(&self) -> String {
        "file_write".into()
    }

    fn definition(
        &self,
        _prompt: String,
    ) -> impl std::future::Future<Output = rig::completion::ToolDefinition>
           + rig::wasm_compat::WasmCompatSend
           + rig::wasm_compat::WasmCompatSync {
        async move {
            rig::completion::ToolDefinition {
                name: "file_write".into(),
                description: "Write content to a file. Overwrites existing content.".into(),
                parameters: serde_json::json!({
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
                }),
            }
        }
    }

    fn call(
        &self,
        args: FileWriteArgs,
    ) -> impl std::future::Future<Output = Result<FileWriteOutput, Self::Error>>
           + rig::wasm_compat::WasmCompatSend {
        async move {
            match tokio::fs::write(&args.path, &args.content).await {
                Ok(()) => Ok(FileWriteOutput {
                    bytes_written: args.content.len(),
                    success: true,
                    error: None,
                }),
                Err(e) => Ok(FileWriteOutput {
                    bytes_written: 0,
                    success: false,
                    error: Some(format!("cannot write '{}': {e}", args.path)),
                }),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// List directory tool
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListDirArgs {
    pub path: String,
}

#[derive(Serialize)]
pub struct ListDirOutput {
    pub entries: Vec<DirEntry>,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct DirEntry {
    pub name: String,
    pub kind: String,
}

pub struct ListDirTool;

#[allow(clippy::manual_async_fn)]
impl rig::tool::Tool for ListDirTool {
    const NAME: &'static str = "list_directory";
    type Error = ToolCallError;
    type Args = ListDirArgs;
    type Output = ListDirOutput;

    fn name(&self) -> String {
        "list_directory".into()
    }

    fn definition(
        &self,
        _prompt: String,
    ) -> impl std::future::Future<Output = rig::completion::ToolDefinition>
           + rig::wasm_compat::WasmCompatSend
           + rig::wasm_compat::WasmCompatSync {
        async move {
            rig::completion::ToolDefinition {
                name: "list_directory".into(),
                description: "List entries in a directory.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the directory to list"
                        }
                    },
                    "required": ["path"]
                }),
            }
        }
    }

    fn call(
        &self,
        args: ListDirArgs,
    ) -> impl std::future::Future<Output = Result<ListDirOutput, Self::Error>>
           + rig::wasm_compat::WasmCompatSend {
        async move {
            let mut entries = Vec::new();
            match tokio::fs::read_dir(&args.path).await {
                Ok(mut rd) => {
                    while let Ok(Some(entry)) = rd.next_entry().await {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let kind = entry
                            .file_type()
                            .await
                            .map(|t| if t.is_dir() { "dir" } else { "file" })
                            .unwrap_or("unknown")
                            .to_string();
                        entries.push(DirEntry { name, kind });
                    }
                    Ok(ListDirOutput {
                        entries,
                        success: true,
                        error: None,
                    })
                }
                Err(e) => Ok(ListDirOutput {
                    entries: vec![],
                    success: false,
                    error: Some(format!("cannot list '{}': {e}", args.path)),
                }),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shared error type for all tools
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
#[error("Tool call error: {0}")]
pub struct ToolCallError(pub String);

impl From<std::io::Error> for ToolCallError {
    fn from(e: std::io::Error) -> Self {
        ToolCallError(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Helper: build a ToolSet with all available tools
// ---------------------------------------------------------------------------

/// Create a rig-compatible ToolSet with all our built-in tools.
/// Pass this to `AgentBuilder::tools()`.
pub fn default_tool_set() -> rig::tool::ToolSet {
    use rig::tool::ToolSetBuilder;

    ToolSetBuilder::default()
        .static_tool(EchoTool)
        .static_tool(FileReadTool)
        .static_tool(FileWriteTool)
        .static_tool(ListDirTool)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::tool::Tool;

    #[tokio::test]
    async fn test_echo_tool() {
        let tool = EchoTool;
        let output = tool.call(EchoArgs { message: "hello".into() }).await.unwrap();
        assert_eq!(output.result, "echo: hello");
    }

    #[tokio::test]
    async fn test_file_read_nonexistent() {
        let tool = FileReadTool;
        let output = tool
            .call(FileReadArgs {
                path: "/tmp/nonexistent_test_file_xyz".into(),
            })
            .await
            .unwrap();
        assert!(!output.success);
        assert!(output.error.as_deref().unwrap().contains("cannot read"));
    }

    #[tokio::test]
    async fn test_tool_set_builds() {
        let _set = default_tool_set();
    }

    #[tokio::test]
    async fn test_list_dir() {
        let tool = ListDirTool;
        let output = tool.call(ListDirArgs { path: ".".into() }).await.unwrap();
        assert!(output.success);
        assert!(!output.entries.is_empty());
    }
}
