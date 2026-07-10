// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! McpClientActor — wraps external MCP server connections behind message-passing.
//!
//! This actor owns the `McpClientManager` and processes connection/disconnection
//! and tool-call requests asynchronously. It also maintains a set of internal
//! tools that appear under the pseudo-MCP server name "spire".

use async_trait::async_trait;
use rust_mcp_sdk::schema::{CallToolResult, Tool};
use serde::Serialize;
use std::path::PathBuf;

use crate::actors::{Actor, ActorError};
use crate::mcp::client::{McpClientManager, McpServerConfig};

/// Structured detail about an MCP server for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct McpServerDetail {
    pub name: String,
    pub description: String,
    pub server_type: String,
    pub tool_count: usize,
    pub properties: serde_json::Value,
}

/// Messages for the MCP Client actor.
pub enum McpClientMessage {
    /// Load config from `~/.spire/mcp-config.json`.
    LoadConfig {
        reply_to: tokio::sync::oneshot::Sender<Result<Option<PathBuf>, ActorError>>,
    },
    /// Add a single server configuration programmatically.
    AddConfig {
        config: McpServerConfig,
        reply_to: tokio::sync::oneshot::Sender<Result<(), ActorError>>,
    },
    /// Connect to all configured servers.
    ConnectAll {
        reply_to: tokio::sync::oneshot::Sender<Result<(), ActorError>>,
    },
    /// Connect to a single server by name.
    Connect {
        server_name: String,
        reply_to: tokio::sync::oneshot::Sender<Result<(), ActorError>>,
    },
    /// Disconnect from all servers.
    DisconnectAll {
        reply_to: tokio::sync::oneshot::Sender<Result<(), ActorError>>,
    },
    /// Disconnect from a single server.
    Disconnect {
        server_name: String,
        reply_to: tokio::sync::oneshot::Sender<Result<(), ActorError>>,
    },
    /// Get the list of tools exposed by a connected server.
    /// For the pseudo-server "spire", returns internal tools.
    GetTools {
        server_name: String,
        reply_to: tokio::sync::oneshot::Sender<Option<Vec<Tool>>>,
    },
    /// Get the names of all connected servers.
    ConnectedServers {
        reply_to: tokio::sync::oneshot::Sender<Vec<String>>,
    },
    /// Get all connected servers with their discovered tools.
    GetConnectedServersWithTools {
        reply_to: tokio::sync::oneshot::Sender<Vec<(String, Vec<rust_mcp_sdk::schema::Tool>)>>,
    },
    /// Get structured details about all servers (for the UI).
    /// Includes the pseudo "spire" server with internal tools.
    GetServerDetails {
        reply_to: tokio::sync::oneshot::Sender<Vec<McpServerDetail>>,
    },
    /// Call a tool on a specific external MCP server.
    CallTool {
        server_name: String,
        tool_name: String,
        arguments: Option<serde_json::Map<String, serde_json::Value>>,
        reply_to: tokio::sync::oneshot::Sender<Result<CallToolResult, ActorError>>,
    },
    /// Set the internal tools that appear under the pseudo "spire" server.
    SetInternalTools {
        tools: Vec<Tool>,
        reply_to: tokio::sync::oneshot::Sender<Result<(), ActorError>>,
    },
}

/// Actor that wraps `McpClientManager` behind message-passing.
///
/// Also holds a set of internal tools that are presented as a pseudo-MCP
/// server named "spire" in the UI.
pub struct McpClientActor {
    manager: McpClientManager,
    /// Internal tools exposed under the pseudo "spire" server.
    internal_tools: Vec<Tool>,
}

impl McpClientActor {
    pub fn new() -> Self {
        Self {
            manager: McpClientManager::new(),
            internal_tools: Vec::new(),
        }
    }
}

impl Default for McpClientActor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Actor for McpClientActor {
    type Message = McpClientMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            McpClientMessage::LoadConfig { reply_to } => {
                let result = self
                    .manager
                    .load_config()
                    .map_err(|e| ActorError::Internal(format!("Failed to load config: {}", e)));
                let _ = reply_to.send(result);
            }
            McpClientMessage::AddConfig { config, reply_to } => {
                self.manager.add_config(config);
                let _ = reply_to.send(Ok(()));
            }
            McpClientMessage::ConnectAll { reply_to } => {
                self.manager.connect_all().await;
                let _ = reply_to.send(Ok(()));
            }
            McpClientMessage::Connect {
                server_name,
                reply_to,
            } => {
                let result = self
                    .manager
                    .connect(&server_name)
                    .await
                    .map_err(|e| ActorError::Internal(format!("Failed to connect: {}", e)));
                let _ = reply_to.send(result);
            }
            McpClientMessage::DisconnectAll { reply_to } => {
                self.manager.disconnect_all().await;
                let _ = reply_to.send(Ok(()));
            }
            McpClientMessage::Disconnect {
                server_name,
                reply_to,
            } => {
                self.manager.disconnect(&server_name).await;
                let _ = reply_to.send(Ok(()));
            }
            McpClientMessage::GetTools {
                server_name,
                reply_to,
            } => {
                // Check the pseudo "spire" server first
                if server_name == "spire" {
                    let _ = reply_to.send(Some(self.internal_tools.clone()));
                    return;
                }
                let tools = self
                    .manager
                    .get_tools(&server_name)
                    .map(|t| t.to_vec());
                let _ = reply_to.send(tools);
            }
            McpClientMessage::ConnectedServers { reply_to } => {
                let mut servers: Vec<String> = self
                    .manager
                    .connected_servers()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect();
                // Always include the pseudo "spire" server if it has tools
                if !self.internal_tools.is_empty() {
                    servers.push("spire".to_string());
                }
                let _ = reply_to.send(servers);
            }
            McpClientMessage::GetConnectedServersWithTools { reply_to } => {
                let mut result: Vec<(String, Vec<rust_mcp_sdk::schema::Tool>)> = self
                    .manager
                    .connected_servers()
                    .into_iter()
                    .map(|name| {
                        let tools = self.manager.get_tools(name)
                            .map(|t| t.to_vec())
                            .unwrap_or_default();
                        (name.to_string(), tools)
                    })
                    .collect();
                // Always include the pseudo "spire" server if it has tools
                if !self.internal_tools.is_empty() {
                    result.push(("spire".to_string(), self.internal_tools.clone()));
                }
                let _ = reply_to.send(result);
            }
            McpClientMessage::GetServerDetails { reply_to } => {
                let details = self.build_server_details();
                let _ = reply_to.send(details);
            }
            McpClientMessage::CallTool {
                server_name,
                tool_name,
                arguments,
                reply_to,
            } => {
                let result = self
                    .manager
                    .call_tool(&server_name, &tool_name, arguments)
                    .await
                    .map_err(|e| ActorError::Internal(format!("Tool call failed: {}", e)));
                let _ = reply_to.send(result);
            }
            McpClientMessage::SetInternalTools { tools, reply_to } => {
                self.internal_tools = tools;
                let _ = reply_to.send(Ok(()));
            }
        }
    }
}

impl McpClientActor {
    /// Build structured server details for the UI, matching the format
    /// expected by the webview (see mock-env-server.mjs).
    ///
    /// Includes the pseudo "spire" server with internal tools.
    /// Only includes servers that have successfully connected (real runtime,
    /// not placeholder connections from failed attempts).
    fn build_server_details(&self) -> Vec<McpServerDetail> {
        let mut details: Vec<McpServerDetail> = self.manager.connected_servers()
            .into_iter()
            .filter_map(|name| {
                // Skip placeholder connections (failed to connect)
                let server_info = self.manager.get_server_info(name)?;
                if server_info.server_info.name.is_empty() {
                    return None;
                }

                let tools = self.manager.get_tools(name).map(|t| t.to_vec()).unwrap_or_default();
                let tool_count = tools.len();

                Some(McpServerDetail {
                    name: name.to_string(),
                    description: String::new(),
                    server_type: "external".to_string(),
                    tool_count,
                    properties: serde_json::json!({
                        "status": "online",
                    }),
                })
            })
            .collect();

        // Add the pseudo "spire" server with internal tools
        if !self.internal_tools.is_empty() {
            details.push(McpServerDetail {
                name: "spire".to_string(),
                description: "Built-in Spire tools (VS Code extension API)".to_string(),
                server_type: "internal".to_string(),
                tool_count: self.internal_tools.len(),
                properties: serde_json::json!({
                    "status": "online",
                }),
            });
        }

        details
    }
}
