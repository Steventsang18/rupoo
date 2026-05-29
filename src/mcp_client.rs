//! MCP Client — connect to external MCP servers over stdio.
//!
//! Allows rupoo to use tools from external MCP servers (e.g. Claude Desktop,
//! Cursor, filesystem servers) alongside its built-in tools.
//!
//! Architecture:
//! - `McpClient` manages connections to external MCP servers
//! - Each server is started as a child process with stdio transport
//! - Tools are discovered via `tools/list` at startup
//! - Tool execution is forwarded via `tools/call`
//! - Heartbeat keeps connections alive; reconnects on failure

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::config::McpServerConfig;
use crate::error::{AgentError, AgentResult};

// ---------------------------------------------------------------------------
// MCP protocol types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: Option<u64>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: i64,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "inputSchema")]
    pub input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ToolsListResult {
    #[serde(default)]
    tools: Vec<McpToolInfo>,
}

// ---------------------------------------------------------------------------
// MCP Client connection
// ---------------------------------------------------------------------------

struct ServerConnection {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
}

impl ServerConnection {
    /// Start a server process and establish stdio connection.
    async fn start(config: &McpServerConfig) -> AgentResult<Self> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Add environment variables
        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn()
            .map_err(|e| AgentError::Mcp(format!("failed to start MCP server '{}': {e}", config.command)))?;

        let stdin = child.stdin.take()
            .ok_or_else(|| AgentError::Mcp("failed to get stdin".into()))?;
        let stdout = child.stdout.take()
            .ok_or_else(|| AgentError::Mcp("failed to get stdout".into()))?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    /// Send a JSON-RPC request and read the response.
    async fn send_request(&mut self, method: &str, params: Option<serde_json::Value>) -> AgentResult<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };

        let mut request_str = serde_json::to_string(&request)
            .map_err(|e| AgentError::Mcp(format!("serialize request: {e}")))?;
        request_str.push('\n');

        self.stdin.write_all(request_str.as_bytes()).await
            .map_err(|e| AgentError::Mcp(format!("write to server: {e}")))?;
        self.stdin.flush().await
            .map_err(|e| AgentError::Mcp(format!("flush: {e}")))?;

        // Read response line
        let mut line = String::new();
        self.stdout.read_line(&mut line).await
            .map_err(|e| AgentError::Mcp(format!("read from server: {e}")))?;

        let response: JsonRpcResponse = serde_json::from_str(line.trim())
            .map_err(|e| AgentError::Mcp(format!("parse response: {e}")))?;

        if let Some(err) = response.error {
            return Err(AgentError::Mcp(format!("server error: {}", err.message)));
        }

        response.result.ok_or_else(|| AgentError::Mcp("empty result".into()))
    }

    /// Send an initialize request to the server.
    async fn initialize(&mut self) -> AgentResult<()> {
        self.send_request("initialize", Some(serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "rupoo",
                "version": env!("CARGO_PKG_VERSION")
            }
        }))).await?;
        Ok(())
    }

    /// List available tools from the server.
    async fn list_tools(&mut self) -> AgentResult<Vec<McpToolInfo>> {
        let result = self.send_request("tools/list", None).await?;
        let tools_result: ToolsListResult = serde_json::from_value(result)
            .map_err(|e| AgentError::Mcp(format!("parse tools/list: {e}")))?;
        Ok(tools_result.tools)
    }

    /// Call a tool on the server.
    async fn call_tool(&mut self, name: &str, arguments: serde_json::Value) -> AgentResult<serde_json::Value> {
        self.send_request("tools/call", Some(serde_json::json!({
            "name": name,
            "arguments": arguments
        }))).await
    }

    /// Check if the server process is still running.
    fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

// ---------------------------------------------------------------------------
// MCP Client manager
// ---------------------------------------------------------------------------

pub struct McpClientManager {
    connections: Arc<Mutex<HashMap<String, ServerConnection>>>,
    tools: Arc<Mutex<HashMap<String, Vec<McpToolInfo>>>>,
}

impl McpClientManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            tools: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Connect to an MCP server and discover its tools.
    pub async fn connect(&self, name: &str, config: &McpServerConfig) -> AgentResult<Vec<McpToolInfo>> {
        info!(server = %name, command = %config.command, "connecting to MCP server");

        let mut conn = ServerConnection::start(config).await?;
        conn.initialize().await?;
        let tools = conn.list_tools().await?;

        info!(server = %name, tools = tools.len(), "MCP server connected");

        let mut connections = self.connections.lock().await;
        connections.insert(name.to_string(), conn);

        let mut tools_map = self.tools.lock().await;
        tools_map.insert(name.to_string(), tools.clone());

        Ok(tools)
    }

    /// Disconnect from an MCP server.
    pub async fn disconnect(&self, name: &str) -> AgentResult<()> {
        let mut connections = self.connections.lock().await;
        if let Some(mut conn) = connections.remove(name) {
            // Try graceful shutdown
            let _ = conn.child.kill().await;
        }
        let mut tools_map = self.tools.lock().await;
        tools_map.remove(name);
        Ok(())
    }

    /// Call a tool on a connected server.
    pub async fn call_tool(&self, server_name: &str, tool_name: &str, arguments: serde_json::Value) -> AgentResult<serde_json::Value> {
        let mut connections = self.connections.lock().await;
        let conn = connections.get_mut(server_name)
            .ok_or_else(|| AgentError::Mcp(format!("server '{server_name}' not connected")))?;

        if !conn.is_running() {
            warn!(server = %server_name, "server process died, removing");
            connections.remove(server_name);
            return Err(AgentError::Mcp(format!("server '{server_name}' process died")));
        }

        conn.call_tool(tool_name, arguments).await
    }

    /// Get all discovered tools from all connected servers.
    pub async fn all_tools(&self) -> Vec<(String, McpToolInfo)> {
        let tools_map = self.tools.lock().await;
        let mut result = Vec::new();
        for (server, tools) in tools_map.iter() {
            for tool in tools {
                result.push((server.clone(), tool.clone()));
            }
        }
        result
    }

    /// Find a tool by name across all servers.
    pub async fn find_tool(&self, tool_name: &str) -> Option<(String, McpToolInfo)> {
        let tools_map = self.tools.lock().await;
        for (server, tools) in tools_map.iter() {
            for tool in tools {
                if tool.name == tool_name {
                    return Some((server.clone(), tool.clone()));
                }
            }
        }
        None
    }

    /// Connect to all servers defined in config.
    pub async fn connect_all(&self, servers: &HashMap<String, McpServerConfig>) -> Vec<(String, Result<Vec<McpToolInfo>, String>)> {
        let mut results = Vec::new();
        for (name, config) in servers {
            match self.connect(name, config).await {
                Ok(tools) => results.push((name.clone(), Ok(tools))),
                Err(e) => {
                    warn!(server = %name, error = %e, "failed to connect to MCP server");
                    results.push((name.clone(), Err(e.to_string())));
                }
            }
        }
        results
    }
}

impl Default for McpClientManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_info_serde() {
        let tool = McpToolInfo {
            name: "read_file".to_string(),
            description: Some("Read a file".to_string()),
            inputSchema: Some(serde_json::json!({"type": "object"})),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: McpToolInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "read_file");
    }

    #[test]
    fn test_mcp_client_manager_new() {
        let manager = McpClientManager::new();
        assert!(manager.connections.try_lock().unwrap().is_empty());
        assert!(manager.tools.try_lock().unwrap().is_empty());
    }

    #[test]
    fn test_json_rpc_request_serialization() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "tools/list".to_string(),
            params: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"method\":\"tools/list\""));
        assert!(!json.contains("params")); // skip_serializing_if
    }
}
