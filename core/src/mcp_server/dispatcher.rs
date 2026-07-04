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

use std::collections::HashMap;
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::framework::{Actor, ToolMessage, ToolInfo, ActorError};

/// Internal message for the dispatcher — either route a tool call or register a tool.
pub enum DispatchMessage {
    /// Route a tool call to the appropriate tool actor.
    Route(ToolMessage),
    /// Register a new tool actor with the dispatcher.
    Register {
        name: String,
        sender: mpsc::Sender<ToolMessage>,
        info: ToolInfo,
    },
}

/// Routes `ToolMessage`s to the correct tool actor based on the tool name.
///
/// The dispatcher maintains a registry of tool name → sender channel mappings.
/// When a `ToolMessage` arrives, it looks up the sender and forwards the call.
/// If no tool matches, it responds with `ActorError::ToolNotFound`.
pub struct DispatcherActor {
    tool_senders: HashMap<String, mpsc::Sender<ToolMessage>>,
    tool_infos: Vec<ToolInfo>,
}

impl DispatcherActor {
    pub fn new() -> Self {
        Self {
            tool_senders: HashMap::new(),
            tool_infos: Vec::new(),
        }
    }

    /// Return the list of registered tool metadata.
    pub fn list_tools(&self) -> &[ToolInfo] {
        &self.tool_infos
    }
}

#[async_trait]
impl Actor for DispatcherActor {
    type Message = DispatchMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            DispatchMessage::Register { name, sender, info } => {
                self.tool_senders.insert(name, sender);
                self.tool_infos.push(info);
            }
            DispatchMessage::Route(tool_msg) => {
                let tool_name = tool_msg.tool.clone();
                if let Some(sender) = self.tool_senders.get(&tool_name) {
                    if let Err(_) = sender.send(tool_msg).await {
                        // Channel closed — tool actor died
                        tracing::warn!("Tool actor channel closed for '{}'", tool_name);
                    }
                } else {
                    let _ = tool_msg.response_tx.send(Err(ActorError::ToolNotFound(tool_name)));
                }
            }
        }
    }
}
