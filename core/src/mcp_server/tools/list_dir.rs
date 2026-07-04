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

/// Lists files and directories.
pub struct ListDirActor {
    root: PathBuf,
}

impl ListDirActor {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn tool_info() -> ToolInfo {
        ToolInfo {
            name: "list_dir".to_string(),
            description: "Lists files and directories".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path relative to workspace root (default: root)"
                    }
                }
            }),
        }
    }
}

#[async_trait]
impl Actor for ListDirActor {
    type Message = ToolMessage;

    async fn handle(&mut self, msg: Self::Message) {
        let result = if msg.tool != "list_dir" {
            Err(ActorError::ToolNotFound(msg.tool))
        } else {
            self.list_dir(&msg.args).await
        };
        let _ = msg.response_tx.send(result);
    }
}

impl ListDirActor {
    async fn list_dir(&self, args: &Value) -> Result<Value, ActorError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let full_path = self.root.join(path);
        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&full_path)
            .await
            .map_err(|e| ActorError::Internal(format!("Failed to read directory: {}", e)))?;

        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| ActorError::Internal(format!("Failed to read entry: {}", e)))?
        {
            let file_name = entry.file_name().to_string_lossy().to_string();
            let metadata = entry
                .metadata()
                .await
                .map_err(|e| ActorError::Internal(format!("Failed to get metadata: {}", e)))?;

            entries.push(serde_json::json!({
                "name": file_name,
                "is_dir": metadata.is_dir(),
                "size": metadata.len(),
            }));
        }

        Ok(Value::Array(entries))
    }
}
