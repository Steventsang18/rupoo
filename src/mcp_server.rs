//! MCP (Model Context Protocol) server for Rupoo.
//!
//! Exposes Rupoo's built-in tools via the standard MCP protocol over stdio,
//! allowing any MCP client (Claude Desktop, Cursor, etc.) to discover and
//! call Rupoo tools.
//!
//! # Protocol
//! - Transport: stdio (JSON-RPC 2.0, one object per line)
//! - Methods: `initialize`, `tools/list`, `tools/call`
//! - Spec: https://spec.modelcontextprotocol.io
//!
//! # Usage
//! ```bash
//! rupoo mcp-server
//! ```
//! Then configure your MCP client to spawn this as a subprocess.

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tracing::{error, info};

use crate::safety::SafetyContext;
use crate::agent::ToolExecutor;
use crate::error::{AgentError, AgentResult};
use crate::mcp::McpToolExecutor;

// ---------------------------------------------------------------------------
// JSON-RPC types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[serde(default)]
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// MCP types
// ---------------------------------------------------------------------------

/// MCP tool definition (returned by tools/list).
#[derive(Debug, Serialize)]
struct McpToolDescription {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Server (dynamically loads tools from executor)
// ---------------------------------------------------------------------------

struct McpServer {
    /// The executor holds all registered tools dynamically.
    executor: McpToolExecutor,
    initialized: bool,
}

impl McpServer {
    fn new(safety_ctx: SafetyContext) -> Self {
        Self {
            executor: McpToolExecutor::with_safety(safety_ctx),
            initialized: false,
        }
    }

    /// Build tool descriptions dynamically from the executor.
    async fn build_tool_descriptions(&self) -> Vec<McpToolDescription> {
        let tools = self.executor.list_tools_with_schema().await;
        tools
            .into_iter()
            .map(|(name, description, schema)| {
                let input_schema = serde_json::json!({
                    "type": "object",
                    "properties": schema["properties"].clone(),
                    "required": schema["required"].clone(),
                });
                McpToolDescription {
                    name,
                    description,
                    input_schema,
                }
            })
            .collect()
    }

    /// Handle a JSON-RPC request and return an optional response.
    async fn handle_request(&mut self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let id = req.id.clone().unwrap_or(serde_json::Value::Null);

        match req.method.as_str() {
            "initialize" => {
                self.initialized = true;
                info!("MCP client initialized");
                Some(JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(serde_json::json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "rupoo",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    })),
                    error: None,
                })
            }
            "notifications/initialized" => None,
            "tools/list" => {
                if !self.initialized {
                    return Some(self.error_response(id, -32000, "Not initialized"));
                }
                let tools = self.build_tool_descriptions().await;

                Some(JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(serde_json::json!({ "tools": tools })),
                    error: None,
                })
            }
            "tools/call" => {
                if !self.initialized {
                    return Some(self.error_response(id, -32000, "Not initialized"));
                }
                let params = req.params.unwrap_or_default();
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::Value::Object(Default::default()));

                // Delegate to McpToolExecutor (which applies path_jail + safety checks)
                match self.executor.execute_tool(&name, arguments).await {
                    Ok(mcp_result) => {
                        let text = if mcp_result.success {
                            mcp_result.content
                        } else {
                            format!("Error: {}", mcp_result.error.unwrap_or_default())
                        };
                        Some(JsonRpcResponse {
                            jsonrpc: "2.0",
                            id,
                            result: Some(serde_json::json!({
                                "content": [{"type": "text", "text": text}]
                            })),
                            error: None,
                        })
                    }
                    Err(e) => Some(JsonRpcResponse {
                        jsonrpc: "2.0",
                        id,
                        result: Some(serde_json::json!({
                            "content": [{"type": "text", "text": format!("Error: {e}")}],
                            "isError": true
                        })),
                        error: None,
                    }),
                }
            }
            _ => Some(self.error_response(
                id,
                -32601,
                &format!("Method not found: '{}'", req.method),
            )),
        }
    }

    fn error_response(&self, id: serde_json::Value, code: i64, message: &str) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
                data: None,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run the MCP server loop (reads JSON-RPC from stdin, writes to stdout).
pub async fn run_mcp_server() -> AgentResult<()> {
    info!("MCP server starting on stdio");

    let safety_ctx = {
        let config_path = std::path::Path::new("rupoo-config.toml");
        if config_path.exists() {
            SafetyContext::from_config(config_path)
        } else {
            SafetyContext::default()
        }
    };

    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    let mut server = McpServer::new(safety_ctx);

    while let Some(line) = lines.next_line().await.map_err(|e| {
        AgentError::Mcp(format!("stdin read error: {e}"))
    })? {
        if line.trim().is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: serde_json::Value::Null,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {e}"),
                        data: None,
                    }),
                };
                write_response(&err_resp).await;
                continue;
            }
        };

        if let Some(resp) = server.handle_request(req).await {
            write_response(&resp).await;
        }
    }

    info!("MCP server shutting down (stdin closed)");
    Ok(())
}

async fn write_response(resp: &JsonRpcResponse) {
    let json = serde_json::to_string(resp).unwrap_or_default();
    let stdout = tokio::io::stdout();
    let mut writer = BufWriter::new(stdout);
    if let Err(e) = writer.write_all(format!("{json}\n").as_bytes()).await {
        error!(error = %e, "failed to write MCP response");
    }
    let _ = writer.flush().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_server() -> McpServer {
        McpServer::new(SafetyContext::default())
    }

    #[tokio::test]
    async fn test_tool_list_requires_init() {
        let mut server = make_server();
        let req = JsonRpcRequest {
            id: Some(serde_json::json!(1)),
            method: "tools/list".into(),
            params: None,
        };
        let resp = server.handle_request(req).await;
        assert!(resp.is_some());
        let r = resp.unwrap();
        assert!(r.error.is_some());
        assert_eq!(r.error.unwrap().code, -32000);
    }

    #[tokio::test]
    async fn test_initialize_then_list() {
        let mut server = make_server();

        let init = JsonRpcRequest {
            id: Some(serde_json::json!(1)),
            method: "initialize".into(),
            params: None,
        };
        let resp = server.handle_request(init).await;
        assert!(resp.is_some());
        assert!(resp.unwrap().result.is_some());

        let list = JsonRpcRequest {
            id: Some(serde_json::json!(2)),
            method: "tools/list".into(),
            params: None,
        };
        let resp = server.handle_request(list).await;
        assert!(resp.is_some());
        let r = resp.unwrap();
        let result_val = r.result.unwrap();
        let tools = result_val["tools"].as_array().unwrap();
        assert!(!tools.is_empty());
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"file_read"));
        // web_search should now be included
        assert!(names.contains(&"web_search"));
    }

    #[tokio::test]
    async fn test_tool_call_echo() {
        let mut server = make_server();

        server.handle_request(JsonRpcRequest {
            id: Some(serde_json::json!(1)),
            method: "initialize".into(),
            params: None,
        }).await;

        let call = JsonRpcRequest {
            id: Some(serde_json::json!(2)),
            method: "tools/call".into(),
            params: Some(serde_json::json!({
                "name": "echo",
                "arguments": {"message": "hello mcp"}
            })),
        };
        let resp = server.handle_request(call).await;
        assert!(resp.is_some());
        let r = resp.unwrap();
        let result_val = r.result.unwrap();
        let content = result_val["content"].as_array().unwrap();
        assert_eq!(content[0]["text"].as_str().unwrap(), "echo: hello mcp");
    }

    #[tokio::test]
    async fn test_unknown_method() {
        let mut server = make_server();
        let req = JsonRpcRequest {
            id: Some(serde_json::json!(1)),
            method: "unknown_method".into(),
            params: None,
        };
        let resp = server.handle_request(req).await;
        assert!(resp.is_some());
        assert!(resp.unwrap().error.is_some());
    }

    #[tokio::test]
    async fn test_file_jail_applied() {
        let mut server = make_server();

        server.handle_request(JsonRpcRequest {
            id: Some(serde_json::json!(1)),
            method: "initialize".into(),
            params: None,
        }).await;

        // file_read with path traversal should be rejected
        let call = JsonRpcRequest {
            id: Some(serde_json::json!(2)),
            method: "tools/call".into(),
            params: Some(serde_json::json!({
                "name": "file_read",
                "arguments": {"path": "../../../etc/passwd"}
            })),
        };
        let resp = server.handle_request(call).await;
        assert!(resp.is_some());
        // McpToolExecutor applies path_jail, so traversal is blocked.
        let r = resp.unwrap();
        let result_val = r.result.unwrap();
        let content = result_val["content"].as_array().unwrap();
        let text = content[0]["text"].as_str().unwrap();
        assert!(text.contains("blocked") || text.contains("denied") || text.contains("Error"),
            "expected path to be blocked, got: {text}");
    }
}
