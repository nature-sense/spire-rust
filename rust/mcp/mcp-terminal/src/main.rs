//! mcp-terminal — MCP server for PTY-based interactive terminal sessions.
//!
//! Tools:
//!   - terminal_spawn    Start a new terminal session (PTY + shell)
//!   - terminal_write    Write data to a terminal session
//!   - terminal_read     Read pending output from a terminal session
//!   - terminal_resize   Resize a terminal session's dimensions
//!   - terminal_kill     Terminate a terminal session
//!   - terminal_list     List active terminal sessions

use async_trait::async_trait;
use rust_mcp_schema::schema_utils::CallToolError;
use rust_mcp_schema::{
    CallToolRequestParams, CallToolResult, ListToolsResult, PaginatedRequestParams, RpcError,
    Tool, ToolInputSchema,
};
use rust_mcp_sdk::mcp_server::ServerHandler;
use rust_mcp_sdk::schema::{
    Implementation, InitializeResult, ProtocolVersion, ServerCapabilities,
    ServerCapabilitiesTools,
};
use rust_mcp_sdk::{
    mcp_server::{server_runtime, McpServerOptions},
    McpServer, StdioTransport, ToMcpServerHandler, TransportOptions,
};
use std::collections::BTreeMap;
use std::sync::Arc;

mod terminal_manager;
use terminal_manager::TerminalManager;

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

struct TerminalHandler {
    manager: TerminalManager,
}

impl TerminalHandler {
    fn new() -> Self {
        Self {
            manager: TerminalManager::new(),
        }
    }
}

#[async_trait]
impl ServerHandler for TerminalHandler {
    async fn handle_list_tools_request(
        &self,
        _request: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> Result<ListToolsResult, RpcError> {
        Ok(ListToolsResult {
            tools: vec![
                Tool {
                    name: "terminal_spawn".into(),
                    description: Some(
                        "Start a new interactive terminal session with a PTY (pseudo-terminal). \
                        Spawns a shell (default: /bin/zsh) connected to a POSIX PTY. \
                        Returns a session_id for subsequent read/write/resize/kill operations."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec![],
                        Some(BTreeMap::from([
                            (
                                "shell".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Shell to spawn (default: /bin/zsh)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "cwd".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Working directory for the shell"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "env".to_string(),
                                serde_json::json!({
                                    "type": "object",
                                    "additionalProperties": { "type": "string" },
                                    "description": "Environment variables"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "cols".to_string(),
                                serde_json::json!({
                                    "type": "integer",
                                    "description": "Terminal width in columns (default: 80)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "rows".to_string(),
                                serde_json::json!({
                                    "type": "integer",
                                    "description": "Terminal height in rows (default: 24)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                        ])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "terminal_write".into(),
                    description: Some(
                        "Write data to a terminal session's stdin (via the PTY master). \
                        The data is sent directly to the shell running in the session. \
                        Include a trailing newline to execute a command."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["session_id".to_string(), "input".to_string()],
                        Some(BTreeMap::from([
                            (
                                "session_id".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Session ID from terminal_spawn"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "input".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Data to write to the terminal"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                        ])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "terminal_read".into(),
                    description: Some(
                        "Read pending output from a terminal session. \
                        This is non-blocking — returns whatever data is currently available \
                        in the PTY output buffer. Use max_bytes to limit the response size. \
                        The output includes raw terminal escape sequences."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["session_id".to_string()],
                        Some(BTreeMap::from([
                            (
                                "session_id".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Session ID from terminal_spawn"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "max_bytes".to_string(),
                                serde_json::json!({
                                    "type": "integer",
                                    "description": "Maximum bytes to read (default: 65536)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                        ])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "terminal_resize".into(),
                    description: Some(
                        "Resize a terminal session's dimensions (cols x rows). \
                        This sends a TIOCSWINSZ ioctl to the PTY master, which causes \
                        the shell to receive SIGWINCH and update its terminal size."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["session_id".to_string(), "cols".to_string(), "rows".to_string()],
                        Some(BTreeMap::from([
                            (
                                "session_id".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Session ID from terminal_spawn"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "cols".to_string(),
                                serde_json::json!({
                                    "type": "integer",
                                    "description": "New terminal width in columns"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "rows".to_string(),
                                serde_json::json!({
                                    "type": "integer",
                                    "description": "New terminal height in rows"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                        ])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "terminal_kill".into(),
                    description: Some(
                        "Terminate a terminal session by sending SIGTERM to the shell process."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["session_id".to_string()],
                        Some(BTreeMap::from([
                            (
                                "session_id".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Session ID from terminal_spawn"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                        ])),
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "terminal_list".into(),
                    description: Some(
                        "List all active terminal sessions with their status and metadata."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec![],
                        None,
                        None,
                    ),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
            ],
            meta: None,
            next_cursor: None,
        })
    }

    async fn handle_call_tool_request(
        &self,
        params: CallToolRequestParams,
        _runtime: Arc<dyn McpServer>,
    ) -> Result<CallToolResult, CallToolError> {
        let args = params.arguments.unwrap_or_default();

        match params.name.as_str() {
            "terminal_spawn" => {
                let shell = args
                    .get("shell")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let cwd = args
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let env = args
                    .get("env")
                    .and_then(|v| v.as_object())
                    .map(|obj| {
                        obj.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect::<std::collections::HashMap<String, String>>()
                    });
                let cols = args
                    .get("cols")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u16);
                let rows = args
                    .get("rows")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u16);

                let result = self.manager.spawn(shell, cwd, env, cols, rows).await;

                match result {
                    Ok(spawn_result) => {
                        let text = serde_json::to_string_pretty(&spawn_result)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e
                        }).to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "terminal_write" => {
                let session_id = args
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing required argument: session_id"))
                    })?;
                let input = args
                    .get("input")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing required argument: input"))
                    })?;

                match self.manager.write(session_id, input).await {
                    Ok(()) => {
                        let text = serde_json::json!({
                            "success": true,
                            "message": format!("Written {} bytes to session {}", input.len(), session_id)
                        }).to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e
                        }).to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "terminal_read" => {
                let session_id = args
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing required argument: session_id"))
                    })?;
                let max_bytes = args
                    .get("max_bytes")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize);

                match self.manager.read(session_id, max_bytes).await {
                    Ok(read_result) => {
                        let text = serde_json::to_string_pretty(&read_result)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e
                        }).to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "terminal_resize" => {
                let session_id = args
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing required argument: session_id"))
                    })?;
                let cols = args
                    .get("cols")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing required argument: cols"))
                    })? as u16;
                let rows = args
                    .get("rows")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing required argument: rows"))
                    })? as u16;

                match self.manager.resize(session_id, cols, rows).await {
                    Ok(()) => {
                        let text = serde_json::json!({
                            "success": true,
                            "message": format!("Resized session {} to {}x{}", session_id, cols, rows)
                        }).to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e
                        }).to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "terminal_kill" => {
                let session_id = args
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing required argument: session_id"))
                    })?;

                match self.manager.kill(session_id).await {
                    Ok(()) => {
                        let text = serde_json::json!({
                            "success": true,
                            "message": format!("Terminated session {}", session_id)
                        }).to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e
                        }).to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "terminal_list" => {
                let result = self.manager.list().await;
                let text = serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                Ok(CallToolResult::text_content(vec![
                    rust_mcp_schema::TextContent::new(text, None, None),
                ]))
            }
            _ => Err(CallToolError::unknown_tool(params.name)),
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();

    let server_details = InitializeResult {
        server_info: Implementation {
            name: "mcp-terminal".into(),
            version: "1.0.0".into(),
            description: Some("MCP server for PTY-based interactive terminal sessions".into()),
            icons: vec![],
            title: Some("Terminal MCP Server".into()),
            website_url: None,
        },
        capabilities: ServerCapabilities {
            tools: Some(ServerCapabilitiesTools { list_changed: None }),
            ..Default::default()
        },
        protocol_version: ProtocolVersion::V2025_11_25.into(),
        instructions: None,
        meta: None,
    };

    let transport = StdioTransport::new(TransportOptions::default())?;
    let handler = TerminalHandler::new().to_mcp_server_handler();
    let server = server_runtime::create_server(McpServerOptions {
        transport,
        handler,
        server_details,
        task_store: None,
        client_task_store: None,
        message_observer: None,
    });

    server.start().await?;
    Ok(())
}
