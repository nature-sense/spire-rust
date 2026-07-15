// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! ToolsActor — manages tool registry and dispatch.
//!
//! This actor stores registered tools and handles tool invocation requests.
//! Tools can be registered from external MCP servers or from the VS Code extension.

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use crate::actors::{Actor, ActorError, ToolInfo, vscode_tool_definitions};
use crate::actors::project_query::ProjectQueryActor;

/// A registered tool with its handler.
pub struct RegisteredTool {
    pub info: ToolInfo,
    /// The server that provides this tool.
    pub server: String,
}

/// Messages for the Tools actor.
pub enum ToolsMessage {
    /// Register a tool.
    RegisterTool {
        server: String,
        info: ToolInfo,
        reply_to: tokio::sync::oneshot::Sender<Result<(), ActorError>>,
    },
    /// Unregister all tools from a server.
    UnregisterServer {
        server: String,
        reply_to: tokio::sync::oneshot::Sender<Result<(), ActorError>>,
    },
    /// List all registered tools.
    ListTools {
        reply_to: tokio::sync::oneshot::Sender<Vec<ToolInfo>>,
    },
    /// Call a tool by name.
    CallTool {
        tool: String,
        args: Value,
        reply_to: tokio::sync::oneshot::Sender<Result<Value, ActorError>>,
    },
    /// Register all VS Code extension tools at once.
    RegisterVscodeTools {
        reply_to: tokio::sync::oneshot::Sender<Result<(), ActorError>>,
    },
}

/// Actor that manages tool registration and dispatch.
pub struct ToolsActor {
    tools: HashMap<String, RegisteredTool>,
}

impl ToolsActor {
    pub fn new() -> Self {
        let mut tools = HashMap::new();

        // Register all VS Code extension tools at startup
        for tool_info in vscode_tool_definitions() {
            tools.insert(tool_info.name.clone(), RegisteredTool {
                info: tool_info,
                server: "vscode-extension".to_string(),
            });
        }

        // Register all project query tools (memory graph queries) at startup
        for tool_info in ProjectQueryActor::tool_definitions() {
            tools.insert(tool_info.name.clone(), RegisteredTool {
                info: tool_info,
                server: "spire-core".to_string(),
            });
        }

        Self {
            tools,
        }
    }
}

impl Default for ToolsActor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Actor for ToolsActor {
    type Message = ToolsMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            ToolsMessage::RegisterTool {
                server,
                info,
                reply_to,
            } => {
                self.tools.insert(info.name.clone(), RegisteredTool {
                    info,
                    server,
                });
                let _ = reply_to.send(Ok(()));
            }
            ToolsMessage::UnregisterServer { server, reply_to } => {
                self.tools.retain(|_, t| t.server != server);
                let _ = reply_to.send(Ok(()));
            }
            ToolsMessage::ListTools { reply_to } => {
                let tools: Vec<ToolInfo> = self.tools.values().map(|t| t.info.clone()).collect();
                let _ = reply_to.send(tools);
            }
            ToolsMessage::CallTool {
                tool,
                args,
                reply_to,
            } => {
                let result = self.call_tool(&tool, args);
                let _ = reply_to.send(result);
            }
            ToolsMessage::RegisterVscodeTools { reply_to } => {
                for tool_info in vscode_tool_definitions() {
                    self.tools.insert(tool_info.name.clone(), RegisteredTool {
                        info: tool_info,
                        server: "vscode-extension".to_string(),
                    });
                }
                let _ = reply_to.send(Ok(()));
            }
        }
    }
}

impl ToolsActor {
    fn call_tool(&self, tool_name: &str, _args: Value) -> Result<Value, ActorError> {
        let tool = self.tools.get(tool_name)
            .ok_or_else(|| ActorError::ToolNotFound(tool_name.to_string()))?;

        // If this is a VS Code extension tool, return a special response
        // that tells the coordinator to forward the call to the extension.
        if tool.server == "vscode-extension" {
            return Err(ActorError::Internal(format!(
                "Tool '{}' is provided by vscode-extension — coordinator must forward it", tool_name
            )));
        }

        // If this is a project query tool (memory graph), tell the coordinator
        // to forward the call to the ProjectQueryActor.
        if tool.server == "spire-core" {
            return Err(ActorError::Internal(format!(
                "Tool '{}' is provided by spire-core — coordinator must forward it to ProjectQueryActor", tool_name
            )));
        }

        // External MCP tools are dispatched by the MCP client actor.
        Err(ActorError::Internal(format!(
            "Tool '{}' is provided by '{}' — dispatch must go through MCP client actor", tool_name, tool.server
        )))
    }
}
