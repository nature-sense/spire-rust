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

/// Writes content to a file.
pub struct WriteFileActor {
    root: PathBuf,
}

impl WriteFileActor {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn tool_info() -> ToolInfo {
        ToolInfo {
            name: "write_file".to_string(),
            description: "Writes content to a file".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file relative to workspace root"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write"
                    }
                },
                "required": ["path", "content"]
            }),
        }
    }
}

#[async_trait]
impl Actor for WriteFileActor {
    type Message = ToolMessage;

    async fn handle(&mut self, msg: Self::Message) {
        let result = if msg.tool != "write_file" {
            Err(ActorError::ToolNotFound(msg.tool))
        } else {
            self.write_file(&msg.args).await
        };
        let _ = msg.response_tx.send(result);
    }
}

impl WriteFileActor {
    async fn write_file(&self, args: &Value) -> Result<Value, ActorError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ActorError::Internal("path must be a string".to_string()))?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ActorError::Internal("content must be a string".to_string()))?;

        let full_path = self.root.join(path);
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ActorError::Internal(format!("Failed to create parent dir: {}", e)))?;
        }

        tokio::fs::write(&full_path, content)
            .await
            .map_err(|e| ActorError::Internal(format!("Failed to write file: {}", e)))?;

        Ok(Value::Null)
    }
}
