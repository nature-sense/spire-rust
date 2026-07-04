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
use serde_json::Value;

use crate::framework::{Actor, ToolMessage, ToolInfo, ActorError};

/// A simple echo tool actor — demonstrates the tool actor pattern.
///
/// Accepts a `message` argument and echoes it back.
pub struct EchoTool;

impl EchoTool {
    pub fn tool_info() -> ToolInfo {
        ToolInfo {
            name: "echo".to_string(),
            description: "Echoes back the input message".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Message to echo back"
                    }
                },
                "required": ["message"]
            }),
        }
    }
}

#[async_trait]
impl Actor for EchoTool {
    type Message = ToolMessage;

    async fn handle(&mut self, msg: Self::Message) {
        let result = if msg.tool != "echo" {
            Err(ActorError::ToolNotFound(msg.tool))
        } else {
            let message = msg
                .args
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("(no message provided)");
            Ok(Value::String(format!("Echo: {}", message)))
        };
        let _ = msg.response_tx.send(result);
    }
}
