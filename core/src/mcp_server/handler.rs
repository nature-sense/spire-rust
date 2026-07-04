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
use std::sync::Arc;
use rust_mcp_sdk::{
    mcp_server::ServerHandler,
    schema::*,
    schema::schema_utils::CallToolError,
    McpServer,
};

use crate::mcp_server::server::MCPServer;

/// Bridges the actor-based `MCPServer` to the `rust-mcp-sdk` `ServerHandler` trait.
///
/// This handler translates MCP protocol requests (list_tools, call_tool) into
/// actor system messages, and converts actor responses back into MCP responses.
pub struct MCPActorHandler {
    server: Arc<MCPServer>,
}

impl MCPActorHandler {
    pub fn new(server: Arc<MCPServer>) -> Self {
        Self { server }
    }
}

#[async_trait]
impl ServerHandler for MCPActorHandler {
    /// Handle `tools/list` — returns all registered tool metadata.
    async fn handle_list_tools_request(
        &self,
        _request: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<ListToolsResult, RpcError> {
        let tools: Vec<Tool> = self
            .server
            .tools()
            .iter()
            .map(|info| {
                let input_schema = build_input_schema(&info.input_schema);

                Tool {
                    name: info.name.clone(),
                    description: Some(info.description.clone()),
                    input_schema,
                    annotations: None,
                    execution: None,
                    icons: vec![],
                    meta: None,
                    output_schema: None,
                    title: Some(info.name.clone()),
                }
            })
            .collect();

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
            meta: None,
        })
    }

    /// Handle `tools/call` — dispatches to the actor system and returns the result.
    async fn handle_call_tool_request(
        &self,
        params: CallToolRequestParams,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<CallToolResult, CallToolError> {
        let args = serde_json::Value::Object(params.arguments.unwrap_or_default());

        let result = self
            .server
            .call_tool(&params.name, args)
            .await
            .map_err(|e| CallToolError::from_message(format!("Tool call failed: {}", e)))?;

        Ok(CallToolResult::text_content(vec![TextContent::new(
            result.to_string(),
            None,
            None,
        )]))
    }
}

/// Convert a `ToolInfo.input_schema` JSON value into a `ToolInputSchema`.
///
/// This parses the JSON schema object and extracts properties and required fields.
fn build_input_schema(schema: &serde_json::Value) -> ToolInputSchema {
    let properties = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|props| {
            props
                .iter()
                .map(|(k, v)| {
                    let prop_schema = v.as_object().cloned().unwrap_or_default();
                    (k.clone(), prop_schema)
                })
                .collect()
        });

    let required = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        });

    ToolInputSchema::new(required.unwrap_or_default(), properties, None)
}
