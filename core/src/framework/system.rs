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

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use crate::framework::Actor;

/// Manages a collection of actors running on the ambient Tokio runtime.
///
/// Unlike the previous version, `ActorSystem` no longer owns its own
/// `tokio::runtime::Runtime`. Instead, it uses `tokio::spawn` to run
/// actors on the current runtime. This avoids the "Cannot drop a runtime
/// in a context where blocking is not allowed" panic when used inside
/// `#[tokio::main]`.
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
