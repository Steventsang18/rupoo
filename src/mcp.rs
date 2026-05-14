//! MCP tool executor — bridges plan-level ToolCall steps with
//! rig_tools implementations.
//!
//! This module delegates to `rig_tools.rs` for all actual tool logic,
//! avoiding code duplication between the two tool systems.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::info;

use crate::agent::ToolExecutor;
use crate::error::{AgentError, AgentResult};
use crate::task::McpToolResult;

// Bring rig Tool trait into scope for .call() method.
use rig::tool::Tool;

// ---------------------------------------------------------------------------
// MCP tool registry (delegates to rig_tools)
// ---------------------------------------------------------------------------

/// A dispatcher that maps tool names to rig_tools Tool implementations.
/// Used by the Agent for explicit ToolCall steps.
pub struct McpToolExecutor {
    registry: Arc<Mutex<HashMap<String, ToolDispatchEntry>>>,
}

struct ToolDispatchEntry {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    description: String,
    call_fn: Box<dyn Fn(serde_json::Value) -> AgentResult<serde_json::Value> + Send + Sync>,
}

impl Default for McpToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl McpToolExecutor {
    pub fn new() -> Self {
        let mut tools = HashMap::new();
        Self::register_builtin(&mut tools);
        Self {
            registry: Arc::new(Mutex::new(tools)),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new()
    }

    fn register_builtin(tools: &mut HashMap<String, ToolDispatchEntry>) {
        // Echo
        tools.insert(
            "echo".into(),
            ToolDispatchEntry {
                name: "echo".into(),
                description: "Echo back a message".into(),
                call_fn: Box::new(|params| {
                    let msg = params
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    Ok(serde_json::json!({
                        "success": true,
                        "content": format!("echo: {msg}"),
                        "error": null
                    }))
                }),
            },
        );

        // File read — delegates to crate::rig_tools::FileReadTool
        tools.insert(
            "file_read".into(),
            ToolDispatchEntry {
                name: "file_read".into(),
                description: "Read the contents of a file at the given path".into(),
                call_fn: Box::new(|params| {
                    let rt = tokio::runtime::Runtime::new()
                        .map_err(|e| AgentError::Other(e.to_string()))?;
                    rt.block_on(async {
                        let path = params
                            .get("path")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let tool = crate::rig_tools::FileReadTool;
                        let args = crate::rig_tools::FileReadArgs { path };
                        match tool.call(args).await {
                            Ok(output) => Ok(serde_json::json!({
                                "success": output.success,
                                "content": output.content,
                                "error": output.error
                            })),
                            Err(e) => Ok(serde_json::json!({
                                "success": false,
                                "content": "",
                                "error": format!("tool error: {e}")
                            })),
                        }
                    })
                }),
            },
        );

        // File write — delegates to crate::rig_tools::FileWriteTool
        tools.insert(
            "file_write".into(),
            ToolDispatchEntry {
                name: "file_write".into(),
                description: "Write content to a file".into(),
                call_fn: Box::new(|params| {
                    let rt = tokio::runtime::Runtime::new()
                        .map_err(|e| AgentError::Other(e.to_string()))?;
                    rt.block_on(async {
                        let path = params
                            .get("path")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let content = params
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let tool = crate::rig_tools::FileWriteTool;
                        let args = crate::rig_tools::FileWriteArgs { path, content };
                        match tool.call(args).await {
                            Ok(output) => Ok(serde_json::json!({
                                "success": output.success,
                                "bytes_written": output.bytes_written,
                                "error": output.error
                            })),
                            Err(e) => Ok(serde_json::json!({
                                "success": false,
                                "error": format!("tool error: {e}")
                            })),
                        }
                    })
                }),
            },
        );

        // List directory — delegates to crate::rig_tools::ListDirTool
        tools.insert(
            "list_directory".into(),
            ToolDispatchEntry {
                name: "list_directory".into(),
                description: "List entries in a directory".into(),
                call_fn: Box::new(|params| {
                    let rt = tokio::runtime::Runtime::new()
                        .map_err(|e| AgentError::Other(e.to_string()))?;
                    rt.block_on(async {
                        let path = params
                            .get("path")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let tool = crate::rig_tools::ListDirTool;
                        let args = crate::rig_tools::ListDirArgs { path };
                        match tool.call(args).await {
                            Ok(output) => {
                                let entries: Vec<String> = output
                                    .entries
                                    .iter()
                                    .map(|e| format!("[{}] {}", e.kind, e.name))
                                    .collect();
                                Ok(serde_json::json!({
                                    "success": output.success,
                                    "content": entries.join("\n"),
                                    "error": output.error
                                }))
                            }
                            Err(e) => Ok(serde_json::json!({
                                "success": false,
                                "content": "",
                                "error": format!("tool error: {e}")
                            })),
                        }
                    })
                }),
            },
        );
    }

    pub async fn list_tools(&self) -> Vec<String> {
        let reg = self.registry.lock().await;
        reg.keys().cloned().collect()
    }
}

#[async_trait]
impl ToolExecutor for McpToolExecutor {
    async fn execute_tool(
        &self,
        tool_name: &str,
        params: serde_json::Value,
    ) -> AgentResult<McpToolResult> {
        // Acquire lock once, call synchronously, then drop
        let result = {
            let reg = self.registry.lock().await;
            let entry = reg.get(tool_name).ok_or_else(|| {
                AgentError::Mcp(format!("unknown tool: '{tool_name}'"))
            })?;
            (entry.call_fn)(params)
        };

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
                Ok(McpToolResult {
                    success,
                    content,
                    error,
                })
            }
            Err(e) => Ok(McpToolResult {
                success: false,
                content: String::new(),
                error: Some(e.to_string()),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Minimal JSON-RPC client for connecting to external MCP server processes
// (stdio transport)
// ---------------------------------------------------------------------------

pub struct McpStdioClient {
    child: tokio::process::Child,
    stdin: tokio::io::BufWriter<tokio::process::ChildStdin>,
    stdout: tokio::io::BufReader<tokio::process::ChildStdout>,
}

impl McpStdioClient {
    /// Spawn an external MCP server process and connect via stdio.
    pub async fn spawn(command: &str, args: &[&str]) -> AgentResult<Self> {
        let mut child = tokio::process::Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AgentError::Mcp("failed to capture stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AgentError::Mcp("failed to capture stdout".into()))?;

        info!("MCP stdio client spawned: {command}");
        Ok(Self {
            child,
            stdin: tokio::io::BufWriter::new(stdin),
            stdout: tokio::io::BufReader::new(stdout),
        })
    }

    #[allow(dead_code)]
    pub async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> AgentResult<serde_json::Value> {
        use tokio::io::AsyncWriteExt;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let mut buf = serde_json::to_vec(&request)?;
        buf.push(b'\n');
        self.stdin.write_all(&buf).await?;
        self.stdin.flush().await?;

        let mut line = String::new();
        use tokio::io::AsyncBufReadExt;
        self.stdout.read_line(&mut line).await?;

        if line.is_empty() {
            return Err(AgentError::Mcp("MCP server closed connection".into()));
        }

        let response: serde_json::Value = serde_json::from_str(&line)?;
        Ok(response)
    }

    #[allow(dead_code)]
    pub async fn shutdown(&mut self) -> AgentResult<()> {
        self.child.kill().await?;
        self.child.wait().await?;
        Ok(())
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
    async fn test_unknown_tool() {
        let executor = McpToolExecutor::new();
        let result = executor
            .execute_tool("nonexistent", serde_json::json!({}))
            .await;
        assert!(result.is_err());
    }
}
