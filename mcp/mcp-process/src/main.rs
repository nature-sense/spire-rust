//! mcp-process — MCP server for starting, managing, and interacting with long-running processes.
//!
//! Tools:
//!   - process_start     Start a new process
//!   - process_stdin     Send input to a process's stdin
//!   - process_kill      Kill a running process
//!   - process_output    Get captured output from a process

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

mod process_manager;
use process_manager::ProcessManager;

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

struct ProcessHandler {
    manager: ProcessManager,
}

impl ProcessHandler {
    fn new() -> Self {
        Self {
            manager: ProcessManager::new(),
        }
    }
}

#[async_trait]
impl ServerHandler for ProcessHandler {
    async fn handle_list_tools_request(
        &self,
        _request: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> Result<ListToolsResult, RpcError> {
        Ok(ListToolsResult {
            tools: vec![
                Tool {
                    name: "process_start".into(),
                    description: Some(
                        "Start a new process and capture its output.".into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["command".to_string()],
                        Some(BTreeMap::from([
                            (
                                "command".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Command to execute"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "cwd".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Working directory (default: current)"
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
                                "timeout".to_string(),
                                serde_json::json!({
                                    "type": "integer",
                                    "description": "Timeout in milliseconds (0 = no timeout)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "shell".to_string(),
                                serde_json::json!({
                                    "type": "boolean",
                                    "description": "Use shell to execute (default: true)"
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
                    name: "process_stdin".into(),
                    description: Some("Send input to a running process's stdin.".into()),
                    input_schema: ToolInputSchema::new(
                        vec!["process_id".to_string(), "input".to_string()],
                        Some(BTreeMap::from([
                            (
                                "process_id".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Process ID"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "input".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Input text to send"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "newline".to_string(),
                                serde_json::json!({
                                    "type": "boolean",
                                    "description": "Append newline (default: true)"
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
                    name: "process_kill".into(),
                    description: Some("Kill a running process.".into()),
                    input_schema: ToolInputSchema::new(
                        vec!["process_id".to_string()],
                        Some(BTreeMap::from([
                            (
                                "process_id".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Process ID"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "signal".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Signal to send (SIGTERM, SIGKILL, SIGINT)"
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
                    name: "process_output".into(),
                    description: Some("Get captured output from a process.".into()),
                    input_schema: ToolInputSchema::new(
                        vec!["process_id".to_string()],
                        Some(BTreeMap::from([
                            (
                                "process_id".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Process ID"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "tail".to_string(),
                                serde_json::json!({
                                    "type": "integer",
                                    "description": "Number of recent lines to return (default: all)"
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
            "process_start" => {
                let command = args
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing required argument: command"))
                    })?;
                let cwd = args
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let env = args
                    .get("env")
                    .and_then(|v| v.as_object())
                    .map(|obj| {
                        obj.iter()
                            .filter_map(|(k, v)| {
                                v.as_str().map(|s| (k.clone(), s.to_string()))
                            })
                            .collect::<std::collections::HashMap<String, String>>()
                    });
                let timeout = args
                    .get("timeout")
                    .and_then(|v| v.as_u64());
                let shell = args
                    .get("shell")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                let result = self
                    .manager
                    .start_process(command, cwd, env, timeout, shell)
                    .await;

                let text = serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                Ok(CallToolResult::text_content(vec![
                    rust_mcp_schema::TextContent::new(text, None, None),
                ]))
            }
            "process_stdin" => {
                let process_id = args
                    .get("process_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing required argument: process_id"))
                    })?;
                let input = args
                    .get("input")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing required argument: input"))
                    })?;
                let newline = args
                    .get("newline")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                match self.manager.send_stdin(process_id, input, newline).await {
                    Ok(()) => {
                        let text = serde_json::json!({
                            "success": true,
                            "message": format!("Sent input to process {}", process_id)
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "process_kill" => {
                let process_id = args
                    .get("process_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing required argument: process_id"))
                    })?;
                let signal = args
                    .get("signal")
                    .and_then(|v| v.as_str())
                    .unwrap_or("SIGTERM");

                match self.manager.kill_process(process_id, signal).await {
                    Ok(result) => {
                        let text = serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }
            "process_output" => {
                let process_id = args
                    .get("process_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing required argument: process_id"))
                    })?;
                let tail = args
                    .get("tail")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize);

                let output = self.manager.get_output(process_id, tail, None).await;
                let text = serde_json::to_string_pretty(&output)
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
            name: "mcp-process".into(),
            version: "1.0.0".into(),
            description: Some("MCP server for process management".into()),
            icons: vec![],
            title: Some("Process MCP Server".into()),
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
    let handler = ProcessHandler::new().to_mcp_server_handler();
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
