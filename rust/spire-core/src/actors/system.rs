// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! SystemActor — lifecycle state machine, health, and configuration management.
//!
//! This actor owns the system lifecycle as a state machine. On `Initialize`,
//! it drives the full startup sequence by sending messages to other actors:
//!
//! 1. Initialize MemoryGraph (creates GraphDb from config)
//! 2. Initialize Embedder (loads ONNX model)
//! 3. Load MCP config + connect servers
//! 4. Sync project (Bootstrap or StartupSync)
//! 5. Analyze project (semantic analysis)
//! 6. Load LLM config from graph
//! 7. Register internal tools
//! 8. Transition to Ready
//!
//! Each step is driven by message-passing — the SystemActor sends a message
//! to the target actor, waits for the response, then advances the state.

use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};

use crate::actors::{Actor, ActorError};
use crate::actors::coordinator::CoordinatorMessage;
use crate::actors::memory_graph::MemoryGraphMessage;
use crate::actors::mcp_client::McpClientMessage;
use crate::actors::project_sync::ProjectSyncMessage;
use crate::actors::project_analyzer::ProjectAnalyzerMessage;
use crate::actors::project_query::ProjectQueryMessage;
use crate::actors::llm::{LlmMessage, LlmConfig};
use crate::actors::progress::{ProgressMessage, ProgressUpdate, ProgressStatus};
use crate::models::embedding::Embedder;
use std::sync::Arc;



// ============================================================================
// SystemState — lifecycle state machine
// ============================================================================

/// Lifecycle states for the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemState {
    /// Initial state — not yet initialized.
    Initializing,
    /// Initializing the graph database.
    InitializingGraph,
    /// Initializing the embedding model.
    InitializingEmbedder,
    /// Loading MCP configuration.
    LoadingMcpConfig,
    /// Connecting to MCP servers.
    ConnectingMcp,
    /// Syncing project structure.
    SyncingProject,
    /// Analyzing project semantics.
    AnalyzingProject,
    /// Loading LLM configuration.
    LoadingLlmConfig,
    /// Registering internal tools.
    RegisteringTools,
    /// System is fully operational.
    Ready,
    /// System is shutting down.
    ShuttingDown,
    /// System encountered a fatal error during initialization.
    Failed(String),
}

impl std::fmt::Display for SystemState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SystemState::Initializing => write!(f, "initializing"),
            SystemState::InitializingGraph => write!(f, "initializing_graph"),
            SystemState::InitializingEmbedder => write!(f, "initializing_embedder"),
            SystemState::LoadingMcpConfig => write!(f, "loading_mcp_config"),
            SystemState::ConnectingMcp => write!(f, "connecting_mcp"),
            SystemState::SyncingProject => write!(f, "syncing_project"),
            SystemState::AnalyzingProject => write!(f, "analyzing_project"),
            SystemState::LoadingLlmConfig => write!(f, "loading_llm_config"),
            SystemState::RegisteringTools => write!(f, "registering_tools"),
            SystemState::Ready => write!(f, "ready"),
            SystemState::ShuttingDown => write!(f, "shutting_down"),
            SystemState::Failed(msg) => write!(f, "failed: {}", msg),
        }
    }
}

// ============================================================================
// SystemMessage
// ============================================================================

/// Messages for the System actor.
pub enum SystemMessage {
    /// Start the full initialization sequence.
    /// The SystemActor will drive the state machine by sending messages
    /// to other actors and waiting for responses.
    Initialize {
        /// Sender for the coordinator actor (to route requests).
        coordinator_tx: mpsc::Sender<CoordinatorMessage>,
        /// Sender for the memory graph actor.
        memory_graph_tx: mpsc::Sender<MemoryGraphMessage>,
        /// Sender for the MCP client actor.
        mcp_client_tx: mpsc::Sender<McpClientMessage>,
        /// Sender for the project sync actor.
        project_sync_tx: mpsc::Sender<ProjectSyncMessage>,
        /// Sender for the project analyzer actor.
        project_analyzer_tx: mpsc::Sender<ProjectAnalyzerMessage>,
        /// Sender for the project query actor.
        project_query_tx: mpsc::Sender<ProjectQueryMessage>,
        /// Sender for the LLM actor.
        llm_tx: mpsc::Sender<LlmMessage>,
        /// Sender for the progress actor (to publish startup progress).
        progress_tx: mpsc::Sender<ProgressMessage>,
        /// Embedder for generating vector embeddings.
        embedder: Arc<dyn Embedder>,
        /// Data directory for persistent storage.
        data_dir: PathBuf,
        /// Project root path.
        project_root: PathBuf,
        /// Reply channel for initialization result.
        reply_to: oneshot::Sender<Result<(), ActorError>>,
    },



    /// Get system status.
    GetStatus {
        reply_to: oneshot::Sender<Value>,
    },
    /// Graceful shutdown.
    Shutdown {
        reply_to: oneshot::Sender<Result<(), ActorError>>,
    },
    /// Get a configuration value by key.
    GetConfig {
        key: String,
        reply_to: oneshot::Sender<Option<Value>>,
    },
}

// ============================================================================
// SystemActor
// ============================================================================

/// Actor that manages system lifecycle via a state machine.
pub struct SystemActor {
    /// Current lifecycle state.
    state: SystemState,
    /// System start time.
    start_time: std::time::Instant,
    /// Configuration key-value store.
    config: std::collections::HashMap<String, Value>,
    /// Sender for the coordinator actor.
    coordinator_tx: Option<mpsc::Sender<CoordinatorMessage>>,
    /// Sender for the memory graph actor.
    memory_graph_tx: Option<mpsc::Sender<MemoryGraphMessage>>,
    /// Sender for the MCP client actor.
    mcp_client_tx: Option<mpsc::Sender<McpClientMessage>>,
    /// Sender for the project sync actor.
    project_sync_tx: Option<mpsc::Sender<ProjectSyncMessage>>,
    /// Sender for the project analyzer actor.
    project_analyzer_tx: Option<mpsc::Sender<ProjectAnalyzerMessage>>,
    /// Sender for the project query actor.
    project_query_tx: Option<mpsc::Sender<ProjectQueryMessage>>,
    /// Sender for the LLM actor.
    llm_tx: Option<mpsc::Sender<LlmMessage>>,
    /// Sender for the progress actor (to publish startup progress).
    progress_tx: Option<mpsc::Sender<ProgressMessage>>,
    /// Embedder for generating vector embeddings.
    embedder: Option<Arc<dyn Embedder>>,
    /// Data directory for persistent storage.
    data_dir: Option<PathBuf>,
    /// Project root path.
    project_root: Option<PathBuf>,

}


impl SystemActor {
    pub fn new() -> Self {
        Self {
            state: SystemState::Initializing,
            start_time: std::time::Instant::now(),
            config: std::collections::HashMap::new(),
            coordinator_tx: None,
            memory_graph_tx: None,
            mcp_client_tx: None,
            project_sync_tx: None,
            project_analyzer_tx: None,
            project_query_tx: None,
            llm_tx: None,
            progress_tx: None,
            embedder: None,
            data_dir: None,
            project_root: None,


        }
    }

    fn uptime_seconds(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    /// Publish a startup progress update to the progress actor.
    async fn publish_progress(&self, phase: &str, message: &str, percent: f64) {
        if let Some(ref progress_tx) = self.progress_tx {
            let update = ProgressUpdate {
                task_id: "system.startup".to_string(),
                message: message.to_string(),
                percent,
                status: ProgressStatus::Running,
                metadata: Some(serde_json::json!({
                    "phase": phase,
                })),
            };
            let _ = progress_tx.send(ProgressMessage::Publish { update }).await;
        }
    }

    /// Run the initialization state machine.
    /// Each step sends a message to the appropriate actor and awaits the response.
    async fn run_initialize(&mut self) -> Result<(), ActorError> {
        // ── Step 1: Initialize MemoryGraph (creates GraphDb from config) ──
        self.state = SystemState::InitializingGraph;
        self.publish_progress("initializing_graph", "Initializing graph database", 5.0).await;
        info!("SystemActor: initializing graph database");

        {
            let mem_tx = self.memory_graph_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("memory_graph_tx not set".to_string())
            })?;
            let data_dir = self.data_dir.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("data_dir not set".to_string())
            })?;
            let (tx, rx) = oneshot::channel();
            mem_tx
                .send(MemoryGraphMessage::Initialize {
                    data_dir: data_dir.clone(),
                    reply_to: tx,
                })
                .await
                .map_err(|e| ActorError::SendError(e.to_string()))?;
            rx.await
                .map_err(|e| ActorError::RecvError(e.to_string()))?
                .map_err(|e| ActorError::SetupFailed(e.to_string()))?;
        }
        info!("SystemActor: graph database initialized");

        // ── Step 2: Initialize Embedder ──
        self.state = SystemState::InitializingEmbedder;
        self.publish_progress("initializing_embedder", "Loading embedding model", 15.0).await;
        info!("SystemActor: initializing embedding model");

        {
            let mem_tx = self.memory_graph_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("memory_graph_tx not set".to_string())
            })?;
            let embedder = self.embedder.clone();
            let (tx, rx) = oneshot::channel();
            mem_tx
                .send(MemoryGraphMessage::InitializeEmbedder {
                    model_path: None,
                    embedder,
                    reply_to: tx,
                })
                .await
                .map_err(|e| ActorError::SendError(e.to_string()))?;
            rx.await
                .map_err(|e| ActorError::RecvError(e.to_string()))?
                .map_err(|e| ActorError::SetupFailed(e.to_string()))?;
        }
        info!("SystemActor: embedding model initialized");

        // ── Step 3: Bootstrap MCP config from JSON file into graph ──
        // This stores the MCP server definitions in the graph database so they
        // can be queried and managed via the MCP UI. If the graph already has
        // MCP config nodes (from a previous bootstrap), this step is skipped.
        {
            let mem_tx = self.memory_graph_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("memory_graph_tx not set".to_string())
            })?;
            let mcp_config_path = self.config.get("mcp_config_path")
                .and_then(|v| v.as_str())
                .map(PathBuf::from);

                if let Some(config_path) = mcp_config_path {
                    if config_path.exists() {
                        info!("SystemActor: bootstrapping MCP config from: {}", config_path.display());
                        let (tx, rx) = oneshot::channel();
                        mem_tx
                            .send(MemoryGraphMessage::BootstrapMcpConfig {
                                config_path,
                                reply_to: tx,
                            })
                            .await
                            .map_err(|e| ActorError::SendError(e.to_string()))?;
                        match rx.await {
                            Ok(Ok(())) => info!("SystemActor: MCP config bootstrapped into graph"),
                            Ok(Err(e)) => warn!("SystemActor: MCP config bootstrap failed: {}", e),
                            Err(e) => warn!("SystemActor: MCP config bootstrap response error: {}", e),
                        }
                    } else {
                        info!("SystemActor: MCP config file not found at: {}", config_path.display());
                    }
                } else {
                    info!("SystemActor: no MCP config path provided, skipping bootstrap");
                }

                // After bootstrap (or skip), load the MCP config from the graph
                // and send it to the MCP client actor so it can connect to servers.
                {
                    let mem_tx = self.memory_graph_tx.as_ref().ok_or_else(|| {
                        ActorError::SetupFailed("memory_graph_tx not set".to_string())
                    })?;
                    let mcp_tx = self.mcp_client_tx.as_ref().ok_or_else(|| {
                        ActorError::SetupFailed("mcp_client_tx not set".to_string())
                    })?;

                    // Fetch MCP config from graph
                    let (tx, rx) = oneshot::channel();
                    mem_tx
                        .send(MemoryGraphMessage::GetMcpConfig { reply_to: tx })
                        .await
                        .map_err(|e| ActorError::SendError(e.to_string()))?;

                    match rx.await {
                        Ok(Ok(servers)) => {
                            if !servers.is_empty() {
                                info!("SystemActor: loading {} MCP server configs from graph into client", servers.len());
                                // Convert McpServerConfigEntry to McpServerConfig
                                let configs: Vec<crate::mcp::client::McpServerConfig> = servers
                                    .into_iter()
                                    .filter_map(|entry| {
                                        let transport = if let Some(url) = entry.url {
                                            crate::mcp::client::TransportConfig::Http {
                                                url,
                                                headers: entry.headers.unwrap_or_default(),
                                            }
                                        } else if let Some(command) = entry.command {
                                            crate::mcp::client::TransportConfig::Stdio {
                                                command,
                                                args: entry.args,
                                                env: entry.env.unwrap_or_default(),
                                            }
                                        } else {
                                            warn!("SystemActor: MCP server '{}' has no transport config, skipping", entry.name);
                                            return None;
                                        };
                                        Some(crate::mcp::client::McpServerConfig {
                                            name: entry.name,
                                            transport,
                                            autostart: entry.autostart,
                                        })
                                    })
                                    .collect();

                                let (tx, rx) = oneshot::channel();
                                mcp_tx
                                    .send(McpClientMessage::LoadConfigFromGraph {
                                        servers: configs,
                                        reply_to: tx,
                                    })
                                    .await
                                    .map_err(|e| ActorError::SendError(e.to_string()))?;
                                let _ = rx.await;
                            } else {
                                info!("SystemActor: no MCP server configs in graph, skipping client load");
                            }
                        }
                        Ok(Err(e)) => warn!("SystemActor: failed to fetch MCP config from graph: {}", e),
                        Err(e) => warn!("SystemActor: MCP config fetch response error: {}", e),
                    }
                }
        }

        // ── Step 4: Initialize ProjectSyncActor ──

        {
            let sync_tx = self.project_sync_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("project_sync_tx not set".to_string())
            })?;
            let mem_tx = self.memory_graph_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("memory_graph_tx not set".to_string())
            })?;
            let embedder = self.embedder.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("embedder not set".to_string())
            })?;
            let (tx, rx) = oneshot::channel();
            sync_tx
                .send(ProjectSyncMessage::Initialize {
                    memory_graph_tx: mem_tx.clone(),
                    embedder: embedder.clone(),
                    reply_to: tx,
                })
                .await
                .map_err(|e| ActorError::SendError(e.to_string()))?;
            rx.await
                .map_err(|e| ActorError::RecvError(e.to_string()))?
                .map_err(|e| ActorError::SetupFailed(e.to_string()))?;
        }
        info!("SystemActor: project sync actor initialized");

        // ── Step 4: Initialize ProjectAnalyzerActor ──
        {
            let analyzer_tx = self.project_analyzer_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("project_analyzer_tx not set".to_string())
            })?;
            let (tx, rx) = oneshot::channel();
            analyzer_tx
                .send(ProjectAnalyzerMessage::Initialize {
                    reply_to: tx,
                })
                .await
                .map_err(|e| ActorError::SendError(e.to_string()))?;
            rx.await
                .map_err(|e| ActorError::RecvError(e.to_string()))?
                .map_err(|e| ActorError::SetupFailed(e.to_string()))?;
        }
        info!("SystemActor: project analyzer actor initialized");

        // ── Step 5: Initialize ProjectQueryActor ──
        {
            let query_tx = self.project_query_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("project_query_tx not set".to_string())
            })?;
            let mem_tx = self.memory_graph_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("memory_graph_tx not set".to_string())
            })?;
            let project_root = self.project_root.clone().ok_or_else(|| {
                ActorError::SetupFailed("project_root not set".to_string())
            })?;
            let (tx, rx) = oneshot::channel();
            query_tx
                .send(ProjectQueryMessage::Initialize {
                    memory_graph_tx: mem_tx.clone(),
                    project_root,
                    reply_to: tx,
                })
                .await
                .map_err(|e| ActorError::SendError(e.to_string()))?;
            rx.await
                .map_err(|e| ActorError::RecvError(e.to_string()))?
                .map_err(|e| ActorError::SetupFailed(e.to_string()))?;
        }
        info!("SystemActor: project query actor initialized");

        // ── Step 6: Connect MCP servers ──
        self.state = SystemState::ConnectingMcp;
        self.publish_progress("connecting_mcp", "Connecting to MCP servers", 50.0).await;
        info!("SystemActor: connecting to MCP servers");

        {
            let mcp_tx = self.mcp_client_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("mcp_client_tx not set".to_string())
            })?;
            let (tx, rx) = oneshot::channel();
            mcp_tx
                .send(McpClientMessage::ConnectAll { reply_to: tx })
                .await
                .map_err(|e| ActorError::SendError(e.to_string()))?;
            // Don't block on connectAll — it may take time
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), rx).await;
        }
        info!("SystemActor: MCP servers connected");

        // ── Step 7: Sync project ──
        self.state = SystemState::SyncingProject;
        self.publish_progress("syncing_project", "Syncing project structure", 65.0).await;
        info!("SystemActor: syncing project structure");

        {
            let sync_tx = self.project_sync_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("project_sync_tx not set".to_string())
            })?;
            let mem_tx = self.memory_graph_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("memory_graph_tx not set".to_string())
            })?;
            let project_root = self.project_root.clone().ok_or_else(|| {
                ActorError::SetupFailed("project_root not set".to_string())
            })?;

            // Check if a Project node already exists (warm start)
            let (tx, rx) = oneshot::channel();
            mem_tx
                .send(MemoryGraphMessage::QueryNodes {
                    filter: crate::models::memory_graph::NodeFilter {
                        node_type: Some(crate::models::memory_graph::NodeType::Project),
                        subtype: None,
                        name: None,
                        status: None,
                        tags: None,
                        limit: Some(1),
                        offset: None,
                    },
                    reply_to: tx,
                })
                .await
                .map_err(|e| ActorError::SendError(e.to_string()))?;

            let has_existing_project = match rx.await {
                Ok(nodes) => nodes.map(|n| !n.is_empty()).unwrap_or(false),
                Err(_) => false,
            };

            if has_existing_project {
                info!("SystemActor: project node exists, performing startup sync");
                let (tx, rx) = oneshot::channel();
                sync_tx
                    .send(ProjectSyncMessage::StartupSync {
                        project_root,
                        reply_to: tx,
                    })
                    .await
                    .map_err(|e| ActorError::SendError(e.to_string()))?;
                match rx.await {
                    Ok(Ok(result)) => info!("Startup sync complete: {:?}", result),
                    Ok(Err(e)) => warn!("Startup sync had issues: {}", e),
                    Err(e) => warn!("Startup sync response error: {}", e),
                }
            } else {
                info!("SystemActor: no project node found, performing full bootstrap");
                let (tx, rx) = oneshot::channel();
                sync_tx
                    .send(ProjectSyncMessage::Bootstrap {
                        project_root,
                        reply_to: tx,
                    })
                    .await
                    .map_err(|e| ActorError::SendError(e.to_string()))?;
                match rx.await {
                    Ok(Ok(result)) => info!("Bootstrap complete: {:?}", result),
                    Ok(Err(e)) => warn!("Bootstrap had issues: {}", e),
                    Err(e) => warn!("Bootstrap response error: {}", e),
                }
            }
        }
        info!("SystemActor: project sync complete");

        // ── Write a snapshot after bootstrap/startup sync ──
        // This ensures the initial project structure is persisted to disk
        // so that on restart, the graph can be recovered from the snapshot
        // rather than starting fresh.
        {
            let mem_tx = self.memory_graph_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("memory_graph_tx not set".to_string())
            })?;
            let (tx, rx) = oneshot::channel();
            mem_tx
                .send(MemoryGraphMessage::Sync { reply_to: tx })
                .await
                .map_err(|e| ActorError::SendError(e.to_string()))?;
            match rx.await {
                Ok(Ok(())) => info!("SystemActor: initial snapshot written after project sync"),
                Ok(Err(e)) => warn!("SystemActor: initial snapshot write failed: {}", e),
                Err(e) => warn!("SystemActor: initial snapshot response error: {}", e),
            }
        }

        // ── Step 8: Analyze project ──
        self.state = SystemState::AnalyzingProject;
        self.publish_progress("analyzing_project", "Analyzing project code", 80.0).await;
        info!("SystemActor: analyzing project semantics");

        {
            let analyzer_tx = self.project_analyzer_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("project_analyzer_tx not set".to_string())
            })?;
            let project_root = self.project_root.clone().ok_or_else(|| {
                ActorError::SetupFailed("project_root not set".to_string())
            })?;
            let (tx, rx) = oneshot::channel();
            analyzer_tx
                .send(ProjectAnalyzerMessage::Analyze {
                    project_root,
                    reply_to: tx,
                })
                .await
                .map_err(|e| ActorError::SendError(e.to_string()))?;
            match rx.await {
                Ok(Ok(analysis)) => {
                    info!(
                        "Project analysis complete: {} files, {} dirs, {} build systems",
                        analysis.total_files,
                        analysis.total_dirs,
                        analysis.build_systems.len(),
                    );
                    // Store the analysis summary in config for later retrieval
                    let summary = analysis.architecture_summary;
                    self.config.insert("project.architecture_summary".to_string(), Value::String(summary));
                }
                Ok(Err(e)) => warn!("Project analysis failed: {}", e),
                Err(e) => warn!("Project analysis response error: {}", e),
            }
        }
        info!("SystemActor: project analysis complete");

        // ── Step 9: Load LLM config from graph ──
        self.state = SystemState::LoadingLlmConfig;
        self.publish_progress("loading_llm_config", "Loading LLM configuration", 90.0).await;
        info!("SystemActor: loading LLM configuration");

        {
            let mem_tx = self.memory_graph_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("memory_graph_tx not set".to_string())
            })?;
            let llm_tx = self.llm_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("llm_tx not set".to_string())
            })?;

            // Fetch all three deepseek config keys
            let keys = ["deepseek.api_key", "deepseek.model", "deepseek.api_url"];
            let mut api_key = String::new();
            let mut model = "deepseek-chat".to_string();
            let mut api_url = "https://api.deepseek.com/v1/chat/completions".to_string();

            for key in &keys {
                let (tx, rx) = oneshot::channel();
                mem_tx
                    .send(MemoryGraphMessage::GetConfig {
                        key: key.to_string(),
                        reply_to: tx,
                    })
                    .await
                    .map_err(|e| ActorError::SendError(e.to_string()))?;
                if let Ok(Ok(Some(value))) = rx.await {
                    if let Some(s) = value.as_str() {
                        match *key {
                            "deepseek.api_key" => api_key = s.to_string(),
                            "deepseek.model" => model = s.to_string(),
                            "deepseek.api_url" => api_url = s.to_string(),
                            _ => {}
                        }
                    }
                }
            }

            if !api_key.is_empty() {
                info!("SystemActor: loading persisted DeepSeek config: model={}", model);
                let llm_config = LlmConfig {
                    api_key,
                    model,
                    api_url,
                    max_tokens: 4096,
                    temperature: 0.7,
                    strict_mode: false,
                };
                let (tx, rx) = oneshot::channel();
                llm_tx
                    .send(LlmMessage::UpdateConfig {
                        config: llm_config,
                        reply_to: tx,
                    })
                    .await
                    .map_err(|e| ActorError::SendError(e.to_string()))?;
                let _ = rx.await;
            } else {
                info!("SystemActor: no persisted DeepSeek config found, using defaults");
            }
        }
        info!("SystemActor: LLM config loaded");

        // ── Step 10: Register internal tools ──
        self.state = SystemState::RegisteringTools;
        self.publish_progress("registering_tools", "Registering tools", 95.0).await;
        info!("SystemActor: registering internal tools");

        {
            let mcp_tx = self.mcp_client_tx.as_ref().ok_or_else(|| {
                ActorError::SetupFailed("mcp_client_tx not set".to_string())
            })?;

            // Collect VS Code extension tools
            let vsc_tools: Vec<rust_mcp_sdk::schema::Tool> =
                crate::actors::vscode_tool_definitions()
                    .into_iter()
                    .filter_map(|def| {
                        let required = def
                            .input_schema
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

            // Collect project query tools (memory graph queries)
            let project_tools: Vec<rust_mcp_sdk::schema::Tool> =
                crate::actors::project_query::ProjectQueryActor::tool_definitions()
                    .into_iter()
                    .filter_map(|def| {
                        let required = def
                            .input_schema
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

            // Combine both sets of tools
            let mut internal_tools = vsc_tools;
            internal_tools.extend(project_tools);

            let (tx, rx) = oneshot::channel();
            mcp_tx
                .send(McpClientMessage::SetInternalTools {
                    tools: internal_tools,
                    reply_to: tx,
                })
                .await
                .map_err(|e| ActorError::SendError(e.to_string()))?;
            let _ = rx.await;
        }
        info!("SystemActor: internal tools registered");

        // ── Done! ──
        self.state = SystemState::Ready;
        self.publish_progress("ready", "Starting Spire — complete!", 100.0).await;
        info!("SystemActor: system is ready");

        Ok(())
    }
}

impl Default for SystemActor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Actor for SystemActor {
    type Message = SystemMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            SystemMessage::Initialize {
                coordinator_tx,
                memory_graph_tx,
                mcp_client_tx,
                project_sync_tx,
                project_analyzer_tx,
                project_query_tx,
                llm_tx,
                progress_tx,
                embedder,
                data_dir,
                project_root,
                reply_to,
            } => {
                // Store the senders for later use
                self.coordinator_tx = Some(coordinator_tx);
                self.memory_graph_tx = Some(memory_graph_tx);
                self.mcp_client_tx = Some(mcp_client_tx);
                self.project_sync_tx = Some(project_sync_tx);
                self.project_analyzer_tx = Some(project_analyzer_tx);
                self.project_query_tx = Some(project_query_tx);
                self.llm_tx = Some(llm_tx);
                self.progress_tx = Some(progress_tx);
                self.embedder = Some(embedder);
                self.data_dir = Some(data_dir);
                self.project_root = Some(project_root);

                let result = self.run_initialize().await;

                if let Err(ref e) = result {
                    error!("SystemActor: initialization failed: {}", e);
                    self.state = SystemState::Failed(e.to_string());
                }
                let _ = reply_to.send(result);
            }
            SystemMessage::GetStatus { reply_to } => {
                let status = serde_json::json!({
                    "status": self.state.to_string(),
                    "uptime_seconds": self.uptime_seconds(),
                    "version": env!("CARGO_PKG_VERSION"),
                    "actors": {
                        "chat": true,
                        "tools": true,
                        "mcp_client": self.mcp_client_tx.is_some(),
                        "llm": self.llm_tx.is_some(),
                        "progress": true,
                        "system": true,
                        "memory_graph": self.memory_graph_tx.is_some(),
                        "project_sync": self.project_sync_tx.is_some(),
                        "project_analyzer": self.project_analyzer_tx.is_some(),
                        "project_query": self.project_query_tx.is_some(),
                    }
                });
                let _ = reply_to.send(status);
            }
            SystemMessage::Shutdown { reply_to } => {
                info!("SystemActor: initiating graceful shutdown");
                self.state = SystemState::ShuttingDown;
                let _ = reply_to.send(Ok(()));
            }
            SystemMessage::GetConfig { key, reply_to } => {
                let value = self.config.get(&key).cloned();
                let _ = reply_to.send(value);
            }
        }
    }
}
