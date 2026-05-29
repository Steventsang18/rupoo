//! Rig-compatible tool definitions that bridge our MCP tools into
//! rig-core's agent tool-calling system.
//!
//! Each tool implements `rig::tool::Tool` with type-safe Args/Output.
//! Note: rig-core's Tool trait uses `impl Future` return types (not
//! #[async_trait]), so all methods use `async move { }` blocks.

use std::path::PathBuf;

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
impl Default for EchoTool {
    fn default() -> Self {
        Self::new()
    }
}

impl EchoTool {
    pub fn new() -> Self { Self }
}

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

pub struct FileReadTool {
    jail_root: Option<PathBuf>,
}

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
            let safe_path = match resolve_path(&self.jail_root, &args.path) {
                Ok(p) => p,
                Err(e) => return async move { Ok(FileReadOutput { content: String::new(), success: false, error: Some(e) }) }.await,
            };
            match tokio::fs::read_to_string(&safe_path).await {
                Ok(content) => {
                    let compressed = crate::signal::compress_file_content(
                        &content, &args.path, None,
                    );
                    Ok(FileReadOutput {
                        content: compressed,
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

pub struct FileWriteTool {
    jail_root: Option<PathBuf>,
}

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
            let safe_path = match resolve_path(&self.jail_root, &args.path) {
                Ok(p) => p,
                Err(e) => return async move { Ok(FileWriteOutput { bytes_written: 0, success: false, error: Some(e) }) }.await,
            };
            match tokio::fs::write(&safe_path, &args.content).await {
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

pub struct ListDirTool {
    jail_root: Option<PathBuf>,
}

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
            let safe_path = match resolve_path(&self.jail_root, &args.path) {
                Ok(p) => p,
                Err(e) => return async move { Ok(ListDirOutput { entries: vec![], success: false, error: Some(e) }) }.await,
            };
            let mut entries = Vec::new();
            match tokio::fs::read_dir(&safe_path).await {
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
// Web search tool
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct WebSearchArgs {
    pub query: String,
}

#[derive(Serialize)]
pub struct WebSearchOutput {
    pub results: String,
    pub success: bool,
    pub error: Option<String>,
}

pub struct WebSearchTool;

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSearchTool {
    pub fn new() -> Self { Self }
}

#[allow(clippy::manual_async_fn)]
impl rig::tool::Tool for WebSearchTool {
    const NAME: &'static str = "web_search";
    type Error = ToolCallError;
    type Args = WebSearchArgs;
    type Output = WebSearchOutput;

    fn name(&self) -> String {
        "web_search".into()
    }

    fn definition(
        &self,
        _prompt: String,
    ) -> impl std::future::Future<Output = rig::completion::ToolDefinition>
           + rig::wasm_compat::WasmCompatSend
           + rig::wasm_compat::WasmCompatSync {
        async move {
            rig::completion::ToolDefinition {
                name: "web_search".into(),
                description: "Search the web using DuckDuckGo. Returns up to 10 search results with titles, snippets, and URLs.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query string"
                        }
                    },
                    "required": ["query"]
                }),
            }
        }
    }

    fn call(
        &self,
        args: WebSearchArgs,
    ) -> impl std::future::Future<Output = Result<WebSearchOutput, Self::Error>>
           + rig::wasm_compat::WasmCompatSend {
        async move {
            let safety = crate::safety::SafetyContext::default();
            match crate::tools::search::web_search(&args.query, &safety).await {
                Ok(results) => Ok(WebSearchOutput {
                    results,
                    success: true,
                    error: None,
                }),
                Err(e) => Ok(WebSearchOutput {
                    results: String::new(),
                    success: false,
                    error: Some(e.to_string()),
                }),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shared error type for all tools
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error, Serialize)]
#[error("Tool call error: {0}")]
pub struct ToolCallError(pub String);

impl From<std::io::Error> for ToolCallError {
    fn from(e: std::io::Error) -> Self {
        ToolCallError(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Path jail resolution
// ---------------------------------------------------------------------------

/// Resolve a path through the jail root if configured.
/// Returns `Err` if path_jail rejects the path (traversal attack detected).
/// When no jail_root is set, defaults to CWD as the sandbox root
/// (never allow unrestricted file access from LLM tool calls).
fn resolve_path(jail_root: &Option<PathBuf>, path: &str) -> Result<String, String> {
    let root = match jail_root {
        Some(ref root) => root.clone(),
        None => std::env::current_dir().map_err(|e| format!("Cannot determine CWD for sandbox: {e}"))?,
    };
    path_jail::join(&root, path)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| format!("Access denied to '{}': {e}", path))
}

// ---------------------------------------------------------------------------
// Tool constructors
// ---------------------------------------------------------------------------

impl FileReadTool {
    pub fn new() -> Self { Self { jail_root: None } }
    pub fn with_jail(root: PathBuf) -> Self { Self { jail_root: Some(root) } }
}
impl Default for FileReadTool { fn default() -> Self { Self::new() } }

impl FileWriteTool {
    pub fn new() -> Self { Self { jail_root: None } }
    pub fn with_jail(root: PathBuf) -> Self { Self { jail_root: Some(root) } }
}
impl Default for FileWriteTool { fn default() -> Self { Self::new() } }

impl ListDirTool {
    pub fn new() -> Self { Self { jail_root: None } }
    pub fn with_jail(root: PathBuf) -> Self { Self { jail_root: Some(root) } }
}
impl Default for ListDirTool { fn default() -> Self { Self::new() } }

// ---------------------------------------------------------------------------
// Helper: build a ToolSet with all available tools
// ---------------------------------------------------------------------------

/// Create a rig-compatible ToolSet with all our built-in tools.
/// Pass this to `AgentBuilder::tools()`.
/// If `jail_root` is provided, file tools will reject path traversal.
pub fn default_tool_set(jail_root: Option<PathBuf>) -> rig::tool::ToolSet {
    use rig::tool::ToolSetBuilder;

    let mut builder = ToolSetBuilder::default()
        .static_tool(EchoTool)
        .static_tool(WebSearchTool::new());
    if let Some(ref root) = jail_root {
        builder = builder
            .static_tool(FileReadTool::with_jail(root.clone()))
            .static_tool(FileWriteTool::with_jail(root.clone()))
            .static_tool(ListDirTool::with_jail(root.clone()));
    } else {
        builder = builder
            .static_tool(FileReadTool::new())
            .static_tool(FileWriteTool::new())
            .static_tool(ListDirTool::new());
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::tool::Tool;

    #[tokio::test]
    async fn test_echo_tool() {
        let tool = EchoTool::new();
        let output = tool.call(EchoArgs { message: "hello".into() }).await.unwrap();
        assert_eq!(output.result, "echo: hello");
    }

    #[tokio::test]
    async fn test_file_read_nonexistent() {
        let tool = FileReadTool::new();
        let output = tool
            .call(FileReadArgs {
                path: "nonexistent_test_file_xyz".into(),
            })
            .await
            .unwrap();
        assert!(!output.success);
        // Error could be "cannot read" (file not found) or "Access denied" (path_jail)
        assert!(output.error.is_some());
    }

    #[tokio::test]
    async fn test_tool_set_builds() {
        let _set = default_tool_set(None);
    }

    #[tokio::test]
    async fn test_list_dir() {
        let tool = ListDirTool::new();
        let output = tool.call(ListDirArgs { path: ".".into() }).await.unwrap();
        assert!(output.success);
        assert!(!output.entries.is_empty());
    }
}
