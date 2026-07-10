// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! CoordinatorActor — main orchestrator that routes JSON-RPC methods to actors.
//!
//! The coordinator receives JSON-RPC requests from the transport layer and
//! dispatches them to the appropriate actor (chat, tools, mcp_client, llm, etc.).

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use crate::actors::Actor;
use crate::actors::chat::ChatMessage;
use crate::actors::tools::ToolsMessage;
use crate::actors::mcp_client::McpClientMessage;
use crate::actors::llm::LlmMessage;
use crate::actors::progress::ProgressMessage;
use crate::actors::system::SystemMessage;
use crate::actors::memory_graph::MemoryGraphMessage;
use crate::transport::stdio::Transport;

/// Messages for the Coordinator actor.
pub enum CoordinatorMessage {
    /// Handle a JSON-RPC request from the extension.
    HandleRequest {
        method: String,
        params: serde_json::Value,
        response_tx: tokio::sync::oneshot::Sender<serde_json::Value>,
    },
    /// Shut down the coordinator.
    Shutdown,
}

/// The Coordinator actor routes requests to the appropriate sub-actors.
#[allow(dead_code)]
pub struct CoordinatorActor {
    /// Sender for the chat actor.
    chat_tx: mpsc::Sender<ChatMessage>,
    /// Sender for the tools actor.
    tools_tx: mpsc::Sender<ToolsMessage>,
    /// Sender for the MCP client actor.
    mcp_client_tx: mpsc::Sender<McpClientMessage>,
    /// Sender for the LLM actor.
    llm_tx: mpsc::Sender<LlmMessage>,
    /// Sender for the progress actor.
    progress_tx: mpsc::Sender<ProgressMessage>,
    /// Sender for the system actor.
    system_tx: mpsc::Sender<SystemMessage>,
    /// Sender for the memory graph actor (knowledge graph + config storage).
    memory_graph_tx: mpsc::Sender<MemoryGraphMessage>,
    /// Transport for forwarding VSC tool calls to the extension.
    transport: Arc<Mutex<Transport>>,
}

impl CoordinatorActor {
    pub fn new(
        chat_tx: mpsc::Sender<ChatMessage>,
        tools_tx: mpsc::Sender<ToolsMessage>,
        mcp_client_tx: mpsc::Sender<McpClientMessage>,
        llm_tx: mpsc::Sender<LlmMessage>,
        progress_tx: mpsc::Sender<ProgressMessage>,
        system_tx: mpsc::Sender<SystemMessage>,
        memory_graph_tx: mpsc::Sender<MemoryGraphMessage>,
        transport: Arc<Mutex<Transport>>,
    ) -> Self {
        Self {
            chat_tx,
            tools_tx,
            mcp_client_tx,
            llm_tx,
            progress_tx,
            system_tx,
            memory_graph_tx,
            transport,
        }
    }
}

#[async_trait]
impl Actor for CoordinatorActor {
    type Message = CoordinatorMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            CoordinatorMessage::HandleRequest {
                method,
                params,
                response_tx,
            } => {
                let result = self.route_request(&method, params).await;
                let _ = response_tx.send(result);
            }
            CoordinatorMessage::Shutdown => {
                tracing::info!("Coordinator: shutting down");
            }
        }
    }
}

impl CoordinatorActor {
    async fn route_request(&self, method: &str, params: serde_json::Value) -> serde_json::Value {
        match method {
            // ── Chat methods ──
            "chat/getActive" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.chat_tx.send(ChatMessage::GetActive { reply_to: tx }).await.is_err() {
                    return serde_json::json!({"error": "Chat actor not available"});
                }
                match rx.await {
                    Ok(Some(dialog)) => serde_json::to_value(dialog).unwrap_or(serde_json::Value::Null),
                    Ok(None) => serde_json::Value::Null,
                    Err(_) => serde_json::json!({"error": "Chat actor response error"}),
                }
            }
            "chat/getHistory" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.chat_tx.send(ChatMessage::GetHistory { reply_to: tx }).await.is_err() {
                    return serde_json::json!({"error": "Chat actor not available"});
                }
                match rx.await {
                    Ok(dialogs) => serde_json::to_value(dialogs).unwrap_or(serde_json::json!([])),
                    Err(_) => serde_json::json!({"error": "Chat actor response error"}),
                }
            }
            "chat/append" => {
                let chat_id = params.get("chatId").and_then(|v| v.as_str()).unwrap_or("default");
                let content = params.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let role = params.get("options").and_then(|o| o.get("role")).and_then(|v| v.as_str()).unwrap_or("assistant");
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.chat_tx.send(ChatMessage::Append {
                    chat_id: chat_id.to_string(),
                    content: content.to_string(),
                    role: role.to_string(),
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "Chat actor not available"});
                }
                match rx.await {
                    Ok(Ok(msg)) => serde_json::to_value(msg).unwrap_or(serde_json::Value::Null),
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "Chat actor response error"}),
                }
            }
            "chat/clear" => {
                let chat_id = params.get("chatId").and_then(|v| v.as_str()).unwrap_or("default");
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.chat_tx.send(ChatMessage::Clear {
                    chat_id: chat_id.to_string(),
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "Chat actor not available"});
                }
                match rx.await {
                    Ok(Ok(())) => serde_json::json!({"success": true}),
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "Chat actor response error"}),
                }
            }
            "chat/setTitle" => {
                let chat_id = params.get("chatId").and_then(|v| v.as_str()).unwrap_or("default");
                let title = params.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.chat_tx.send(ChatMessage::SetTitle {
                    chat_id: chat_id.to_string(),
                    title: title.to_string(),
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "Chat actor not available"});
                }
                match rx.await {
                    Ok(Ok(())) => serde_json::json!({"success": true}),
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "Chat actor response error"}),
                }
            }

            // ── Tool methods ──
            "tools/list" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.tools_tx.send(ToolsMessage::ListTools { reply_to: tx }).await.is_err() {
                    return serde_json::json!({"error": "Tools actor not available"});
                }
                match rx.await {
                    Ok(tools) => serde_json::to_value(tools).unwrap_or(serde_json::json!([])),
                    Err(_) => serde_json::json!({"error": "Tools actor response error"}),
                }
            }
            "tools/call" => {
                let tool = params.get("tool").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("args").cloned().unwrap_or(serde_json::Value::Null);

                // Check if this is a VS Code extension tool by looking at the tool name prefix
                let is_vsc_tool = tool.starts_with("workspace/")
                    || tool.starts_with("document/")
                    || tool.starts_with("diagnostics/")
                    || tool.starts_with("git/")
                    || tool.starts_with("symbols/");

                if is_vsc_tool {
                    // Forward the tool call to the VS Code extension via JSON-RPC
                    // The extension's Router handles these methods locally
                    let transport = self.transport.lock().await;
                    match transport.call_extension(tool, &args).await {
                        Ok(result) => result,
                        Err(e) => serde_json::json!({"error": format!("VSC tool call failed: {}", e)}),
                    }
                } else {
                    // External MCP tool — route through the MCP client actor
                    let server_name = params.get("serverName")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let arguments = args.as_object().cloned();
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    if self.mcp_client_tx.send(McpClientMessage::CallTool {
                        server_name,
                        tool_name: tool.to_string(),
                        arguments,
                        reply_to: tx,
                    }).await.is_err() {
                        return serde_json::json!({"error": "MCP client actor not available"});
                    }
                    match rx.await {
                        Ok(Ok(result)) => serde_json::to_value(result).unwrap_or(serde_json::json!({"error": "serialization error"})),
                        Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                        Err(_) => serde_json::json!({"error": "MCP client actor response error"}),
                    }
                }
            }

            // ── MCP Client methods ──
            "mcp/listServers" | "mcp/servers" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.mcp_client_tx.send(McpClientMessage::GetServerDetails { reply_to: tx }).await.is_err() {
                    return serde_json::json!({"error": "MCP client actor not available"});
                }
                match rx.await {
                    Ok(details) => serde_json::to_value(details).unwrap_or(serde_json::json!([])),
                    Err(_) => serde_json::json!({"error": "MCP client actor response error"}),
                }
            }

            "mcp/loadConfig" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.mcp_client_tx.send(McpClientMessage::LoadConfig { reply_to: tx }).await.is_err() {
                    return serde_json::json!({"error": "MCP client actor not available"});
                }
                match rx.await {
                    Ok(Ok(path)) => {
                        if let Some(p) = path {
                            serde_json::json!({"success": true, "configPath": p.to_string_lossy()})
                        } else {
                            serde_json::json!({"success": true, "configPath": null})
                        }
                    }
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "MCP client actor response error"}),
                }
            }
            "mcp/connectAll" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.mcp_client_tx.send(McpClientMessage::ConnectAll { reply_to: tx }).await.is_err() {
                    return serde_json::json!({"error": "MCP client actor not available"});
                }
                match rx.await {
                    Ok(Ok(())) => serde_json::json!({"success": true}),
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "MCP client actor response error"}),
                }
            }
            "mcp/connect" => {
                let server_name = params.get("serverName").and_then(|v| v.as_str()).unwrap_or("");
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.mcp_client_tx.send(McpClientMessage::Connect {
                    server_name: server_name.to_string(),
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "MCP client actor not available"});
                }
                match rx.await {
                    Ok(Ok(())) => serde_json::json!({"success": true}),
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "MCP client actor response error"}),
                }
            }
            "mcp/disconnect" => {
                let server_name = params.get("serverName").and_then(|v| v.as_str()).unwrap_or("");
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.mcp_client_tx.send(McpClientMessage::Disconnect {
                    server_name: server_name.to_string(),
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "MCP client actor not available"});
                }
                match rx.await {
                    Ok(Ok(())) => serde_json::json!({"success": true}),
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "MCP client actor response error"}),
                }
            }
            "mcp/disconnectAll" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.mcp_client_tx.send(McpClientMessage::DisconnectAll { reply_to: tx }).await.is_err() {
                    return serde_json::json!({"error": "MCP client actor not available"});
                }
                match rx.await {
                    Ok(Ok(())) => serde_json::json!({"success": true}),
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "MCP client actor response error"}),
                }
            }
            "mcp/listServerTools" | "mcp/getTools" => {
                let server_name = params.get("serverName").and_then(|v| v.as_str()).unwrap_or("");
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.mcp_client_tx.send(McpClientMessage::GetTools {
                    server_name: server_name.to_string(),
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "MCP client actor not available"});
                }
                match rx.await {
                    Ok(Some(tools)) => serde_json::to_value(tools).unwrap_or(serde_json::json!([])),
                    Ok(None) => serde_json::json!([]),
                    Err(_) => serde_json::json!({"error": "MCP client actor response error"}),
                }
            }
            "mcp/setInternalTools" => {
                let tools: Vec<rust_mcp_sdk::schema::Tool> = params
                    .get("tools")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.mcp_client_tx.send(McpClientMessage::SetInternalTools {
                    tools,
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "MCP client actor not available"});
                }
                match rx.await {
                    Ok(Ok(())) => serde_json::json!({"success": true}),
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "MCP client actor response error"}),
                }
            }
            "mcp/callTool" => {
                let server_name = params.get("serverName").and_then(|v| v.as_str()).unwrap_or("");
                let tool_name = params.get("toolName").and_then(|v| v.as_str()).unwrap_or("");
                let arguments = params.get("arguments")
                    .and_then(|v| v.as_object())
                    .cloned();
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.mcp_client_tx.send(McpClientMessage::CallTool {
                    server_name: server_name.to_string(),
                    tool_name: tool_name.to_string(),
                    arguments,
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "MCP client actor not available"});
                }
                match rx.await {
                    Ok(Ok(result)) => serde_json::to_value(result).unwrap_or(serde_json::json!({"error": "serialization error"})),
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "MCP client actor response error"}),
                }
            }

            // ── LLM methods ──
            "llm/complete" => {
                let prompt = params.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.llm_tx.send(LlmMessage::Complete {
                    prompt: prompt.to_string(),
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "LLM actor not available"});
                }
                match rx.await {
                    Ok(Ok(content)) => serde_json::json!({"content": content}),
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "LLM actor response error"}),
                }
            }
            "llm/stream" => {
                let prompt = params.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.llm_tx.send(LlmMessage::Stream {
                    prompt: prompt.to_string(),
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "LLM actor not available"});
                }
                match rx.await {
                    Ok(Ok(mut chunk_rx)) => {
                        // Collect all chunks into a single response
                        let mut full = String::new();
                        while let Some(chunk) = chunk_rx.recv().await {
                            full.push_str(&chunk);
                        }
                        serde_json::json!({"content": full})
                    }
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "LLM actor response error"}),
                }
            }

            // ── System methods ──
            "system/status" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.system_tx.send(SystemMessage::GetStatus { reply_to: tx }).await.is_err() {
                    return serde_json::json!({"error": "System actor not available"});
                }
                match rx.await {
                    Ok(status) => status,
                    Err(_) => serde_json::json!({"error": "System actor response error"}),
                }
            }
            "system/shutdown" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.system_tx.send(SystemMessage::Shutdown { reply_to: tx }).await.is_err() {
                    return serde_json::json!({"error": "System actor not available"});
                }
                match rx.await {
                    Ok(Ok(())) => serde_json::json!({"success": true}),
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "System actor response error"}),
                }
            }
            "system/config/get" => {
                let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.system_tx.send(SystemMessage::GetConfig {
                    key: key.to_string(),
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "System actor not available"});
                }
                match rx.await {
                    Ok(Some(value)) => serde_json::json!({"value": value}),
                    Ok(None) => serde_json::json!({"value": null}),
                    Err(_) => serde_json::json!({"error": "System actor response error"}),
                }
            }

            // ── Config Storage (via MemoryGraph) ──
            "config/get" => {
                let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.memory_graph_tx.send(MemoryGraphMessage::GetConfig {
                    key: key.to_string(),
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "MemoryGraph actor not available"});
                }
                match rx.await {
                    Ok(Ok(Some(value))) => serde_json::json!({"value": value}),
                    Ok(Ok(None)) => serde_json::json!({"value": null}),
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "MemoryGraph actor response error"}),
                }
            }
            "config/getAll" => {
                // Fetch all deepseek config keys in one call
                let keys = ["deepseek.api_key", "deepseek.model", "deepseek.api_url"];
                let mut result = serde_json::Map::new();
                for key in &keys {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    if self.memory_graph_tx.send(MemoryGraphMessage::GetConfig {
                        key: key.to_string(),
                        reply_to: tx,
                    }).await.is_err() {
                        result.insert(key.to_string(), serde_json::Value::Null);
                        continue;
                    }
                    match rx.await {
                        Ok(Ok(Some(value))) => { result.insert(key.to_string(), value); }
                        _ => { result.insert(key.to_string(), serde_json::Value::Null); }
                    }
                }
                serde_json::json!({"config": result})
            }
            "config/set" => {
                let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
                let value = params.get("value").cloned().unwrap_or(serde_json::Value::Null);
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.memory_graph_tx.send(MemoryGraphMessage::SetConfig {
                    key: key.to_string(),
                    value: value.clone(),
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "MemoryGraph actor not available"});
                }
                let store_result = rx.await;
                if store_result.is_err() {
                    return serde_json::json!({"error": "MemoryGraph actor response error"});
                }
                if store_result.unwrap().is_err() {
                    return serde_json::json!({"error": "Failed to store config"});
                }

                // If this is a deepseek config key, also update the LLM actor at runtime
                if key.starts_with("deepseek.") {
                    // Fetch all deepseek config values to build a complete LlmConfig
                    let (tx_key, rx_key) = tokio::sync::oneshot::channel();
                    let _ = self.memory_graph_tx.send(MemoryGraphMessage::GetConfig {
                        key: "deepseek.api_key".to_string(),
                        reply_to: tx_key,
                    }).await;
                    let api_key = rx_key.await.ok().and_then(|r| r.ok()).flatten()
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_default();

                    let (tx_model, rx_model) = tokio::sync::oneshot::channel();
                    let _ = self.memory_graph_tx.send(MemoryGraphMessage::GetConfig {
                        key: "deepseek.model".to_string(),
                        reply_to: tx_model,
                    }).await;
                    let model = rx_model.await.ok().and_then(|r| r.ok()).flatten()
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_else(|| "deepseek-chat".to_string());

                    let (tx_url, rx_url) = tokio::sync::oneshot::channel();
                    let _ = self.memory_graph_tx.send(MemoryGraphMessage::GetConfig {
                        key: "deepseek.api_url".to_string(),
                        reply_to: tx_url,
                    }).await;
                    let api_url = rx_url.await.ok().and_then(|r| r.ok()).flatten()
                        .and_then(|v| v.as_str().map(|s| s.to_string()))
                        .unwrap_or_else(|| "https://api.deepseek.com/v1/chat/completions".to_string());

                    let llm_config = crate::actors::LlmConfig {
                        api_key,
                        model,
                        api_url,
                        max_tokens: 4096,
                        temperature: 0.7,
                    };

                    let (tx_llm, rx_llm) = tokio::sync::oneshot::channel();
                    if self.llm_tx.send(crate::actors::LlmMessage::UpdateConfig {
                        config: llm_config,
                        reply_to: tx_llm,
                    }).await.is_ok() {
                        let _ = rx_llm.await;
                    }
                }

                serde_json::json!({"success": true})
            }

            // ── Ping / Health ──
            "ping" => {
                serde_json::json!({"pong": true})
            }

            // ── Unknown method ──
            _ => {
                serde_json::json!({"error": format!("Method not found: {}", method)})
            }
        }
    }
}
