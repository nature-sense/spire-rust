// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Spire Core — standalone binary entry point.
//!
//! This binary runs as a subprocess of the VS Code extension, communicating
//! via JSON-RPC 2.0 over stdin/stdout. It can also be run standalone for testing.
//!
//! Architecture:
//!   VS Code Extension (BidirectionalClient) ←→ spire-core (this binary)
//!     - Extension sends requests to core via stdin
//!     - Core sends responses to extension via stdout
//!     - Core sends requests to extension (for VS Code API) via stdout
//!     - Extension sends responses to core via stdin
//!
//! Usage:
//!   cargo run --bin spire-core           # standalone (testing)
//!   cargo run --bin spire-core -- --mcp  # with MCP client connections

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

use rust_mcp_sdk::schema::Tool;

use spire_core::actors::{
    ActorSystem, ChatActor, ToolsActor, McpClientActor,
    CoordinatorActor, CoordinatorMessage,
    LlmActor, LlmConfig,
    ProgressActor, SystemActor,
    MemoryGraphActor, MemoryGraphMessage,
    vscode_tool_definitions,
};
use spire_core::graph::GraphDb;
use spire_core::embedder::create_embedder;
use spire_core::transport::stdio::Transport;



/// Determine the log directory.
/// Priority: SPIRE_LOG_DIR env var, then ~/.spire/logs, then a temp dir.
fn log_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SPIRE_LOG_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(home) = dirs::home_dir() {
        let dir = home.join(".spire").join("logs");
        // Create the directory if it doesn't exist
        let _ = std::fs::create_dir_all(&dir);
        return dir;
    }
    // Fallback: use a temp directory
    let dir = std::env::temp_dir().join("spire-core-logs");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── Initialise tracing — log to a file (never stdout, which is JSON-RPC) ──
    let log_dir = log_dir();
    let log_file = log_dir.join("spire-core.log");

    // Use tracing-appender for non-blocking file I/O
    let file_appender = tracing_appender::rolling::daily(log_dir.clone(), "spire-core.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(non_blocking)
        .with_ansi(false) // No ANSI escape codes in log files
        .init();

    info!("Spire Core starting...");
    info!("Logging to: {}", log_file.display());

    // ── Initialise SeleneDB graph database ──
    let graph_db = Arc::new(GraphDb::new_in_memory()
        .expect("Failed to create in-memory graph database"));
    info!("SeleneDB graph database initialised (in-memory)");

    // ── Initialise embedding model ──
    let embedder = create_embedder()
        .expect("Failed to create embedding model");
    info!("Embedding model loaded");

    // ── Create the actor system ──
    let system = ActorSystem::new();

    // Spawn the chat actor
    let (chat_tx, _chat_handle) = system.spawn(ChatActor::new());

    // Spawn the tools actor
    let (tools_tx, _tools_handle) = system.spawn(ToolsActor::new());

    // Spawn the MCP client actor
    let (mcp_client_tx, _mcp_client_handle) = system.spawn(McpClientActor::new());

    // Spawn the LLM actor
    let (llm_tx, _llm_handle) = system.spawn(LlmActor::new(LlmConfig::default()));

    // Spawn the progress actor
    let (progress_tx, _progress_handle) = system.spawn(ProgressActor::new());

    // Spawn the system actor
    let (system_tx, _system_handle) = system.spawn(SystemActor::new());

    // Spawn the memory graph actor (knowledge graph + config storage)
    let (memory_graph_tx, _memory_graph_handle) = system.spawn(
        MemoryGraphActor::new(graph_db, embedder),
    );

    // ── Create the JSON-RPC transport ──
    // This must be created BEFORE the coordinator so the coordinator can
    // forward VS Code tool calls to the extension via the transport.
    let transport_arc = Arc::new(Mutex::new(Transport::new()));

    // Spawn the coordinator actor with all sub-actor senders + transport
    let (coordinator_tx, _coordinator_handle) = system.spawn(
        CoordinatorActor::new(
            chat_tx,
            tools_tx,
            mcp_client_tx,
            llm_tx,
            progress_tx,
            system_tx,
            memory_graph_tx,
            transport_arc.clone(),
        ),
    );

    // ── Load MCP config and connect to external servers ──
    // This must happen BEFORE starting the transport so that the config
    // is available when the first requests arrive.
    {
        // Step 1: Load MCP config from ~/.spire/mcp-config.json
        {
            let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
            if coordinator_tx
                .send(CoordinatorMessage::HandleRequest {
                    method: "mcp/loadConfig".to_string(),
                    params: serde_json::json!({}),
                    response_tx: resp_tx,
                })
                .await
                .is_ok()
            {
                let _ = resp_rx.await;
            }
        }

        // Step 2: Connect to all configured MCP servers (non-blocking)
        {
            let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
            if coordinator_tx
                .send(CoordinatorMessage::HandleRequest {
                    method: "mcp/connectAll".to_string(),
                    params: serde_json::json!({}),
                    response_tx: resp_tx,
                })
                .await
                .is_ok()
            {
                // Don't block on connectAll — it may take time to connect
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), resp_rx).await;
            }
        }
    }

    // ── Register internal (built-in) tools as a pseudo "spire" MCP server ──
    {
        // Convert ToolInfo → Tool by round-tripping through JSON
        let internal_tools: Vec<Tool> = vscode_tool_definitions()
            .into_iter()
            .filter_map(|def| {
                let required = def.input_schema
                    .get("required")
                    .cloned()
                    .unwrap_or(serde_json::json!([]));
                let tool_json = serde_json::json!({
                    "name": def.name,
                    "description": def.description,
                    "inputSchema": {
                        "type": "object",
                        "properties": def.input_schema.get("properties"),
                        "required": required,
                    }
                });
                serde_json::from_value(tool_json).ok()
            })
            .collect();

        // Send via coordinator since mcp_client_tx was moved into CoordinatorActor
        let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
        if coordinator_tx
            .send(CoordinatorMessage::HandleRequest {
                method: "mcp/setInternalTools".to_string(),
                params: serde_json::json!({ "tools": internal_tools }),
                response_tx: resp_tx,
            })
            .await
            .is_ok()
        {
            let _ = resp_rx.await;
        }
    }

    // Set up the request handler: incoming requests from the extension
    // are forwarded to the coordinator actor.
    let coordinator_tx_clone = coordinator_tx.clone();
    {
        let transport = transport_arc.lock().await;
        transport.set_request_handler(Arc::new(move |params| {
            // The handler is called from the transport's processing task.
            // We need to send a message to the coordinator and wait for the response.
            // Since the handler is synchronous (Fn, not async Fn), we use
            // tokio::task::block_in_place to await the response.
            //
            // The params here are actually the full JSON-RPC request object
            // (method + params), but the transport extracts method separately.
            // We use a channel to communicate with the coordinator.
            let (tx, rx) = tokio::sync::oneshot::channel();

            // The method and params are embedded in the params value
            // by the transport layer. We extract them here.
            let method = params.get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let req_params = params.get("params").cloned().unwrap_or(serde_json::Value::Null);

            let coordinator_tx = coordinator_tx_clone.clone();

            // Spawn a task to send the request to the coordinator
            tokio::spawn(async move {
                if let Err(e) = coordinator_tx
                    .send(CoordinatorMessage::HandleRequest {
                        method,
                        params: req_params,
                        response_tx: tx,
                    })
                    .await
                {
                    error!("Failed to send request to coordinator: {}", e);
                }
            });

            // Wait for the response
            match tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(rx)
            }) {
                Ok(result) => result,
                Err(e) => {
                    error!("Coordinator response error: {}", e);
                    serde_json::json!({"error": format!("Internal error: {}", e)})
                }
            }
        })).await;
    }

    // Start the transport's stdin reader (must be done AFTER setting the handler)
    {
        let mut transport = transport_arc.lock().await;
        transport.start();
    }

    info!("Spire Core is ready. Listening on stdin/stdout for JSON-RPC messages.");

    // ── Wait for shutdown signal ──
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    // Send shutdown to coordinator
    let _ = coordinator_tx
        .send(CoordinatorMessage::Shutdown)
        .await;

    info!("Spire Core shut down gracefully");
    Ok(())
}
