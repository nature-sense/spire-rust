// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use crate::framework::Actor;

/// Manages a collection of actors running on the ambient Tokio runtime.
///
/// Uses `tokio::spawn` to run actors on the current runtime.
pub struct ActorSystem;

impl ActorSystem {
    /// Create a new actor system.
    pub fn new() -> Self {
        Self
    }

    /// Spawn an actor, returning a sender channel and join handle.
    ///
    /// The actor will process messages from the returned sender on a background
    /// Tokio task until the sender is dropped.
    pub fn spawn<A: Actor>(&self, actor: A) -> (mpsc::Sender<A::Message>, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel::<A::Message>(32);
        let handle = actor.spawn(rx);
        (tx, handle)
    }
}

impl Default for ActorSystem {
    fn default() -> Self {
        Self::new()
    }
}
