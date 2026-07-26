//! mcp-search — MCP server for grep-like content search across files.
//!
//! Tools:
//!   - search_files    Search for a pattern across files in a directory

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

mod search_engine;
use search_engine::SearchEngine;

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

struct SearchHandler {
    engine: SearchEngine,
}

impl SearchHandler {
    fn new() -> Self {
        Self {
            engine: SearchEngine::new(),
        }
    }
}

#[async_trait]
impl ServerHandler for SearchHandler {
    async fn handle_list_tools_request(
        &self,
        _request: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> Result<ListToolsResult, RpcError> {
        Ok(ListToolsResult {
            tools: vec![Tool {
                name: "search_files".into(),
                description: Some(
                    "Search for a pattern across files in a directory.".into(),
                ),
                input_schema: ToolInputSchema::new(
                    vec!["pattern".to_string(), "root_path".to_string()],
                    Some(BTreeMap::from([
                        (
                            "pattern".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Search pattern (plain text or regex)"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        ),
                        (
                            "root_path".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Root directory or file to search"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        ),
                        (
                            "is_regex".to_string(),
                            serde_json::json!({
                                "type": "boolean",
                                "description": "Treat pattern as regex (default: false)"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        ),
                        (
                            "case_sensitive".to_string(),
                            serde_json::json!({
                                "type": "boolean",
                                "description": "Case-sensitive search (default: false)"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        ),
                        (
                            "context_lines".to_string(),
                            serde_json::json!({
                                "type": "integer",
                                "description": "Number of context lines around matches (default: 0)"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        ),
                        (
                            "max_results".to_string(),
                            serde_json::json!({
                                "type": "integer",
                                "description": "Maximum number of results (default: 100)"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        ),
                        (
                            "include".to_string(),
                            serde_json::json!({
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Glob patterns to include (e.g. [\"**/*.rs\"])"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        ),
                        (
                            "exclude".to_string(),
                            serde_json::json!({
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Glob patterns to exclude (e.g. [\"**/node_modules/**\"])"
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
            }],
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
            "search_files" => {
                let pattern = args
                    .get("pattern")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing required argument: pattern"))
                    })?;
                let root_path = args
                    .get("root_path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CallToolError::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing required argument: root_path"))
                    })?;
                let is_regex = args
                    .get("is_regex")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let case_sensitive = args
                    .get("case_sensitive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let context_lines = args
                    .get("context_lines")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                let max_results = args
                    .get("max_results")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(100) as usize;
                let include = args
                    .get("include")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<String>>()
                    });
                let exclude = args
                    .get("exclude")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<String>>()
                    });

                let result = self.engine.search(
                    pattern,
                    root_path,
                    is_regex,
                    case_sensitive,
                    context_lines,
                    max_results,
                    include,
                    exclude,
                );

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

/// Initialise tracing for an MCP server.
///
/// If `SPIRE_LOG_DIR` is set, logs are written to a file at
/// `$SPIRE_LOG_DIR/<server_name>/<server_name>.log.YYYY-MM-DD`.
/// Otherwise, logs go to stderr (the default).
///
/// Leaks a `WorkerGuard` so the non-blocking writer thread lives for the
/// entire process lifetime. Without this, the guard is dropped when the
/// function returns and no log output is ever written to the file.
fn init_mcp_logging(server_name: &str) {
    if let Ok(log_dir) = std::env::var("SPIRE_LOG_DIR") {
        let server_log_dir = std::path::PathBuf::from(&log_dir).join(server_name);
        let _ = std::fs::create_dir_all(&server_log_dir);

        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let log_path = server_log_dir.join(format!("{}.log.{}", server_name, date));

        let log_file = std::fs::File::create(&log_path)
            .expect("Failed to create MCP log file");
        let (non_blocking, guard) = tracing_appender::non_blocking::NonBlockingBuilder::default()
            .buffered_lines_limit(128)
            .finish(log_file);

        // Leak the guard so the non-blocking writer thread lives for the
        // entire process lifetime.
        Box::leak(Box::new(guard));

        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info".into()),
            )
            .with_writer(non_blocking)
            .with_ansi(false)
            .with_target(false)
            .init();

        eprintln!("[{}] Logging to: {}", server_name, log_path.display());
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info".into()),
            )
            .with_target(false)
            .init();
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_mcp_logging("mcp-search");

    let server_details = InitializeResult {
        server_info: Implementation {
            name: "mcp-search".into(),
            version: "1.0.0".into(),
            description: Some("MCP server for file content search".into()),
            icons: vec![],
            title: Some("Search MCP Server".into()),
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
    let handler = SearchHandler::new().to_mcp_server_handler();
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
