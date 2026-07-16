//! Rig-compatible tool definitions that bridge our MCP tools into
//! rig-core's agent tool-calling system.
//!
//! Each tool implements `rig::tool::Tool` with type-safe Args/Output.
//! Note: rig-core's Tool trait uses `impl Future` return types (not
//! #[async_trait]), so all methods use `async move { }` blocks.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Canonical tool registry — single source of truth
// ---------------------------------------------------------------------------

/// The single source of truth for Rupoo's built-in tool set.
///
/// Every place that needs the list of tools — the rig `ToolSet` builder
/// (`default_tool_set`), the per-provider agent builders (`providers.rs`),
/// and the MCP dispatcher (`mcp.rs`) — expands this macro with its own
/// `$register!($name, $tool)` callback. This guarantees all surfaces expose
/// exactly the same tools and removes the previously triplicated list.
///
/// `$register` receives the canonical tool *name* (used by the MCP dispatcher)
/// and an owned tool instance. File tools are jail-aware via `$jail`
/// (`Option<PathBuf>`).
#[macro_export]
macro_rules! rupoo_tools {
    ($register:ident, $jail:expr) => {{
        let __jail: Option<std::path::PathBuf> = $jail;

        $register!("echo", $crate::rig_tools::EchoTool::new());
        $register!("web_search", $crate::rig_tools::WebSearchTool::new());
        $register!("shell_exec", $crate::rig_tools::ShellExecTool::new());
        $register!("run_tests", $crate::tools::verify::RunTestsTool);
        $register!("check_output", $crate::tools::verify::CheckOutputTool);
        $register!("diff_check", $crate::tools::verify::DiffCheckTool);

        match __jail {
            Some(ref __root) => {
                $register!(
                    "file_read",
                    $crate::rig_tools::FileReadTool::with_jail(__root.clone())
                );
                $register!(
                    "file_write",
                    $crate::rig_tools::FileWriteTool::with_jail(__root.clone())
                );
                $register!(
                    "list_directory",
                    $crate::rig_tools::ListDirTool::with_jail(__root.clone())
                );
            }
            None => {
                $register!("file_read", $crate::rig_tools::FileReadTool::new());
                $register!("file_write", $crate::rig_tools::FileWriteTool::new());
                $register!("list_directory", $crate::rig_tools::ListDirTool::new());
            }
        }
    }};
}

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
    pub fn new() -> Self {
        Self
    }
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
                parameters: crate::tools::schema::echo(),
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
                parameters: crate::tools::schema::file_read(),
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
                Err(e) => {
                    return async move {
                        Ok(FileReadOutput {
                            content: String::new(),
                            success: false,
                            error: Some(e),
                        })
                    }
                    .await
                }
            };
            match tokio::fs::read_to_string(&safe_path).await {
                Ok(content) => {
                    let compressed =
                        crate::signal::compress_file_content(&content, &args.path, None);
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
                parameters: crate::tools::schema::file_write(),
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
                Err(e) => {
                    return async move {
                        Ok(FileWriteOutput {
                            bytes_written: 0,
                            success: false,
                            error: Some(e),
                        })
                    }
                    .await
                }
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
                parameters: crate::tools::schema::list_directory(),
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
                Err(e) => {
                    return async move {
                        Ok(ListDirOutput {
                            entries: vec![],
                            success: false,
                            error: Some(e),
                        })
                    }
                    .await
                }
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
    pub fn new() -> Self {
        Self
    }
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
                parameters: crate::tools::schema::web_search(),
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
// Shell execution tool
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ShellExecArgs {
    pub command: String,
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Serialize)]
pub struct ShellExecOutput {
    pub stdout: String,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub error: Option<String>,
}

pub struct ShellExecTool;

impl Default for ShellExecTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellExecTool {
    pub fn new() -> Self {
        Self
    }
}

#[allow(clippy::manual_async_fn)]
impl rig::tool::Tool for ShellExecTool {
    const NAME: &'static str = "shell_exec";
    type Error = ToolCallError;
    type Args = ShellExecArgs;
    type Output = ShellExecOutput;

    fn name(&self) -> String {
        "shell_exec".into()
    }

    fn definition(
        &self,
        _prompt: String,
    ) -> impl std::future::Future<Output = rig::completion::ToolDefinition>
           + rig::wasm_compat::WasmCompatSend
           + rig::wasm_compat::WasmCompatSync {
        async move {
            rig::completion::ToolDefinition {
                name: "shell_exec".into(),
                description: "Execute a shell command and return its output. Commands run in the current working directory with safety validation (sudo/rm/etc. are blocked). Use for: running code, installing packages, git operations, file manipulation, building projects, etc.".into(),
                parameters: crate::tools::schema::shell_exec(),
            }
        }
    }

    fn call(
        &self,
        args: ShellExecArgs,
    ) -> impl std::future::Future<Output = Result<ShellExecOutput, Self::Error>>
           + rig::wasm_compat::WasmCompatSend {
        async move {
            let safety = crate::safety::SafetyContext::default();

            // Pass the full command; validate_command resolves PATH / wrappers
            if let Err(e) = safety.validate_command(&args.command) {
                return Ok(ShellExecOutput {
                    stdout: String::new(),
                    exit_code: None,
                    success: false,
                    error: Some(format!("Command blocked: {e}")),
                });
            }

            // Execute via shell for full pipeline/glob support
            let timeout = args.timeout.unwrap_or(30);
            let result = tokio::time::timeout(std::time::Duration::from_secs(timeout), async {
                let mut cmd = tokio::process::Command::new("sh");
                cmd.args(["-c", &args.command])
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .kill_on_drop(true);

                // Strip sensitive env vars
                crate::safety::SafetyContext::forward_safe_env_async(&mut cmd);

                let child = cmd.spawn();
                match child {
                    Ok(c) => c.wait_with_output().await,
                    Err(e) => Err(std::io::Error::other(e)),
                }
            })
            .await;

            match result {
                Ok(Ok(output)) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    let combined = if stderr.is_empty() {
                        stdout
                    } else {
                        format!("{stdout}\n{stderr}")
                    };

                    // Truncate if too long — char-boundary safe for UTF-8
                    let truncated = if combined.len() > 10_000 {
                        let mut byte_end = 10_000.min(combined.len());
                        while byte_end > 0 && !combined.is_char_boundary(byte_end) {
                            byte_end -= 1;
                        }
                        format!(
                            "{}...[truncated, {} bytes total]",
                            &combined[..byte_end],
                            combined.len()
                        )
                    } else {
                        combined
                    };

                    let exit_code = output.status.code();
                    let success = output.status.success();

                    Ok(ShellExecOutput {
                        stdout: truncated,
                        exit_code,
                        success,
                        error: if !success {
                            Some(format!("Exit code: {}", exit_code.unwrap_or(-1)))
                        } else {
                            None
                        },
                    })
                }
                Ok(Err(e)) => Ok(ShellExecOutput {
                    stdout: String::new(),
                    exit_code: None,
                    success: false,
                    error: Some(format!("Execution failed: {e}")),
                }),
                Err(_) => Ok(ShellExecOutput {
                    stdout: String::new(),
                    exit_code: None,
                    success: false,
                    error: Some(format!("Command timed out after {}s", timeout)),
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
        None => {
            std::env::current_dir().map_err(|e| format!("Cannot determine CWD for sandbox: {e}"))?
        }
    };
    path_jail::join(&root, path)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| format!("Access denied to '{}': {e}", path))
}

// ---------------------------------------------------------------------------
// Tool constructors
// ---------------------------------------------------------------------------

impl FileReadTool {
    pub fn new() -> Self {
        Self { jail_root: None }
    }
    pub fn with_jail(root: PathBuf) -> Self {
        Self {
            jail_root: Some(root),
        }
    }
}
impl Default for FileReadTool {
    fn default() -> Self {
        Self::new()
    }
}

impl FileWriteTool {
    pub fn new() -> Self {
        Self { jail_root: None }
    }
    pub fn with_jail(root: PathBuf) -> Self {
        Self {
            jail_root: Some(root),
        }
    }
}
impl Default for FileWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ListDirTool {
    pub fn new() -> Self {
        Self { jail_root: None }
    }
    pub fn with_jail(root: PathBuf) -> Self {
        Self {
            jail_root: Some(root),
        }
    }
}
impl Default for ListDirTool {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helper: build a ToolSet with all available tools
// ---------------------------------------------------------------------------

/// Create a rig-compatible ToolSet with all our built-in tools.
/// Pass this to `AgentBuilder::tools()`.
/// If `jail_root` is provided, file tools will reject path traversal.
pub fn default_tool_set(jail_root: Option<PathBuf>) -> rig::tool::ToolSet {
    use rig::tool::ToolSetBuilder;

    let mut builder = ToolSetBuilder::default();
    macro_rules! reg {
        ($name:expr, $tool:expr) => {
            builder = builder.static_tool($tool);
        };
    }
    rupoo_tools!(reg, jail_root);
    builder.build()
}

/// Build the canonical tool set as boxed `ToolDyn` instances.
///
/// Used by the per-provider agent builders via `AgentBuilder::tools(...)`, so
/// every LLM provider exposes exactly the same tools as the MCP dispatcher and
/// `default_tool_set`. Shares the single `rupoo_tools!` source of truth.
pub fn build_boxed_tools(jail_root: Option<PathBuf>) -> Vec<Box<dyn rig::tool::ToolDyn>> {
    let mut tools: Vec<Box<dyn rig::tool::ToolDyn>> = Vec::new();
    macro_rules! reg {
        ($name:expr, $tool:expr) => {
            tools.push(Box::new($tool));
        };
    }
    rupoo_tools!(reg, jail_root);
    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::tool::Tool;

    #[tokio::test]
    async fn test_echo_tool() {
        let tool = EchoTool::new();
        let output = tool
            .call(EchoArgs {
                message: "hello".into(),
            })
            .await
            .unwrap();
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

    #[tokio::test]
    async fn test_shell_exec_echo() {
        let tool = ShellExecTool::new();
        let output = tool
            .call(ShellExecArgs {
                command: "echo hello rupoo".into(),
                timeout: None,
            })
            .await
            .unwrap();
        assert!(output.success);
        assert!(output.stdout.contains("hello rupoo"));
    }

    #[tokio::test]
    async fn test_shell_exec_blocked() {
        let tool = ShellExecTool::new();
        let output = tool
            .call(ShellExecArgs {
                command: "sudo echo test".into(),
                timeout: None,
            })
            .await
            .unwrap();
        assert!(!output.success);
        assert!(output.error.unwrap().contains("blocked"));
    }
}
