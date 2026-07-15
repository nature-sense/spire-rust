// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Spire Core — standalone binary entry point.
//!
//! This binary runs as a subprocess of the VS Code extension, communicating
//! via JSON-RPC 2.0 over a TCP loopback socket.
//!
//! Architecture:
//!   VS Code Extension (BidirectionalClient) ←→ spire-core (this binary)
//!     - Core binds to 127.0.0.1:0 and prints "SPIRE_PORT=<port>" to stdout
//!     - Extension reads the port from stdout and connects via TCP
//!     - All JSON-RPC messages flow over the TCP connection
//!
//! Usage:
//!   cargo run --bin spire-core           # standalone (testing)
//!   cargo run --bin spire-core -- --mcp  # with MCP client connections

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use chrono::Local;

use spire_core::actors::{
    ActorSystem, ChatActor, ToolsActor, McpClientActor,
    CoordinatorActor, CoordinatorMessage,
    LlmActor, LlmConfig,
    ProgressActor, SystemActor,
    MemoryGraphActor, ProjectSyncActor, ProjectAnalyzerActor, ProjectQueryActor,
};
use spire_core::models::embedding::Embedder;
use spire_core::embedder::candle_embedder::CandleEmbedder;
use spire_core::transport::socket::Transport;




/// Determine the log directory.
/// Priority: SPIRE_LOG_DIR env var, then {project_root}/.spire/logs,
/// then a temp dir.
fn log_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SPIRE_LOG_DIR") {
        return PathBuf::from(dir);
    }
    // Prefer project-local .spire/logs directory
    if let Ok(project_root) = std::env::var("SPIRE_PROJECT_ROOT") {
        let dir = PathBuf::from(project_root).join(".spire").join("logs");
        let _ = std::fs::create_dir_all(&dir);
        return dir;
    }
    // Fallback: use a temp directory
    let dir = std::env::temp_dir().join("spire-core-logs");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Resolve a log file path that restarts on every extension start.
///
/// Naming scheme:
///   - 1st start on a given day: `spire-core.log.YYYY-MM-DD`
///   - 2nd start on the same day: `spire-core.log.YYYY-MM-DD.1`
///   - 3rd start: `spire-core.log.YYYY-MM-DD.2`
///   - etc.
///
/// The index resets on a new calendar day.
fn resolve_log_path(log_dir: &PathBuf) -> PathBuf {
    let date = Local::now().format("%Y-%m-%d").to_string();
    let base = log_dir.join(format!("spire-core.log.{}", date));

    if !base.exists() {
        return base;
    }

    // Scan for the next available index
    for i in 1.. {
        let candidate = log_dir.join(format!("spire-core.log.{}.{}", date, i));
        if !candidate.exists() {
            return candidate;
        }
    }

    // Safety valve: should never reach here in practice
    base
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── Initialise tracing — log to a file (never stdout, which is JSON-RPC) ──
    let log_dir = log_dir();
    let log_path = resolve_log_path(&log_dir);

    // Open the log file directly (no daily rolling — we handle rotation ourselves)
    let log_file = std::fs::File::create(&log_path)
        .expect("Failed to create log file");
    let (non_blocking, _guard) = tracing_appender::non_blocking(log_file);

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| {
                    tracing_subscriber::EnvFilter::new("info")
                        .add_directive(
                            "rust_mcp_sdk::mcp_runtimes::client_runtime=error"
                                .parse()
                                .expect("valid filter directive"),
                        )
                }),
        )
        .with_writer(non_blocking)
        .with_ansi(false) // No ANSI escape codes in log files
        .init();

    info!("Spire Core starting...");
    info!("Logging to: {}", log_path.display());

    // ── Initialise SeleneDB graph database with WAL persistence ──
    // The data directory is project-root/.spire/data so that
    // SeleneDB's snapshot and WAL files live in a dedicated subdirectory,
    // separate from logs and MCP config.
    let data_dir = if let Ok(dir) = std::env::var("SPIRE_DATA_DIR") {
        PathBuf::from(dir)
    } else if let Ok(project_root) = std::env::var("SPIRE_PROJECT_ROOT") {
        PathBuf::from(project_root).join(".spire").join("data")
    } else {
        std::env::temp_dir().join("spire-core-data")
    };

    std::fs::create_dir_all(&data_dir)
        .expect("Failed to create data directory");

    info!("Data directory: {}", data_dir.display());

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
    // Initialized via Initialize message from SystemActor
    let (memory_graph_tx, _memory_graph_handle) = system.spawn(
        MemoryGraphActor::new(),
    );

    // Spawn the project sync actor (three-phase project structure sync)
    // Initialized via Initialize message from SystemActor
    let (project_sync_tx, _project_sync_handle) = system.spawn(
        ProjectSyncActor::new(),
    );

    // Spawn the project analyzer actor (semantic project analysis for LLM)
    let (project_analyzer_tx, _project_analyzer_handle) = system.spawn(
        ProjectAnalyzerActor::new(),
    );

    // Spawn the project query actor (semantic project queries for LLM)
    let (project_query_tx, _project_query_handle) = system.spawn(
        ProjectQueryActor::new(),
    );

    // ── Create the JSON-RPC transport ──
    // This must be created BEFORE the coordinator so the coordinator can
    // forward VS Code tool calls to the extension via the transport.
    let transport_arc = Arc::new(Mutex::new(Transport::new()));

    // ── Bind the transport early and print the port ──
    // We bind BEFORE the blocking initialization so the extension can discover
    // the port immediately, rather than waiting for the full startup sequence
    // (which includes downloading the ~85MB embedding model from HuggingFace).
    let port = {
        let mut transport = transport_arc.lock().await;
        transport.bind().await?
    };
    println!("SPIRE_PORT={}", port);
    info!("Spire Core transport bound to port {}. Extension can connect now.", port);

    // ── Accept the extension's TCP connection immediately ──
    // We accept BEFORE the blocking initialization so that:
    // 1. The extension's TCP connection doesn't hang in the backlog
    // 2. The transport writer is available for sending progress notifications
    //    during the SystemActor initialization
    {
        let mut transport = transport_arc.lock().await;
        transport.accept().await?;
    }
    info!("Spire Core accepted extension connection on port {}.", port);

    // ── Subscribe to progress updates BEFORE SystemActor initialization ──
    // We subscribe synchronously here (not in a spawned task) to ensure the
    // broadcast receiver is registered before the SystemActor starts sending
    // progress updates during initialization. Otherwise, the first few updates
    // would be lost (broadcast channel only keeps messages for active receivers).
    let progress_rx = {
        let (subscribe_tx, subscribe_rx) = tokio::sync::oneshot::channel();
        if progress_tx
            .send(spire_core::actors::ProgressMessage::Subscribe { reply_to: subscribe_tx })
            .await
            .is_ok()
        {
            subscribe_rx.await.ok()
        } else {
            None
        }
    };

    // ── Forward progress updates to the extension as JSON-RPC notifications ──
    // This is spawned as a background task that reads from the broadcast receiver
    // and forwards each update to the extension via the transport.
    if let Some(progress_rx) = progress_rx {
        let transport = transport_arc.clone();
        tokio::spawn(async move {
            let mut rx = progress_rx;
            while let Ok(update) = rx.recv().await {
                let transport = transport.lock().await;
                let params = serde_json::json!({
                    "taskId": update.task_id,
                    "message": update.message,
                    "percent": update.percent,
                    "status": match update.status {
                        spire_core::actors::ProgressStatus::Running => "running",
                        spire_core::actors::ProgressStatus::Completed => "completed",
                        spire_core::actors::ProgressStatus::Failed => "failed",
                    },
                    "metadata": update.metadata,
                });
                transport.send_notification("event/system/progress", &params).await;
            }
        });
    }

    // Clone senders before moving originals into CoordinatorActor
    let mcp_client_tx_for_system = mcp_client_tx.clone();
    let llm_tx_for_system = llm_tx.clone();
    let system_tx_for_system = system_tx.clone();

    // Spawn the coordinator actor with all sub-actor senders + transport
    let progress_tx_for_coordinator = progress_tx.clone();
    let (coordinator_tx, _coordinator_handle) = system.spawn(
        CoordinatorActor::new(
            chat_tx,
            tools_tx,
            mcp_client_tx,
            llm_tx,
            progress_tx_for_coordinator,
            system_tx,
            memory_graph_tx.clone(),
            project_query_tx.clone(),
            transport_arc.clone(),
        ),
    );


    // ── Initialize the SystemActor (drives the full startup state machine) ──
    {
        let project_root = std::env::var("SPIRE_PROJECT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                // Safety guard: never scan the root filesystem
                if cwd == PathBuf::from("/") {
                    let fallback = PathBuf::from(".");
                    warn!("SPIRE_PROJECT_ROOT not set and current_dir is '/', falling back to '.' — project sync will be skipped");
                    fallback
                } else {
                    cwd
                }
            });

        // Create the embedder (spawn_blocking because CandleEmbedder::new()
        // does blocking I/O via hf_hub to load model weights from cache/network).
        let embedder: Arc<dyn Embedder> = match tokio::task::spawn_blocking(|| {
            CandleEmbedder::new()
        })
        .await
        {
            Ok(Ok(embedder)) => {
                info!("CandleEmbedder created successfully");
                Arc::new(embedder)
            }
            Ok(Err(e)) => {
                error!("Failed to create CandleEmbedder: {}. Running without embeddings.", e);
                // Use a no-op embedder so the system can still start
                Arc::new(spire_core::embedder::NoopEmbedder)
            }
            Err(e) => {
                error!("CandleEmbedder creation task panicked: {}. Running without embeddings.", e);
                Arc::new(spire_core::embedder::NoopEmbedder)
            }
        };

        let (tx, rx) = tokio::sync::oneshot::channel();
        if system_tx_for_system
            .send(spire_core::actors::SystemMessage::Initialize {
                coordinator_tx: coordinator_tx.clone(),
                memory_graph_tx: memory_graph_tx.clone(),
                mcp_client_tx: mcp_client_tx_for_system,
                project_sync_tx: project_sync_tx.clone(),
                project_analyzer_tx: project_analyzer_tx.clone(),
                project_query_tx: project_query_tx.clone(),
                llm_tx: llm_tx_for_system,
                progress_tx: progress_tx.clone(),
                embedder,
                data_dir: data_dir.clone(),
                project_root,
                reply_to: tx,
            })


            .await
            .is_ok()
        {
            match rx.await {
                Ok(Ok(())) => info!("SystemActor initialization complete"),
                Ok(Err(e)) => error!("SystemActor initialization failed: {}", e),
                Err(e) => error!("SystemActor initialization response error: {}", e),
            }
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

    info!("Spire Core is ready. Connected on 127.0.0.1:{} for JSON-RPC messages.", port);


    // ── Wait for shutdown signal ──

    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    // Sync/flush the WAL before shutdown.
    // Send Sync directly to the MemoryGraphActor to write a snapshot
    // immediately, bypassing the 2-second debounce delay.
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if memory_graph_tx
            .send(spire_core::actors::MemoryGraphMessage::Sync { reply_to: tx })
            .await
            .is_ok()
        {
            let _ = rx.await;
        }
    }

    // Send shutdown to coordinator
    let _ = coordinator_tx
        .send(CoordinatorMessage::Shutdown)
        .await;

    info!("Spire Core shut down gracefully");
    Ok(())
}
