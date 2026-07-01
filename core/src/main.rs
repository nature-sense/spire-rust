use anyhow::Result;
use rust_mcp_sdk::{
    mcp_server::{server_runtime, McpServerOptions},
    schema::*,
    McpServer, ToMcpServerHandler,
};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

mod actors;
mod embedder;
mod graph;
mod mcp;
mod models;

use crate::models::embedding::Embedder;
use crate::mcp::server::SpireMcpHandler;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,spire_rust=debug")),
        )
        .init();

    tracing::info!("Starting Spire Rust MCP server...");

    // Initialize the embedding model (all-MiniLM-L6-v2 via Candle)
    // This downloads ~85 MB of model weights on first run to ~/.cache/huggingface/
    tracing::info!("Initializing embedding model...");
    let embedder = match embedder::candle_embedder::create_embedder() {
        Ok(emb) => {
            tracing::info!(
                "Embedding model loaded successfully ({} dimensions)",
                emb.dimensions()
            );
            emb
        }
        Err(e) => {
            tracing::warn!(
                "Failed to load embedding model (continuing without embeddings): {}",
                e
            );
            return Err(e);
        }
    };
    let embedder = Arc::new(embedder);

    // Initialize the knowledge graph with WAL persistence.
    let wal_path = std::env::var("SPIRE_WAL_PATH")
        .unwrap_or_else(|_| "spire-graph.wal".to_string());
    tracing::info!("Opening knowledge graph (WAL: {})", wal_path);
    let graph_db = Arc::new(graph::GraphDb::new_with_wal(&wal_path)?);
    tracing::info!(
        "Knowledge graph loaded: {} nodes, {} edges",
        graph_db.node_count(),
        graph_db.edge_count()
    );

    // Build server info
    let server_details = InitializeResult {
        server_info: Implementation {
            name: "spire-rust".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            title: Some("Spire Rust MCP Server".into()),
            description: Some(
                "A Rust-powered MCP server for code analysis and knowledge graph operations"
                    .into(),
            ),
            icons: vec![],
            website_url: Some("https://github.com/naturesense/spire-rust".into()),
        },
        capabilities: ServerCapabilities {
            tools: Some(ServerCapabilitiesTools { list_changed: None }),
            ..Default::default()
        },
        protocol_version: ProtocolVersion::V2025_11_28.into(),
        instructions: None,
        meta: None,
    };

    // Create handler and wrap it for MCP
    let handler = SpireMcpHandler::new().to_mcp_server_handler();

    // Build and start the server
    let transport = rust_mcp_sdk::StdioTransport::new(rust_mcp_sdk::TransportOptions::default())
        .map_err(|e| anyhow::anyhow!("Failed to create transport: {}", e))?;

    let server = server_runtime::create_server(McpServerOptions {
        transport,
        handler,
        server_details,
        task_store: None,
        client_task_store: None,
        message_observer: None,
    });

    tracing::info!("Spire Rust MCP server initialized, listening on stdio");
    server.start().await.map_err(|e| anyhow::anyhow!("Server error: {}", e))?;

    Ok(())
}
