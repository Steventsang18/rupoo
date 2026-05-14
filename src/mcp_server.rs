//! MCP (Model Context Protocol) server for Yupoo.
//!
//! Exposes Yupoo's built-in tools via the standard MCP protocol over stdio,
//! allowing any MCP client (Claude Desktop, Cursor, etc.) to discover and
//! call Yupoo tools.
//!
//! # Protocol
//! - Transport: stdio (JSON-RPC 2.0, one object per line)
//! - Methods: `initialize`, `tools/list`, `tools/call`
//! - Spec: https://spec.modelcontextprotocol.io
//!
//! # Usage
//! ```bash
//! yupoo mcp-server
//! ```
//! Then configure your MCP client to spawn this as a subprocess.

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tracing::{error, info};

use crate::error::{AgentError, AgentResult};

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
// Tool registry
// ---------------------------------------------------------------------------

/// A registered tool in the MCP server.
struct McpToolEntry {
    name: &'static str,
    description: &'static str,
    parameters: serde_json::Value,
    handler: Box<dyn Fn(serde_json::Value) -> AgentResult<String> + Send + Sync>,
}

struct McpServer {
    tools: Vec<McpToolEntry>,
    initialized: bool,
}

impl McpServer {
    fn new() -> Self {
        Self {
            tools: Self::builtin_tools(),
            initialized: false,
        }
    }

    fn builtin_tools() -> Vec<McpToolEntry> {
        vec![
            McpToolEntry {
                name: "echo",
                description: "Echo back a message. Useful for testing.",
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
                handler: Box::new(|params| {
                    let msg = params.get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    Ok(format!("echo: {msg}"))
                }),
            },
            McpToolEntry {
                name: "file_read",
                description: "Read the contents of a file at the given path.",
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
                handler: Box::new(|params| {
                    let path = params.get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    match std::fs::read_to_string(path) {
                        Ok(content) => {
                            let end = content.floor_char_boundary(4096);
                            if end < content.len() {
                                Ok(format!("{}...(truncated {end} bytes)", &content[..end]))
                            } else {
                                Ok(content)
                            }
                        }
                        Err(e) => Err(AgentError::Other(format!("cannot read '{}': {e}", path))),
                    }
                }),
            },
            McpToolEntry {
                name: "file_write",
                description: "Write content to a file. Overwrites existing content.",
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
                handler: Box::new(|params| {
                    let path = params.get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let content = params.get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    match std::fs::write(path, content) {
                        Ok(()) => Ok(format!("wrote {} bytes to '{}'", content.len(), path)),
                        Err(e) => Err(AgentError::Other(format!("cannot write '{}': {e}", path))),
                    }
                }),
            },
            McpToolEntry {
                name: "list_directory",
                description: "List entries in a directory.",
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
                handler: Box::new(|params| {
                    let path = params.get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or(".");
                    match std::fs::read_dir(path) {
                        Ok(entries) => {
                            let mut listing = String::new();
                            for entry in entries.flatten() {
                                let name = entry.file_name().to_string_lossy().to_string();
                                let kind = if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                                    "dir"
                                } else {
                                    "file"
                                };
                                listing.push_str(&format!("  [{kind}] {name}\n"));
                            }
                            if listing.is_empty() {
                                Ok("(empty directory)".into())
                            } else {
                                Ok(listing)
                            }
                        }
                        Err(e) => Err(AgentError::Other(format!("cannot list '{}': {e}", path))),
                    }
                }),
            },
        ]
    }

    /// Handle a JSON-RPC request and return an optional response.
    fn handle_request(&mut self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
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
                            "version": "0.1.0"
                        }
                    })),
                    error: None,
                })
            }
            "notifications/initialized" => {
                // Notification: no response expected
                None
            }
            "tools/list" => {
                if !self.initialized {
                    return Some(self.error_response(id, -32000, "Not initialized"));
                }
                let tools: Vec<McpToolDescription> = self
                    .tools
                    .iter()
                    .map(|t| McpToolDescription {
                        name: t.name.to_string(),
                        description: t.description.to_string(),
                        input_schema: serde_json::json!({
                            "type": "object",
                            "properties": t.parameters["properties"].clone(),
                            "required": t.parameters["required"].clone(),
                        }),
                    })
                    .collect();

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

                match self.tools.iter().find(|t| t.name == name) {
                    Some(tool) => {
                        match (tool.handler)(arguments) {
                            Ok(text) => Some(JsonRpcResponse {
                                jsonrpc: "2.0",
                                id,
                                result: Some(serde_json::json!({
                                    "content": [{"type": "text", "text": text}]
                                })),
                                error: None,
                            }),
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
                    None => Some(self.error_response(
                        id,
                        -32602,
                        &format!("Unknown tool: '{name}'"),
                    )),
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

    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    let mut server = McpServer::new();

    while let Some(line) = lines.next_line().await.map_err(|e| {
        AgentError::Other(format!("stdin read error: {e}"))
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

        if let Some(resp) = server.handle_request(req) {
            write_response(&resp).await;
        }
    }

    info!("MCP server shutting down (stdin closed)");
    Ok(())
}

async fn write_response(resp: &JsonRpcResponse) {
    let json = serde_json::to_string(resp).unwrap_or_default();
    // Use BufWriter to allow writing to stdout
    let stdout = tokio::io::stdout();
    let mut writer = BufWriter::new(stdout);
    if let Err(e) = writer.write_all(format!("{json}\n").as_bytes()).await {
        error!(error = %e, "failed to write MCP response");
    }
    // Flush so client receives immediately
    let _ = writer.flush().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_list_requires_init() {
        let mut server = McpServer::new();
        let req = JsonRpcRequest {
            id: Some(serde_json::json!(1)),
            method: "tools/list".into(),
            params: None,
        };
        let resp = server.handle_request(req);
        assert!(resp.is_some());
        let r = resp.unwrap();
        assert!(r.error.is_some());
        assert_eq!(r.error.unwrap().code, -32000);
    }

    #[test]
    fn test_initialize_then_list() {
        let mut server = McpServer::new();

        // Initialize
        let init = JsonRpcRequest {
            id: Some(serde_json::json!(1)),
            method: "initialize".into(),
            params: None,
        };
        let resp = server.handle_request(init);
        assert!(resp.is_some());
        assert!(resp.unwrap().result.is_some());

        // List tools
        let list = JsonRpcRequest {
            id: Some(serde_json::json!(2)),
            method: "tools/list".into(),
            params: None,
        };
        let resp = server.handle_request(list);
        assert!(resp.is_some());
        let r = resp.unwrap();
        let result_val = r.result.unwrap();
        let tools = result_val["tools"].as_array().unwrap();
        assert!(!tools.is_empty());
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"file_read"));
    }

    #[test]
    fn test_tool_call_echo() {
        let mut server = McpServer::new();

        // Initialize
        server.handle_request(JsonRpcRequest {
            id: Some(serde_json::json!(1)),
            method: "initialize".into(),
            params: None,
        });

        // Call echo
        let call = JsonRpcRequest {
            id: Some(serde_json::json!(2)),
            method: "tools/call".into(),
            params: Some(serde_json::json!({
                "name": "echo",
                "arguments": {"message": "hello mcp"}
            })),
        };
        let resp = server.handle_request(call);
        assert!(resp.is_some());
        let r = resp.unwrap();
        let result_val = r.result.unwrap();
        let content = result_val["content"].as_array().unwrap();
        assert_eq!(content[0]["text"].as_str().unwrap(), "echo: hello mcp");
    }

    #[test]
    fn test_unknown_method() {
        let mut server = McpServer::new();
        let req = JsonRpcRequest {
            id: Some(serde_json::json!(1)),
            method: "unknown_method".into(),
            params: None,
        };
        let resp = server.handle_request(req);
        assert!(resp.is_some());
        assert!(resp.unwrap().error.is_some());
    }
}
