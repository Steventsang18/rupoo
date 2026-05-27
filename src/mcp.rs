//! MCP tool executor — bridges plan-level ToolCall steps with
//! rig_tools implementations.
//!
//! Architecture: a typed enum replaces the old Box<dyn Fn> dispatch.
//! Each variant holds the concrete tool struct, with async execute
//! implemented directly via match, eliminating both the closure heap
//! allocation and the nested Runtime::block_on per call.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::safety::SafetyContext;
use crate::agent::ToolExecutor;
use crate::error::{AgentError, AgentResult};
use crate::task::McpToolResult;

use crate::rig_tools::{
    EchoArgs,
    FileReadArgs,
    FileWriteArgs,
    ListDirArgs,
};

use rig::tool::Tool;

// -----------------------------------------------------------------------------
// ToolKind enum — replaces Box<dyn Fn> dispatch
// -----------------------------------------------------------------------------

enum ToolKind {
    Echo,
    FileRead { jail_root: Option<std::path::PathBuf> },
    FileWrite { jail_root: Option<std::path::PathBuf> },
    ListDir { jail_root: Option<std::path::PathBuf> },
}

impl ToolKind {
    fn name(&self) -> &'static str {
        match self {
            ToolKind::Echo => "echo",
            ToolKind::FileRead { .. } => "file_read",
            ToolKind::FileWrite { .. } => "file_write",
            ToolKind::ListDir { .. } => "list_directory",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            ToolKind::Echo => "Echo back a message",
            ToolKind::FileRead { .. } => "Read the contents of a file at the given path",
            ToolKind::FileWrite { .. } => "Write content to a file. Overwrites existing content.",
            ToolKind::ListDir { .. } => "List entries in a directory",
        }
    }

    async fn execute(&self, params: serde_json::Value) -> Result<serde_json::Value, String> {
        match self {
            ToolKind::Echo => {
                let args: EchoArgs = serde_json::from_value(params)
                    .map_err(|e| format!("bad args: {e}"))?;
                let output = crate::rig_tools::EchoTool::new().call(args).await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(McpToolResult {
                    success: true,
                    content: output.result,
                    error: None,
                }).map_err(|e| e.to_string())
            }
            ToolKind::FileRead { jail_root } => {
                let args: FileReadArgs = serde_json::from_value(params)
                    .map_err(|e| format!("bad args: {e}"))?;
                let tool = match jail_root {
                    Some(ref root) => crate::rig_tools::FileReadTool::with_jail(root.clone()),
                    None => crate::rig_tools::FileReadTool::new(),
                };
                let output = tool.call(args).await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(McpToolResult {
                    success: output.success,
                    content: output.content,
                    error: output.error,
                }).map_err(|e| e.to_string())
            }
            ToolKind::FileWrite { jail_root } => {
                let args: FileWriteArgs = serde_json::from_value(params)
                    .map_err(|e| format!("bad args: {e}"))?;
                let tool = match jail_root {
                    Some(ref root) => crate::rig_tools::FileWriteTool::with_jail(root.clone()),
                    None => crate::rig_tools::FileWriteTool::new(),
                };
                let output = tool.call(args).await
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(McpToolResult {
                    success: output.success,
                    content: format!("{} bytes written", output.bytes_written),
                    error: output.error,
                }).map_err(|e| e.to_string())
            }
            ToolKind::ListDir { jail_root } => {
                let args: ListDirArgs = serde_json::from_value(params)
                    .map_err(|e| format!("bad args: {e}"))?;
                let tool = match jail_root {
                    Some(ref root) => crate::rig_tools::ListDirTool::with_jail(root.clone()),
                    None => crate::rig_tools::ListDirTool::new(),
                };
                let output = tool.call(args).await
                    .map_err(|e| e.to_string())?;
                let content = output.entries.iter()
                    .map(|e| format!("{} ({})", e.name, e.kind))
                    .collect::<Vec<_>>()
                    .join("\n");
                serde_json::to_value(McpToolResult {
                    success: output.success,
                    content,
                    error: output.error,
                }).map_err(|e| e.to_string())
            }
        }
    }
}

// -----------------------------------------------------------------------------
// McpToolExecutor — holds ToolKind enum variants in a RwLock registry
// -----------------------------------------------------------------------------

/// A dispatcher that maps tool names to typed ToolKind variants.
/// Used by the Agent for explicit ToolCall steps and by the MCP server.
///
/// Cloning shares the same registry (Arc<RwLock<...>>), so both copies
/// always see the same registered tools.
#[derive(Clone)]
pub struct McpToolExecutor {
    registry: Arc<RwLock<HashMap<String, Arc<ToolKind>>>>,
}

impl Default for McpToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl McpToolExecutor {
    pub fn new() -> Self {
        Self::with_safety(SafetyContext::default())
    }

    pub fn with_defaults() -> Self {
        Self::new()
    }

    /// Create with a pre-configured SafetyContext for file jail enforcement.
    /// File tools will use the SafetyContext's jail_root for path validation.
    pub fn with_safety(safety_ctx: SafetyContext) -> Self {
        let jail_root = safety_ctx.jail_root().map(|p| p.to_path_buf());
        let tools = Self::build_tools(jail_root);
        Self {
            registry: Arc::new(RwLock::new(tools)),
        }
    }

    fn build_tools(jail_root: Option<std::path::PathBuf>) -> HashMap<String, Arc<ToolKind>> {
        let mut tools = HashMap::new();
        tools.insert("echo".into(), Arc::new(ToolKind::Echo));
        tools.insert("file_read".into(), Arc::new(ToolKind::FileRead { jail_root: jail_root.clone() }));
        tools.insert("file_write".into(), Arc::new(ToolKind::FileWrite { jail_root: jail_root.clone() }));
        tools.insert("list_directory".into(), Arc::new(ToolKind::ListDir { jail_root }));
        tools
    }

    /// Return all registered tool names.
    pub async fn list_tools(&self) -> Vec<String> {
        let reg = self.registry.read().await;
        reg.keys().cloned().collect()
    }

    /// Return tool descriptions for MCP server.
    pub async fn list_tools_with_desc(&self) -> Vec<(&'static str, &'static str)> {
        let reg = self.registry.read().await;
        reg.values().map(|t| (t.name(), t.description())).collect()
    }
}

#[async_trait]
impl ToolExecutor for McpToolExecutor {
    async fn execute_tool(
        &self,
        tool_name: &str,
        params: serde_json::Value,
    ) -> AgentResult<McpToolResult> {
        // Path validation is handled inside ToolKind via resolve_path (jail_root).
        // No separate path_jail step needed — avoids double-validation conflicts.

        let entry = {
            let reg = self.registry.read().await;
            reg.get(tool_name).map(Arc::clone).ok_or_else(|| {
                AgentError::Mcp(format!("unknown tool: '{tool_name}'"))
            })?
        };

        // Direct async call — no spawn_blocking, no nested Runtime
        let result = entry.execute(params).await;

        match result {
            Ok(value) => {
                let success = value
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let content = value
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let error = value
                    .get("error")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                Ok(McpToolResult { success, content, error })
            }
            Err(e) => Ok(McpToolResult {
                success: false,
                content: String::new(),
                error: Some(e),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_echo_tool() {
        let executor = McpToolExecutor::new();
        let result = executor
            .execute_tool("echo", serde_json::json!({"message": "hello world"}))
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.content, "echo: hello world");
    }

    #[tokio::test]
    async fn test_file_read_nonexistent() {
        let executor = McpToolExecutor::new();
        let result = executor
            .execute_tool("file_read", serde_json::json!({"path": "target/_nonexistent_xyz_test_file"}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[tokio::test]
    async fn test_unknown_tool() {
        let executor = McpToolExecutor::new();
        let result = executor
            .execute_tool("nonexistent", serde_json::json!({}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_directory() {
        let executor = McpToolExecutor::new();
        let result = executor
            .execute_tool("list_directory", serde_json::json!({"path": "."}))
            .await
            .unwrap();
        assert!(result.success, "list_directory failed: {:?}", result.error);
        assert!(!result.content.is_empty());
    }

    #[tokio::test]
    async fn test_file_write_and_read() {
        use std::path::Path;
        let test_dir = Path::new("target/_rupoo_mcp_test");
        let _ = std::fs::create_dir_all(test_dir);
        let test_path = test_dir.join("test_write.txt");
        let test_path_str = test_path.to_string_lossy().to_string();

        let executor = McpToolExecutor::new();
        let write_result = executor
            .execute_tool(
                "file_write",
                serde_json::json!({
                    "path": test_path_str,
                    "content": "hello from mcp test"
                }),
            )
            .await
            .unwrap();
        assert!(write_result.success);

        let read_result = executor
            .execute_tool(
                "file_read",
                serde_json::json!({"path": test_path.to_string_lossy()}),
            )
            .await
            .unwrap();
        assert!(read_result.success);
        assert!(read_result.content.contains("hello from mcp test"));

        // cleanup
        let _ = std::fs::remove_file(&test_path);
        let _ = std::fs::remove_dir(test_dir);
    }

    #[tokio::test]
    async fn test_list_tools() {
        let executor = McpToolExecutor::new();
        let tools = executor.list_tools().await;
        assert_eq!(tools.len(), 4);
        assert!(tools.contains(&"echo".into()));
        assert!(tools.contains(&"file_read".into()));
    }
}