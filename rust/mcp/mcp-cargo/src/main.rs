//! mcp-cargo — MCP server for Rust/Cargo project analysis and building.
//!
//! Tools:
//!   - analyze        Parse Cargo.toml and return structured BuildMetadata
//!   - build          Run `cargo build` with JSON output
//!   - check          Run `cargo check`
//!   - test           Run `cargo test`
//!   - clippy         Run `cargo clippy`
//!   - fmt            Run `cargo fmt`
//!   - add_dependency Add a dependency via `cargo add`

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

mod analyze;
mod build;

use analyze::analyze_cargo_project;
use build::{run_cargo_build, run_cargo_command};

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

struct CargoHandler;

#[async_trait]
impl ServerHandler for CargoHandler {
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
                        "Parse a Cargo.toml and return structured build metadata including \
                        project name, version, dependencies, features, targets, and workspace members. \
                        Uses `cargo metadata` for rich data when available."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([(
                            "path".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Absolute path to the Rust project directory (containing Cargo.toml)"
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
                        "Build a Rust project using `cargo build`. Supports debug/release mode, \
                        workspace scope, target triple, feature flags, and parallel jobs. \
                        Returns structured results with errors, warnings, and artifact paths."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([
                            (
                                "path".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Absolute path to the Rust project directory"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "mode".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Build profile: \"debug\" (default) or \"release\"",
                                    "enum": ["debug", "release"]
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "scope".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Build scope: \"package\" (default) or \"workspace\"",
                                    "enum": ["package", "workspace"]
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "target".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Optional target triple for cross-compilation (e.g. \"x86_64-unknown-linux-gnu\")"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "features".to_string(),
                                serde_json::json!({
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Optional list of feature flags to enable"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "jobs".to_string(),
                                serde_json::json!({
                                    "type": "integer",
                                    "description": "Number of parallel jobs (default: number of CPU cores)"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "package".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Optional package name to build (cargo build -p <package>). \
                                        Only valid when scope=\"package\"."
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
                    name: "check".into(),
                    description: Some(
                        "Run `cargo check` to quickly verify a project compiles without producing artifacts. \
                        Faster than a full build."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([(
                            "path".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Absolute path to the Rust project directory"
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
                    name: "test".into(),
                    description: Some(
                        "Run `cargo test` to execute unit and integration tests. \
                        Returns structured results with test outcomes."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([(
                            "path".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Absolute path to the Rust project directory"
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
                    name: "clippy".into(),
                    description: Some(
                        "Run `cargo clippy` to lint the project. Returns structured results \
                        with warnings and suggestions."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([(
                            "path".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Absolute path to the Rust project directory"
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
                    name: "fmt".into(),
                    description: Some(
                        "Run `cargo fmt` to format all Rust source files in the project. \
                        Returns the list of files that were formatted."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string()],
                        Some(BTreeMap::from([(
                            "path".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "Absolute path to the Rust project directory"
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
                    name: "add_dependency".into(),
                    description: Some(
                        "Add a dependency to the project via `cargo add`. \
                        Supports specifying version, features, and dependency kind."
                            .into(),
                    ),
                    input_schema: ToolInputSchema::new(
                        vec!["path".to_string(), "crate".to_string()],
                        Some(BTreeMap::from([
                            (
                                "path".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Absolute path to the Rust project directory"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "crate".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Name of the crate to add"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "version".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "Optional version requirement (e.g. \"1.0\", \"^2.5\")"
                                })
                                .as_object()
                                .unwrap()
                                .clone(),
                            ),
                            (
                                "features".to_string(),
                                serde_json::json!({
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Optional list of features to enable"
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
                    "name": "mcp-cargo",
                    "build_systems": ["Cargo"],
                    "build_type": "rust",
                    "capabilities": ["build", "test", "check", "clean", "clippy", "fmt", "add_dependency"],
                    "analyzer_tool": "analyze",
                    "config_files": ["Cargo.toml"]
                });
                let text = serde_json::to_string_pretty(&info)
                    .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
                Ok(CallToolResult::text_content(vec![
                    rust_mcp_schema::TextContent::new(text, None, None),
                ]))
            }

            "describe_analysis_capabilities" => {
                let capabilities = serde_json::json!({
                    "server_name": "mcp-cargo",
                    "supported_files": ["Cargo.toml"],
                    "project_type": "rust_crate",
                    "build_system": "Cargo",
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

                match analyze_cargo_project(path) {
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
                let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("debug");
                let scope = args.get("scope").and_then(|v| v.as_str()).unwrap_or("package");
                let package = args.get("package").and_then(|v| v.as_str());
                let target = args.get("target").and_then(|v| v.as_str()).map(|s| s.to_string());
                let features: Vec<String> = args
                    .get("features")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let jobs = args.get("jobs").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

                match run_cargo_build(path, mode, scope, package, target.as_deref(), &features, jobs).await {

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

            "check" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;

                match run_cargo_command(path, &["check"]).await {
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

                match run_cargo_command(path, &["test"]).await {
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

            "clippy" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;

                match run_cargo_command(path, &["clippy"]).await {
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

            "fmt" => {
                let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: path",
                    ))
                })?;

                match run_cargo_command(path, &["fmt"]).await {
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
                let crate_name = args.get("crate").and_then(|v| v.as_str()).ok_or_else(|| {
                    CallToolError::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Missing required argument: crate",
                    ))
                })?;

                let mut cargo_args = vec!["add".to_string(), crate_name.to_string()];
                if let Some(version) = args.get("version").and_then(|v| v.as_str()) {
                    cargo_args.push("--vers".to_string());
                    cargo_args.push(version.to_string());
                }
                if let Some(features) = args.get("features").and_then(|v| v.as_array()) {
                    let feat_str: Vec<String> = features
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                    if !feat_str.is_empty() {
                        cargo_args.push("--features".to_string());
                        cargo_args.push(feat_str.join(","));
                    }
                }

                match run_cargo_command(path, &cargo_args.iter().map(|s| s.as_str()).collect::<Vec<_>>()).await {
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
/// Returns a leaked `WorkerGuard` that keeps the non-blocking writer thread
/// alive for the lifetime of the process. Without this, the guard is dropped
/// when the function returns and no log output is ever written to the file.
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
        // entire process lifetime. Without this, the guard is dropped when
        // this function returns and the writer thread terminates immediately.
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
    init_mcp_logging("mcp-cargo");

    let server_details = InitializeResult {
        server_info: Implementation {
            name: "mcp-cargo".into(),
            version: "1.0.0".into(),
            description: Some("MCP server for Rust/Cargo project analysis and building".into()),
            icons: vec![],
            title: Some("Cargo MCP Server".into()),
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
    let handler = CargoHandler.to_mcp_server_handler();
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
