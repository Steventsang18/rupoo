//! MCP tool executor — bridges plan-level ToolCall steps with
//! rig_tools implementations.
//!
//! Architecture: McpToolExecutor directly holds `Box<dyn rig::tool::Tool>`,
//! eliminating the intermediate ToolKind enum and manual serialization.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::RwLock;

use crate::agent::ToolExecutor;
use crate::error::{AgentError, AgentResult};
use crate::safety::SafetyContext;
use crate::task::McpToolResult;

use rig::tool::Tool;

// -----------------------------------------------------------------------------
// Helper function to extract content from tool output
// -----------------------------------------------------------------------------

/// Extract content from tool output JSON and convert to McpToolResult.
/// Handles different output structures:
/// - EchoOutput: { result: String }
/// - FileReadOutput: { content: String, success: bool, error: Option<String> }
/// - FileWriteOutput: { bytes_written: usize, success: bool, error: Option<String> }
/// - ListDirOutput: { entries: Vec<DirEntry>, success: bool, error: Option<String> }
/// - WebSearchOutput: { results: String, success: bool, error: Option<String> }
/// - ShellExecOutput: { stdout: String, exit_code: Option<i32>, success: bool, error: Option<String> }
fn extract_mcp_result(value: &serde_json::Value) -> McpToolResult {
    // Check if it's already a McpToolResult
    if let Ok(result) = serde_json::from_value::<McpToolResult>(value.clone()) {
        return result;
    }

    // Check for success field
    let success = value
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if !success {
        // Error case
        let error = value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("tool execution failed")
            .to_string();
        return McpToolResult::Error { message: error };
    }

    // Success case - extract content from different output types
    let content = if let Some(result) = value.get("result").and_then(|v| v.as_str()) {
        // EchoOutput
        result.to_string()
    } else if let Some(content) = value.get("content").and_then(|v| v.as_str()) {
        // FileReadOutput
        content.to_string()
    } else if let Some(bytes_written) = value.get("bytes_written").and_then(|v| v.as_u64()) {
        // FileWriteOutput
        format!("{} bytes written", bytes_written)
    } else if let Some(results) = value.get("results").and_then(|v| v.as_str()) {
        // WebSearchOutput
        results.to_string()
    } else if let Some(stdout) = value.get("stdout").and_then(|v| v.as_str()) {
        // ShellExecOutput
        stdout.to_string()
    } else if let Some(entries) = value.get("entries").and_then(|v| v.as_array()) {
        // ListDirOutput - format entries
        entries
            .iter()
            .filter_map(|e| {
                let name = e.get("name")?.as_str()?;
                let kind = e.get("kind")?.as_str()?;
                Some(format!("{} ({})", name, kind))
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        // Fallback: serialize the entire value
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    };

    McpToolResult::Success { content }
}

// -----------------------------------------------------------------------------
// BoxedTool trait - wrapper for type-erased tool execution
// -----------------------------------------------------------------------------

/// Trait for type-erased tool execution from JSON parameters.
#[async_trait]
trait BoxedTool: Send + Sync {
    async fn execute_json(&self, params: serde_json::Value) -> Result<serde_json::Value, String>;
}

/// Wrapper that adapts rig::tool::Tool to BoxedTool.
struct ToolWrapper<T>(T);

#[async_trait]
impl<T> BoxedTool for ToolWrapper<T>
where
    T: Tool + Send + Sync,
    T::Args: DeserializeOwned + Send,
    T::Output: Serialize + Send,
{
    async fn execute_json(&self, params: serde_json::Value) -> Result<serde_json::Value, String> {
        let args: T::Args = serde_json::from_value(params)
            .map_err(|e| format!("failed to deserialize args: {}", e))?;

        let output = self
            .0
            .call(args)
            .await
            .map_err(|e| format!("tool execution failed: {}", e))?;

        serde_json::to_value(output).map_err(|e| format!("failed to serialize output: {}", e))
    }
}

// -----------------------------------------------------------------------------
// McpToolExecutor — holds Box<dyn Tool> instances
// -----------------------------------------------------------------------------

/// A dispatcher that holds tool instances implementing rig::tool::Tool.
/// Used by the Agent for explicit ToolCall steps and by the MCP server.
///
/// Cloning shares the same registry (Arc<RwLock<...>>), so both copies
/// always see the same registered tools.
#[derive(Clone)]
pub struct McpToolExecutor {
    registry: Arc<RwLock<HashMap<String, Arc<dyn BoxedTool>>>>,
    /// Keep tool definitions for schema listing
    definitions: Arc<RwLock<HashMap<String, rig::completion::ToolDefinition>>>,
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
        let (tools, defs) = Self::build_tools(jail_root);
        Self {
            registry: Arc::new(RwLock::new(tools)),
            definitions: Arc::new(RwLock::new(defs)),
        }
    }

    fn build_tools(
        jail_root: Option<std::path::PathBuf>,
    ) -> (
        HashMap<String, Arc<dyn BoxedTool>>,
        HashMap<String, rig::completion::ToolDefinition>,
    ) {
        let mut tools: HashMap<String, Arc<dyn BoxedTool>> = HashMap::new();
        let mut defs: HashMap<String, rig::completion::ToolDefinition> = HashMap::new();

        // Helper to register a tool
        macro_rules! register {
            ($name:expr, $tool:expr) => {{
                let tool = $tool;
                let def = futures::executor::block_on(tool.definition(String::new()));
                tools.insert($name.into(), Arc::new(ToolWrapper(tool)));
                defs.insert($name.into(), def);
            }};
        }

        // Echo
        register!("echo", crate::rig_tools::EchoTool::new());

        // File tools with optional jail
        match jail_root {
            Some(ref root) => {
                register!(
                    "file_read",
                    crate::rig_tools::FileReadTool::with_jail(root.clone())
                );
                register!(
                    "file_write",
                    crate::rig_tools::FileWriteTool::with_jail(root.clone())
                );
                register!(
                    "list_directory",
                    crate::rig_tools::ListDirTool::with_jail(root.clone())
                );
            }
            None => {
                register!("file_read", crate::rig_tools::FileReadTool::new());
                register!("file_write", crate::rig_tools::FileWriteTool::new());
                register!("list_directory", crate::rig_tools::ListDirTool::new());
            }
        }

        register!("web_search", crate::rig_tools::WebSearchTool::new());
        register!("shell_exec", crate::rig_tools::ShellExecTool::new());
        register!("run_tests", crate::tools::verify::RunTestsTool);
        register!("check_output", crate::tools::verify::CheckOutputTool);
        register!("diff_check", crate::tools::verify::DiffCheckTool);

        (tools, defs)
    }

    /// Return all registered tool names.
    pub async fn list_tools(&self) -> Vec<String> {
        let reg = self.registry.read().await;
        reg.keys().cloned().collect()
    }

    /// Return tool descriptions for MCP server.
    pub async fn list_tools_with_desc(&self) -> Vec<(String, String)> {
        let defs = self.definitions.read().await;
        defs.values()
            .map(|d| (d.name.clone(), d.description.clone()))
            .collect()
    }

    /// Return tool schemas for MCP server. Returns (name, description, parameters_json) tuples.
    pub async fn list_tools_with_schema(&self) -> Vec<(String, String, serde_json::Value)> {
        let defs = self.definitions.read().await;
        defs.values()
            .map(|d| (d.name.clone(), d.description.clone(), d.parameters.clone()))
            .collect()
    }

    /// Unregister a tool at runtime.
    pub async fn unregister_tool(&self, name: &str) {
        let mut reg = self.registry.write().await;
        reg.remove(name);
        let mut defs = self.definitions.write().await;
        defs.remove(name);
    }
}

#[async_trait]
impl ToolExecutor for McpToolExecutor {
    async fn execute_tool(
        &self,
        tool_name: &str,
        params: serde_json::Value,
    ) -> AgentResult<McpToolResult> {
        let entry = {
            let reg = self.registry.read().await;
            reg.get(tool_name)
                .map(Arc::clone)
                .ok_or_else(|| AgentError::Mcp(format!("unknown tool: '{tool_name}'")))?
        };

        // Direct async call via BoxedTool trait
        let result = entry.execute_json(params).await;

        match result {
            Ok(value) => Ok(extract_mcp_result(&value)),
            Err(e) => Ok(McpToolResult::Error { message: e }),
        }
    }

    /// Execute multiple tools in parallel using tokio's join_all.
    async fn execute_tools_parallel(
        &self,
        tool_calls: Vec<(String, serde_json::Value)>,
    ) -> Vec<AgentResult<McpToolResult>> {
        let executor = Arc::new(self.clone());
        let futures: Vec<_> = tool_calls
            .into_iter()
            .map(move |(name, params)| {
                let executor_clone = Arc::clone(&executor);
                async move { executor_clone.execute_tool(&name, params).await }
            })
            .collect();
        futures::future::join_all(futures).await
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
        assert!(result.is_success());
        assert_eq!(result.content(), "echo: hello world");
    }

    #[tokio::test]
    async fn test_echo() {
        let executor = McpToolExecutor::new();
        let result = executor
            .execute_tool("echo", serde_json::json!({"message": "hello world"}))
            .await
            .unwrap();
        assert!(result.is_success());
        assert_eq!(result.content(), "echo: hello world");
    }

    #[tokio::test]
    async fn test_file_read_nonexistent() {
        let executor = McpToolExecutor::new();
        let result = executor
            .execute_tool(
                "file_read",
                serde_json::json!({"path": "target/_nonexistent_xyz_test_file"}),
            )
            .await
            .unwrap();
        assert!(!result.is_success());
        assert!(result.error_message().is_some());
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
        assert!(
            result.is_success(),
            "list_directory failed: {:?}",
            result.error_message()
        );
        assert!(!result.content().is_empty());
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
        assert!(write_result.is_success());

        let read_result = executor
            .execute_tool(
                "file_read",
                serde_json::json!({"path": test_path.to_string_lossy()}),
            )
            .await
            .unwrap();
        assert!(read_result.is_success());
        assert!(read_result.content().contains("hello from mcp test"));

        // cleanup
        let _ = std::fs::remove_file(&test_path);
        let _ = std::fs::remove_dir(test_dir);
    }

    #[tokio::test]
    async fn test_list_tools() {
        let executor = McpToolExecutor::new();
        let tools = executor.list_tools().await;
        assert_eq!(tools.len(), 9);
        assert!(tools.contains(&"echo".into()));
        assert!(tools.contains(&"file_read".into()));
        assert!(tools.contains(&"file_write".into()));
        assert!(tools.contains(&"list_directory".into()));
        assert!(tools.contains(&"web_search".into()));
        assert!(tools.contains(&"run_tests".into()));
        assert!(tools.contains(&"check_output".into()));
        assert!(tools.contains(&"diff_check".into()));
    }
}
