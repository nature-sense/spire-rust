// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

mod framework;
mod mcp_server;

use std::path::PathBuf;
use std::sync::Arc;
use rust_mcp_sdk::{
    mcp_server::{server_runtime, McpServerOptions},
    schema::{InitializeResult, Implementation, ServerCapabilities, ServerCapabilitiesTools, LATEST_PROTOCOL_VERSION},
    McpServer, ToMcpServerHandler,
};
use tracing_subscriber::EnvFilter;

use mcp_server::server::MCPServer;
use mcp_server::handler::MCPActorHandler;
use mcp_server::tools::{EchoTool, ReadFileActor, WriteFileActor, ListDirActor};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting actor-based MCP server...");

    // Get workspace root from environment or default to current dir
    let root = std::env::var("MCP_ROOT")
        .unwrap_or_else(|_| ".".to_string());
    let root = PathBuf::from(root);

    // Build server info
    let server_details = InitializeResult {
        server_info: Implementation {
            name: "spire-rust".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            title: Some("Spire Rust MCP Server (Actor)".into()),
            description: Some("Actor-based MCP server for code analysis".into()),
            icons: vec![],
            website_url: None,
        },
        capabilities: ServerCapabilities {
            tools: Some(ServerCapabilitiesTools { list_changed: None }),
            ..Default::default()
        },
        protocol_version: LATEST_PROTOCOL_VERSION.into(),
        instructions: None,
        meta: None,
    };

    // Create MCPServer and register tools
    let mut server = MCPServer::new();
    server.register_tool(EchoTool::tool_info(), EchoTool);
    server.register_tool(ReadFileActor::tool_info(), ReadFileActor::new(root.clone()));
    server.register_tool(WriteFileActor::tool_info(), WriteFileActor::new(root.clone()));
    server.register_tool(ListDirActor::tool_info(), ListDirActor::new(root.clone()));
    tracing::info!("Registered {} tool(s)", server.tools().len());

    // Create handler and wrap it for MCP
    let server = Arc::new(server);
    let handler = MCPActorHandler::new(server).to_mcp_server_handler();

    // Build and start the server
    let transport = rust_mcp_sdk::StdioTransport::new(
        rust_mcp_sdk::TransportOptions::default(),
    )
    .map_err(|e| format!("Failed to create transport: {}", e))?;

    let mcp_server = server_runtime::create_server(McpServerOptions {
        transport,
        handler,
        server_details,
        task_store: None,
        client_task_store: None,
        message_observer: None,
    });

    tracing::info!("Actor-based MCP server initialized, listening on stdio");
    mcp_server
        .start()
        .await
        .map_err(|e| format!("Server error: {}", e))?;

    Ok(())
}
