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
