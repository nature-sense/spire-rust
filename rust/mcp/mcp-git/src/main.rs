//! mcp-git — MCP server for git operations.
//!
//! Tools:
//!   - git_status     Show working tree status
//!   - git_diff       Show diff of working tree changes
//!   - git_log        Show commit log
//!   - git_add        Stage files
//!   - git_commit     Create a commit
//!   - git_branch     List / create branches
//!   - git_checkout   Switch branches or restore files
//!   - git_pull       Pull from remote
//!   - git_push       Push to remote

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
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

mod git_ops;
use git_ops::GitOperations;

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

struct GitHandler {
    ops: GitOperations,
}

impl GitHandler {
    fn new(repo_path: Option<String>) -> Self {
        Self {
            ops: GitOperations::new(repo_path),
        }
    }
}

#[async_trait]
impl ServerHandler for GitHandler {
    async fn handle_list_tools_request(
        &self,
        _request: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> Result<ListToolsResult, RpcError> {
        Ok(ListToolsResult {
            tools: vec![
                Tool {
                    name: "git_status".into(),
                    description: Some("Show the working tree status.".into()),
                    input_schema: ToolInputSchema::new(vec![], None, None),
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: None,
                },
                Tool {
                    name: "git_diff".into(),
                    description: Some("Show diff of working tree changes.".into()),
                    input_schema: ToolInputSchema::new(
                        vec![],
                        Some(BTreeMap::from([(
                            "files".to_string(),
                            serde_json::json!({
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Files to include in diff (default: all)"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        )])),
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
                    name: "git_log".into(),
                    description: Some("Show commit log.".into()),
                    input_schema: ToolInputSchema::new(
                        vec![],
                        Some(BTreeMap::from([
                            (
                                "limit".to_string(),
                                serde_json::json!({
                                    "type": "integer",
                                    "description": "Number of commits to show (default: 10)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "file".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Filter commits by file path"
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
                    name: "git_add".into(),
                    description: Some("Stage files for commit.".into()),
                    input_schema: ToolInputSchema::new(
                        vec![],
                        Some(BTreeMap::from([(
                            "files".to_string(),
                            serde_json::json!({
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Files to stage (default: [\".\"])"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        )])),
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
                    name: "git_commit".into(),
                    description: Some("Create a commit.".into()),
                    input_schema: ToolInputSchema::new(
                        vec!["message".to_string()],
                        Some(BTreeMap::from([(
                            "message".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Commit message"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        )])),
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
                    name: "git_branch".into(),
                    description: Some("List branches or create a new branch.".into()),
                    input_schema: ToolInputSchema::new(
                        vec![],
                        Some(BTreeMap::from([(
                            "branch".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Branch name to create (omit to list branches)"
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        )])),
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
                    name: "git_checkout".into(),
                    description: Some("Switch branches or restore files.".into()),
                    input_schema: ToolInputSchema::new(
                        vec![],
                        Some(BTreeMap::from([
                            (
                                "branch".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Branch to switch to"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "files".to_string(),
                                serde_json::json!({
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Files to restore"
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
                    name: "git_pull".into(),
                    description: Some("Pull from remote.".into()),
                    input_schema: ToolInputSchema::new(
                        vec![],
                        Some(BTreeMap::from([
                            (
                                "remote".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Remote name (default: origin)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "branch".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Branch to pull (default: main)"
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
                    name: "git_push".into(),
                    description: Some("Push to remote.".into()),
                    input_schema: ToolInputSchema::new(
                        vec![],
                        Some(BTreeMap::from([
                            (
                                "remote".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Remote name (default: origin)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "branch".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Branch to push (default: main)"
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

        let operation = match params.name.as_str() {
            "git_status" => "status",
            "git_diff" => "diff",
            "git_log" => "log",
            "git_add" => "add",
            "git_commit" => "commit",
            "git_branch" => "branch",
            "git_checkout" => "checkout",
            "git_pull" => "pull",
            "git_push" => "push",
            _ => return Err(CallToolError::unknown_tool(params.name)),
        };

        let result = self.ops.execute(operation, Some(HashMap::from_iter(args))).await;

        let text = serde_json::to_string_pretty(&result).unwrap_or_else(|e| {
            format!("{{\"error\": \"Serialization failed: {}\"}}", e)
        });

        Ok(CallToolResult::text_content(vec![
            rust_mcp_schema::TextContent::new(text, None, None),
        ]))
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
    init_mcp_logging("mcp-git");

    let server_details = InitializeResult {
        server_info: Implementation {
            name: "mcp-git".into(),
            version: "1.0.0".into(),
            description: Some("MCP server for git operations".into()),
            icons: vec![],
            title: Some("Git MCP Server".into()),
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
    let handler = GitHandler::new(None).to_mcp_server_handler();
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
