// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! ToolRouterActor — routes tool calls to the appropriate backend as an actor.
//!
//! This module replaces the previous `ToolDispatcher` + three `ToolProvider`
//! implementations with a single actor that routes tool calls by prefix matching.
//!
//! Routing:
//!
//! | Prefix | Backend |
//! |---|---|
//! | `workspace/`, `document/`, `diagnostics/`, `git/`, `symbols/` | VS Code extension via TransportActor |
//! | `project/` (except `project/build`) | ProjectQueryActor |
//! | `project/build` | ProjectBuildActor |
//! | (catch-all) | McpClientActor (external MCP servers, e.g. mcp-cargo, mcp-node) |

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::actors::ToolInfo;
use crate::actors::project_query::ProjectQueryMessage;
use crate::actors::project_build::ProjectBuildMessage;
use crate::actors::project_test::ProjectTestMessage;
use crate::actors::project_lint::ProjectLintMessage;
use crate::actors::project_install::ProjectInstallMessage;
use crate::actors::mcp_client::McpClientMessage;
use crate::actors::vscode_tool_definitions;
use crate::transport::socket::TransportMessage;

/// Messages for the ToolRouterActor.
pub enum ToolRouterMessage {
    /// List all available tools from all backends.
    ListTools {
        reply_to: oneshot::Sender<Vec<ToolInfo>>,
    },
    /// Call a tool by name, routing to the appropriate backend.
    CallTool {
        tool_name: String,
        args: Value,
        reply_to: oneshot::Sender<Result<Value, String>>,
    },
}

/// Actor that routes tool calls to the appropriate backend.
pub struct ToolRouterActor {
    /// Sender to the TransportActor (for forwarding VS Code extension tool calls).
    transport_tx: mpsc::Sender<TransportMessage>,
    /// Sender to the McpClientActor (for external MCP servers).
    mcp_client_tx: mpsc::Sender<McpClientMessage>,
    /// Sender to the ProjectQueryActor (for project/ tools).
    project_query_tx: mpsc::Sender<ProjectQueryMessage>,
    /// Sender to the ProjectBuildActor (for project/build).
    project_build_tx: mpsc::Sender<ProjectBuildMessage>,
    /// Sender to the ProjectTestActor (for project/test, project/check).
    project_test_tx: mpsc::Sender<ProjectTestMessage>,
    /// Sender to the ProjectLintActor (for project/lint, project/format).
    project_lint_tx: mpsc::Sender<ProjectLintMessage>,
    /// Sender to the ProjectInstallActor (for project/install, project/add_dependency).
    project_install_tx: mpsc::Sender<ProjectInstallMessage>,
}

impl ToolRouterActor {
    pub fn new(
        transport_tx: mpsc::Sender<TransportMessage>,
        mcp_client_tx: mpsc::Sender<McpClientMessage>,
        project_query_tx: mpsc::Sender<ProjectQueryMessage>,
        project_build_tx: mpsc::Sender<ProjectBuildMessage>,
        project_test_tx: mpsc::Sender<ProjectTestMessage>,
        project_lint_tx: mpsc::Sender<ProjectLintMessage>,
        project_install_tx: mpsc::Sender<ProjectInstallMessage>,
    ) -> Self {
        Self {
            transport_tx,
            mcp_client_tx,
            project_query_tx,
            project_build_tx,
            project_test_tx,
            project_lint_tx,
            project_install_tx,
        }
    }

    /// Check if a tool name belongs to the VS Code extension.
    fn is_extension_tool(tool_name: &str) -> bool {
        let prefixes = [
            "workspace/",
            "document/",
            "diagnostics/",
            "git/",
            "symbols/",
        ];
        prefixes.iter().any(|p| tool_name.starts_with(p))
    }

    /// List tools from the VS Code extension (static definitions).
    async fn list_extension_tools(&self) -> Vec<ToolInfo> {
        vscode_tool_definitions()
    }

    /// List tools from the MCP client (connected servers).
    /// Filters out the pseudo "spire" server since its tools are already
    /// covered by list_extension_tools() and list_embedded_tools().
    /// MCP tool names are prefixed with their server name (e.g. "mcp-cargo/build")
    /// to avoid collisions between servers that define tools with the same name
    /// (e.g. both mcp-cargo and mcp-node define "build", "test", "analyze", etc.).
    async fn list_mcp_tools(&self) -> Vec<ToolInfo> {
        let (tx, rx) = oneshot::channel();
        if self.mcp_client_tx
            .send(McpClientMessage::GetConnectedServersWithTools { reply_to: tx })
            .await
            .is_err()
        {
            return Vec::new();
        }

        match rx.await {
            Ok(servers) => {
                let mut tools = Vec::new();
                for (server_name, server_tools) in servers {
                    // Skip the pseudo "spire" server — its tools are already
                    // included via list_extension_tools() and list_embedded_tools().
                    if server_name == "spire" {
                        continue;
                    }
                    for t in server_tools {
                        // Prefix tool names with the server name to avoid collisions
                        // between MCP servers that define tools with the same name.
                        // E.g. "mcp-cargo/build", "mcp-node/build", "mcp-filesystem/search_files"
                        tools.push(ToolInfo {
                            name: format!("{}/{}", server_name, t.name),
                            description: t.description.unwrap_or_default(),
                            input_schema: serde_json::to_value(t.input_schema)
                                .unwrap_or(serde_json::Value::Null),
                        });
                    }
                }
                tools
            }
            Err(_) => Vec::new(),
        }
    }

    /// List tools from embedded actors.
    async fn list_embedded_tools(&self) -> Vec<ToolInfo> {
        let mut tools = Vec::new();
        tools.extend(crate::actors::project_query::ProjectQueryActor::tool_definitions());
        tools.extend(crate::actors::project_build::ProjectBuildActor::tool_definitions());
        tools.extend(crate::actors::project_test::ProjectTestActor::tool_definitions());
        tools.extend(crate::actors::project_lint::ProjectLintActor::tool_definitions());
        tools.extend(crate::actors::project_install::ProjectInstallActor::tool_definitions());
        tools
    }

    /// Call a VS Code extension tool via the TransportActor.
    async fn call_extension_tool(&self, tool_name: &str, args: Value) -> Result<Value, String> {
        let (tx, rx) = oneshot::channel();
        self.transport_tx
            .send(TransportMessage::CallExtension {
                method: tool_name.to_string(),
                params: args,
                reply_to: tx,
            })
            .await
            .map_err(|e| format!("Transport send error: {}", e))?;

        rx.await
            .map_err(|e| format!("Transport response error: {}", e))?
    }

    /// Call an embedded tool (project/).
    async fn call_embedded_tool(&self, tool_name: &str, args: Value) -> Result<Value, String> {
        match tool_name {
            "project/build" => {
                let (tx, rx) = oneshot::channel();
                self.project_build_tx
                    .send(ProjectBuildMessage::CallTool {
                        tool: tool_name.to_string(),
                        args,
                        reply_to: tx,
                    })
                    .await
                    .map_err(|e| format!("ProjectBuild actor send error: {}", e))?;
                rx.await.map_err(|e| format!("ProjectBuild actor response error: {}", e))?
            }
            "project/test" | "project/check" => {
                let (tx, rx) = oneshot::channel();
                self.project_test_tx
                    .send(ProjectTestMessage::CallTool {
                        tool: tool_name.to_string(),
                        args,
                        reply_to: tx,
                    })
                    .await
                    .map_err(|e| format!("ProjectTest actor send error: {}", e))?;
                rx.await.map_err(|e| format!("ProjectTest actor response error: {}", e))?
            }
            "project/lint" | "project/format" => {
                let (tx, rx) = oneshot::channel();
                self.project_lint_tx
                    .send(ProjectLintMessage::CallTool {
                        tool: tool_name.to_string(),
                        args,
                        reply_to: tx,
                    })
                    .await
                    .map_err(|e| format!("ProjectLint actor send error: {}", e))?;
                rx.await.map_err(|e| format!("ProjectLint actor response error: {}", e))?
            }
            "project/install" | "project/add_dependency" => {
                let (tx, rx) = oneshot::channel();
                self.project_install_tx
                    .send(ProjectInstallMessage::CallTool {
                        tool: tool_name.to_string(),
                        args,
                        reply_to: tx,
                    })
                    .await
                    .map_err(|e| format!("ProjectInstall actor send error: {}", e))?;
                rx.await.map_err(|e| format!("ProjectInstall actor response error: {}", e))?
            }
            _ if tool_name.starts_with("project/") => {
                // Fallback: route to ProjectQueryActor
                let (tx, rx) = oneshot::channel();
                self.project_query_tx
                    .send(ProjectQueryMessage::CallTool {
                        tool: tool_name.to_string(),
                        args,
                        reply_to: tx,
                    })
                    .await
                    .map_err(|e| format!("ProjectQuery actor send error: {}", e))?;
                rx.await
                    .map_err(|e| format!("ProjectQuery actor response error: {}", e))
                    .map(Ok)?
            }
            _ => Err(format!("ToolRouterActor: unknown embedded tool '{}'", tool_name)),
        }
    }

    /// Call an MCP tool via the McpClientActor.
    ///
    /// MCP tool names are expected to be prefixed with their server name
    /// (e.g. "mcp-cargo/build", "mcp-node/test"). This method parses the
    /// prefix to route the call to the correct MCP server.
    async fn call_mcp_tool(&self, tool_name: &str, args: Value) -> Result<Value, String> {
        // Parse the server name from the prefixed tool name.
        // Format: "server_name/tool_name" (e.g. "mcp-cargo/build")
        let (server_name, actual_tool_name) = match tool_name.split_once('/') {
            Some((server, tool)) => (server.to_string(), tool.to_string()),
            None => {
                // No prefix found — use empty server name (let McpClientActor find it)
                // and the full tool_name as-is. This handles backward compatibility
                // for any direct tool calls that don't use the prefixed format.
                (String::new(), tool_name.to_string())
            }
        };

        let (tx, rx) = oneshot::channel();
        self.mcp_client_tx
            .send(McpClientMessage::CallTool {
                server_name,
                tool_name: actual_tool_name,
                arguments: args.as_object().cloned(),
                reply_to: tx,
            })
            .await
            .map_err(|e| format!("MCP client send error: {}", e))?;

        match rx.await {
            Ok(Ok(result)) => {
                serde_json::to_value(result)
                    .map_err(|e| format!("MCP result serialization error: {}", e))
            }
            Ok(Err(e)) => Err(format!("MCP tool call error: {}", e)),
            Err(_) => Err("MCP client response error".to_string()),
        }
    }
}

#[async_trait]
impl crate::actors::Actor for ToolRouterActor {
    type Message = ToolRouterMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            ToolRouterMessage::ListTools { reply_to } => {
                let mut all_tools = Vec::new();

                // Extension tools
                all_tools.extend(self.list_extension_tools().await);

                // Embedded tools
                all_tools.extend(self.list_embedded_tools().await);

                // MCP tools
                all_tools.extend(self.list_mcp_tools().await);

                let _ = reply_to.send(all_tools);
            }

            ToolRouterMessage::CallTool { tool_name, args, reply_to } => {
                let result = if Self::is_extension_tool(&tool_name) {
                    self.call_extension_tool(&tool_name, args).await
                } else if tool_name.starts_with("project/") {
                    self.call_embedded_tool(&tool_name, args).await
                } else {
                    // Catch-all: MCP
                    self.call_mcp_tool(&tool_name, args).await
                };

                let _ = reply_to.send(result);
            }
        }
    }
}
