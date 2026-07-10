// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! SystemActor — lifecycle, health, and configuration management.
//!
//! This actor handles system-level operations such as status reporting,
//! graceful shutdown, and configuration queries.

use async_trait::async_trait;
use serde_json::Value;

use crate::actors::{Actor, ActorError};

/// Messages for the System actor.
pub enum SystemMessage {
    /// Get system status.
    GetStatus {
        reply_to: tokio::sync::oneshot::Sender<Value>,
    },
    /// Graceful shutdown.
    Shutdown {
        reply_to: tokio::sync::oneshot::Sender<Result<(), ActorError>>,
    },
    /// Get a configuration value by key.
    GetConfig {
        key: String,
        reply_to: tokio::sync::oneshot::Sender<Option<Value>>,
    },
}

/// Actor that manages system lifecycle and health.
pub struct SystemActor {
    start_time: std::time::Instant,
    config: std::collections::HashMap<String, Value>,
}

impl SystemActor {
    pub fn new() -> Self {
        Self {
            start_time: std::time::Instant::now(),
            config: std::collections::HashMap::new(),
        }
    }

    fn uptime_seconds(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }
}

impl Default for SystemActor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Actor for SystemActor {
    type Message = SystemMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            SystemMessage::GetStatus { reply_to } => {
                let status = serde_json::json!({
                    "status": "running",
                    "uptime_seconds": self.uptime_seconds(),
                    "version": env!("CARGO_PKG_VERSION"),
                    "actors": {
                        "chat": true,
                        "tools": true,
                        "mcp_client": true,
                        "llm": true,
                        "progress": true,
                        "system": true,
                    }
                });
                let _ = reply_to.send(status);
            }
            SystemMessage::Shutdown { reply_to } => {
                tracing::info!("SystemActor: initiating graceful shutdown");
                // Signal shutdown — the main loop will handle actual process exit
                let _ = reply_to.send(Ok(()));
            }
            SystemMessage::GetConfig { key, reply_to } => {
                let value = self.config.get(&key).cloned();
                let _ = reply_to.send(value);
            }
        }
    }
}
