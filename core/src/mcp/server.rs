use async_trait::async_trait;
use rust_mcp_sdk::{
    mcp_server::ServerHandler,
    schema::*,
    McpServer,
};
use std::sync::Arc;

use crate::actors::coordinator::CoordinatorActor;
use crate::mcp::tools::{get_tools, handle_tool_call};

/// The main MCP server handler for Spire.
///
/// Implements the `ServerHandler` trait to handle MCP protocol requests
/// such as listing tools and calling tools.
pub struct SpireMcpHandler {
    /// Reference to the coordinator actor for orchestrating work
    coordinator: Option<CoordinatorActor>,
}

impl SpireMcpHandler {
    pub fn new() -> Self {
        Self { coordinator: None }
    }

    /// Initialize the coordinator actor system
    pub async fn init_coordinator(&mut self) {
        // TODO: Initialize the tonari-actor system and coordinator
        // This will be implemented when the actor system is set up
        tracing::info!("Coordinator initialization placeholder");
    }
}

impl Default for SpireMcpHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServerHandler for SpireMcpHandler {
    /// Handles requests to list available tools.
    async fn handle_list_tools_request(
        &self,
        _request: Option<PaginatedRequestParams>,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<ListToolsResult, RpcError> {
        tracing::debug!("Handling list_tools request");
        Ok(ListToolsResult {
            tools: get_tools(),
            meta: None,
            next_cursor: None,
        })
    }

    /// Handles requests to call a specific tool.
    async fn handle_call_tool_request(
        &self,
        params: CallToolRequestParams,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<CallToolResult, rust_mcp_sdk::schema::schema_utils::CallToolError> {
        tracing::debug!("Handling call_tool request: {}", params.name);

        let arguments = params.arguments.unwrap_or_default();
        handle_tool_call(&params.name, arguments)
    }
}
