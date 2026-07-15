// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! CoordinatorActor — main orchestrator that routes JSON-RPC methods to actors.
//!
//! The coordinator receives JSON-RPC requests from the transport layer and
//! dispatches them to the appropriate actor (chat, tools, mcp_client, llm, etc.).

use async_trait::async_trait;
use regex::Regex;
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
use crate::actors::project_query::ProjectQueryMessage;
use crate::models::memory_graph::{McpConfigFile, McpServerConfigEntry};
use crate::transport::socket::Transport;

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
    /// Sender for the project query actor (semantic project queries).
    project_query_tx: mpsc::Sender<ProjectQueryMessage>,
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
        project_query_tx: mpsc::Sender<ProjectQueryMessage>,
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
            project_query_tx,
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
        // Helper to send a tool event notification
        async fn send_tool_event(transport: &Arc<Mutex<Transport>>, event: &str, payload: &serde_json::Value) {
            let t = transport.lock().await;
            let _ = t.send_notification(&format!("event/tool/{}", event), payload).await;
        }

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

                // Check if this is a project query tool (memory graph)
                let is_project_tool = tool.starts_with("project/");

                // Emit tool/start event
                let tool_call_id = format!("call_direct_{}", std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0));
                send_tool_event(&self.transport, "start", &serde_json::json!({
                    "tool_name": tool,
                    "args": args,
                    "tool_call_id": tool_call_id,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                })).await;

                let start = std::time::Instant::now();
                let result = if is_vsc_tool {
                    // Forward the tool call to the VS Code extension via JSON-RPC
                    // The extension's Router handles these methods locally
                    let transport = self.transport.lock().await;
                    match transport.call_extension(tool, &args).await {
                        Ok(result) => result,
                        Err(e) => serde_json::json!({"error": format!("VSC tool call failed: {}", e)}),
                    }
                } else if is_project_tool {
                    // Route to ProjectQueryActor
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    if self.project_query_tx.send(ProjectQueryMessage::CallTool {
                        tool: tool.to_string(),
                        args: args.clone(),
                        reply_to: tx,
                    }).await.is_err() {
                        serde_json::json!({"error": "ProjectQuery actor not available"})
                    } else {
                        match rx.await {
                            Ok(result) => result,
                            Err(_) => serde_json::json!({"error": "ProjectQuery actor response error"}),
                        }
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
                        serde_json::json!({"error": "MCP client actor not available"})
                    } else {
                        match rx.await {
                            Ok(Ok(result)) => serde_json::to_value(result).unwrap_or(serde_json::json!({"error": "serialization error"})),
                            Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                            Err(_) => serde_json::json!({"error": "MCP client actor response error"}),
                        }
                    }
                };
                let duration_ms = start.elapsed().as_millis() as u64;

                // Emit tool/result or tool/error event
                if result.get("error").is_some() {
                    send_tool_event(&self.transport, "error", &serde_json::json!({
                        "tool_name": tool,
                        "error": result["error"],
                        "duration_ms": duration_ms,
                        "tool_call_id": tool_call_id,
                    })).await;
                } else {
                    send_tool_event(&self.transport, "result", &serde_json::json!({
                        "tool_name": tool,
                        "result": result,
                        "duration_ms": duration_ms,
                        "tool_call_id": tool_call_id,
                    })).await;
                }

                result
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
                // Load MCP config from the graph database (not from file)
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.memory_graph_tx.send(MemoryGraphMessage::GetMcpConfig { reply_to: tx }).await.is_err() {
                    return serde_json::json!({"error": "Memory graph actor not available"});
                }
                match rx.await {
                    Ok(Ok(servers)) => {
                        let count = servers.len();
                        // Convert and send to MCP client
                        let configs: Vec<crate::mcp::client::McpServerConfig> = servers
                            .into_iter()
                            .filter_map(|entry| {
                                let transport = if let Some(url) = entry.url {
                                    crate::mcp::client::TransportConfig::Http {
                                        url,
                                        headers: entry.headers.unwrap_or_default(),
                                    }
                                } else if let Some(command) = entry.command {
                                    crate::mcp::client::TransportConfig::Stdio {
                                        command,
                                        args: entry.args,
                                        env: entry.env.unwrap_or_default(),
                                    }
                                } else {
                                    return None;
                                };
                                Some(crate::mcp::client::McpServerConfig {
                                    name: entry.name,
                                    transport,
                                    autostart: entry.autostart,
                                })
                            })
                            .collect();
                        let (tx2, rx2) = tokio::sync::oneshot::channel();
                        if self.mcp_client_tx.send(McpClientMessage::LoadConfigFromGraph {
                            servers: configs,
                            reply_to: tx2,
                        }).await.is_err() {
                            return serde_json::json!({"error": "MCP client actor not available"});
                        }
                        let _ = rx2.await;
                        serde_json::json!({"success": true, "serverCount": count})
                    }
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "Memory graph actor response error"}),
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

                // 1. Fetch the active chat dialog to get message history
                let chat_history = {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    if self.chat_tx.send(ChatMessage::GetActive { reply_to: tx }).await.is_err() {
                        None
                    } else {
                        rx.await.ok().flatten()
                    }
                };

                // 2. Fetch registered tools
                let tools = {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    if self.tools_tx.send(ToolsMessage::ListTools { reply_to: tx }).await.is_err() {
                        vec![]
                    } else {
                        rx.await.unwrap_or_default()
                    }
                };

                // 3. Build a system message — tools are sent via the native
                //    OpenAI `tools` array (see CompleteWithTools below), so we
                //    keep the system prompt minimal to avoid confusing the LLM
                //    with redundant text descriptions that conflict with the
                //    structured tool definitions.
                let system_msg = "You are a helpful AI assistant. When you need to use a tool, respond using the native function-calling mechanism (tool_calls) provided by the API — do not describe tool calls in plain text.".to_string();

                // 4. Build the full messages array
                let mut messages: Vec<crate::actors::chat::ChatMessageData> = Vec::new();

                // System message with tool descriptions
                messages.push(crate::actors::chat::ChatMessageData {
                    id: "sys-tools".to_string(),
                    role: "system".to_string(),
                    content: system_msg,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                });

                // Chat history (skip system messages from history to avoid duplication)
                if let Some(ref dialog) = chat_history {
                    for msg in &dialog.messages {
                        if msg.role != "system" {
                            messages.push(msg.clone());
                        }
                    }
                }

                // Ensure the last message is the current user prompt
                let has_user_prompt = messages.last()
                    .map(|m| m.role == "user" && m.content == prompt)
                    .unwrap_or(false);

                if !has_user_prompt {
                    messages.push(crate::actors::chat::ChatMessageData {
                        id: "user-prompt".to_string(),
                        role: "user".to_string(),
                        content: prompt.to_string(),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    });
                }

                tracing::info!("Coordinator: llm/complete with {} messages and {} tools described",
                    messages.len(), tools.len());

                // 5. Send to LLM with tool definitions (OpenAI-compatible tools array)
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.llm_tx.send(LlmMessage::CompleteWithTools {
                    messages: messages.clone(),
                    tools: tools.clone(),
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "LLM actor not available"});
                }

                let llm_response = match rx.await {
                    Ok(Ok(content)) => content,
                    Ok(Err(e)) => return serde_json::json!({"error": e.to_string()}),
                    Err(_) => return serde_json::json!({"error": "LLM actor response error"}),
                };

                // 6. Check if the response contains tool_calls (JSON format)
                // The response is either plain text or a JSON string with tool_calls
                let final_content = if let Ok(json_msg) = serde_json::from_str::<serde_json::Value>(&llm_response) {
                    if let Some(tool_calls) = json_msg["tool_calls"].as_array() {
                        if !tool_calls.is_empty() {
                            // Execute each tool call and collect results
                            let mut tool_results: Vec<serde_json::Value> = Vec::new();

                            for tc in tool_calls {
                                let function_name = tc["function"]["name"].as_str().unwrap_or("unknown");
                                let function_args: serde_json::Value = tc["function"]["arguments"]
                                    .as_str()
                                    .and_then(|s| serde_json::from_str(s).ok())
                                    .unwrap_or(serde_json::Value::Null);
                                let tool_call_id = tc["id"].as_str().unwrap_or("call_unknown");

                                tracing::info!("Coordinator: executing tool call: {} with args: {:?}", function_name, function_args);

                                // Emit tool/start event
                                send_tool_event(&self.transport, "start", &serde_json::json!({
                                    "tool_name": function_name,
                                    "args": function_args,
                                    "tool_call_id": tool_call_id,
                                    "timestamp": chrono::Utc::now().to_rfc3339(),
                                })).await;

                                let is_vsc_tool = function_name.starts_with("workspace/")
                                    || function_name.starts_with("document/")
                                    || function_name.starts_with("diagnostics/")
                                    || function_name.starts_with("git/")
                                    || function_name.starts_with("symbols/");
                                let is_project_tool = function_name.starts_with("project/");

                                let tool_start = std::time::Instant::now();
                                let tool_result: Result<serde_json::Value, String> = if is_vsc_tool {
                                    // Forward to VS Code extension
                                    let transport = self.transport.lock().await;
                                    transport.call_extension(function_name, &function_args).await
                                } else if is_project_tool {
                                    // Route to ProjectQueryActor
                                    let (tx, rx) = tokio::sync::oneshot::channel();
                                    if self.project_query_tx.send(ProjectQueryMessage::CallTool {
                                        tool: function_name.to_string(),
                                        args: function_args.clone(),
                                        reply_to: tx,
                                    }).await.is_ok() {
                                        match rx.await {
                                            Ok(result) => Ok(result),
                                            Err(e) => Err(format!("ProjectQuery actor response error: {}", e)),
                                        }
                                    } else {
                                        Err("ProjectQuery actor not available".to_string())
                                    }
                                } else {
                                    // Try MCP client
                                    let (tool_tx, tool_rx) = tokio::sync::oneshot::channel();
                                    if self.mcp_client_tx.send(McpClientMessage::CallTool {
                                        server_name: String::new(),
                                        tool_name: function_name.to_string(),
                                        arguments: function_args.as_object().cloned(),
                                        reply_to: tool_tx,
                                    }).await.is_ok() {
                                        match tool_rx.await {
                                            Ok(Ok(result)) => Ok(serde_json::to_value(result).unwrap_or(serde_json::json!({"error": "serialization error"}))),
                                            Ok(Err(e)) => Err(e.to_string()),
                                            Err(_) => Err("MCP client response error".to_string()),
                                        }
                                    } else {
                                        Err("MCP client not available".to_string())
                                    }
                                };
                                let tool_duration_ms = tool_start.elapsed().as_millis() as u64;

                                match &tool_result {
                                    Ok(result) => {
                                        // Emit tool/result event
                                        send_tool_event(&self.transport, "result", &serde_json::json!({
                                            "tool_name": function_name,
                                            "result": result,
                                            "duration_ms": tool_duration_ms,
                                            "tool_call_id": tool_call_id,
                                        })).await;
                                        tool_results.push(serde_json::json!({
                                            "tool_call_id": tool_call_id,
                                            "tool_name": function_name,
                                            "result": result,
                                        }));
                                    }
                                    Err(e) => {
                                        // Emit tool/error event
                                        send_tool_event(&self.transport, "error", &serde_json::json!({
                                            "tool_name": function_name,
                                            "error": e,
                                            "duration_ms": tool_duration_ms,
                                            "tool_call_id": tool_call_id,
                                        })).await;
                                        tool_results.push(serde_json::json!({
                                            "tool_call_id": tool_call_id,
                                            "tool_name": function_name,
                                            "error": e.to_string(),
                                        }));
                                    }
                                }
                            }

                            // Append tool results as a new user message and get final response
                            let tool_results_text = serde_json::to_string_pretty(&tool_results)
                                .unwrap_or_else(|_| "[]".to_string());

                            messages.push(crate::actors::chat::ChatMessageData {
                                id: "tool-results".to_string(),
                                role: "user".to_string(),
                                content: format!("Tool execution results:\n{}", tool_results_text),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                            });

                            // Send follow-up request to LLM with tool results
                            let (tx2, rx2) = tokio::sync::oneshot::channel();
                            if self.llm_tx.send(LlmMessage::CompleteWithMessages {
                                messages,
                                reply_to: tx2,
                            }).await.is_err() {
                                return serde_json::json!({"error": "LLM actor not available", "tool_results": tool_results});
                            }

                            match rx2.await {
                                Ok(Ok(content)) => content,
                                Ok(Err(e)) => return serde_json::json!({"error": e.to_string(), "tool_results": tool_results}),
                                Err(_) => return serde_json::json!({"error": "LLM actor response error", "tool_results": tool_results}),
                            }
                        } else {
                            llm_response
                        }
                    } else {
                        // JSON but no tool_calls — check for content field
                        json_msg["content"].as_str().unwrap_or(&llm_response).to_string()
                    }
                } else {
                    // Plain text response — check for XML/Claude-format tool calls
                    // as a defensive fallback (the LLM actor should have already
                    // parsed these, but this catches edge cases from follow-up calls)
                    if let Some(xml_tool_calls) = Self::parse_xml_tool_calls(&llm_response) {
                        tracing::info!(
                            "Coordinator: detected {} XML-format tool call(s) in plain text response, executing",
                            xml_tool_calls.len()
                        );
                        // Execute each tool call and collect results
                        let mut tool_results: Vec<serde_json::Value> = Vec::new();

                        for tc in &xml_tool_calls {
                            let function_name = tc["function"]["name"].as_str().unwrap_or("unknown");
                            let function_args: serde_json::Value = tc["function"]["arguments"]
                                .as_str()
                                .and_then(|s| serde_json::from_str(s).ok())
                                .unwrap_or(serde_json::Value::Null);
                            let tool_call_id = tc["id"].as_str().unwrap_or("call_xml_unknown");

                            tracing::info!("Coordinator: executing XML tool call: {} with args: {:?}", function_name, function_args);

                            // Emit tool/start event
                            send_tool_event(&self.transport, "start", &serde_json::json!({
                                "tool_name": function_name,
                                "args": function_args,
                                "tool_call_id": tool_call_id,
                                "timestamp": chrono::Utc::now().to_rfc3339(),
                            })).await;

                            let is_vsc_tool = function_name.starts_with("workspace/")
                                || function_name.starts_with("document/")
                                || function_name.starts_with("diagnostics/")
                                || function_name.starts_with("git/")
                                || function_name.starts_with("symbols/");
                            let is_project_tool = function_name.starts_with("project/");

                            let tool_start = std::time::Instant::now();
                            let tool_result: Result<serde_json::Value, String> = if is_vsc_tool {
                                let transport = self.transport.lock().await;
                                transport.call_extension(function_name, &function_args).await
                            } else if is_project_tool {
                                let (tx, rx) = tokio::sync::oneshot::channel();
                                if self.project_query_tx.send(ProjectQueryMessage::CallTool {
                                    tool: function_name.to_string(),
                                    args: function_args.clone(),
                                    reply_to: tx,
                                }).await.is_ok() {
                                    match rx.await {
                                        Ok(result) => Ok(result),
                                        Err(e) => Err(format!("ProjectQuery actor response error: {}", e)),
                                    }
                                } else {
                                    Err("ProjectQuery actor not available".to_string())
                                }
                            } else {
                                let (tool_tx, tool_rx) = tokio::sync::oneshot::channel();
                                if self.mcp_client_tx.send(McpClientMessage::CallTool {
                                    server_name: String::new(),
                                    tool_name: function_name.to_string(),
                                    arguments: function_args.as_object().cloned(),
                                    reply_to: tool_tx,
                                }).await.is_ok() {
                                    match tool_rx.await {
                                        Ok(Ok(result)) => Ok(serde_json::to_value(result).unwrap_or(serde_json::json!({"error": "serialization error"}))),
                                        Ok(Err(e)) => Err(e.to_string()),
                                        Err(_) => Err("MCP client response error".to_string()),
                                    }
                                } else {
                                    Err("MCP client not available".to_string())
                                }
                            };
                            let tool_duration_ms = tool_start.elapsed().as_millis() as u64;

                            match &tool_result {
                                Ok(result) => {
                                    // Emit tool/result event
                                    send_tool_event(&self.transport, "result", &serde_json::json!({
                                        "tool_name": function_name,
                                        "result": result,
                                        "duration_ms": tool_duration_ms,
                                        "tool_call_id": tool_call_id,
                                    })).await;
                                    tool_results.push(serde_json::json!({
                                        "tool_call_id": tool_call_id,
                                        "tool_name": function_name,
                                        "result": result,
                                    }));
                                }
                                Err(e) => {
                                    // Emit tool/error event
                                    send_tool_event(&self.transport, "error", &serde_json::json!({
                                        "tool_name": function_name,
                                        "error": e,
                                        "duration_ms": tool_duration_ms,
                                        "tool_call_id": tool_call_id,
                                    })).await;
                                    tool_results.push(serde_json::json!({
                                        "tool_call_id": tool_call_id,
                                        "tool_name": function_name,
                                        "error": e.to_string(),
                                    }));
                                }
                            }
                        }

                        // Append tool results as a new user message and get final response
                        let tool_results_text = serde_json::to_string_pretty(&tool_results)
                            .unwrap_or_else(|_| "[]".to_string());

                        messages.push(crate::actors::chat::ChatMessageData {
                            id: "tool-results".to_string(),
                            role: "user".to_string(),
                            content: format!("Tool execution results:\n{}", tool_results_text),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        });

                        // Send follow-up request to LLM with tool results
                        let (tx2, rx2) = tokio::sync::oneshot::channel();
                        if self.llm_tx.send(LlmMessage::CompleteWithMessages {
                            messages,
                            reply_to: tx2,
                        }).await.is_err() {
                            return serde_json::json!({"error": "LLM actor not available", "tool_results": tool_results});
                        }

                        match rx2.await {
                            Ok(Ok(content)) => content,
                            Ok(Err(e)) => return serde_json::json!({"error": e.to_string(), "tool_results": tool_results}),
                            Err(_) => return serde_json::json!({"error": "LLM actor response error", "tool_results": tool_results}),
                        }
                    } else {
                        llm_response
                    }
                };

                serde_json::json!({"content": final_content})
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
            "llm/updateConfig" => {
                let api_key = params.get("apiKey").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let model = params.get("model").and_then(|v| v.as_str()).unwrap_or("deepseek-chat").to_string();
                let api_url = params.get("apiUrl").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let max_tokens = params.get("maxTokens").and_then(|v| v.as_u64()).unwrap_or(4096) as u32;
                let temperature = params.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.7) as f32;
                let strict_mode = params.get("strictMode").and_then(|v| v.as_bool()).unwrap_or(false);
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.llm_tx.send(LlmMessage::UpdateConfig {
                    config: crate::actors::LlmConfig {
                        api_key,
                        model,
                        api_url,
                        max_tokens,
                        temperature,
                        strict_mode,
                    },
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "LLM actor not available"});
                }
                match rx.await {
                    Ok(Ok(())) => serde_json::json!({"success": true}),
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

                // Write a snapshot after config/set to persist the change immediately
                {
                    let (tx_sync, rx_sync) = tokio::sync::oneshot::channel();
                    let _ = self.memory_graph_tx.send(MemoryGraphMessage::Sync { reply_to: tx_sync }).await;
                    let _ = rx_sync.await;
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
                        strict_mode: false,
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

            // ── Config Sync (flush WAL) ──
            "config/sync" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.memory_graph_tx.send(MemoryGraphMessage::Sync { reply_to: tx }).await.is_err() {
                    return serde_json::json!({"error": "MemoryGraph actor not available"});
                }
                match rx.await {
                    Ok(Ok(())) => serde_json::json!({"success": true}),
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "MemoryGraph actor response error"}),
                }
            }

            // ── MCP Config (stored in MemoryGraph) ──
            "mcp/config/get" => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.memory_graph_tx.send(MemoryGraphMessage::GetMcpConfig { reply_to: tx }).await.is_err() {
                    return serde_json::json!({"error": "MemoryGraph actor not available"});
                }
                match rx.await {
                    Ok(Ok(servers)) => serde_json::json!({"servers": servers}),
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "MemoryGraph actor response error"}),
                }
            }
            "mcp/config/import" => {
                // Accept the config object directly (from extension which already parsed the file)
                // or fall back to a file path for backward compatibility.
                let servers: Vec<McpServerConfigEntry> = if let Some(config_val) = params.get("config") {
                    // Parse the config object: { servers: [...] }
                    match serde_json::from_value::<McpConfigFile>(config_val.clone()) {
                        Ok(cfg) => cfg.servers,
                        Err(e) => {
                            return serde_json::json!({"error": format!("Invalid config format: {}", e)});
                        }
                    }
                } else if let Some(config_path) = params.get("path").and_then(|v| v.as_str()) {
                    if config_path.is_empty() {
                        return serde_json::json!({"error": "Missing 'path' parameter"});
                    }
                    // Read and parse the JSON file
                    let content = match std::fs::read_to_string(config_path) {
                        Ok(c) => c,
                        Err(e) => return serde_json::json!({"error": format!("Failed to read config file: {}", e)}),
                    };
                    match serde_json::from_str::<McpConfigFile>(&content) {
                        Ok(cfg) => cfg.servers,
                        Err(e) => return serde_json::json!({"error": format!("Failed to parse config file: {}", e)}),
                    }
                } else {
                    return serde_json::json!({"error": "Missing 'config' or 'path' parameter"});
                };

                // ── Delete stale servers not in the new import ──
                // First, fetch all existing servers from the graph
                let (get_tx, get_rx) = tokio::sync::oneshot::channel();
                if self.memory_graph_tx.send(MemoryGraphMessage::GetMcpConfig { reply_to: get_tx }).await.is_err() {
                    return serde_json::json!({"error": "MemoryGraph actor not available"});
                }
                let existing_servers = match get_rx.await {
                    Ok(Ok(srv)) => srv,
                    Ok(Err(e)) => return serde_json::json!({"error": format!("Failed to get existing config: {}", e)}),
                    Err(_) => return serde_json::json!({"error": "MemoryGraph actor response error"}),
                };

                // Build a set of imported server names for quick lookup
                let imported_names: std::collections::HashSet<&str> =
                    servers.iter().map(|s| s.name.as_str()).collect();

                // Delete any existing server whose name is NOT in the new import
                for existing in &existing_servers {
                    if !imported_names.contains(existing.name.as_str()) {
                        tracing::info!("Coordinator: removing stale MCP server '{}' from import", existing.name);
                        let (del_tx, del_rx) = tokio::sync::oneshot::channel();
                        if self.memory_graph_tx.send(MemoryGraphMessage::DeleteMcpConfig {
                            name: existing.name.clone(),
                            reply_to: del_tx,
                        }).await.is_err() {
                            return serde_json::json!({"error": "MemoryGraph actor not available"});
                        }
                        if let Err(e) = del_rx.await {
                            tracing::warn!("Coordinator: failed to delete stale server '{}': {}", existing.name, e);
                        }
                    }
                }

                // Store each server in the graph (replacing existing)
                for server in &servers {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    if self.memory_graph_tx.send(MemoryGraphMessage::SaveMcpConfig {
                        entry: server.clone(),
                        reply_to: tx,
                    }).await.is_err() {
                        return serde_json::json!({"error": "MemoryGraph actor not available"});
                    }
                    if let Err(e) = rx.await {
                        return serde_json::json!({"error": format!("Failed to save server '{}': {}", server.name, e)});
                    }
                }

                // After successful import, reload the MCP client from the graph
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.memory_graph_tx.send(MemoryGraphMessage::GetMcpConfig { reply_to: tx }).await.is_err() {
                    return serde_json::json!({"error": "MemoryGraph actor not available"});
                }
                match rx.await {
                    Ok(Ok(servers)) => {
                        // Convert McpServerConfigEntry to McpServerConfig
                        let configs: Vec<crate::mcp::client::McpServerConfig> = servers
                            .into_iter()
                            .filter_map(|entry| {
                                let transport = if let Some(url) = entry.url {
                                    crate::mcp::client::TransportConfig::Http {
                                        url,
                                        headers: entry.headers.unwrap_or_default(),
                                    }
                                } else if let Some(command) = entry.command {
                                    crate::mcp::client::TransportConfig::Stdio {
                                        command,
                                        args: entry.args,
                                        env: entry.env.unwrap_or_default(),
                                    }
                                } else {
                                    tracing::warn!("Coordinator: MCP server '{}' has no transport config, skipping", entry.name);
                                    return None;
                                };
                                Some(crate::mcp::client::McpServerConfig {
                                    name: entry.name,
                                    transport,
                                    autostart: entry.autostart,
                                })
                            })
                            .collect();

                        let (tx, rx) = tokio::sync::oneshot::channel();
                        if self.mcp_client_tx.send(McpClientMessage::LoadConfigFromGraph {
                            servers: configs,
                            reply_to: tx,
                        }).await.is_err() {
                            return serde_json::json!({"error": "McpClient actor not available"});
                        }
                        let _ = rx.await;

                        // Reconnect all servers
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        if self.mcp_client_tx.send(McpClientMessage::ConnectAll { reply_to: tx }).await.is_err() {
                            return serde_json::json!({"error": "McpClient actor not available"});
                        }
                        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), rx).await;

                        serde_json::json!({"success": true})
                    }
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "MemoryGraph actor response error"}),
                }
            }
            "mcp/config/save" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if name.is_empty() {
                    return serde_json::json!({"error": "Missing 'name' parameter"});
                }
                let entry = McpServerConfigEntry {
                    name: name.clone(),
                    command: params.get("command").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    args: params.get("args").and_then(|v| v.as_array()).map(|arr| {
                        arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
                    }).unwrap_or_default(),

                    env: params.get("env").and_then(|v| v.as_object()).map(|obj| {
                        let mut map = std::collections::HashMap::new();
                        for (k, v) in obj {
                            if let Some(val) = v.as_str() {
                                map.insert(k.clone(), val.to_string());
                            }
                        }
                        map
                    }),
                    url: params.get("url").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    headers: params.get("headers").and_then(|v| v.as_object()).map(|obj| {
                        let mut map = std::collections::HashMap::new();
                        for (k, v) in obj {
                            if let Some(val) = v.as_str() {
                                map.insert(k.clone(), val.to_string());
                            }
                        }
                        map
                    }),
                    autostart: params.get("autostart").and_then(|v| v.as_bool()).unwrap_or(true),
                };

                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.memory_graph_tx.send(MemoryGraphMessage::SaveMcpConfig {
                    entry,
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "MemoryGraph actor not available"});
                }
                match rx.await {
                    Ok(Ok(())) => {
                        // Reload MCP client from graph
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        if self.memory_graph_tx.send(MemoryGraphMessage::GetMcpConfig { reply_to: tx }).await.is_err() {
                            return serde_json::json!({"error": "MemoryGraph actor not available"});
                        }
                        match rx.await {
                            Ok(Ok(servers)) => {
                                let configs: Vec<crate::mcp::client::McpServerConfig> = servers
                                    .into_iter()
                                    .filter_map(|entry| {
                                        let transport = if let Some(url) = entry.url {
                                            crate::mcp::client::TransportConfig::Http {
                                                url,
                                                headers: entry.headers.unwrap_or_default(),
                                            }
                                        } else if let Some(command) = entry.command {
                                            crate::mcp::client::TransportConfig::Stdio {
                                                command,
                                                args: entry.args,
                                                env: entry.env.unwrap_or_default(),
                                            }
                                        } else {
                                            tracing::warn!("Coordinator: MCP server '{}' has no transport config, skipping", entry.name);
                                            return None;
                                        };
                                        Some(crate::mcp::client::McpServerConfig {
                                            name: entry.name,
                                            transport,
                                            autostart: entry.autostart,
                                        })
                                    })
                                    .collect();

                                let (tx, rx) = tokio::sync::oneshot::channel();
                                if self.mcp_client_tx.send(McpClientMessage::LoadConfigFromGraph {
                                    servers: configs,
                                    reply_to: tx,
                                }).await.is_err() {
                                    return serde_json::json!({"error": "McpClient actor not available"});
                                }
                                let _ = rx.await;

                                let (tx, rx) = tokio::sync::oneshot::channel();
                                if self.mcp_client_tx.send(McpClientMessage::ConnectAll { reply_to: tx }).await.is_err() {
                                    return serde_json::json!({"error": "McpClient actor not available"});
                                }
                                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), rx).await;

                                serde_json::json!({"success": true})
                            }
                            Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                            Err(_) => serde_json::json!({"error": "MemoryGraph actor response error"}),
                        }
                    }
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "MemoryGraph actor response error"}),
                }
            }
            "mcp/config/delete" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if name.is_empty() {
                    return serde_json::json!({"error": "Missing 'name' parameter"});
                }
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.memory_graph_tx.send(MemoryGraphMessage::DeleteMcpConfig {
                    name,
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "MemoryGraph actor not available"});
                }
                match rx.await {
                    Ok(Ok(())) => {
                        // Reload MCP client from graph
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        if self.memory_graph_tx.send(MemoryGraphMessage::GetMcpConfig { reply_to: tx }).await.is_err() {
                            return serde_json::json!({"error": "MemoryGraph actor not available"});
                        }
                        match rx.await {
                            Ok(Ok(servers)) => {
                                let configs: Vec<crate::mcp::client::McpServerConfig> = servers
                                    .into_iter()
                                    .filter_map(|entry| {
                                        let transport = if let Some(url) = entry.url {
                                            crate::mcp::client::TransportConfig::Http {
                                                url,
                                                headers: entry.headers.unwrap_or_default(),
                                            }
                                        } else if let Some(command) = entry.command {
                                            crate::mcp::client::TransportConfig::Stdio {
                                                command,
                                                args: entry.args,
                                                env: entry.env.unwrap_or_default(),
                                            }
                                        } else {
                                            tracing::warn!("Coordinator: MCP server '{}' has no transport config, skipping", entry.name);
                                            return None;
                                        };
                                        Some(crate::mcp::client::McpServerConfig {
                                            name: entry.name,
                                            transport,
                                            autostart: entry.autostart,
                                        })
                                    })
                                    .collect();

                                let (tx, rx) = tokio::sync::oneshot::channel();
                                if self.mcp_client_tx.send(McpClientMessage::LoadConfigFromGraph {
                                    servers: configs,
                                    reply_to: tx,
                                }).await.is_err() {
                                    return serde_json::json!({"error": "McpClient actor not available"});
                                }
                                let _ = rx.await;

                                let (tx, rx) = tokio::sync::oneshot::channel();
                                if self.mcp_client_tx.send(McpClientMessage::ConnectAll { reply_to: tx }).await.is_err() {
                                    return serde_json::json!({"error": "McpClient actor not available"});
                                }
                                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), rx).await;

                                serde_json::json!({"success": true})
                            }
                            Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                            Err(_) => serde_json::json!({"error": "MemoryGraph actor response error"}),
                        }
                    }
                    Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
                    Err(_) => serde_json::json!({"error": "MemoryGraph actor response error"}),
                }
            }


            // ── Project Query tools (memory graph queries) ──
            // All project/* tools are handled by the ProjectQueryActor.
            // This catch-all matches any method starting with "project/".
            method if method.starts_with("project/") => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                if self.project_query_tx.send(ProjectQueryMessage::CallTool {
                    tool: method.to_string(),
                    args: params.clone(),
                    reply_to: tx,
                }).await.is_err() {
                    return serde_json::json!({"error": "ProjectQuery actor not available"});
                }
                match rx.await {
                    Ok(result) => result,
                    Err(_) => serde_json::json!({"error": "ProjectQuery actor response error"}),
                }
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

    /// Parse XML/Claude-format tool calls from a response content string.
    ///
    /// DeepSeek sometimes returns tool calls in this format instead of the
    /// native JSON `tool_calls` field:
    ///
    /// ```xml
    /// <｜DSML｜function_calls>
    ///   <｜DSML｜invoke name="get_weather">
    ///     <｜DSML｜parameter name="location" string="true">San Francisco</｜DSML｜parameter>
    ///   </｜DSML｜invoke>
    /// </｜DSML｜function_calls>
    /// ```
    ///
    /// Returns `None` if no XML tool calls are found.
    fn parse_xml_tool_calls(content: &str) -> Option<Vec<serde_json::Value>> {
        // Check if the content contains function_calls markup
        if !content.contains("function_calls") {
            return None;
        }

        // Regex to extract each invoke block
        // Use a raw string with hashes to avoid escaping issues with quotes
        let invoke_re = Regex::new(
            r#"(?s)<(?:｜DSML｜)?invoke\s+name\s*=\s*"([^"]+)">(.*?)</(?:｜DSML｜)?invoke>"#
        ).ok()?;

        let mut tool_calls = Vec::new();
        let mut call_id_counter = 0u64;

        for cap in invoke_re.captures_iter(content) {
            let function_name = cap.get(1)?.as_str().to_string();
            let params_body = cap.get(2)?.as_str();

            // Parse parameters
            let param_re = Regex::new(
                r#"<(?:｜DSML｜)?parameter\s+name\s*=\s*"([^"]+)"(?:\s+string\s*=\s*"(true|false)")?\s*>(.*?)</(?:｜DSML｜)?parameter>"#
            ).ok()?;

            let mut args = serde_json::Map::new();
            for param_cap in param_re.captures_iter(params_body) {
                let param_name = param_cap.get(1)?.as_str().to_string();
                let param_value = param_cap.get(3)?.as_str().to_string();
                args.insert(param_name, serde_json::json!(param_value));
            }

            call_id_counter += 1;
            tool_calls.push(serde_json::json!({
                "id": format!("call_xml_{}", call_id_counter),
                "type": "function",
                "function": {
                    "name": function_name,
                    "arguments": serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string()),
                }
            }));
        }

        if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        }
    }
}
