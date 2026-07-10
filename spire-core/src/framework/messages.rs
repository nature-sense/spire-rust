// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

use serde_json::Value;
use tokio::sync::oneshot;
use thiserror::Error;

/// A sender for responding to actor requests.
pub type Responder<T> = oneshot::Sender<Result<T, ActorError>>;

/// Typed errors for the actor system.
#[derive(Debug, Error)]
pub enum ActorError {
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Channel closed")]
    ChannelClosed,

    #[error("Internal error: {0}")]
    Internal(String),
}

/// A request to list all registered tools.
pub struct ListToolsMessage {
    pub response_tx: Responder<Vec<ToolInfo>>,
}

/// A generic tool invocation message.
pub struct ToolMessage {
    pub tool: String,
    pub args: Value,
    pub response_tx: Responder<Value>,
}

/// Metadata describing a registered tool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}
