//! mcp-node — MCP server for Node.js/npm/pnpm/Yarn project analysis and building.
//!
//! Tools:
//!   - analyze        Parse package.json and return structured BuildMetadata
//!   - build          Run `npm run build` / `pnpm build` / `yarn build`
//!   - test           Run `npm test` / `pnpm test` / `yarn test`
//!   - install        Run `npm install` / `pnpm install` / `yarn install`
//!   - add_dependency Add a dependency via `npm install <pkg>` / `pnpm add <pkg>`
//!   - run_script     Run an arbitrary npm script

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
use tracing::info;

mod analyze;
mod build;

use analyze::analyze_node_project;
use build::{run_node_command, run_node_script};

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

struct NodeHandler;

#[async_trait]
impl ServerHandler for NodeHandler {
    async fn handle_list_tools_request(
        &self,
        _request: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> Result<ListToolsResult, RpcError> {
        Ok(ListToolsResult {
            tools: vec![
                Tool {
                    name: "_build_system".into(),
                    description: Some(
                        "Self-description: returns metadata about this build MCP server, \
                        including supported build systems, capabilities, and config file patterns."
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
                Tool {
                    name: "describe_analysis_capabilities".into(),
                    description: Some(
                        "Describe what build files this server can analyze. \
                        Returns a list of supported file patterns and analysis metadata."
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
                Tool {
                    name: "analyze".into(),
                    description: Some(
                        "Parse a package.json and return structured build metadata including \
                        project name, version, scripts, dependencies, and workspace members. \
                        Auto-detects npm, pnpm, or Yarn as the package manager."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([(
                            "path".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Absolute path to the Node.js project directory (containing package.json)"
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
                    name: "build".into(),
                    description: Some(
                        "Run the build script for a Node.js project. Auto-detects the package manager \
                        (npm, pnpm, yarn) and runs the appropriate build command. \
                        Returns structured results with errors, warnings, and output."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([
                            (
                                "path".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Absolute path to the Node.js project directory"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "script".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Script name to run (default: \"build\")"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "package_manager".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Package manager to use: \"npm\", \"pnpm\", \"yarn\", or \"auto\" (default)",
                                    "enum": ["auto", "npm", "pnpm", "yarn"]
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "package".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Optional package/workspace name to build (e.g. pnpm --filter=<package> build)"
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
                    name: "test".into(),
                    description: Some(
                        "Run the test script for a Node.js project. Auto-detects the package manager."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([
                            (
                                "path".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Absolute path to the Node.js project directory"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "package_manager".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Package manager to use: \"npm\", \"pnpm\", \"yarn\", or \"auto\" (default)",
                                    "enum": ["auto", "npm", "pnpm", "yarn"]
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
                    name: "install".into(),
                    description: Some(
                        "Install dependencies for a Node.js project. Auto-detects the package manager \
                        and runs the appropriate install command."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([
                            (
                                "path".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Absolute path to the Node.js project directory"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "package_manager".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Package manager to use: \"npm\", \"pnpm\", \"yarn\", or \"auto\" (default)",
                                    "enum": ["auto", "npm", "pnpm", "yarn"]
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
                    name: "add_dependency".into(),
                    description: Some(
                        "Add a dependency to a Node.js project. Auto-detects the package manager \
                        and runs the appropriate add command."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string(), "package".to_string()],
                        Some(BTreeMap::from([
                            (
                                "path".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Absolute path to the Node.js project directory"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "package".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Package name to install (e.g. \"lodash\", \"express@4\")"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "dev".to_string(),
                                serde_json::json!({
                                    "type": "boolean",
                                    "description": "Install as dev dependency (default: false)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "package_manager".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Package manager to use: \"npm\", \"pnpm\", \"yarn\", or \"auto\" (default)",
                                    "enum": ["auto", "npm", "pnpm", "yarn"]
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
                    name: "run_script".into(),
                    description: Some(
                        "Run an arbitrary npm script by name. Auto-detects the package manager."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string(), "script".to_string()],
                        Some(BTreeMap::from([
                            (
                                "path".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Absolute path to the Node.js project directory"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "script".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Name of the script to run (e.g. \"lint\", \"format\", \"start\")"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "package_manager".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Package manager to use: \"npm\", \"pnpm\", \"yarn\", or \"auto\" (default)",
                                    "enum": ["auto", "npm", "pnpm", "yarn"]
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
            "_build_system" => {
                let info = serde_json::json!({
                    "name": "mcp-node",
                    "build_systems": ["npm", "pnpm", "yarn"],
                    "build_type": "node",
                    "capabilities": ["build", "test", "install", "add_dependency", "run_script"],
                    "analyzer_tool": "analyze",
                    "config_files": ["package.json", "pnpm-workspace.yaml"]
                });
                let text = serde_json::to_string_pretty(&info)
                    .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                Ok(CallToolResult::text_content(vec![
                    rust_mcp_schema::TextContent::new(text, None, None),
                ]))
            }

            "describe_analysis_capabilities" => {
                let capabilities = serde_json::json!({
                    "server_name": "mcp-node",
                    "supported_files": ["package.json", "pnpm-workspace.yaml"],
                    "project_type": "node_package",
                    "build_system": "npm",
                    "analyzer_tool": "analyze",
                    "confidence": 0.95
                });
                let text = serde_json::to_string_pretty(&capabilities)
                    .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                Ok(CallToolResult::text_content(vec![
                    rust_mcp_schema::TextContent::new(text, None, None),
                ]))
            }

            "analyze" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;
                info!("Tool called: analyze on {}", path);

                match analyze_node_project(path) {
                    Ok(metadata) => {
                        let text = serde_json::to_string_pretty(&metadata)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }

            "build" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;
                let script = args.get("script").and_then(|v| v.as_str()).unwrap_or("build");
                let pm = args.get("package_manager").and_then(|v| v.as_str()).unwrap_or("auto");
                let package = args.get("package").and_then(|v| v.as_str());

                // If a package filter is specified, use it with the appropriate flag
                let result = if let Some(_pkg) = package {
                    // For filtered builds, we run the script with the filter flag
                    // e.g. `pnpm --filter=<package> run build`
                    // TODO: pass filter flag to run_node_script when it supports it
                    run_node_script(path, script, pm).await
                } else {
                    run_node_script(path, script, pm).await
                };


                match result {

                    Ok(output) => {
                        let text = serde_json::to_string_pretty(&output)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }

            "test" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;
                let pm = args.get("package_manager").and_then(|v| v.as_str()).unwrap_or("auto");

                match run_node_script(path, "test", pm).await {
                    Ok(output) => {
                        let text = serde_json::to_string_pretty(&output)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }

            "install" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;
                let pm = args.get("package_manager").and_then(|v| v.as_str()).unwrap_or("auto");

                match run_node_command(path, &[], pm).await {
                    Ok(output) => {
                        let text = serde_json::to_string_pretty(&output)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }

            "add_dependency" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;
                let package = args.get("package").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: package",
                    ))
                })?;
                let dev = args.get("dev").and_then(|v| v.as_bool()).unwrap_or(false);
                let pm = args.get("package_manager").and_then(|v| v.as_str()).unwrap_or("auto");

                let mut extra_args = Vec::new();
                if dev {
                    match pm {
                        "pnpm" => extra_args.push("-D"),
                        "yarn" => extra_args.push("--dev"),
                        _ => extra_args.push("--save-dev"),
                    }
                }
                extra_args.push(package);

                match run_node_command(path, &extra_args, pm).await {
                    Ok(output) => {
                        let text = serde_json::to_string_pretty(&output)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }

            "run_script" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;
                let script = args.get("script").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: script",
                    ))
                })?;
                let pm = args.get("package_manager").and_then(|v| v.as_str()).unwrap_or("auto");

                match run_node_script(path, script, pm).await {
                    Ok(output) => {
                        let text = serde_json::to_string_pretty(&output)
                            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                    Err(e) => {
                        let text = serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        })
                        .to_string();
                        Ok(CallToolResult::text_content(vec![
                            rust_mcp_schema::TextContent::new(text, None, None),
                        ]))
                    }
                }
            }

            _ => Err(CallToolError::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Unknown tool: {}", params.name),
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
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
    init_mcp_logging("mcp-node");

    let server_details = InitializeResult {
        server_info: Implementation {
            name: "mcp-node".into(),
            version: "1.0.0".into(),
            description: Some("MCP server for Node.js/npm/pnpm/Yarn project analysis and building".into()),
            icons: vec![],
            title: Some("Node MCP Server".into()),
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
    let handler = NodeHandler.to_mcp_server_handler();
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
