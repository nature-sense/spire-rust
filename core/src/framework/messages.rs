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
use tokio::sync::oneshot;
use thiserror::Error;

/// A sender for responding to actor requests.
///
/// Used with `oneshot::channel()` to send a result back to the requester.
pub type Responder<T> = oneshot::Sender<Result<T, ActorError>>;

/// Typed errors for the actor system.
#[derive(Debug, Error)]
pub enum ActorError {
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Internal error: {0}")]
    Internal(String),
}

/// A generic tool invocation message.
///
/// Any tool actor receives this message type, dispatches on `tool`,
/// and sends the result back via `response_tx`.
pub struct ToolMessage {
    pub tool: String,
    pub args: Value,
    pub response_tx: Responder<Value>,
}

/// Metadata describing a registered tool.
///
/// Returned in response to `tools/list` requests.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}
