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

use anyhow::{Context, Result};
use rust_mcp_sdk::mcp_client::{
    client_runtime_core::create_client,
    ClientHandlerCore, McpClientOptions, ToMcpClientHandlerCore,
};
use rust_mcp_sdk::schema::{
    CallToolRequestParams, CallToolResult, InitializeRequestParams, InitializeResult,
    LATEST_PROTOCOL_VERSION, Tool,
};
use rust_mcp_sdk::McpClient;
use rust_mcp_sdk::StdioTransport;
use rust_mcp_sdk::TransportOptions;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info, warn};

/// Configuration for an external MCP server connection.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Option<HashMap<String, String>>,
}

/// Top-level config file format (~/.spine/mcp-servers.json).
#[derive(Debug, Clone, serde::Deserialize)]
struct McpServersFile {
    servers: Vec<McpServerConfig>,
}

/// Represents a live connection to an external MCP server.
struct McpClientConnection {
    config: McpServerConfig,
    runtime: Arc<dyn McpClient>,
    server_info: InitializeResult,
    tools: Vec<Tool>,
}

/// Manages connections to external MCP servers.
///
/// This client connects to specialized MCP servers (e.g., filesystem, github, postgres)
/// and provides a unified interface for discovering their capabilities and calling tools on them.
///
/// Configuration is loaded from `~/.spine/mcp-servers.json`.
#[allow(dead_code)]
pub struct McpClientManager {
    connections: HashMap<String, McpClientConnection>,
}

impl McpClientManager {
    /// Create a new, empty manager.
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }

    /// Return the path to the MCP servers config file.
    fn config_path() -> Result<PathBuf> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .context("Cannot determine home directory (HOME or USERPROFILE not set)")?;
        Ok(PathBuf::from(home).join(".spine").join("mcp-servers.json"))
    }

    /// Load server configurations from `~/.spine/mcp-servers.json`.
    ///
    /// Returns the path that was attempted, or `None` if the file doesn't exist.
    pub fn load_config(&mut self) -> Result<Option<PathBuf>> {
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            info!(
                "MCP config file not found at {}, skipping",
                config_path.display()
            );
            return Ok(None);
        }

        let contents = std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read {}", config_path.display()))?;

        let file: McpServersFile = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse {}", config_path.display()))?;

        for config in &file.servers {
            info!(
                "MCP Client: loaded config for server '{}' ({} {})",
                config.name,
                config.command,
                config.args.join(" ")
            );
        }

        // Store configs in a temporary map for later connection
        for config in file.servers {
            self.connections
                .entry(config.name.clone())
                .or_insert_with(|| McpClientConnection {
                    config: config.clone(),
                    runtime: Arc::new(PlaceholderClient),
                    server_info: InitializeResult {
                        server_info: rust_mcp_sdk::schema::Implementation {
                            name: String::new(),
                            version: String::new(),
                            description: None,
                            icons: vec![],
                            website_url: None,
                            title: None,
                        },
                        capabilities: rust_mcp_sdk::schema::ServerCapabilities::default(),
                        protocol_version: String::new(),
                        instructions: None,
                        meta: None,
                    },
                    tools: vec![],
                });
        }

        Ok(Some(config_path))
    }

    /// Connect to all configured servers.
    ///
    /// For each server, this spawns the subprocess, performs the MCP initialize handshake,
    /// and discovers available tools.
    pub async fn connect_all(&mut self) {
        let names: Vec<String> = self.connections.keys().cloned().collect();
        for name in names {
            if let Err(e) = self.connect(&name).await {
                error!("MCP Client: failed to connect to '{}': {:#}", name, e);
            }
        }
    }

    /// Connect to a single server by name.
    pub async fn connect(&mut self, server_name: &str) -> Result<()> {
        let config = self
            .connections
            .get(server_name)
            .map(|c| c.config.clone())
            .ok_or_else(|| anyhow::anyhow!("Unknown MCP server: {}", server_name))?;

        info!(
            "MCP Client: connecting to '{}' ({} {})",
            server_name,
            config.command,
            config.args.join(" ")
        );

        // Create a stdio transport that spawns the server subprocess
        let transport = StdioTransport::create_with_server_launch(
            config.command.clone(),
            config.args.clone(),
            config.env.clone(),
            TransportOptions::default(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to create transport for '{}': {}", server_name, e))?;

        // Build client details
        let client_details = InitializeRequestParams {
            protocol_version: LATEST_PROTOCOL_VERSION.into(),
            capabilities: rust_mcp_sdk::schema::ClientCapabilities::default(),
            client_info: rust_mcp_sdk::schema::Implementation {
                name: "spire-rust".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: None,
                icons: vec![],
                website_url: None,
                title: None,
            },
            meta: None,
        };

        // Create the client runtime
        let handler = SimpleClientHandler;
        let runtime = create_client(McpClientOptions {
            client_details,
            transport,
            handler: handler.to_mcp_client_handler(),
            task_store: None,
            server_task_store: None,
            message_observer: None,
        });

        // Start the client (performs initialize handshake)
        runtime.clone().start().await.map_err(|e| {
            anyhow::anyhow!("Failed to start MCP client for '{}': {}", server_name, e)
        })?;

        let server_info = runtime
            .server_info()
            .ok_or_else(|| anyhow::anyhow!("Server '{}' did not return server info", server_name))?;

        info!(
            "MCP Client: connected to '{}' (v{}, tools: {:?})",
            server_info.server_info.name,
            server_info.server_info.version,
            server_info.capabilities.tools.is_some()
        );

        // Discover tools
        let tools = if server_info.capabilities.tools.is_some() {
            match runtime.request_tool_list(None).await {
                Ok(list) => {
                    info!(
                        "MCP Client: '{}' exposes {} tools",
                        server_name,
                        list.tools.len()
                    );
                    list.tools
                }
                Err(e) => {
                    warn!(
                        "MCP Client: failed to list tools for '{}': {}",
                        server_name, e
                    );
                    vec![]
                }
            }
        } else {
            vec![]
        };

        // Store the connection
        self.connections.insert(
            server_name.to_string(),
            McpClientConnection {
                config,
                runtime,
                server_info,
                tools,
            },
        );

        Ok(())
    }

    /// Disconnect from all servers.
    pub async fn disconnect_all(&mut self) {
        let names: Vec<String> = self.connections.keys().cloned().collect();
        for name in names {
            self.disconnect(&name).await;
        }
    }

    /// Disconnect from a single server.
    pub async fn disconnect(&mut self, server_name: &str) {
        if let Some(conn) = self.connections.get(server_name) {
            info!("MCP Client: disconnecting from '{}'", server_name);
            if let Err(e) = conn.runtime.shut_down().await {
                warn!(
                    "MCP Client: error shutting down '{}': {}",
                    server_name, e
                );
            }
        }
        self.connections.remove(server_name);
    }

    /// Get the server info (identity + capabilities) for a connected server.
    pub fn get_server_info(&self, server_name: &str) -> Option<&InitializeResult> {
        self.connections.get(server_name).map(|c| &c.server_info)
    }

    /// Get the list of tools exposed by a connected server.
    pub fn get_tools(&self, server_name: &str) -> Option<&[Tool]> {
        self.connections
            .get(server_name)
            .map(|c| c.tools.as_slice())
    }

    /// Get the names of all connected servers.
    pub fn connected_servers(&self) -> Vec<&str> {
        self.connections.keys().map(|s| s.as_str()).collect()
    }

    /// Call a tool on a specific external MCP server.
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Result<CallToolResult> {
        let conn = self
            .connections
            .get(server_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown or disconnected MCP server: {}", server_name))?;

        info!(
            "MCP Client: calling '{}' on '{}'",
            tool_name, server_name
        );

        let result = conn
            .runtime
            .request_tool_call(CallToolRequestParams {
                name: tool_name.into(),
                arguments,
                meta: None,
                task: None,
            })
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Tool call '{}' on '{}' failed: {}",
                    tool_name,
                    server_name,
                    e
                )
            })?;

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Placeholder client used before real connections are established
// ---------------------------------------------------------------------------
struct PlaceholderClient;

#[async_trait::async_trait]
impl McpClient for PlaceholderClient {
    async fn start(self: Arc<Self>) -> rust_mcp_sdk::error::SdkResult<()> {
        Ok(())
    }
    fn set_server_details(&self, _: InitializeResult) -> rust_mcp_sdk::error::SdkResult<()> {
        Ok(())
    }
    async fn terminate_session(&self) {}
    fn task_store(&self) -> Option<Arc<rust_mcp_sdk::task_store::ClientTaskStore>> {
        None
    }
    fn server_task_store(&self) -> Option<Arc<rust_mcp_sdk::task_store::ServerTaskStore>> {
        None
    }
    async fn shut_down(&self) -> rust_mcp_sdk::error::SdkResult<()> {
        Ok(())
    }
    async fn is_shut_down(&self) -> bool {
        true
    }
    fn client_info(&self) -> &InitializeRequestParams {
        static FALLBACK: std::sync::LazyLock<InitializeRequestParams> =
            std::sync::LazyLock::new(|| InitializeRequestParams {
                protocol_version: LATEST_PROTOCOL_VERSION.into(),
                capabilities: rust_mcp_sdk::schema::ClientCapabilities::default(),
                client_info: rust_mcp_sdk::schema::Implementation {
                    name: "spire-rust".into(),
                    version: "0.1.0".into(),
                    description: None,
                    icons: vec![],
                    website_url: None,
                    title: None,
                },
                meta: None,
            });
        &FALLBACK
    }
    fn server_info(&self) -> Option<InitializeResult> {
        None
    }
    async fn session_id(&self) -> Option<rust_mcp_sdk::SessionId> {
        None
    }
    async fn send(
        &self,
        _: rust_mcp_sdk::schema::schema_utils::MessageFromClient,
        _: Option<rust_mcp_sdk::schema::RequestId>,
        _: Option<std::time::Duration>,
    ) -> rust_mcp_sdk::error::SdkResult<
        Option<rust_mcp_sdk::schema::schema_utils::ServerMessage>,
    > {
        Ok(None)
    }
    async fn send_batch(
        &self,
        _: Vec<rust_mcp_sdk::schema::schema_utils::ClientMessage>,
        _: Option<std::time::Duration>,
    ) -> rust_mcp_sdk::error::SdkResult<
        Option<Vec<rust_mcp_sdk::schema::schema_utils::ServerMessage>>,
    > {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Simple client handler — does nothing for incoming requests/notifications
// ---------------------------------------------------------------------------
struct SimpleClientHandler;

#[async_trait::async_trait]
impl ClientHandlerCore for SimpleClientHandler {
    async fn handle_request(
        &self,
        _request: rust_mcp_sdk::schema::schema_utils::ServerJsonrpcRequest,
        _runtime: &dyn McpClient,
    ) -> std::result::Result<
        rust_mcp_sdk::schema::schema_utils::ResultFromClient,
        rust_mcp_sdk::schema::RpcError,
    > {
        Err(rust_mcp_sdk::schema::RpcError::internal_error()
            .with_message("No request handler implemented".to_string()))
    }

    async fn handle_notification(
        &self,
        _notification: rust_mcp_sdk::schema::schema_utils::NotificationFromServer,
        _runtime: &dyn McpClient,
    ) -> std::result::Result<(), rust_mcp_sdk::schema::RpcError> {
        Ok(())
    }

    async fn handle_error(
        &self,
        _error: &rust_mcp_sdk::schema::RpcError,
        _runtime: &dyn McpClient,
    ) -> std::result::Result<(), rust_mcp_sdk::schema::RpcError> {
        Err(rust_mcp_sdk::schema::RpcError::internal_error()
            .with_message("handle_error() Not implemented".to_string()))
    }
}
