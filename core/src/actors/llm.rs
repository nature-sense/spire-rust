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

use anyhow::Result;
use tonari_actor::{Actor, Context};
use tracing::info;

/// Messages for the LLM gateway actor.
#[allow(dead_code)]
pub enum LlmMessage {
    Complete {
        prompt: String,
        reply_to: tokio::sync::oneshot::Sender<Result<String>>,
    },
    Stream {
        prompt: String,
        reply_to: tokio::sync::oneshot::Sender<tokio::sync::mpsc::Receiver<String>>,
    },
}

/// LLM gateway client actor.
///
/// Handles LLM requests and can stream tokens back via progress events.
#[allow(dead_code)]
pub struct LlmActor;

impl LlmActor {
    pub fn new() -> Self {
        Self
    }
}

impl Actor for LlmActor {
    type Message = LlmMessage;
    type Error = anyhow::Error;
    type Context = Context<Self::Message>;

    fn handle(
        &mut self,
        _ctx: &mut Self::Context,
        msg: Self::Message,
    ) -> Result<(), Self::Error> {
        match msg {
            LlmMessage::Complete { prompt, reply_to } => {
                info!("LLM: complete ({} chars)", prompt.len());
                // Stub: echo back
                let _ = reply_to.send(Ok(format!("Echo: {}", &prompt[..prompt.len().min(100)])));
            }
            LlmMessage::Stream { prompt, reply_to } => {
                info!("LLM: stream ({} chars)", prompt.len());
                let (tx, rx) = tokio::sync::mpsc::channel(64);
                let _ = reply_to.send(rx);
                // Stub: send a single token
                let _ = tx.try_send("Stub response".to_string());
            }
        }
        Ok(())
    }
}
