// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Core Actor trait for the actor-based architecture.
///
/// All components in the system implement this trait, ensuring consistent
/// message-passing semantics across the entire codebase.
#[async_trait]
pub trait Actor: Send + 'static {
    type Message: Send + 'static;

    /// Handle a single message asynchronously.
    async fn handle(&mut self, msg: Self::Message);

    /// Spawn this actor on a Tokio task, processing messages from the receiver.
    fn spawn(mut self, mut rx: mpsc::Receiver<Self::Message>) -> JoinHandle<()>
    where
        Self: Sized,
    {
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                self.handle(msg).await;
            }
        })
    }
}
