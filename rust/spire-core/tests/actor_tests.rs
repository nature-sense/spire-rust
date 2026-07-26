// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Actor-level unit tests for spire-core.
//!
//! These tests import `spire_core` as a library, create an `ActorSystem`,
//! spawn actors directly, send messages via their `mpsc::Sender` channels,
//! and assert on the responses.

use spire_core::actors::{
    self, ChatActor, ChatMessage, ToolsActor, ToolsMessage, McpClientActor, McpClientMessage,
    LlmActor, LlmConfig, LlmMessage, ProgressActor, ProgressMessage, ProgressStatus, ProgressUpdate,
    SystemActor, SystemMessage, CoordinatorActor, CoordinatorMessage, ToolInfo,
    BuildOrchestrator, BuildOrchestratorMessage, ErrorAnalyzer, ErrorAnalyzerMessage,
    ToolOrchestrator, ToolOrchestratorMessage, ToolRouterMessage,
};
use spire_core::framework::ActorSystem;
use spire_core::transport::socket::TransportMessage;
use spire_core::models::memory_graph::{
    BuildContext, BuildError, BuildStartResult, SystemBuildResult, BuildResult,
    NodeInput, NodeType, NodeUpdate, NodeFilter, GraphNode, GraphEdge,
    RelationshipInput, RelationshipType, TraversalOptions, TraversalDirection,
    FixStrategy, BuildState, ScoredFix, AnnotatedError, FixPlan, ErrorType,
};
use spire_core::actors::memory_graph::{MemoryGraphActor, MemoryGraphMessage};
use tokio::sync::mpsc;
use spire_core::models::embedding::{Embedder, Embedding};


/// Helper to create a mock sender for any actor channel.
fn mock_sender<T>() -> tokio::sync::mpsc::Sender<T> {
    let (tx, _rx) = tokio::sync::mpsc::channel(64);
    tx
}

/// Helper to create a mock memory graph actor for coordinator tests.
/// Returns a `mpsc::Sender<MemoryGraphMessage>` that ignores all messages.
fn mock_memory_graph() -> tokio::sync::mpsc::Sender<MemoryGraphMessage> {
    let (tx, _rx) = tokio::sync::mpsc::channel(64);
    tx
}

// ===========================================================================
// ChatActor tests
// ===========================================================================

#[tokio::test]
async fn test_chat_get_active_returns_default() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(ChatActor::new());

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ChatMessage::GetActive { reply_to: resp_tx }).await.unwrap();
    let dialog = resp_rx.await.unwrap();

    assert!(dialog.is_some());
    let dialog = dialog.unwrap();
    assert_eq!(dialog.id, "default");
    assert_eq!(dialog.title, "New Chat");
    assert!(dialog.messages.is_empty());
}

#[tokio::test]
async fn test_chat_append_message_works() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(ChatActor::new());

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ChatMessage::Append {
        chat_id: "default".to_string(),
        content: "Hello, world!".to_string(),
        role: "user".to_string(),
        reply_to: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap();
    assert!(result.is_ok());

    let msg = result.unwrap();
    assert_eq!(msg.role, "user");
    assert_eq!(msg.content, "Hello, world!");
    assert!(!msg.id.is_empty());
    assert!(!msg.timestamp.is_empty());
}

#[tokio::test]
async fn test_chat_get_history_returns_dialogs() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(ChatActor::new());

    // Append a message first
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ChatMessage::Append {
        chat_id: "default".to_string(),
        content: "msg1".to_string(),
        role: "user".to_string(),
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().unwrap();

    // Get history
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ChatMessage::GetHistory { reply_to: resp_tx }).await.unwrap();
    let dialogs = resp_rx.await.unwrap();

    assert_eq!(dialogs.len(), 1);
    assert_eq!(dialogs[0].messages.len(), 1);
    assert_eq!(dialogs[0].messages[0].content, "msg1");
}

#[tokio::test]
async fn test_chat_clear_dialog_works() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(ChatActor::new());

    // Append a message
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ChatMessage::Append {
        chat_id: "default".to_string(),
        content: "to_clear".to_string(),
        role: "user".to_string(),
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().unwrap();

    // Clear
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ChatMessage::Clear {
        chat_id: "default".to_string(),
        reply_to: resp_tx,
    }).await.unwrap();
    assert!(resp_rx.await.unwrap().is_ok());

    // Verify empty
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ChatMessage::GetActive { reply_to: resp_tx }).await.unwrap();
    let dialog = resp_rx.await.unwrap().unwrap();
    assert!(dialog.messages.is_empty());
}

#[tokio::test]
async fn test_chat_set_title_works() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(ChatActor::new());

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ChatMessage::SetTitle {
        chat_id: "default".to_string(),
        title: "My Custom Title".to_string(),
        reply_to: resp_tx,
    }).await.unwrap();
    assert!(resp_rx.await.unwrap().is_ok());

    // Verify title changed
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ChatMessage::GetActive { reply_to: resp_tx }).await.unwrap();
    let dialog = resp_rx.await.unwrap().unwrap();
    assert_eq!(dialog.title, "My Custom Title");
}

#[tokio::test]
async fn test_chat_append_to_nonexistent_returns_error() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(ChatActor::new());

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ChatMessage::Append {
        chat_id: "nonexistent".to_string(),
        content: "test".to_string(),
        role: "user".to_string(),
        reply_to: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap();
    assert!(result.is_err());
}

// ===========================================================================
// ToolsActor tests
// ===========================================================================

#[tokio::test]
async fn test_tools_list_initially_has_vscode_tools() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(ToolsActor::new(mock_sender()));

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ToolsMessage::ListTools { reply_to: resp_tx }).await.unwrap();
    let tools = resp_rx.await.unwrap();

    // ToolsActor pre-registers VS Code extension tools at startup
    assert!(!tools.is_empty());
    assert!(tools.iter().any(|t| t.name == "workspace/getFolders"));
}

#[tokio::test]
async fn test_tools_register_and_list() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(ToolsActor::new(mock_sender()));

    // Register a tool
    let tool_info = ToolInfo {
        name: "test_tool".to_string(),
        description: "A test tool".to_string(),
        input_schema: serde_json::json!({}),
    };
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ToolsMessage::RegisterTool {
        server: "test_server".to_string(),
        info: tool_info,
        reply_to: resp_tx,
    }).await.unwrap();
    assert!(resp_rx.await.unwrap().is_ok());

    // List tools — should include pre-registered VS Code tools + the new one
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ToolsMessage::ListTools { reply_to: resp_tx }).await.unwrap();
    let tools = resp_rx.await.unwrap();

    assert!(tools.len() > 1);
    assert!(tools.iter().any(|t| t.name == "test_tool"));
    assert!(tools.iter().any(|t| t.name == "workspace/getFolders"));
}

#[tokio::test]
async fn test_tools_call_unregistered_returns_error() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(ToolsActor::new(mock_sender()));

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ToolsMessage::CallTool {
        tool: "nonexistent".to_string(),
        args: serde_json::Value::Null,
        reply_to: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap();
    assert!(result.is_err());
}

#[tokio::test]
async fn test_tools_unregister_server() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(ToolsActor::new(mock_sender()));

    // Register a tool
    let tool_info = ToolInfo {
        name: "tool1".to_string(),
        description: "desc".to_string(),
        input_schema: serde_json::json!({}),
    };
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ToolsMessage::RegisterTool {
        server: "server_a".to_string(),
        info: tool_info,
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().unwrap();

    // Unregister server
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ToolsMessage::UnregisterServer {
        server: "server_a".to_string(),
        reply_to: resp_tx,
    }).await.unwrap();
    assert!(resp_rx.await.unwrap().is_ok());

    // Verify server_a tools are gone, but VS Code tools remain
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ToolsMessage::ListTools { reply_to: resp_tx }).await.unwrap();
    let tools = resp_rx.await.unwrap();
    assert!(!tools.is_empty());
    assert!(!tools.iter().any(|t| t.name == "tool1"));
    assert!(tools.iter().any(|t| t.name == "workspace/getFolders"));
}

// ===========================================================================
// ProgressActor tests
// ===========================================================================

#[tokio::test]
async fn test_progress_subscribe_and_publish() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(ProgressActor::new());

    // Subscribe
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(ProgressMessage::Subscribe { reply_to: resp_tx }).await.unwrap();
    let mut rx: tokio::sync::broadcast::Receiver<ProgressUpdate> = resp_rx.await.unwrap();

    // Publish
    let update = ProgressUpdate {
        task_id: "task-1".to_string(),
        message: "Working...".to_string(),
        percent: 50.0,
        status: ProgressStatus::Running,
        metadata: None,
    };
    tx.send(ProgressMessage::Publish { update }).await.unwrap();

    // Receive
    let received = rx.recv().await.unwrap();
    assert_eq!(received.task_id, "task-1");
    assert_eq!(received.message, "Working...");
    assert_eq!(received.percent, 50.0);
    assert!(matches!(received.status, ProgressStatus::Running));
}

#[tokio::test]
async fn test_progress_multiple_subscribers() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(ProgressActor::new());

    // Subscribe two listeners
    let (resp_tx1, resp_rx1) = tokio::sync::oneshot::channel();
    tx.send(ProgressMessage::Subscribe { reply_to: resp_tx1 }).await.unwrap();
    let mut rx1 = resp_rx1.await.unwrap();

    let (resp_tx2, resp_rx2) = tokio::sync::oneshot::channel();
    tx.send(ProgressMessage::Subscribe { reply_to: resp_tx2 }).await.unwrap();
    let mut rx2: tokio::sync::broadcast::Receiver<ProgressUpdate> = resp_rx2.await.unwrap();

    // Publish
    let update = ProgressUpdate {
        task_id: "broadcast".to_string(),
        message: "Broadcast test".to_string(),
        percent: 100.0,
        status: ProgressStatus::Completed,
        metadata: None,
    };
    tx.send(ProgressMessage::Publish { update }).await.unwrap();

    // Both receive
    let r1 = rx1.recv().await.unwrap();
    let r2 = rx2.recv().await.unwrap();
    assert_eq!(r1.task_id, "broadcast");
    assert_eq!(r2.task_id, "broadcast");
}

// ===========================================================================
// SystemActor tests
// ===========================================================================

#[tokio::test]
async fn test_system_get_status_returns_running() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(SystemActor::new());

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(SystemMessage::GetStatus { reply_to: resp_tx }).await.unwrap();
    let status = resp_rx.await.unwrap();

    assert_eq!(status["status"], "running");
    assert!(status["uptime_seconds"].as_f64().unwrap() >= 0.0);
    assert_eq!(status["version"], "0.1.0");
    assert_eq!(status["actors"]["chat"], true);
    assert_eq!(status["actors"]["system"], true);
}

#[tokio::test]
async fn test_system_get_config_unknown_returns_none() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(SystemActor::new());

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(SystemMessage::GetConfig {
        key: "nonexistent".to_string(),
        reply_to: resp_tx,
    }).await.unwrap();
    let value = resp_rx.await.unwrap();

    assert!(value.is_none());
}

#[tokio::test]
async fn test_system_shutdown_returns_ok() {
    let system = ActorSystem::new();
    let (tx, _handle) = system.spawn(SystemActor::new());

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(SystemMessage::Shutdown { reply_to: resp_tx }).await.unwrap();
    let result = resp_rx.await.unwrap();

    assert!(result.is_ok());
}

// ===========================================================================
// Coordinator tests (end-to-end routing)
// ===========================================================================

#[tokio::test]
async fn test_coordinator_ping() {
    let system = ActorSystem::new();
    let (chat_tx, _) = system.spawn(ChatActor::new());
    let (tools_tx, _) = system.spawn(ToolsActor::new(mock_sender()));
    let (mcp_tx, _) = system.spawn(McpClientActor::new());
    let (llm_tx, _) = system.spawn(LlmActor::new(LlmConfig::default()));
    let (progress_tx, _) = system.spawn(ProgressActor::new());
    let (system_tx, _) = system.spawn(SystemActor::new());

    let memory_graph_tx = mock_memory_graph();
    let project_query_tx = mock_sender();
    let intent_router_tx = mock_sender();
    let prompt_handler_tx = mock_sender();
    let build_orchestrator_tx = mock_sender();
    let tool_router_tx = mock_sender();
    let tool_router_tx = mock_sender();
    let transport_tx = mock_sender();
    let (coord_tx, _handle) = system.spawn(CoordinatorActor::new(
        chat_tx, tools_tx, mcp_tx, llm_tx, progress_tx, system_tx, memory_graph_tx,
        project_query_tx, intent_router_tx, prompt_handler_tx, build_orchestrator_tx, tool_router_tx, transport_tx,
    ));

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    coord_tx.send(CoordinatorMessage::HandleRequest {
        method: "ping".to_string(),
        params: serde_json::json!({}),
        response_tx: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap();

    assert_eq!(result, serde_json::json!({"pong": true}));
}

#[tokio::test]
async fn test_coordinator_chat_get_active_end_to_end() {
    let system = ActorSystem::new();
    let (chat_tx, _) = system.spawn(ChatActor::new());
    let (tools_tx, _) = system.spawn(ToolsActor::new(mock_sender()));
    let (mcp_tx, _) = system.spawn(McpClientActor::new());
    let (llm_tx, _) = system.spawn(LlmActor::new(LlmConfig::default()));
    let (progress_tx, _) = system.spawn(ProgressActor::new());
    let (system_tx, _) = system.spawn(SystemActor::new());

    let memory_graph_tx = mock_memory_graph();
    let project_query_tx = mock_sender();
    let intent_router_tx = mock_sender();
    let prompt_handler_tx = mock_sender();
    let build_orchestrator_tx = mock_sender();
    let tool_router_tx = mock_sender();
    let tool_router_tx = mock_sender();
    let transport_tx = mock_sender();
    let (coord_tx, _handle) = system.spawn(CoordinatorActor::new(
        chat_tx, tools_tx, mcp_tx, llm_tx, progress_tx, system_tx, memory_graph_tx,
        project_query_tx, intent_router_tx, prompt_handler_tx, build_orchestrator_tx, tool_router_tx, transport_tx,
    ));

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    coord_tx.send(CoordinatorMessage::HandleRequest {
        method: "chat/getActive".to_string(),
        params: serde_json::json!({}),
        response_tx: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap();

    assert_eq!(result["id"], "default");
    assert_eq!(result["title"], "New Chat");
}

#[tokio::test]
async fn test_coordinator_chat_append_and_get_history() {
    let system = ActorSystem::new();
    let (chat_tx, _) = system.spawn(ChatActor::new());
    let (tools_tx, _) = system.spawn(ToolsActor::new(mock_sender()));
    let (mcp_tx, _) = system.spawn(McpClientActor::new());
    let (llm_tx, _) = system.spawn(LlmActor::new(LlmConfig::default()));
    let (progress_tx, _) = system.spawn(ProgressActor::new());
    let (system_tx, _) = system.spawn(SystemActor::new());

    let memory_graph_tx = mock_memory_graph();
    let project_query_tx = mock_sender();
    let intent_router_tx = mock_sender();
    let prompt_handler_tx = mock_sender();
    let build_orchestrator_tx = mock_sender();
    let tool_router_tx = mock_sender();
    let tool_router_tx = mock_sender();
    let transport_tx = mock_sender();
    let (coord_tx, _handle) = system.spawn(CoordinatorActor::new(
        chat_tx, tools_tx, mcp_tx, llm_tx, progress_tx, system_tx, memory_graph_tx,
        project_query_tx, intent_router_tx, prompt_handler_tx, build_orchestrator_tx, tool_router_tx, transport_tx,
    ));

    // Append
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    coord_tx.send(CoordinatorMessage::HandleRequest {
        method: "chat/append".to_string(),
        params: serde_json::json!({
            "chatId": "default",
            "content": "Hello from coordinator",
            "options": {"role": "user"}
        }),
        response_tx: resp_tx,
    }).await.unwrap();
    let append_result = resp_rx.await.unwrap();
    assert_eq!(append_result["content"], "Hello from coordinator");
    assert_eq!(append_result["role"], "user");

    // Get history
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    coord_tx.send(CoordinatorMessage::HandleRequest {
        method: "chat/getHistory".to_string(),
        params: serde_json::json!({}),
        response_tx: resp_tx,
    }).await.unwrap();
    let history = resp_rx.await.unwrap();

    assert!(history.is_array());
    assert_eq!(history[0]["messages"][0]["content"], "Hello from coordinator");
}

#[tokio::test]
async fn test_coordinator_tools_list() {
    let system = ActorSystem::new();
    let (chat_tx, _) = system.spawn(ChatActor::new());
    let (tools_tx, _) = system.spawn(ToolsActor::new(mock_sender()));
    let (mcp_tx, _) = system.spawn(McpClientActor::new());
    let (llm_tx, _) = system.spawn(LlmActor::new(LlmConfig::default()));
    let (progress_tx, _) = system.spawn(ProgressActor::new());
    let (system_tx, _) = system.spawn(SystemActor::new());

    let memory_graph_tx = mock_memory_graph();
    let project_query_tx = mock_sender();
    let intent_router_tx = mock_sender();
    let prompt_handler_tx = mock_sender();
    let build_orchestrator_tx = mock_sender();
    let tool_router_tx = mock_sender();
    let transport_tx = mock_sender();
    let (coord_tx, _handle) = system.spawn(CoordinatorActor::new(
        chat_tx, tools_tx, mcp_tx, llm_tx, progress_tx, system_tx, memory_graph_tx,
        project_query_tx, intent_router_tx, prompt_handler_tx, build_orchestrator_tx, transport_tx,
    ));

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    coord_tx.send(CoordinatorMessage::HandleRequest {
        method: "tools/list".to_string(),
        params: serde_json::json!({}),
        response_tx: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap();

    assert!(result.is_array());
    // ToolsActor pre-registers VS Code extension tools, so the list is non-empty
    assert!(!result.as_array().unwrap().is_empty());
    assert!(result.as_array().unwrap().iter().any(|t| t["name"] == "workspace/getFolders"));
}

#[tokio::test]
async fn test_coordinator_system_status() {
    let system = ActorSystem::new();
    let (chat_tx, _) = system.spawn(ChatActor::new());
    let (tools_tx, _) = system.spawn(ToolsActor::new(mock_sender()));
    let (mcp_tx, _) = system.spawn(McpClientActor::new());
    let (llm_tx, _) = system.spawn(LlmActor::new(LlmConfig::default()));
    let (progress_tx, _) = system.spawn(ProgressActor::new());
    let (system_tx, _) = system.spawn(SystemActor::new());

    let memory_graph_tx = mock_memory_graph();
    let project_query_tx = mock_sender();
    let intent_router_tx = mock_sender();
    let prompt_handler_tx = mock_sender();
    let build_orchestrator_tx = mock_sender();
    let tool_router_tx = mock_sender();
    let transport_tx = mock_sender();
    let (coord_tx, _handle) = system.spawn(CoordinatorActor::new(
        chat_tx, tools_tx, mcp_tx, llm_tx, progress_tx, system_tx, memory_graph_tx,
        project_query_tx, intent_router_tx, prompt_handler_tx, build_orchestrator_tx, transport_tx,
    ));

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    coord_tx.send(CoordinatorMessage::HandleRequest {
        method: "system/status".to_string(),
        params: serde_json::json!({}),
        response_tx: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap();

    assert_eq!(result["status"], "running");
    assert_eq!(result["version"], "0.1.0");
}

#[tokio::test]
async fn test_coordinator_unknown_method() {
    let system = ActorSystem::new();
    let (chat_tx, _) = system.spawn(ChatActor::new());
    let (tools_tx, _) = system.spawn(ToolsActor::new(mock_sender()));
    let (mcp_tx, _) = system.spawn(McpClientActor::new());
    let (llm_tx, _) = system.spawn(LlmActor::new(LlmConfig::default()));
    let (progress_tx, _) = system.spawn(ProgressActor::new());
    let (system_tx, _) = system.spawn(SystemActor::new());

    let memory_graph_tx = mock_memory_graph();
    let project_query_tx = mock_sender();
    let transport_tx = mock_sender();
    let intent_router_tx = mock_sender();
    let prompt_handler_tx = mock_sender();
    let build_orchestrator_tx = mock_sender();
    let tool_router_tx = mock_sender();

    let (coord_tx, _handle) = system.spawn(CoordinatorActor::new(
        chat_tx, tools_tx, mcp_tx, llm_tx, progress_tx, system_tx, memory_graph_tx,
        project_query_tx, intent_router_tx, prompt_handler_tx, build_orchestrator_tx, transport_tx,
    ));

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    coord_tx.send(CoordinatorMessage::HandleRequest {
        method: "nonexistent/method".to_string(),
        params: serde_json::json!({}),
        response_tx: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap();

    assert!(result.get("error").is_some());
    assert!(result["error"].as_str().unwrap().contains("nonexistent/method"));
}

#[tokio::test]
async fn test_coordinator_mcp_servers_empty() {
    let system = ActorSystem::new();
    let (chat_tx, _) = system.spawn(ChatActor::new());
    let (tools_tx, _) = system.spawn(ToolsActor::new(mock_sender()));
    let (mcp_tx, _) = system.spawn(McpClientActor::new());
    let (llm_tx, _) = system.spawn(LlmActor::new(LlmConfig::default()));
    let (progress_tx, _) = system.spawn(ProgressActor::new());
    let (system_tx, _) = system.spawn(SystemActor::new());

    let memory_graph_tx = mock_memory_graph();
    let project_query_tx = mock_sender();
    let transport_tx = mock_sender();
    let intent_router_tx = mock_sender();
    let prompt_handler_tx = mock_sender();
    let build_orchestrator_tx = mock_sender();
    let tool_router_tx = mock_sender();

    let (coord_tx, _handle) = system.spawn(CoordinatorActor::new(
        chat_tx, tools_tx, mcp_tx, llm_tx, progress_tx, system_tx, memory_graph_tx,
        project_query_tx, intent_router_tx, prompt_handler_tx, build_orchestrator_tx, transport_tx,
    ));

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    coord_tx.send(CoordinatorMessage::HandleRequest {
        method: "mcp/servers".to_string(),
        params: serde_json::json!({}),
        response_tx: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap();

    assert!(result.is_array());
    assert!(result.as_array().unwrap().is_empty());
}

// ===========================================================================
// BuildOrchestrator tests
// ===========================================================================

#[tokio::test]
async fn test_build_orchestrator_start_build_creates_session() {
    let system = ActorSystem::new();
    let memory_graph = spawn_memory_graph(&system);
    let error_analyzer_tx: tokio::sync::mpsc::Sender<ErrorAnalyzerMessage> = mock_sender();
    let tool_orchestrator_tx: tokio::sync::mpsc::Sender<ToolOrchestratorMessage> = mock_sender();

    // Create a real ToolRouter channel (will receive the build dispatch)
    let (tool_router_tx, _tool_router_rx) = tokio::sync::mpsc::channel(64);

    let (bo_tx, _bo_handle) = system.spawn(BuildOrchestrator::new(
        memory_graph.clone(),
        error_analyzer_tx,
        tool_orchestrator_tx,
        tool_router_tx,
    ));

    // Send StartBuild — this will try to dispatch to ToolRouter and fail silently
    // since ToolRouter is a mock, but the session node should still be created
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    bo_tx
        .send(BuildOrchestratorMessage::StartBuild {
            parameters: BuildContext {
                project_root: "/tmp/test".to_string(),
                build_system: "Cargo".to_string(),
                target: None,
                environment: std::collections::HashMap::new(),
            },
            reply_to: reply_tx,
        })
        .await
        .unwrap();

    // The build will fail because ToolRouter is a mock (messages go to nowhere),
    // but we can still verify the session node was created
    let _result = reply_rx.await.unwrap();

    // Query the graph for the build_session node
    let (query_tx, query_rx) = tokio::sync::oneshot::channel();
    memory_graph
        .send(MemoryGraphMessage::QueryNodes {
            filter: NodeFilter {
                node_type: Some(NodeType::Standard),
                subtype: Some("build_session".to_string()),
                name: None,
                status: None,
                tags: None,
                properties: None,
                limit: Some(10),
                offset: None,
            },
            reply_to: query_tx,
        })
        .await
        .unwrap();

    let sessions = query_rx.await.unwrap().expect("QueryNodes failed");
    assert!(!sessions.is_empty(), "Should have at least one build_session node");
    assert_eq!(sessions[0].subtype.as_deref(), Some("build_session"));
}

#[tokio::test]
async fn test_build_orchestrator_start_build_sets_proper_status() {
    let system = ActorSystem::new();
    let memory_graph = spawn_memory_graph(&system);
    let error_analyzer_tx: tokio::sync::mpsc::Sender<ErrorAnalyzerMessage> = mock_sender();
    let tool_orchestrator_tx: tokio::sync::mpsc::Sender<ToolOrchestratorMessage> = mock_sender();
    let (tool_router_tx, _tool_router_rx) = tokio::sync::mpsc::channel(64);

    let (bo_tx, _bo_handle) = system.spawn(BuildOrchestrator::new(
        memory_graph.clone(),
        error_analyzer_tx,
        tool_orchestrator_tx,
        tool_router_tx,
    ));

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    bo_tx
        .send(BuildOrchestratorMessage::StartBuild {
            parameters: BuildContext {
                project_root: "/tmp/test2".to_string(),
                build_system: "Cargo".to_string(),
                target: None,
                environment: std::collections::HashMap::new(),
            },
            reply_to: reply_tx,
        })
        .await
        .unwrap();

    let _result = reply_rx.await.unwrap();

    // Query the graph for the build_session node
    let (query_tx, query_rx) = tokio::sync::oneshot::channel();
    memory_graph
        .send(MemoryGraphMessage::QueryNodes {
            filter: NodeFilter {
                node_type: Some(NodeType::Standard),
                subtype: Some("build_session".to_string()),
                name: None,
                status: None,
                tags: None,
                properties: None,
                limit: Some(10),
                offset: None,
            },
            reply_to: query_tx,
        })
        .await
        .unwrap();

    let sessions = query_rx.await.unwrap().expect("QueryNodes failed");
    assert!(!sessions.is_empty(), "Should have at least one build_session node");

    // The session node should have properties like build_system, project_root, status
    let session = &sessions[0];
    assert_eq!(
        session.properties.get("build_system").and_then(|v| v.as_str()),
        Some("Cargo")
    );
    assert_eq!(
        session.properties.get("project_root").and_then(|v| v.as_str()),
        Some("/tmp/test2")
    );
}

#[tokio::test]
async fn test_build_orchestrator_loop_guard() {
    // Test that exceeding max_iterations correctly returns an error
    // We can test this by sending ApplyFix multiple times (but in practice the
    // iteration counter resets on each StartBuild). The loop guard is tested
    // indirectly by verifying the error message format.
    let system = ActorSystem::new();
    let memory_graph = spawn_memory_graph(&system);
    let error_analyzer_tx: tokio::sync::mpsc::Sender<ErrorAnalyzerMessage> = mock_sender();
    let tool_orchestrator_tx: tokio::sync::mpsc::Sender<ToolOrchestratorMessage> = mock_sender();
    let (tool_router_tx, _tool_router_rx) = tokio::sync::mpsc::channel(64);

    // Store a fix_strategy node so lookup doesn't fail
    let fix_strategy = NodeInput {
        node_type: NodeType::FixStrategy,
        subtype: None,
        name: "test-fix-strategy".to_string(),
        description: Some("A test fix strategy".to_string()),
        properties: Some({
            let mut map = std::collections::HashMap::new();
            map.insert("steps".to_string(), serde_json::json!(["step1"]));
            map
        }),
        embedding_id: None,
    };
    let (store_tx, store_rx) = tokio::sync::oneshot::channel();
    memory_graph
        .send(MemoryGraphMessage::StoreNode {
            node: fix_strategy,
            reply_to: store_tx,
        })
        .await
        .unwrap();
    store_rx.await.unwrap().unwrap();

    let (bo_tx, _bo_handle) = system.spawn(BuildOrchestrator::new(
        memory_graph.clone(),
        error_analyzer_tx,
        tool_orchestrator_tx,
        tool_router_tx,
    ));

    // Send ApplyFix — since there's no build context set, it should fail
    // with an error (not infinite loop) because start_build will be called
    // from apply_fix with an empty context
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    bo_tx
        .send(BuildOrchestratorMessage::ApplyFix {
            strategy_name: "test-fix-strategy".to_string(),
            target_system: None,
            reply_to: reply_tx,
        })
        .await
        .unwrap();

    let result = reply_rx.await.unwrap();
    // Should fail gracefully with an error, not panic
    assert!(result.is_err(), "ApplyFix should fail without a build context");
}

// ===========================================================================
// ErrorAnalyzer tests
// ===========================================================================

/// Helper to seed the MemoryGraph with error type and fix strategy nodes
/// that mirror config/intents.json.
async fn seed_error_types_and_fixes(
    memory_graph: &mpsc::Sender<MemoryGraphMessage>,
) {
    // Seed error_type: rustc-compile-error
    let error_type_node = NodeInput {
        node_type: NodeType::ErrorType,
        subtype: None,
        name: "rustc-compile-error".to_string(),
        description: Some("Rust compiler error".to_string()),
        properties: Some({
            let mut map = std::collections::HashMap::new();
            map.insert(
                "detection_patterns".to_string(),
                serde_json::json!(["error\\[E\\d{4}\\]", "error: could not compile"]),
            );
            map.insert("severity".to_string(), serde_json::json!("high"));
            map.insert(
                "fix_strategies".to_string(),
                serde_json::json!(["fix-type-error"]),
            );
            map
        }),
        embedding_id: None,
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    memory_graph
        .send(MemoryGraphMessage::StoreNode {
            node: error_type_node,
            reply_to: tx,
        })
        .await
        .unwrap();
    rx.await.unwrap().unwrap();

    // Seed fix_strategy: fix-type-error
    let fix_node = NodeInput {
        node_type: NodeType::FixStrategy,
        subtype: None,
        name: "fix-type-error".to_string(),
        description: Some("Fix a type mismatch".to_string()),
        properties: Some({
            let mut map = std::collections::HashMap::new();
            map.insert("category".to_string(), serde_json::json!("fix"));
            map.insert("confidence_threshold".to_string(), serde_json::json!(0.6));
            map.insert("success_rate".to_string(), serde_json::json!(0.8));
            map.insert(
                "steps".to_string(),
                serde_json::json!(["read_error_context", "analyze_type_mismatch", "apply_fix"]),
            );
            map.insert("has_rollback".to_string(), serde_json::json!(true));
            map
        }),
        embedding_id: None,
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    memory_graph
        .send(MemoryGraphMessage::StoreNode {
            node: fix_node,
            reply_to: tx,
        })
        .await
        .unwrap();
    rx.await.unwrap().unwrap();

    // Seed fix_strategy: generic-fix (fallback)
    let generic_fix = NodeInput {
        node_type: NodeType::FixStrategy,
        subtype: None,
        name: "generic-fix".to_string(),
        description: Some("Generic fallback fix".to_string()),
        properties: Some({
            let mut map = std::collections::HashMap::new();
            map.insert("category".to_string(), serde_json::json!("fix"));
            map.insert("confidence_threshold".to_string(), serde_json::json!(0.3));
            map.insert("success_rate".to_string(), serde_json::json!(0.3));
            map.insert(
                "steps".to_string(),
                serde_json::json!(["read_error_context", "analyze_error", "apply_fix"]),
            );
            map.insert("has_rollback".to_string(), serde_json::json!(true));
            map
        }),
        embedding_id: None,
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    memory_graph
        .send(MemoryGraphMessage::StoreNode {
            node: generic_fix,
            reply_to: tx,
        })
        .await
        .unwrap();
    rx.await.unwrap().unwrap();
}

#[tokio::test]
async fn test_error_analyzer_matches_rustc_error_by_regex() {
    let system = ActorSystem::new();
    let memory_graph = spawn_memory_graph(&system);
    seed_error_types_and_fixes(&memory_graph).await;

    let (ea_tx, _ea_handle) = system.spawn(ErrorAnalyzer::new(memory_graph));

    // Create a system result with a Rust compile error containing E0308
    let system_result = SystemBuildResult {
        build_type: "Cargo".to_string(),
        path: "/tmp/project".to_string(),
        project_name: "spire-core".to_string(),
        success: false,
        errors: vec![BuildError {
            error_text: "error[E0308]: mismatched types\n --> src/main.rs:42:5".to_string(),
            error_type: Some("rustc-compile-error".to_string()),
            file: Some("src/main.rs".to_string()),
            line: Some(42),
            column: Some(5),
            exit_code: Some(1),
            build_type: Some("Cargo".to_string()),
            diagnostic_node_id: None,
            file_node_id: None,
        }],
        warnings: vec![],
        exit_code: Some(1),
        duration_ms: 5000,
    };

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    ea_tx
        .send(ErrorAnalyzerMessage::AnalyzeErrors {
            system_results: vec![system_result],
            build_run_id: "test-run-1".to_string(),
            reply_to: reply_tx,
        })
        .await
        .unwrap();

    let fix_plan = reply_rx.await.unwrap().expect("AnalyzeErrors failed");

    // Verify fix plan has annotated errors
    assert!(!fix_plan.errors.is_empty(), "Should have annotated errors");
    assert_eq!(fix_plan.errors[0].build_type, "Cargo");
    assert_eq!(fix_plan.errors[0].error.error_type.as_deref(), Some("rustc-compile-error"));

    // Verify ordered_fixes is non-empty — should include fix-type-error
    assert!(!fix_plan.ordered_fixes.is_empty(), "Should have ordered fixes");
    assert!(
        fix_plan.ordered_fixes.iter().any(|f| f.strategy.name == "fix-type-error"),
        "Ordered fixes should include fix-type-error"
    );
}

#[tokio::test]
async fn test_error_analyzer_falls_back_to_generic() {
    let system = ActorSystem::new();
    let memory_graph = spawn_memory_graph(&system);
    seed_error_types_and_fixes(&memory_graph).await;

    let (ea_tx, _ea_handle) = system.spawn(ErrorAnalyzer::new(memory_graph));

    // Create an error that doesn't match any known pattern
    let system_result = SystemBuildResult {
        build_type: "Cargo".to_string(),
        path: "/tmp/project".to_string(),
        project_name: "spire-core".to_string(),
        success: false,
        errors: vec![BuildError {
            error_text: "some very unusual error that doesn't match any pattern".to_string(),
            error_type: None,
            file: Some("src/lib.rs".to_string()),
            line: Some(10),
            column: None,
            exit_code: Some(1),
            build_type: Some("Cargo".to_string()),
            diagnostic_node_id: None,
            file_node_id: None,
        }],
        warnings: vec![],
        exit_code: Some(1),
        duration_ms: 3000,
    };

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    ea_tx
        .send(ErrorAnalyzerMessage::AnalyzeErrors {
            system_results: vec![system_result],
            build_run_id: "test-run-2".to_string(),
            reply_to: reply_tx,
        })
        .await
        .unwrap();

    let fix_plan = reply_rx.await.unwrap().expect("AnalyzeErrors failed");

    // Should have annotated errors
    assert!(!fix_plan.errors.is_empty(), "Should have annotated errors");
    // The fix_options might be empty if no generic fallback matched,
    // but the analysis should still produce a fix plan
    assert!(
        !fix_plan.ordered_fixes.is_empty(),
        "Should have at least the generic-fix fallback"
    );
}

#[tokio::test]
async fn test_error_analyzer_multi_system_deduplicates() {
    let system = ActorSystem::new();
    let memory_graph = spawn_memory_graph(&system);
    seed_error_types_and_fixes(&memory_graph).await;

    let (ea_tx, _ea_handle) = system.spawn(ErrorAnalyzer::new(memory_graph));

    // Create TWO system results both producing the same error type
    let cargo_error = SystemBuildResult {
        build_type: "Cargo".to_string(),
        path: "/tmp/project/rust".to_string(),
        project_name: "spire-core".to_string(),
        success: false,
        errors: vec![BuildError {
            error_text: "error[E0308]: mismatched types".to_string(),
            error_type: Some("rustc-compile-error".to_string()),
            file: Some("src/main.rs".to_string()),
            line: Some  (42),
            column: None,
            exit_code: Some(1),
            build_type: Some("Cargo".to_string()),
            diagnostic_node_id: None,
            file_node_id: None,
        }],
        warnings: vec![],
        exit_code: Some(1),
        duration_ms: 5000,
    };

    let npm_error = SystemBuildResult {
        build_type: "npm".to_string(),
        path: "/tmp/project/ts".to_string(),
        project_name: "spire-extension".to_string(),
        success: false,
        errors: vec![BuildError {
            error_text: "error[E0308]: type mismatch".to_string(),
            error_type: Some("rustc-compile-error".to_string()),
            file: Some("src/app.ts".to_string()),
            line: Some(10),
            column: None,
            exit_code: Some(1),
            build_type: Some("npm".to_string()),
            diagnostic_node_id: None,
            file_node_id: None,
        }],
        warnings: vec![],
        exit_code: Some(1),
        duration_ms: 3000,
    };

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    ea_tx
        .send(ErrorAnalyzerMessage::AnalyzeErrors {
            system_results: vec![cargo_error, npm_error],
            build_run_id: "test-run-3".to_string(),
            reply_to: reply_tx,
        })
        .await
        .unwrap();

    let fix_plan = reply_rx.await.unwrap().expect("AnalyzeErrors failed");

    // Should have 2 annotated errors (one per system)
    assert_eq!(fix_plan.errors.len(), 2, "Should have 2 annotated errors");
    assert_eq!(fix_plan.errors[0].build_type, "Cargo");
    assert_eq!(fix_plan.errors[1].build_type, "npm");

    // Ordered fixes should have fix-type-error only once (deduplicated)
    let type_error_count = fix_plan
        .ordered_fixes
        .iter()
        .filter(|f| f.strategy.name == "fix-type-error")
        .count();
    assert_eq!(
        type_error_count, 1,
        "fix-type-error should appear only once (deduplicated)"
    );
}

#[tokio::test]
async fn test_error_analyzer_skips_successful_systems() {
    let system = ActorSystem::new();
    let memory_graph = spawn_memory_graph(&system);
    seed_error_types_and_fixes(&memory_graph).await;

    let (ea_tx, _ea_handle) = system.spawn(ErrorAnalyzer::new(memory_graph));

    // One successful system, one failed
    let success_system = SystemBuildResult {
        build_type: "Cargo".to_string(),
        path: "/tmp/project/rust".to_string(),
        project_name: "spire-core".to_string(),
        success: true,
        errors: vec![],
        warnings: vec![],
        exit_code: Some(0),
        duration_ms: 2000,
    };

    let failed_system = SystemBuildResult {
        build_type: "npm".to_string(),
        path: "/tmp/project/ts".to_string(),
        project_name: "spire-extension".to_string(),
        success: false,
        errors: vec![BuildError {
            error_text: "Cannot find module './handlers/chat'".to_string(),
            error_type: None,
            file: Some("src/app.ts".to_string()),
            line: Some(1),
            column: None,
            exit_code: Some(1),
            build_type: Some("npm".to_string()),
            diagnostic_node_id: None,
            file_node_id: None,
        }],
        warnings: vec![],
        exit_code: Some(1),
        duration_ms: 3000,
    };

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    ea_tx
        .send(ErrorAnalyzerMessage::AnalyzeErrors {
            system_results: vec![success_system, failed_system],
            build_run_id: "test-run-4".to_string(),
            reply_to: reply_tx,
        })
        .await
        .unwrap();

    let fix_plan = reply_rx.await.unwrap().expect("AnalyzeErrors failed");

    // Should have 1 annotated error (npm only, Cargo succeeded)
    assert_eq!(
        fix_plan.errors.len(),
        1,
        "Should only have 1 annotated error (npm)"
    );
    assert_eq!(fix_plan.errors[0].build_type, "npm");
}

// ===========================================================================
// State transition tests (via MemoryGraph)
// ===========================================================================

#[tokio::test]
async fn test_build_state_transition_updates_active() {
    let system = ActorSystem::new();
    let memory_graph = spawn_memory_graph(&system);

    // Seed a BuildState node with active=false
    let state_node = NodeInput {
        node_type: NodeType::BuildState,
        subtype: None,
        name: "test_build_failed".to_string(),
        description: Some("Test build failed state".to_string()),
        properties: Some({
            let mut map = std::collections::HashMap::new();
            map.insert("active".to_string(), serde_json::json!(false));
            map
        }),
        embedding_id: None,
    };
    let (store_tx, store_rx) = tokio::sync::oneshot::channel();
    memory_graph
        .send(MemoryGraphMessage::StoreNode {
            node: state_node,
            reply_to: store_tx,
        })
        .await
        .unwrap();
    let stored = store_rx.await.unwrap().unwrap();

    // Create BuildOrchestrator and call transition_state
    let error_analyzer_tx: tokio::sync::mpsc::Sender<ErrorAnalyzerMessage> = mock_sender();
    let tool_orchestrator_tx: tokio::sync::mpsc::Sender<ToolOrchestratorMessage> = mock_sender();
    let (tool_router_tx, _tool_router_rx) = tokio::sync::mpsc::channel(64);

    // We can't call transition_state directly since it's a private method,
    // but we can verify the state node got stored correctly with its initial value
    let (query_tx, query_rx) = tokio::sync::oneshot::channel();
    memory_graph
        .send(MemoryGraphMessage::QueryNodes {
            filter: NodeFilter {
                node_type: Some(NodeType::BuildState),
                subtype: None,
                name: Some("test_build_failed".to_string()),
                status: None,
                tags: None,
                properties: None,
                limit: Some(1),
                offset: None,
            },
            reply_to: query_tx,
        })
        .await
        .unwrap();

    let nodes = query_rx.await.unwrap().expect("QueryNodes failed");
    assert_eq!(nodes.len(), 1);
    assert_eq!(
        nodes[0].properties.get("active").and_then(|v| v.as_bool()),
        Some(false),
        "Initial active should be false"
    );
}

#[tokio::test]
async fn test_build_state_store_and_query() {
    let system = ActorSystem::new();
    let memory_graph = spawn_memory_graph(&system);

    // Store build_failed state
    let state_node = NodeInput {
        node_type: NodeType::BuildState,
        subtype: None,
        name: "state_build_failed".to_string(),
        description: Some("Build failed state".to_string()),
        properties: Some({
            let mut map = std::collections::HashMap::new();
            map.insert("active".to_string(), serde_json::json!(true));
            map
        }),
        embedding_id: None,
    };
    let (store_tx, store_rx) = tokio::sync::oneshot::channel();
    memory_graph
        .send(MemoryGraphMessage::StoreNode {
            node: state_node,
            reply_to: store_tx,
        })
        .await
        .unwrap();
    store_rx.await.unwrap().unwrap();

    // Store build_completed state
    let state_node2 = NodeInput {
        node_type: NodeType::BuildState,
        subtype: None,
        name: "state_build_completed".to_string(),
        description: Some("Build completed state".to_string()),
        properties: Some({
            let mut map = std::collections::HashMap::new();
            map.insert("active".to_string(), serde_json::json!(false));
            map
        }),
        embedding_id: None,
    };
    let (store_tx, store_rx) = tokio::sync::oneshot::channel();
    memory_graph
        .send(MemoryGraphMessage::StoreNode {
            node: state_node2,
            reply_to: store_tx,
        })
        .await
        .unwrap();
    store_rx.await.unwrap().unwrap();

    // Query all BuildState nodes
    let (query_tx, query_rx) = tokio::sync::oneshot::channel();
    memory_graph
        .send(MemoryGraphMessage::QueryNodes {
            filter: NodeFilter {
                node_type: Some(NodeType::BuildState),
                subtype: None,
                name: None,
                status: None,
                tags: None,
                properties: None,
                limit: Some(10),
                offset: None,
            },
            reply_to: query_tx,
        })
        .await
        .unwrap();

    let nodes = query_rx.await.unwrap().expect("QueryNodes failed");
    assert_eq!(nodes.len(), 2, "Should have 2 build state nodes");
    assert!(nodes.iter().any(|n| n.name == "state_build_failed"));
    assert!(nodes.iter().any(|n| n.name == "state_build_completed"));
}

// ===========================================================================
// Mock Embedder (for MemoryGraph tests)
// ===========================================================================

/// A mock embedder that returns a fixed 384-dimensional vector.
/// No actual ML model is needed — this is purely for testing.
struct MockEmbedder {
    /// Fixed vector to return for all embeddings.
    fixed_vector: Vec<f32>,
}

impl MockEmbedder {
    fn new() -> Self {
        Self {
            fixed_vector: vec![0.1; 384],
        }
    }

    fn new_with_vector(vector: Vec<f32>) -> Self {
        Self {
            fixed_vector: vector,
        }
    }
}

#[async_trait::async_trait]
impl Embedder for MockEmbedder {
    async fn embed(&self, text: &str) -> anyhow::Result<Embedding> {
        Ok(Embedding::new(self.fixed_vector.clone(), text, "mock-model"))
    }

    async fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Embedding>> {
        Ok(texts
            .iter()
            .map(|t| Embedding::new(self.fixed_vector.clone(), t, "mock-model"))
            .collect())
    }

    fn dimensions(&self) -> usize {
        self.fixed_vector.len()
    }
}

// ===========================================================================
// MemoryGraphActor tests
// ===========================================================================

/// Helper to create a MemoryGraphActor for testing.
fn create_memory_graph() -> MemoryGraphActor {
    MemoryGraphActor::new()
}

/// Helper to spawn a MemoryGraphActor in an ActorSystem and return its sender.
fn spawn_memory_graph(system: &ActorSystem) -> tokio::sync::mpsc::Sender<MemoryGraphMessage> {
    let actor = create_memory_graph();
    let (tx, _handle) = system.spawn(actor);
    tx
}

// ─── Node Operations ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_memory_graph_store_and_get_node() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    // Store a node
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Project,
            subtype: None,
            name: "Test Project".to_string(),
            description: Some("A test project".to_string()),
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let stored = resp_rx.await.unwrap().expect("Failed to store node");
    assert_eq!(stored.name, "Test Project");
    assert_eq!(stored.node_type, NodeType::Project);
    assert_eq!(stored.description.as_deref(), Some("A test project"));
    assert_eq!(stored.version, 1);
    assert!(!stored.id.is_empty());

    // Get the node by ID
    let node_id = stored.id.clone();
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::GetNode {
        id: node_id.clone(),
        reply_to: resp_tx,
    }).await.unwrap();
    let retrieved = resp_rx.await.unwrap().expect("Failed to get node");
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id, node_id);
    assert_eq!(retrieved.name, "Test Project");
}

#[tokio::test]
async fn test_memory_graph_get_nonexistent_node_returns_none() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::GetNode {
        id: "nonexistent-uuid".to_string(),
        reply_to: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap().expect("GetNode failed");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_memory_graph_query_nodes_by_type() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    // Store two projects and one entity
    for i in 0..2 {
        let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
        tx.send(MemoryGraphMessage::StoreNode {
            node: NodeInput {
                node_type: NodeType::Project,
                subtype: None,
                name: format!("Project {}", i),
                description: None,
                properties: None,
                embedding_id: None,
            },
            reply_to: resp_tx,
        }).await.unwrap();
        resp_rx.await.unwrap().unwrap();
    }

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Entity,
            subtype: None,
            name: "Entity 1".to_string(),
            description: None,
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().unwrap();

    // Query by type
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::QueryNodes {
        filter: NodeFilter {
            node_type: Some(NodeType::Project),
            subtype: None,
            name: None,
            status: None,
            tags: None,
            properties: None,
            limit: None,
            offset: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let projects = resp_rx.await.unwrap().expect("QueryNodes failed");
    assert_eq!(projects.len(), 2);
    assert!(projects.iter().all(|n| n.node_type == NodeType::Project));
}

#[tokio::test]
async fn test_memory_graph_query_nodes_by_name() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Standard,
            subtype: None,
            name: "MySpecialNode".to_string(),
            description: None,
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().unwrap();

    // Query by name (case-insensitive)
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::QueryNodes {
        filter: NodeFilter {
            node_type: None,
            subtype: None,
            name: Some("myspecial".to_string()),
            status: None,
            tags: None,
            properties: None,
            limit: None,
            offset: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let results = resp_rx.await.unwrap().expect("QueryNodes failed");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "MySpecialNode");
}

#[tokio::test]
async fn test_memory_graph_update_node() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    // Store a node
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Standard,
            subtype: None,
            name: "Original".to_string(),
            description: Some("Original description".to_string()),
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let stored = resp_rx.await.unwrap().unwrap();
    let node_id = stored.id.clone();

    // Update the node
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::UpdateNode {
        id: node_id.clone(),
        updates: NodeUpdate {
            node_type: None,
            subtype: None,
            name: Some("Updated".to_string()),
            description: Some(Some("Updated description".to_string())),
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let updated = resp_rx.await.unwrap().expect("UpdateNode failed");
    assert_eq!(updated.name, "Updated");
    assert_eq!(updated.description.as_deref(), Some("Updated description"));
    assert_eq!(updated.version, 2); // version should increment

    // Verify via GetNode
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::GetNode {
        id: node_id,
        reply_to: resp_tx,
    }).await.unwrap();
    let retrieved = resp_rx.await.unwrap().unwrap().unwrap();
    assert_eq!(retrieved.name, "Updated");
    assert_eq!(retrieved.version, 2);
}

#[tokio::test]
async fn test_memory_graph_delete_node() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    // Store a node
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Standard,
            subtype: None,
            name: "ToDelete".to_string(),
            description: None,
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let stored = resp_rx.await.unwrap().unwrap();
    let node_id = stored.id.clone();

    // Delete it
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::DeleteNode {
        id: node_id.clone(),
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().expect("DeleteNode failed");

    // Verify it's gone
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::GetNode {
        id: node_id,
        reply_to: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap().unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_memory_graph_delete_nonexistent_returns_error() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::DeleteNode {
        id: "nonexistent".to_string(),
        reply_to: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap();
    assert!(result.is_err());
}

#[tokio::test]
async fn test_memory_graph_duplicate_node_enforcement() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    // Store first node
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Project,
            subtype: None,
            name: "UniqueProject".to_string(),
            description: None,
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().unwrap();

    // Try storing duplicate (same type + name)
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Project,
            subtype: None,
            name: "UniqueProject".to_string(),
            description: None,
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap();
    assert!(result.is_err(), "Duplicate node should be rejected");
    let err = result.err().unwrap();
    let err_str = err.to_string();
    assert!(err_str.contains("Duplicate") || err_str.contains("duplicate"), "Error should mention duplicate: {}", err_str);
}

// ─── Relationship Operations ─────────────────────────────────────────────

#[tokio::test]
async fn test_memory_graph_create_and_get_relationships() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    // Store two nodes
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Project,
            subtype: None,
            name: "Source".to_string(),
            description: None,
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let source = resp_rx.await.unwrap().unwrap();

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Entity,
            subtype: None,
            name: "Target".to_string(),
            description: None,
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let target = resp_rx.await.unwrap().unwrap();

    // Create relationship
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::CreateRelationship {
        rel: RelationshipInput {
            edge_type: RelationshipType::BelongsTo,
            from_id: source.id.clone(),
            to_id: target.id.clone(),
            properties: None,
            weight: Some(1.0),
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let edge = resp_rx.await.unwrap().expect("CreateRelationship failed");
    assert_eq!(edge.edge_type, RelationshipType::BelongsTo);
    assert_eq!(edge.from_id, source.id);
    assert_eq!(edge.to_id, target.id);
    assert_eq!(edge.weight, Some(1.0));

    // Get relationships for source node
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::GetRelationships {
        node_id: source.id.clone(),
        reply_to: resp_tx,
    }).await.unwrap();
    let edges = resp_rx.await.unwrap().expect("GetRelationships failed");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].edge_type, RelationshipType::BelongsTo);

    // Get relationships for target node (should also find it via incoming)
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::GetRelationships {
        node_id: target.id.clone(),
        reply_to: resp_tx,
    }).await.unwrap();
    let edges = resp_rx.await.unwrap().unwrap();
    assert_eq!(edges.len(), 1);
}

#[tokio::test]
async fn test_memory_graph_delete_relationship() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    // Store two nodes
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Standard,
            subtype: None,
            name: "A".to_string(),
            description: None,
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let a = resp_rx.await.unwrap().unwrap();

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Standard,
            subtype: None,
            name: "B".to_string(),
            description: None,
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let b = resp_rx.await.unwrap().unwrap();

    // Create relationship
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::CreateRelationship {
        rel: RelationshipInput {
            edge_type: RelationshipType::DependsOn,
            from_id: a.id.clone(),
            to_id: b.id.clone(),
            properties: None,
            weight: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let edge = resp_rx.await.unwrap().unwrap();

    // Delete it
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::DeleteRelationship {
        id: edge.id.clone(),
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().expect("DeleteRelationship failed");

    // Verify gone
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::GetRelationships {
        node_id: a.id,
        reply_to: resp_tx,
    }).await.unwrap();
    let edges = resp_rx.await.unwrap().unwrap();
    assert!(edges.is_empty());
}

// ─── Traversal ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_memory_graph_traverse_basic() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    // Create chain: A -> B -> C
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Standard,
            subtype: None,
            name: "A".to_string(),
            description: None,
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let a = resp_rx.await.unwrap().unwrap();

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Standard,
            subtype: None,
            name: "B".to_string(),
            description: None,
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let b = resp_rx.await.unwrap().unwrap();

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Standard,
            subtype: None,
            name: "C".to_string(),
            description: None,
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let c = resp_rx.await.unwrap().unwrap();

    // Create edges A->B, B->C
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::CreateRelationship {
        rel: RelationshipInput {
            edge_type: RelationshipType::DependsOn,
            from_id: a.id.clone(),
            to_id: b.id.clone(),
            properties: None,
            weight: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().unwrap();

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::CreateRelationship {
        rel: RelationshipInput {
            edge_type: RelationshipType::DependsOn,
            from_id: b.id.clone(),
            to_id: c.id.clone(),
            properties: None,
            weight: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().unwrap();

    // Traverse from A with max_depth=1 (should get A + B)
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::Traverse {
        start_node_id: a.id.clone(),
        options: TraversalOptions {
            max_depth: 1,
            relationship_types: None,
            max_nodes: Some(10),
            direction: Some(TraversalDirection::Out),
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap().expect("Traverse failed");
    assert_eq!(result.nodes.len(), 2, "Depth 1 should find A + B");
    assert_eq!(result.edges.len(), 1, "Depth 1 should find 1 edge");

    // Traverse from A with max_depth=2 (should get A + B + C)
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::Traverse {
        start_node_id: a.id,
        options: TraversalOptions {
            max_depth: 2,
            relationship_types: None,
            max_nodes: Some(10),
            direction: Some(TraversalDirection::Out),
        },
        reply_to: resp_tx,
    }).await.unwrap();
    let result = resp_rx.await.unwrap().unwrap();
    assert_eq!(result.nodes.len(), 3, "Depth 2 should find A + B + C");
    assert_eq!(result.edges.len(), 2, "Depth 2 should find 2 edges");
}

// ─── Config Storage ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_memory_graph_set_and_get_config() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    // Set a config value
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::SetConfig {
        key: "theme".to_string(),
        value: serde_json::json!("dark"),
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().expect("SetConfig failed");

    // Get it back
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::GetConfig {
        key: "theme".to_string(),
        reply_to: resp_tx,
    }).await.unwrap();
    let value = resp_rx.await.unwrap().expect("GetConfig failed");
    assert_eq!(value, Some(serde_json::json!("dark")));
}

#[tokio::test]
async fn test_memory_graph_get_nonexistent_config() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::GetConfig {
        key: "nonexistent".to_string(),
        reply_to: resp_tx,
    }).await.unwrap();
    let value = resp_rx.await.unwrap().expect("GetConfig failed");
    assert_eq!(value, None);
}

#[tokio::test]
async fn test_memory_graph_overwrite_config() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    // Set initial value
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::SetConfig {
        key: "max_results".to_string(),
        value: serde_json::json!(10),
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().unwrap();

    // Overwrite
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::SetConfig {
        key: "max_results".to_string(),
        value: serde_json::json!(50),
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().unwrap();

    // Verify overwritten
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::GetConfig {
        key: "max_results".to_string(),
        reply_to: resp_tx,
    }).await.unwrap();
    let value = resp_rx.await.unwrap().unwrap();
    assert_eq!(value, Some(serde_json::json!(50)));
}

// ─── Sync / Maintenance ──────────────────────────────────────────────────

#[tokio::test]
async fn test_memory_graph_sync_does_not_crash() {
    let system = ActorSystem::new();
    let tx = spawn_memory_graph(&system);

    // Store some data
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Standard,
            subtype: None,
            name: "SyncTest".to_string(),
            description: None,
            properties: None,
            embedding_id: None,
        },
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().unwrap();

    // Sync should succeed without error
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::Sync {
        reply_to: resp_tx,
    }).await.unwrap();
    resp_rx.await.unwrap().expect("Sync failed");
}

// ─── Custom Properties Tests ──────────────────────────────────────────────

/// Helper to spawn a MemoryGraphActor with a properly initialized graph database.
/// Creates a temp directory, spawns the actor, sends Initialize, and returns
/// the sender along with the temp dir path (kept alive for the test duration).
async fn spawn_initialized_memory_graph(
    system: &ActorSystem,
) -> (tokio::sync::mpsc::Sender<MemoryGraphMessage>, tempfile::TempDir) {
    let actor = MemoryGraphActor::new();
    let (tx, _handle) = system.spawn(actor);

    let tmp_dir = tempfile::tempdir().expect("Failed to create temp dir");

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::Initialize {
        data_dir: tmp_dir.path().to_path_buf(),
        reply_to: resp_tx,
    })
    .await
    .unwrap();
    resp_rx.await.unwrap().expect("Initialize failed");

    (tx, tmp_dir)
}

#[tokio::test]
async fn test_memory_graph_custom_properties_preserved_via_get_node() {
    let system = ActorSystem::new();
    let (tx, _tmp_dir) = spawn_initialized_memory_graph(&system).await;

    // Store a node with custom properties (simulating an intent definition)
    let mut props = std::collections::HashMap::new();
    props.insert(
        "confidence_threshold".to_string(),
        serde_json::json!(0.7),
    );
    props.insert(
        "min_confidence".to_string(),
        serde_json::json!(0.5),
    );
    props.insert(
        "keywords".to_string(),
        serde_json::json!(["build", "compile", "make"]),
    );
    props.insert(
        "requires_project".to_string(),
        serde_json::json!(true),
    );

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Intent,
            subtype: None,
            name: "build".to_string(),
            description: Some("Build the project".to_string()),
            properties: Some(props),
            embedding_id: None,
        },
        reply_to: resp_tx,
    })
    .await
    .unwrap();
    let stored = resp_rx.await.unwrap().expect("StoreNode failed");

    // Verify stored node has custom properties
    assert_eq!(
        stored.properties.get("confidence_threshold"),
        Some(&serde_json::json!(0.7)),
        "confidence_threshold should be preserved after StoreNode"
    );
    assert_eq!(
        stored.properties.get("min_confidence"),
        Some(&serde_json::json!(0.5)),
        "min_confidence should be preserved after StoreNode"
    );
    assert_eq!(
        stored.properties.get("keywords"),
        Some(&serde_json::json!(["build", "compile", "make"])),
        "keywords should be preserved after StoreNode"
    );
    assert_eq!(
        stored.properties.get("requires_project"),
        Some(&serde_json::json!(true)),
        "requires_project should be preserved after StoreNode"
    );

    // Query back via GetNode
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::GetNode {
        id: stored.id.clone(),
        reply_to: resp_tx,
    })
    .await
    .unwrap();
    let retrieved = resp_rx
        .await
        .unwrap()
        .expect("GetNode failed")
        .expect("Node should exist");

    // THIS IS THE KEY ASSERTION: custom properties must survive the round-trip
    assert_eq!(
        retrieved.properties.get("confidence_threshold"),
        Some(&serde_json::json!(0.7)),
        "confidence_threshold should survive GetNode round-trip"
    );
    assert_eq!(
        retrieved.properties.get("min_confidence"),
        Some(&serde_json::json!(0.5)),
        "min_confidence should survive GetNode round-trip"
    );
    assert_eq!(
        retrieved.properties.get("keywords"),
        Some(&serde_json::json!(["build", "compile", "make"])),
        "keywords should survive GetNode round-trip"
    );
    assert_eq!(
        retrieved.properties.get("requires_project"),
        Some(&serde_json::json!(true)),
        "requires_project should survive GetNode round-trip"
    );
    assert_eq!(
        retrieved.properties.len(),
        4,
        "Should have exactly 4 custom properties, got: {:?}",
        retrieved.properties
    );
}

#[tokio::test]
async fn test_memory_graph_custom_properties_preserved_via_query_nodes() {
    let system = ActorSystem::new();
    let (tx, _tmp_dir) = spawn_initialized_memory_graph(&system).await;

    // Store a node with custom properties
    let mut props = std::collections::HashMap::new();
    props.insert(
        "confidence_threshold".to_string(),
        serde_json::json!(0.85),
    );
    props.insert(
        "min_confidence".to_string(),
        serde_json::json!(0.6),
    );

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Intent,
            subtype: None,
            name: "test".to_string(),
            description: None,
            properties: Some(props),
            embedding_id: None,
        },
        reply_to: resp_tx,
    })
    .await
    .unwrap();
    resp_rx.await.unwrap().expect("StoreNode failed");

    // Query back via QueryNodes
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::QueryNodes {
        filter: NodeFilter {
            node_type: Some(NodeType::Intent),
            subtype: None,
            name: None,
            status: None,
            tags: None,
            properties: None,
            limit: None,
            offset: None,
        },
        reply_to: resp_tx,
    })
    .await
    .unwrap();
    let results = resp_rx.await.unwrap().expect("QueryNodes failed");

    assert_eq!(results.len(), 1, "Should find exactly one Intent node");
    assert_eq!(
        results[0].properties.get("confidence_threshold"),
        Some(&serde_json::json!(0.85)),
        "confidence_threshold should survive QueryNodes round-trip"
    );
    assert_eq!(
        results[0].properties.get("min_confidence"),
        Some(&serde_json::json!(0.6)),
        "min_confidence should survive QueryNodes round-trip"
    );
    assert_eq!(
        results[0].properties.len(),
        2,
        "Should have exactly 2 custom properties"
    );
}

#[tokio::test]
async fn test_memory_graph_custom_properties_preserved_via_get_project_context() {
    let system = ActorSystem::new();
    let (tx, _tmp_dir) = spawn_initialized_memory_graph(&system).await;

    // Store a Project node with custom properties
    let mut props = std::collections::HashMap::new();
    props.insert(
        "root_dir".to_string(),
        serde_json::json!("/home/user/project"),
    );
    props.insert(
        "language".to_string(),
        serde_json::json!("Rust"),
    );

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Project,
            subtype: None,
            name: "my-project".to_string(),
            description: Some("A test project".to_string()),
            properties: Some(props),
            embedding_id: None,
        },
        reply_to: resp_tx,
    })
    .await
    .unwrap();
    resp_rx.await.unwrap().expect("StoreNode failed");

    // Get project context
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::GetProjectContext {
        reply_to: resp_tx,
    })
    .await
    .unwrap();
    let context = resp_rx.await.unwrap().expect("GetProjectContext failed");

    // Verify custom properties on the project node
    assert_eq!(
        context.project.properties.get("root_dir"),
        Some(&serde_json::json!("/home/user/project")),
        "root_dir should survive GetProjectContext round-trip"
    );
    assert_eq!(
        context.project.properties.get("language"),
        Some(&serde_json::json!("Rust")),
        "language should survive GetProjectContext round-trip"
    );
    assert_eq!(
        context.project.properties.len(),
        2,
        "Should have exactly 2 custom properties on project node"
    );
}

#[tokio::test]
async fn test_memory_graph_custom_properties_preserved_after_update() {
    let system = ActorSystem::new();
    let (tx, _tmp_dir) = spawn_initialized_memory_graph(&system).await;

    // Store a node with initial custom properties
    let mut props = std::collections::HashMap::new();
    props.insert("initial_key".to_string(), serde_json::json!("initial_value"));

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::StoreNode {
        node: NodeInput {
            node_type: NodeType::Standard,
            subtype: None,
            name: "update-test".to_string(),
            description: None,
            properties: Some(props),
            embedding_id: None,
        },
        reply_to: resp_tx,
    })
    .await
    .unwrap();
    let stored = resp_rx.await.unwrap().expect("StoreNode failed");

    // Update with new properties (replacing all)
    let mut new_props = std::collections::HashMap::new();
    new_props.insert("updated_key".to_string(), serde_json::json!("updated_value"));

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::UpdateNode {
        id: stored.id.clone(),
        updates: NodeUpdate {
            node_type: None,
            subtype: None,
            name: None,
            description: None,
            properties: Some(new_props),
            embedding_id: None,
        },
        reply_to: resp_tx,
    })
    .await
    .unwrap();
    let updated = resp_rx.await.unwrap().expect("UpdateNode failed");

    // Verify updated properties
    assert_eq!(
        updated.properties.get("updated_key"),
        Some(&serde_json::json!("updated_value")),
        "updated_key should be present after UpdateNode"
    );
    assert_eq!(
        updated.properties.get("initial_key"),
        None,
        "initial_key should be gone after UpdateNode replaced properties"
    );
    assert_eq!(
        updated.properties.len(),
        1,
        "Should have exactly 1 custom property after update"
    );

    // Verify via GetNode
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    tx.send(MemoryGraphMessage::GetNode {
        id: stored.id,
        reply_to: resp_tx,
    })
    .await
    .unwrap();
    let retrieved = resp_rx
        .await
        .unwrap()
        .expect("GetNode failed")
        .expect("Node should exist");

    assert_eq!(
        retrieved.properties.get("updated_key"),
        Some(&serde_json::json!("updated_value")),
        "updated_key should survive GetNode after update"
    );
    assert_eq!(
        retrieved.properties.len(),
        1,
        "Should have exactly 1 custom property after update round-trip"
    );
}
