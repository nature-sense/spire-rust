use std::collections::BTreeMap;
use std::sync::Arc;

use rust_mcp_sdk::mcp_server::{server_runtime, McpServerOptions, ServerHandlerCore, ServerRuntime};
use rust_mcp_sdk::schema::{
    CallToolResult, Implementation, InitializeResult, ListToolsResult, ProtocolVersion, RpcError,
    ServerCapabilities, ServerCapabilitiesTools, TextContent, Tool, ToolInputSchema,
    ToolOutputSchema,
};
use rust_mcp_sdk::schema::schema_utils::{
    NotificationFromClient, RequestFromClient, ResultFromServer,
};
use rust_mcp_sdk::McpServer;
use rust_mcp_sdk::{StdioTransport, ToMcpServerHandlerCore, TransportOptions};
use async_trait::async_trait;
use tonari_actor::System;

use spire_rust::actors::{
    CoordinatorActor, LlmActor, MemoryGraphActor, ProgressActor,
};
use spire_rust::models::embedding::Embedder;

/// A no-op embedder for testing.
struct TestEmbedder;

#[async_trait::async_trait]
impl Embedder for TestEmbedder {
    async fn embed(&self, _text: &str) -> anyhow::Result<spire_rust::models::embedding::Embedding> {
        Ok(spire_rust::models::embedding::Embedding::new(
            vec![0.0f32; 384],
            "test",
            "test-model",
        ))
    }

    async fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<spire_rust::models::embedding::Embedding>> {
        Ok(texts
            .iter()
            .map(|t| spire_rust::models::embedding::Embedding::new(vec![0.0f32; 384], t, "test-model"))
            .collect())
    }

    fn dimensions(&self) -> usize {
        384
    }
}

/// A test handler that echoes tool calls.
struct TestHandler;

#[async_trait]
impl ServerHandlerCore for TestHandler {
    async fn handle_request(
        &self,
        request: RequestFromClient,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<ResultFromServer, RpcError> {
        match request {
            RequestFromClient::ListToolsRequest(_) => Ok(ResultFromServer::ListToolsResult(
                ListToolsResult {
                    tools: vec![Tool {
                        name: "echo".to_string(),
                        description: Some("Echo test tool".to_string()),
                        title: None,
                        input_schema: ToolInputSchema::new(
                            vec!["message".to_string()],
                            Some(
                                [(
                                    "message".to_string(),
                                    serde_json::json!({
                                        "type": "string",
                                        "description": "Message to echo"
                                    })
                                    .as_object()
                                    .unwrap()
                                    .clone(),
                                )]
                                .into_iter()
                                .collect::<BTreeMap<_, _>>(),
                            ),
                            None,
                        ),
                        output_schema: None,
                        annotations: None,
                        execution: None,
                        icons: vec![],
                        meta: None,
                    }],
                    meta: None,
                    next_cursor: None,
                },
            )),
            RequestFromClient::CallToolRequest(params) => {
                let message = params
                    .arguments
                    .as_ref()
                    .and_then(|args| args.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                Ok(ResultFromServer::CallToolResult(CallToolResult {
                    content: vec![
                        TextContent::new(format!("Echo: {}", message), None, None).into(),
                    ],
                    is_error: Some(false),
                    meta: None,
                    structured_content: None,
                }))
            }
            _ => Err(RpcError::method_not_found()),
        }
    }

    async fn handle_notification(
        &self,
        _notification: NotificationFromClient,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<(), RpcError> {
        Ok(())
    }

    async fn handle_error(
        &self,
        _error: &RpcError,
        _runtime: Arc<dyn McpServer>,
    ) -> std::result::Result<(), RpcError> {
        Ok(())
    }
}

#[tokio::test]
async fn test_actor_system_initialization() {
    let mut system = tonari_actor::System::new("spire-test");

    let embedder = Arc::new(TestEmbedder);
    let memory_graph_addr = system.spawn(MemoryGraphActor::new(embedder)).unwrap();
    let llm_addr = system.spawn(LlmActor::new()).unwrap();
    let progress_addr = system.spawn(ProgressActor::new()).unwrap();

    let _coordinator_addr = system
        .spawn(CoordinatorActor::new(
            memory_graph_addr,
            llm_addr,
            progress_addr,
        ))
        .unwrap();

    // If we got here without panicking, the actor system works
}

#[tokio::test]
async fn test_mcp_server_initialization() {
    let handler = TestHandler;

    let server_info = InitializeResult {
        protocol_version: ProtocolVersion::V2025_11_25.into(),
        capabilities: ServerCapabilities {
            tools: Some(ServerCapabilitiesTools {
                list_changed: Some(false),
            }),
            ..Default::default()
        },
        server_info: Implementation {
            name: "spire-rust-test".to_string(),
            version: "0.1.0".to_string(),
            title: None,
            description: None,
            icons: vec![],
            website_url: None,
        },
        instructions: Some("Test server".to_string()),
        meta: None,
    };

    let transport = StdioTransport::new(TransportOptions::default()).unwrap();

    let runtime: Arc<ServerRuntime> = server_runtime::create_server(McpServerOptions {
        server_details: server_info,
        transport,
        handler: handler.to_mcp_server_handler(),
        task_store: None,
        client_task_store: None,
        message_observer: None,
    });

    // Verify the runtime was created successfully
    assert_eq!(
        runtime.server_info().server_info.name,
        "spire-rust-test"
    );
}

#[tokio::test]
async fn test_mcp_tool_list() {
    let handler = TestHandler;

    let server_info = InitializeResult {
        protocol_version: ProtocolVersion::V2025_11_25.into(),
        capabilities: ServerCapabilities {
            tools: Some(ServerCapabilitiesTools {
                list_changed: Some(false),
            }),
            ..Default::default()
        },
        server_info: Implementation {
            name: "spire-rust-test".to_string(),
            version: "0.1.0".to_string(),
            title: None,
            description: None,
            icons: vec![],
            website_url: None,
        },
        instructions: Some("Test server".to_string()),
        meta: None,
    };

    let transport = StdioTransport::new(TransportOptions::default()).unwrap();

    let runtime: Arc<ServerRuntime> = server_runtime::create_server(McpServerOptions {
        server_details: server_info,
        transport,
        handler: handler.to_mcp_server_handler(),
        task_store: None,
        client_task_store: None,
        message_observer: None,
    });

    // Verify server info
    assert_eq!(
        runtime.server_info().protocol_version,
        "2025-11-25"
    );
    assert!(runtime.server_info().capabilities.tools.is_some());
}
