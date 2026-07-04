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

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Core Actor trait for the new actor-based architecture.
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
