//! Rig-compatible tool definitions that bridge our MCP tools into
//! rig-core's agent tool-calling system.
//!
//! Each tool implements `rig::tool::Tool` with type-safe Args/Output.
//! Note: rig-core's Tool trait uses `impl Future` return types (not
//! #[async_trait]), so all methods use `async move { }` blocks.

use std::path::{Path, PathBuf};

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
                    "file_edit",
                    $crate::rig_tools::FileEditTool::with_jail(__root.clone())
                );
                $register!(
                    "list_directory",
                    $crate::rig_tools::ListDirTool::with_jail(__root.clone())
                );
                $register!(
                    "code_search",
                    $crate::rig_tools::CodeSearchTool::with_jail(__root.clone())
                );
            }
            None => {
                $register!("file_read", $crate::rig_tools::FileReadTool::new());
                $register!("file_write", $crate::rig_tools::FileWriteTool::new());
                $register!("file_edit", $crate::rig_tools::FileEditTool::new());
                $register!("list_directory", $crate::rig_tools::ListDirTool::new());
                $register!("code_search", $crate::rig_tools::CodeSearchTool::new());
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
// File edit tool (str_replace)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct FileEditArgs {
    pub path: String,
    pub old_string: String,
    pub new_string: String,
    #[serde(default)]
    pub replace_all: Option<bool>,
}

#[derive(Serialize)]
pub struct FileEditOutput {
    pub success: bool,
    pub replacements: usize,
    pub diff: String,
    pub error: Option<String>,
}

pub struct FileEditTool {
    jail_root: Option<PathBuf>,
}

#[allow(clippy::manual_async_fn)]
impl rig::tool::Tool for FileEditTool {
    const NAME: &'static str = "file_edit";
    type Error = ToolCallError;
    type Args = FileEditArgs;
    type Output = FileEditOutput;

    fn name(&self) -> String {
        "file_edit".into()
    }

    fn definition(
        &self,
        _prompt: String,
    ) -> impl std::future::Future<Output = rig::completion::ToolDefinition>
           + rig::wasm_compat::WasmCompatSend
           + rig::wasm_compat::WasmCompatSync {
        async move {
            rig::completion::ToolDefinition {
                name: "file_edit".into(),
                description: "Make a precise local edit to an existing file by replacing an exact string (str_replace). Prefer this over rewriting whole files. Returns a diff preview of the change.".into(),
                parameters: crate::tools::schema::file_edit(),
            }
        }
    }

    fn call(
        &self,
        args: FileEditArgs,
    ) -> impl std::future::Future<Output = Result<FileEditOutput, Self::Error>>
           + rig::wasm_compat::WasmCompatSend {
        async move {
            let safe_path = match resolve_path(&self.jail_root, &args.path) {
                Ok(p) => p,
                Err(e) => {
                    return async move {
                        Ok(FileEditOutput {
                            success: false,
                            replacements: 0,
                            diff: String::new(),
                            error: Some(e),
                        })
                    }
                    .await
                }
            };

            let current = match tokio::fs::read_to_string(&safe_path).await {
                Ok(c) => c,
                Err(e) => {
                    return Ok(FileEditOutput {
                        success: false,
                        replacements: 0,
                        diff: String::new(),
                        error: Some(format!("cannot read '{}' for editing: {e}", args.path)),
                    });
                }
            };

            let count = current.matches(&args.old_string).count();

            if count == 0 {
                return Ok(FileEditOutput {
                    success: false,
                    replacements: 0,
                    diff: String::new(),
                    error: Some(
                        "old_string not found in file. Make sure it matches exactly (including whitespace and line endings).".into(),
                    ),
                });
            }

            let replace_all = args.replace_all.unwrap_or(false);
            if count > 1 && !replace_all {
                return Ok(FileEditOutput {
                    success: false,
                    replacements: 0,
                    diff: String::new(),
                    error: Some(format!(
                        "old_string is not unique: found {count} occurrences. Set replace_all=true to replace all, or make old_string more specific."
                    )),
                });
            }

            let new_content = if replace_all {
                current.replace(&args.old_string, &args.new_string)
            } else {
                current.replacen(&args.old_string, &args.new_string, 1)
            };

            match tokio::fs::write(&safe_path, &new_content).await {
                Ok(()) => {
                    let diff = unified_diff(&current, &new_content);
                    // Truncate at char boundary to avoid UTF-8 panic
                    let diff = if diff.len() > 6000 {
                        let end = diff.floor_char_boundary(6000);
                        format!("{}...[truncated]", &diff[..end])
                    } else {
                        diff
                    };
                    Ok(FileEditOutput {
                        success: true,
                        replacements: count,
                        diff,
                        error: None,
                    })
                }
                Err(e) => Ok(FileEditOutput {
                    success: false,
                    replacements: 0,
                    diff: String::new(),
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
// Local code search tool (ripgrep-like, dependency-free)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CodeSearchArgs {
    pub pattern: String,
    pub path: Option<String>,
    pub file_glob: Option<String>,
    #[serde(default)]
    pub ignore_case: Option<bool>,
    pub max_results: Option<usize>,
}

#[derive(Serialize)]
pub struct SearchMatch {
    pub file: String,
    pub line: usize,
    pub text: String,
}

#[derive(Serialize)]
pub struct CodeSearchOutput {
    pub matches: Vec<SearchMatch>,
    pub match_count: usize,
    pub success: bool,
    pub error: Option<String>,
}

pub struct CodeSearchTool {
    jail_root: Option<PathBuf>,
}

#[allow(clippy::manual_async_fn)]
impl rig::tool::Tool for CodeSearchTool {
    const NAME: &'static str = "code_search";
    type Error = ToolCallError;
    type Args = CodeSearchArgs;
    type Output = CodeSearchOutput;

    fn name(&self) -> String {
        "code_search".into()
    }

    fn definition(
        &self,
        _prompt: String,
    ) -> impl std::future::Future<Output = rig::completion::ToolDefinition>
           + rig::wasm_compat::WasmCompatSend
           + rig::wasm_compat::WasmCompatSync {
        async move {
            rig::completion::ToolDefinition {
                name: "code_search".into(),
                description: "Search local files for a substring (like grep/ripgrep). Use to find definitions, references, or usages across the codebase without running a shell command. Skips .git/node_modules/target and binary files.".into(),
                parameters: crate::tools::schema::code_search(),
            }
        }
    }

    fn call(
        &self,
        args: CodeSearchArgs,
    ) -> impl std::future::Future<Output = Result<CodeSearchOutput, Self::Error>>
           + rig::wasm_compat::WasmCompatSend {
        async move {
            let root_str = match resolve_path(&self.jail_root, args.path.as_deref().unwrap_or("."))
            {
                Ok(p) => p,
                Err(e) => {
                    return async move {
                        Ok(CodeSearchOutput {
                            matches: vec![],
                            match_count: 0,
                            success: false,
                            error: Some(e),
                        })
                    }
                    .await
                }
            };
            let root = std::path::PathBuf::from(&root_str);
            let ignore_case = args.ignore_case.unwrap_or(false);
            let max_results = args.max_results.unwrap_or(200).max(1);
            let glob = args.file_glob.as_deref();

            let mut matches = Vec::new();
            let err = search_in_path(
                &root,
                &root,
                &args.pattern,
                ignore_case,
                glob,
                max_results,
                &mut matches,
            )
            .err()
            .map(|e| e.to_string());

            let success = err.is_none();
            let match_count = matches.len();
            Ok(CodeSearchOutput {
                matches,
                match_count,
                success,
                error: err,
            })
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
// Helpers: local code search (dependency-free, grep-like)
// ---------------------------------------------------------------------------

/// Directories skipped during local code search to avoid noise / bloat.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    ".venv",
    "venv",
    "vendor",
    ".idea",
    ".cache",
    "build",
    "out",
];

/// Recursively search `path` for `pattern`, collecting matches into `matches`.
fn search_in_path(
    root: &Path,
    path: &Path,
    pattern: &str,
    ignore_case: bool,
    glob: Option<&str>,
    max_results: usize,
    matches: &mut Vec<SearchMatch>,
) -> std::io::Result<()> {
    if matches.len() >= max_results {
        return Ok(());
    }
    let meta = std::fs::metadata(path)?;
    if meta.is_file() {
        search_file(path, root, pattern, ignore_case, max_results, matches)?;
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(path)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|a| a.file_name());
    for entry in entries {
        if matches.len() >= max_results {
            break;
        }
        let ft = entry.file_type()?;
        let name = entry.file_name().to_string_lossy().to_string();
        if ft.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            search_in_path(
                root,
                &entry.path(),
                pattern,
                ignore_case,
                glob,
                max_results,
                matches,
            )?;
        } else if ft.is_file() {
            if let Some(g) = glob {
                if !glob_match(&name, g) {
                    continue;
                }
            }
            search_file(
                &entry.path(),
                root,
                pattern,
                ignore_case,
                max_results,
                matches,
            )?;
        }
    }
    Ok(())
}

/// Search a single file for `pattern`, appending matches to `matches`.
fn search_file(
    path: &Path,
    root: &Path,
    pattern: &str,
    ignore_case: bool,
    max_results: usize,
    matches: &mut Vec<SearchMatch>,
) -> std::io::Result<()> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > 5_000_000 {
        return Ok(());
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // skip binary / non-utf8
    };
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();
    let needle = if ignore_case {
        pattern.to_lowercase()
    } else {
        pattern.to_string()
    };
    for (idx, line) in content.lines().enumerate() {
        let hay = if ignore_case {
            line.to_lowercase()
        } else {
            line.to_string()
        };
        if hay.contains(&needle) {
            matches.push(SearchMatch {
                file: rel.clone(),
                line: idx + 1,
                text: line.to_string(),
            });
            if matches.len() >= max_results {
                break;
            }
        }
    }
    Ok(())
}

/// Simple glob matcher supporting `*` (any sequence) and `?` (any char).
fn glob_match(name: &str, pat: &str) -> bool {
    glob_match_impl(name.as_bytes(), pat.as_bytes())
}

fn glob_match_impl(name: &[u8], pat: &[u8]) -> bool {
    let (n, p) = (name.len(), pat.len());
    let mut i = 0;
    let mut j = 0;
    let mut star: Option<usize> = None;
    let mut mark = 0;
    while i < n {
        if j < p && (pat[j] == b'?' || pat[j] == name[i]) {
            i += 1;
            j += 1;
        } else if j < p && pat[j] == b'*' {
            star = Some(j);
            mark = i;
            j += 1;
        } else if let Some(s) = star {
            j = s + 1;
            mark += 1;
            i = mark;
        } else {
            return false;
        }
    }
    while j < p && pat[j] == b'*' {
        j += 1;
    }
    j == p
}

/// Produce a simple line-level diff preview between `old` and `new`.
/// Removed lines are prefixed `-`, added lines `+`. Sufficient for an
/// agent to preview an edit (not a full unified-diff with context).
fn unified_diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.split_inclusive('\n').collect();
    let b: Vec<&str> = new.split_inclusive('\n').collect();
    let n = a.len();
    let m = b.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j + 1].max(dp[i + 1][j]).max(dp[i][j + 1])
            };
        }
    }
    let mut out = String::from("--- a\n+++ b\n");
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            i += 1;
            j += 1;
        } else if dp[i + 1][j + 1] >= dp[i + 1][j] && dp[i + 1][j + 1] >= dp[i][j + 1] {
            out.push('-');
            out.push_str(a[i]);
            out.push('+');
            out.push_str(b[j]);
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            out.push('-');
            out.push_str(a[i]);
            i += 1;
        } else {
            out.push('+');
            out.push_str(b[j]);
            j += 1;
        }
    }
    while i < n {
        out.push('-');
        out.push_str(a[i]);
        i += 1;
    }
    while j < m {
        out.push('+');
        out.push_str(b[j]);
        j += 1;
    }
    out
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

impl FileEditTool {
    pub fn new() -> Self {
        Self { jail_root: None }
    }
    pub fn with_jail(root: PathBuf) -> Self {
        Self {
            jail_root: Some(root),
        }
    }
}
impl Default for FileEditTool {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeSearchTool {
    pub fn new() -> Self {
        Self { jail_root: None }
    }
    pub fn with_jail(root: PathBuf) -> Self {
        Self {
            jail_root: Some(root),
        }
    }
}
impl Default for CodeSearchTool {
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

    #[tokio::test]
    async fn test_file_edit_str_replace() {
        let dir = tempfile::tempdir().unwrap();
        let tool = FileEditTool::with_jail(dir.path().to_path_buf());
        std::fs::write(dir.path().join("sample.txt"), "hello world\nfoo bar\n").unwrap();
        let out = tool
            .call(FileEditArgs {
                path: "sample.txt".into(),
                old_string: "world".into(),
                new_string: "rupoo".into(),
                replace_all: None,
            })
            .await
            .unwrap();
        assert!(out.success);
        assert_eq!(out.replacements, 1);
        let after = std::fs::read_to_string(dir.path().join("sample.txt")).unwrap();
        assert!(after.contains("hello rupoo"));
        assert!(out.diff.contains('-') && out.diff.contains('+'));
    }

    #[tokio::test]
    async fn test_file_edit_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let tool = FileEditTool::with_jail(dir.path().to_path_buf());
        std::fs::write(dir.path().join("sample.txt"), "hello world\n").unwrap();
        let out = tool
            .call(FileEditArgs {
                path: "sample.txt".into(),
                old_string: "not present".into(),
                new_string: "x".into(),
                replace_all: None,
            })
            .await
            .unwrap();
        assert!(!out.success);
        assert!(out.error.unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn test_file_edit_not_unique_without_replace_all() {
        let dir = tempfile::tempdir().unwrap();
        let tool = FileEditTool::with_jail(dir.path().to_path_buf());
        std::fs::write(dir.path().join("sample.txt"), "a a a\n").unwrap();
        let out = tool
            .call(FileEditArgs {
                path: "sample.txt".into(),
                old_string: "a".into(),
                new_string: "b".into(),
                replace_all: None,
            })
            .await
            .unwrap();
        assert!(!out.success);
        assert!(out.error.unwrap().contains("not unique"));
    }

    #[tokio::test]
    async fn test_file_edit_replace_all() {
        let dir = tempfile::tempdir().unwrap();
        let tool = FileEditTool::with_jail(dir.path().to_path_buf());
        std::fs::write(dir.path().join("sample.txt"), "a a a\n").unwrap();
        let out = tool
            .call(FileEditArgs {
                path: "sample.txt".into(),
                old_string: "a".into(),
                new_string: "b".into(),
                replace_all: Some(true),
            })
            .await
            .unwrap();
        assert!(out.success);
        assert_eq!(out.replacements, 3);
        let after = std::fs::read_to_string(dir.path().join("sample.txt")).unwrap();
        assert_eq!(after, "b b b\n");
    }

    #[tokio::test]
    async fn test_code_search_finds_match() {
        let dir = tempfile::tempdir().unwrap();
        let tool = CodeSearchTool::with_jail(dir.path().to_path_buf());
        std::fs::write(dir.path().join("a.rs"), "fn main() {}\nfn helper() {}\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "nothing here\n").unwrap();
        let out = tool
            .call(CodeSearchArgs {
                pattern: "fn ".into(),
                path: Some(".".into()),
                file_glob: None,
                ignore_case: None,
                max_results: None,
            })
            .await
            .unwrap();
        assert!(out.success);
        assert_eq!(out.match_count, 2);
        assert!(out.matches.iter().all(|m| m.file.ends_with("a.rs")));
    }

    #[tokio::test]
    async fn test_code_search_glob_and_case() {
        let dir = tempfile::tempdir().unwrap();
        let tool = CodeSearchTool::with_jail(dir.path().to_path_buf());
        std::fs::write(dir.path().join("a.rs"), "HELLO world\n").unwrap();
        std::fs::write(dir.path().join("b.py"), "hello world\n").unwrap();
        let out = tool
            .call(CodeSearchArgs {
                pattern: "hello".into(),
                path: Some(".".into()),
                file_glob: Some("*.rs".into()),
                ignore_case: Some(true),
                max_results: None,
            })
            .await
            .unwrap();
        assert!(out.success);
        assert_eq!(out.match_count, 1);
        assert!(out.matches[0].file.ends_with("a.rs"));
    }

    #[test]
    fn test_glob_match_helper() {
        assert!(glob_match("foo.rs", "*.rs"));
        assert!(glob_match("src/foo.rs", "src/*.rs"));
        assert!(!glob_match("foo.py", "*.rs"));
        assert!(glob_match("any", "*"));
        assert!(glob_match("abc", "a?c"));
    }

    #[test]
    fn test_unified_diff_helper() {
        let d = unified_diff("a\nb\nc\n", "a\nB\nc\n");
        assert!(d.contains("-b"));
        assert!(d.contains("+B"));
    }

    /// M6 regression: diff truncation must not panic on multi-byte UTF-8.
    #[test]
    fn test_diff_truncation_respects_char_boundary() {
        // Build a diff > 6000 bytes containing multi-byte chars at the boundary.
        let old = "a\n".repeat(4000); // 8000 bytes
        let new_line = "你\n"; // 3-byte UTF-8 char
        let new = format!("{}{}", "b\n".repeat(3000), new_line.repeat(1000));
        let diff = unified_diff(&old, &new);
        // Simulate the truncation logic from file_edit
        if diff.len() > 6000 {
            let end = diff.floor_char_boundary(6000);
            let truncated = format!("{}...[truncated]", &diff[..end]);
            // Must end with the marker and be valid UTF-8 (no panic)
            assert!(truncated.ends_with("...[truncated]"));
            assert!(truncated.len() <= 6000 + "...[truncated]".len());
        }
    }
}
