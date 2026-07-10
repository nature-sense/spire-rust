// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Actor-level unit tests for spire-core.
//!
//! These tests import `spire_core` as a library, create an `ActorSystem`,
//! spawn actors directly, send messages via their `mpsc::Sender` channels,
//! and assert on the responses.

use std::sync::Arc;
use tokio::sync::Mutex;
use spire_core::actors::*;
use spire_core::framework::ActorSystem;
use spire_core::transport::stdio::Transport;
use spire_core::graph::GraphDb;
use spire_core::models::embedding::{Embedder, Embedding};
use spire_core::models::memory_graph::*;
use spire_core::actors::memory_graph::{MemoryGraphActor, MemoryGraphMessage};

/// Helper to create a mock transport for coordinator tests.
fn mock_transport() -> Arc<Mutex<Transport>> {
    Arc::new(Mutex::new(Transport::new()))
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
    let (tx, _handle) = system.spawn(ToolsActor::new());

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
    let (tx, _handle) = system.spawn(ToolsActor::new());

    // Register a tool
    let tool_info = ToolInfo {
        name: "test_tool".to_string(),
        description: "A test tool".to_string(),
        input_schema: serde_json::json!({"type": "object"}),
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
    let (tx, _handle) = system.spawn(ToolsActor::new());

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
    let (tx, _handle) = system.spawn(ToolsActor::new());

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
    let mut rx = resp_rx.await.unwrap();

    // Publish
    let update = ProgressUpdate {
        task_id: "task-1".to_string(),
        message: "Working...".to_string(),
        percent: 50.0,
        status: ProgressStatus::Running,
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
    let mut rx2 = resp_rx2.await.unwrap();

    // Publish
    let update = ProgressUpdate {
        task_id: "broadcast".to_string(),
        message: "Broadcast test".to_string(),
        percent: 100.0,
        status: ProgressStatus::Completed,
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
    let (tools_tx, _) = system.spawn(ToolsActor::new());
    let (mcp_tx, _) = system.spawn(McpClientActor::new());
    let (llm_tx, _) = system.spawn(LlmActor::new(LlmConfig::default()));
    let (progress_tx, _) = system.spawn(ProgressActor::new());
    let (system_tx, _) = system.spawn(SystemActor::new());

    let transport = mock_transport();
    let (coord_tx, _handle) = system.spawn(CoordinatorActor::new(
        chat_tx, tools_tx, mcp_tx, llm_tx, progress_tx, system_tx, transport,
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
    let (tools_tx, _) = system.spawn(ToolsActor::new());
    let (mcp_tx, _) = system.spawn(McpClientActor::new());
    let (llm_tx, _) = system.spawn(LlmActor::new(LlmConfig::default()));
    let (progress_tx, _) = system.spawn(ProgressActor::new());
    let (system_tx, _) = system.spawn(SystemActor::new());

    let transport = mock_transport();
    let (coord_tx, _handle) = system.spawn(CoordinatorActor::new(
        chat_tx, tools_tx, mcp_tx, llm_tx, progress_tx, system_tx, transport,
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
    let (tools_tx, _) = system.spawn(ToolsActor::new());
    let (mcp_tx, _) = system.spawn(McpClientActor::new());
    let (llm_tx, _) = system.spawn(LlmActor::new(LlmConfig::default()));
    let (progress_tx, _) = system.spawn(ProgressActor::new());
    let (system_tx, _) = system.spawn(SystemActor::new());

    let transport = mock_transport();
    let (coord_tx, _handle) = system.spawn(CoordinatorActor::new(
        chat_tx, tools_tx, mcp_tx, llm_tx, progress_tx, system_tx, transport,
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
    let (tools_tx, _) = system.spawn(ToolsActor::new());
    let (mcp_tx, _) = system.spawn(McpClientActor::new());
    let (llm_tx, _) = system.spawn(LlmActor::new(LlmConfig::default()));
    let (progress_tx, _) = system.spawn(ProgressActor::new());
    let (system_tx, _) = system.spawn(SystemActor::new());

    let transport = mock_transport();
    let (coord_tx, _handle) = system.spawn(CoordinatorActor::new(
        chat_tx, tools_tx, mcp_tx, llm_tx, progress_tx, system_tx, transport,
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
    let (tools_tx, _) = system.spawn(ToolsActor::new());
    let (mcp_tx, _) = system.spawn(McpClientActor::new());
    let (llm_tx, _) = system.spawn(LlmActor::new(LlmConfig::default()));
    let (progress_tx, _) = system.spawn(ProgressActor::new());
    let (system_tx, _) = system.spawn(SystemActor::new());

    let transport = mock_transport();
    let (coord_tx, _handle) = system.spawn(CoordinatorActor::new(
        chat_tx, tools_tx, mcp_tx, llm_tx, progress_tx, system_tx, transport,
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
    let (tools_tx, _) = system.spawn(ToolsActor::new());
    let (mcp_tx, _) = system.spawn(McpClientActor::new());
    let (llm_tx, _) = system.spawn(LlmActor::new(LlmConfig::default()));
    let (progress_tx, _) = system.spawn(ProgressActor::new());
    let (system_tx, _) = system.spawn(SystemActor::new());

    let transport = mock_transport();
    let (coord_tx, _handle) = system.spawn(CoordinatorActor::new(
        chat_tx, tools_tx, mcp_tx, llm_tx, progress_tx, system_tx, transport,
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
    let (tools_tx, _) = system.spawn(ToolsActor::new());
    let (mcp_tx, _) = system.spawn(McpClientActor::new());
    let (llm_tx, _) = system.spawn(LlmActor::new(LlmConfig::default()));
    let (progress_tx, _) = system.spawn(ProgressActor::new());
    let (system_tx, _) = system.spawn(SystemActor::new());

    let transport = mock_transport();
    let (coord_tx, _handle) = system.spawn(CoordinatorActor::new(
        chat_tx, tools_tx, mcp_tx, llm_tx, progress_tx, system_tx, transport,
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
    let graph_db = Arc::new(GraphDb::new_in_memory().expect("Failed to create in-memory graph"));
    let embedder: Arc<dyn Embedder> = Arc::new(MockEmbedder::new());
    MemoryGraphActor::new(graph_db, embedder)
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
