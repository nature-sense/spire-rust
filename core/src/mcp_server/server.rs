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

use serde_json::Value;
use tokio::sync::mpsc;

use crate::framework::{ActorSystem, ToolInfo, ToolMessage, ActorError};
use crate::mcp_server::dispatcher::{DispatcherActor, DispatchMessage};

/// Actor-based MCP server that owns the actor system and tool dispatcher.
///
/// `MCPServer` is the top-level coordinator for the new architecture. It:
/// - Owns the `ActorSystem` runtime
/// - Manages the `DispatcherActor` for routing tool calls
/// - Provides a clean API for registering tools and invoking them
///
/// # Example
///
/// ```ignore
/// let mut server = MCPServer::new();
/// server.register_tool(EchoTool::tool_info(), EchoTool);
/// let result = server.call_tool("echo", json!({"message": "hello"})).await;
/// ```
pub struct MCPServer {
    system: ActorSystem,
    dispatcher_tx: mpsc::Sender<DispatchMessage>,
    tools: Vec<ToolInfo>,
}

impl MCPServer {
    /// Create a new MCPServer and initialize the actor system.
    ///
    /// The dispatcher actor is spawned immediately. Tools can be registered
    /// after construction via `register_tool()`.
    pub fn new() -> Self {
        let system = ActorSystem::new();

        // Create and spawn the dispatcher actor
        let dispatcher = DispatcherActor::new();
        let (dispatcher_tx, _handle) = system.spawn(dispatcher);

        Self {
            system,
            dispatcher_tx,
            tools: Vec::new(),
        }
    }

    /// Register a tool actor and record its metadata.
    ///
    /// The tool actor is spawned on the actor system, and the dispatcher
    /// is updated to route calls for this tool name to the new actor.
    pub fn register_tool<A: crate::framework::Actor<Message = ToolMessage>>(
        &mut self,
        tool_info: ToolInfo,
        tool_actor: A,
    ) {
        // Spawn the tool actor and get its sender channel
        let (tx, _handle) = self.system.spawn(tool_actor);

        // Send a registration message to the dispatcher
        let dispatcher_tx = self.dispatcher_tx.clone();
        let name = tool_info.name.clone();
        let info = tool_info.clone();
        tokio::spawn(async move {
            let _ = dispatcher_tx
                .send(DispatchMessage::Register {
                    name,
                    sender: tx,
                    info,
                })
                .await;
        });

        // Store tool info locally for tools/list queries
        self.tools.push(tool_info);
    }

    /// Get the list of registered tool metadata.
    pub fn tools(&self) -> &[ToolInfo] {
        &self.tools
    }

    /// Call a tool by name with the given arguments.
    ///
    /// Sends a `ToolMessage` to the dispatcher actor and awaits the response.
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value, ActorError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.dispatcher_tx
            .send(DispatchMessage::Route(ToolMessage {
                tool: name.to_string(),
                args,
                response_tx: tx,
            }))
            .await
            .map_err(|_| ActorError::ChannelClosed)?;
        rx.await.map_err(|_| ActorError::ChannelClosed)?
    }

    /// Get a reference to the underlying actor system.
    pub fn system(&self) -> &ActorSystem {
        &self.system
    }
}

impl Default for MCPServer {
    fn default() -> Self {
        Self::new()
    }
}
