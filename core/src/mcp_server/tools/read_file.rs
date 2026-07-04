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
use std::path::PathBuf;

use crate::framework::{Actor, ToolMessage, ToolInfo, ActorError};

/// Reads a file from the filesystem.
pub struct ReadFileActor {
    root: PathBuf,
}

impl ReadFileActor {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn tool_info() -> ToolInfo {
        ToolInfo {
            name: "read_file".to_string(),
            description: "Reads a file from the filesystem".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file relative to workspace root"
                    }
                },
                "required": ["path"]
            }),
        }
    }
}

#[async_trait]
impl Actor for ReadFileActor {
    type Message = ToolMessage;

    async fn handle(&mut self, msg: Self::Message) {
        let result = if msg.tool != "read_file" {
            Err(ActorError::ToolNotFound(msg.tool))
        } else {
            self.read_file(&msg.args).await
        };
        let _ = msg.response_tx.send(result);
    }
}

impl ReadFileActor {
    async fn read_file(&self, args: &Value) -> Result<Value, ActorError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ActorError::Internal("path must be a string".to_string()))?;

        let full_path = self.root.join(path);
        let content = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| ActorError::Internal(format!("Failed to read file: {}", e)))?;

        Ok(Value::String(content))
    }
}
