# Agent Infrastructure Implementation Instructions

> Implementation guide for adding an actor-based agent system to Spire Rust.
>
> **Key rule:** Use existing APIs, types, and patterns wherever possible. The config, MCP client, graph DB, and actor framework already exist — this guide builds on them.

---

## Table of Contents

1. [Project Map](#1-project-map)
2. [Architecture Overview](#2-architecture-overview)
3. [Phase 1: Upgrade LlmActor to Real DeepSeek API](#3-phase-1-upgrade-llmactor-to-real-deepseek-api)
4. [Phase 2: Create AgentMessage and AgentActor Types](#4-phase-2-create-agentmessage-and-agentactor-types)
5. [Phase 3: Build GeneralistAgent Actor](#5-phase-3-build-generalistagent-actor)
6. [Phase 4: Build PlannerAgent Actor](#6-phase-4-build-planneragent-actor)
7. [Phase 5: Upgrade CoordinatorActor to Orchestrator](#7-phase-5-upgrade-coordinatoractor-to-orchestrator)
8. [Phase 6: Registration and Startup](#8-phase-6-registration-and-startup)
9. [Key API Reference](#9-key-api-reference)

---

## 1. Project Map

| Path | What it is |
|------|------------|
| `core/src/framework/trait_.rs` | `Actor` trait (`handle()`, `spawn()`) |
| `core/src/framework/messages.rs` | `ToolMessage`, `ToolInfo`, `ActorError`, `Responder<T>` |
| `core/src/framework/system.rs` | `ActorSystem` — spawns actors, returns `(Sender, JoinHandle)` |
| `core/src/actors/mod.rs` | Re-exports all actors, add new ones here |
| `core/src/actors/llm.rs` | `LlmActor` — currently a stub, upgrade to real DeepSeek |
| `core/src/actors/memory_graph.rs` | `MemoryGraphActor` + `MemoryGraphMessage` — data store |
| `core/src/actors/mcp_client.rs` | `McpClientActor` + `McpClientMessage` — external MCP servers |
| `core/src/actors/mcp_handler.rs` | `McpHandlerActor` + `McpHandlerMessage` — embedded tool dispatch |
| `core/src/actors/coordinator.rs` | `CoordinatorActor` — upgrade to orchestrator |
| `core/src/actors/tools/` | Tool actors (`read_file`, `write_file`, etc.) via `ToolMessage` |
| `core/src/mcp_server/config.rs` | `McpConfig`, `EmbeddedToolsConfig`, `ExternalServerConfig` |
| `core/src/models/memory_graph.rs` | `GraphNode`, `NodeType`, `RelationshipType`, `SearchOptions`, etc. |
| `core/src/models/embedding.rs` | `Embedding`, `Embedder` trait |
| `core/src/graph/mod.rs` | `GraphDb` — SeleneDB wrapper (no Cypher) |

---

## 2. Architecture Overview

```
User Query
    │
    ▼
CoordinatorActor (orchestrator)
    │  classifies intent, selects agent type
    ▼
GeneralistAgent / PlannerAgent
    │  each is an Actor with custom AgentMessage
    ├──► LlmActor (DeepSeek API via reqwest)
    ├──► MemoryGraphActor (for prompt components via MemoryGraphMessage)
    ├──► McpHandlerActor (for embedded tools via ToolMessage)
    └──► McpClientActor (for external MCP servers via McpClientMessage)
```

**Key design decisions (confirmed):**
- **No new `GraphDBActor`** — agents use existing `MemoryGraphActor` via `MemoryGraphMessage`
- **No separate `LLMClient`** — upgrade `LlmActor` to call DeepSeek API directly
- **Custom `AgentMessage`** per agent type, agents internally use `ToolMessage`
- **Prefix caching** — internal `HashMap<String, String>` per agent, keyed by hash of component versions

---

## 3. Phase 1: Upgrade LlmActor to Real DeepSeek API

**File:** `core/src/actors/llm.rs`

The existing `LlmActor` is a stub that echoes input. Replace it with a real DeepSeek API client.

```rust
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use tracing::info;

use crate::actors::{Actor, ActorError};

/// Messages for the LLM gateway actor.
pub enum LlmMessage {
    /// Complete a prompt (non-streaming).
    Complete {
        prompt: String,
        reply_to: tokio::sync::oneshot::Sender<Result<String, ActorError>>,
    },
    /// Stream a response token-by-token.
    Stream {
        prompt: String,
        reply_to: tokio::sync::oneshot::Sender<Result<tokio::sync::mpsc::Receiver<String>, ActorError>>,
    },
}

/// LLM gateway client actor backed by DeepSeek API.
pub struct LlmActor {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl LlmActor {
    /// Create a new LlmActor. Reads DEEPSEEK_API_KEY from env.
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            api_key: std::env::var("DEEPSEEK_API_KEY").unwrap_or_default(),
            model: std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-chat".to_string()),
            base_url: std::env::var("DEEPSEEK_BASE_URL")
                .unwrap_or_else(|_| "https://api.deepseek.com/v1".to_string()),
        }
    }

    /// Call the DeepSeek chat completions API.
    async fn call_deepseek(&self, prompt: &str) -> Result<String, ActorError> {
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&json!({
                "model": self.model,
                "messages": [{"role": "user", "content": prompt}],
                "temperature": 0.7,
                "max_tokens": 4096,
            }))
            .send()
            .await
            .map_err(|e| ActorError::Internal(format!("DeepSeek request failed: {}", e)))?;

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ActorError::Internal(format!("DeepSeek parse failed: {}", e)))?;

        let content = data["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(content)
    }
}

impl Default for LlmActor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Actor for LlmActor {
    type Message = LlmMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            LlmMessage::Complete { prompt, reply_to } => {
                info!("LLM: complete ({} chars, model: {})", prompt.len(), self.model);
                let result = self.call_deepseek(&prompt).await;
                let _ = reply_to.send(result);
            }
            LlmMessage::Stream { prompt, reply_to } => {
                info!("LLM: stream ({} chars, model: {})", prompt.len(), self.model);
                let (tx, rx) = tokio::sync::mpsc::channel(64);
                let _ = reply_to.send(Ok(rx));
                // Stub: streaming TBD — for now just send one response
                let content = self.call_deepseek(&prompt).await.unwrap_or_default();
                let _ = tx.try_send(content);
            }
        }
    }
}
```

**Dependencies to add** to `core/Cargo.toml`:

```toml
reqwest = { version = "0.12", features = ["json"] }
```

---

## 4. Phase 2: Create AgentMessage and AgentActor Types

### 4.1 Add new model types

**File:** `core/src/models/memory_graph.rs` — types already exist for agent infrastructure:

- `NodeType::Agent`, `NodeType::Plan`, `NodeType::PlanStep`, `NodeType::Execution`, `NodeType::TaskResult`, `NodeType::Artifact`, `NodeType::ErrorPattern`
- `RelationshipType::UsesTool`, `RelationshipType::FollowsPlan`, `RelationshipType::ContainsStep`, `RelationshipType::Precedes`, `RelationshipType::Produced`, `RelationshipType::EncounteredError`, `RelationshipType::ResolvedBy`, `RelationshipType::PartOfExecution`, `RelationshipType::ExecutedBy`
- `ExecutionStatus`, `ToolExecution`, `ErrorFingerprint`, `PlanStepOrder`, `PlanStepDependency`, `SuggestedFix`

These are already in the file and use `#[allow(dead_code)]` — they're ready to use.

### 4.2 Create agent-specific message types

**File:** `core/src/actors/agent/mod.rs` (new directory)

```rust
pub mod generalist;
pub mod planner;

use tokio::sync::oneshot;

/// Shared agent response type.
pub struct AgentResponse {
    pub content: String,
    pub correlation_id: String,
}

/// Strategies for prompt augmentation and caching.
pub enum CacheMode {
    Full,   // Reuse global cached prefix
    Domain, // Reuse domain-specific cached prefix
    None,   // No caching, build from scratch
}

pub enum AgentIntent {
    Generalist,
    Planner,
    DomainSpecific(String),
}

/// The message type for all agent actors.
pub enum AgentMessage {
    /// Execute a query with the given augmentation strategy.
    Query {
        query: String,
        user_id: String,
        session_id: String,
        intent: AgentIntent,
        correlation_id: String,
        reply_to: oneshot::Sender<Result<AgentResponse, crate::actors::ActorError>>,
    },
    /// Result from a tool call that was routed via McpHandlerActor.
    ToolResult {
        tool_name: String,
        result: String,
        correlation_id: String,
    },
    /// Invalidate the cached prefix with the given hash.
    InvalidatePrefix {
        prefix_hash: String,
    },
}
```

### 4.3 Register module

**File:** `core/src/actors/mod.rs` — add:

```rust
pub mod agent;
```

And re-export:

```rust
pub use agent::{AgentMessage, AgentResponse, AgentIntent, CacheMode};
```

---

## 5. Phase 3: Build GeneralistAgent Actor

**File:** `core/src/actors/agent/generalist.rs`

The GeneralistAgent:
1. Receives an `AgentMessage::Query`
2. Builds a caching-aware prompt by querying the graph for components
3. Calls `LlmActor` with the assembled prompt
4. Parses tool calls from the LLM response, routes them via `McpHandlerActor`
5. Feeds tool results back to the LLM in a loop
6. Returns the final response

### Overview

```
AgentMessage::Query
    │
    ▼
Build prefix (cached by component version hash):
    ├── System prompt (MemoryGraphMessage::QueryNodes for NodeType::SystemPrompt)
    ├── Tool definitions (MemoryGraphMessage::QueryNodes for NodeType::Tool)
    ├── Static knowledge (MemoryGraphMessage::QueryNodes for NodeType::Knowledge)
    └── Few-shot examples (MemoryGraphMessage::QueryNodes for NodeType::Example)
    │
    ▼
Build dynamic suffix (never cached):
    ├── User-specific knowledge (MemoryGraphMessage::SearchContext with query)
    ├── Conversation history (MemoryGraphMessage::QueryNodes for NodeType::Conversation)
    └── Current query
    │
    ▼
Call LlmActor with prefix + suffix
    │
    ▼
If tool_calls in response → route via McpHandlerActor::CallTool
    │  (loop back to LLM with tool results)
    ▼
Return AgentResponse::content
```

### Implementation

```rust
use async_trait::async_trait;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use tokio::sync::mpsc;
use tracing::info;

use crate::actors::agent::{AgentMessage, AgentResponse, AgentIntent, CacheMode};
use crate::actors::llm::LlmMessage;
use crate::actors::memory_graph::MemoryGraphMessage;
use crate::actors::mcp_handler::McpHandlerMessage;
use crate::actors::{Actor, ActorError, ToolInfo, ToolMessage};
use crate::models::memory_graph::{
    NodeFilter, NodeType, NodeInput, RelationshipType, SearchOptions,
};

/// Generalist agent — handles simple queries with prefix caching.
pub struct GeneralistAgent {
    agent_id: String,
    /// Sender to LlmActor.
    llm_tx: mpsc::Sender<LlmMessage>,
    /// Sender to MemoryGraphActor.
    memory_graph_tx: mpsc::Sender<MemoryGraphMessage>,
    /// Sender to McpHandlerActor (for embedded tools).
    mcp_handler_tx: mpsc::Sender<McpHandlerMessage>,
    /// Prefix cache: hash → cached prefix string.
    prefix_cache: HashMap<String, String>,
    /// Current component version hashes (for cache invalidation).
    component_hashes: ComponentHashes,
}

/// Tracks the version hashes of each prompt component type.
/// When any of these change, the cached prefix is invalidated.
struct ComponentHashes {
    system_prompt_hash: u64,
    tools_hash: u64,
    knowledge_hash: u64,
    examples_hash: u64,
}

impl GeneralistAgent {
    /// Max iterations for the LLM + tool-call loop.
    const MAX_TOOL_ITERATIONS: u8 = 10;

    pub fn new(
        agent_id: String,
        llm_tx: mpsc::Sender<LlmMessage>,
        memory_graph_tx: mpsc::Sender<MemoryGraphMessage>,
        mcp_handler_tx: mpsc::Sender<McpHandlerMessage>,
    ) -> Self {
        Self {
            agent_id,
            llm_tx,
            memory_graph_tx,
            mcp_handler_tx,
            prefix_cache: HashMap::new(),
            component_hashes: ComponentHashes {
                system_prompt_hash: 0,
                tools_hash: 0,
                knowledge_hash: 0,
                examples_hash: 0,
            },
        }
    }

    // ─── Prompt Assembly ─────────────────────────────────────────

    /// Build the full prompt: cached prefix + dynamic suffix.
    async fn build_prompt(
        &mut self,
        query: &str,
        user_id: &str,
        session_id: &str,
        intent: &AgentIntent,
    ) -> String {
        let prefix = self.get_or_build_prefix(intent).await;
        let suffix = self.build_dynamic_suffix(query, user_id, session_id).await;
        format!("{}\n\n{}", prefix, suffix)
    }

    /// Get the cached prefix, or build it from GraphDB if cache miss.
    async fn get_or_build_prefix(&mut self, intent: &AgentIntent) -> String {
        let prefix_hash = self.compute_prefix_hash(intent);

        if let Some(cached) = self.prefix_cache.get(&prefix_hash) {
            return cached.clone();
        }

        // Cache miss — build fresh prefix from GraphDB
        let prefix = self.build_prefix_from_graphdb(intent).await;
        self.prefix_cache.insert(prefix_hash, prefix.clone());
        prefix
    }

    /// Compute a hash of all component versions for cache keying.
    fn compute_prefix_hash(&self, _intent: &AgentIntent) -> String {
        let mut hasher = DefaultHasher::new();
        self.component_hashes.system_prompt_hash.hash(&mut hasher);
        self.component_hashes.tools_hash.hash(&mut hasher);
        self.component_hashes.knowledge_hash.hash(&mut hasher);
        self.component_hashes.examples_hash.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Build the prompt prefix by querying GraphDB for static components.
    /// Queries use hash-sorted ordering for byte-exact reproducibility.
    async fn build_prefix_from_graphdb(&mut self, _intent: &AgentIntent) -> String {
        let mut parts: Vec<String> = Vec::new();

        // 1. System prompt — query NodeType::SystemPrompt nodes
        if let Some(system) = self.query_system_prompt().await {
            parts.push(system);
        }

        // 2. Tool definitions — query NodeType::Tool nodes
        let tools = self.query_tools().await;
        if !tools.is_empty() {
            parts.push("Available tools:".to_string());
            parts.push(tools.join("\n"));
        }

        // 3. Static knowledge — query NodeType::Knowledge nodes
        let knowledge = self.query_knowledge().await;
        if !knowledge.is_empty() {
            parts.push("Knowledge:".to_string());
            parts.push(knowledge.join("\n"));
        }

        // 4. Few-shot examples — query NodeType::Example nodes
        let examples = self.query_examples().await;
        if !examples.is_empty() {
            parts.push("Examples:".to_string());
            parts.push(examples.join("\n"));
        }

        parts.join("\n\n")
    }

    /// Build the dynamic suffix (never cached — user-specific).
    async fn build_dynamic_suffix(
        &mut self,
        query: &str,
        _user_id: &str,
        _session_id: &str,
    ) -> String {
        let mut parts: Vec<String> = Vec::new();

        // Semantic search for relevant context
        let context = self.search_context(query).await;
        if !context.is_empty() {
            parts.push("Relevant context:".to_string());
            parts.push(context.join("\n"));
        }

        // Conversation history (last N messages)
        // (Can query NodeType::Conversation nodes filtered by session_id)

        parts.push(format!("User query: {}", query));
        parts.join("\n\n")
    }

    // ─── GraphDB Query Helpers ──────────────────────────────────

    /// Query the most recent active SystemPrompt node.
    async fn query_system_prompt(&self) -> Option<String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let filter = NodeFilter {
            node_type: Some(NodeType::Agent),
            subtype: Some("system_prompt".to_string()),
            ..Default::default()
        };
        let _ = self.memory_graph_tx
            .send(MemoryGraphMessage::QueryNodes { filter, reply_to: tx })
            .await;
        if let Ok(Ok(nodes)) = rx.await {
            nodes.first().and_then(|n| n.description.clone())
        } else {
            None
        }
    }

    /// Query all enabled Tool nodes — hash-sorted by GraphDB.
    async fn query_tools(&self) -> Vec<String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let filter = NodeFilter {
            node_type: Some(NodeType::Tool),
            ..Default::default()
        };
        let _ = self.memory_graph_tx
            .send(MemoryGraphMessage::QueryNodes { filter, reply_to: tx })
            .await;
        if let Ok(Ok(nodes)) = rx.await {
            nodes
                .iter()
                .map(|n| format!("- {}: {}", n.name, n.description.as_deref().unwrap_or("")))
                .collect()
        } else {
            vec![]
        }
    }

    /// Query all active Knowledge nodes — hash-sorted by GraphDB.
    async fn query_knowledge(&self) -> Vec<String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let filter = NodeFilter {
            node_type: Some(NodeType::Entity),
            subtype: Some("knowledge".to_string()),
            ..Default::default()
        };
        let _ = self.memory_graph_tx
            .send(MemoryGraphMessage::QueryNodes { filter, reply_to: tx })
            .await;
        if let Ok(Ok(nodes)) = rx.await {
            nodes.iter().filter_map(|n| n.description.clone()).collect()
        } else {
            vec![]
        }
    }

    /// Query Example nodes — hash-sorted by GraphDB.
    async fn query_examples(&self) -> Vec<String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let filter = NodeFilter {
            node_type: Some(NodeType::Entity),
            subtype: Some("example".to_string()),
            ..Default::default()
        };
        let _ = self.memory_graph_tx
            .send(MemoryGraphMessage::QueryNodes { filter, reply_to: tx })
            .await;
        if let Ok(Ok(nodes)) = rx.await {
            nodes.iter().filter_map(|n| n.description.clone()).collect()
        } else {
            vec![]
        }
    }

    /// Semantic search for context relevant to the query.
    async fn search_context(&self, query: &str) -> Vec<String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self.memory_graph_tx
            .send(MemoryGraphMessage::SearchContext {
                query: query.to_string(),
                options: Some(SearchOptions {
                    top_k: Some(5),
                    threshold: Some(0.6),
                    ..Default::default()
                }),
                reply_to: tx,
            })
            .await;
        if let Ok(Ok(result)) = rx.await {
            result
                .nodes
                .into_iter()
                .map(|sn| format!("[{}] {}: {}", sn.source.as_ref(), sn.node.name, sn.node.description.unwrap_or_default()))
                .collect()
        } else {
            vec![]
        }
    }

    // ─── Tool Call Loop ─────────────────────────────────────────

    /// Parse tool calls from an LLM response and execute them.
    /// Returns the tool results as formatted text for re-injection.
    async fn execute_tool_calls(&self, response: &str) -> Vec<String> {
        // The LLM response may contain tool call JSON blocks.
        // Parse them and route to McpHandlerActor.
        //
        // Expected format (as emitted by the system prompt):
        // ```tool
        // {"name": "read_file", "arguments": {"path": "/tmp/x"}}
        // ```
        let mut results = Vec::new();

        // Simple parser: find ```tool ... ``` blocks
        for block in response.split("```tool") {
            if let Some(json_str) = block.split("```").next() {
                let json_str = json_str.trim();
                if json_str.is_empty() {
                    continue;
                }
                if let Ok(call) = serde_json::from_str::<serde_json::Value>(json_str) {
                    let tool_name = call["name"].as_str().unwrap_or("").to_string();
                    let arguments = call["arguments"].clone();

                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = self.mcp_handler_tx
                        .send(McpHandlerMessage::CallTool {
                            name: tool_name.clone(),
                            args: arguments,
                            reply_to: tx,
                        })
                        .await;

                    match rx.await {
                        Ok(Ok(value)) => {
                            results.push(format!("Tool '{}' result: {}", tool_name, value));
                        }
                        Ok(Err(e)) => {
                            results.push(format!("Tool '{}' error: {}", tool_name, e));
                        }
                        Err(_) => {
                            results.push(format!("Tool '{}' error: channel closed", tool_name));
                        }
                    }
                }
            }
        }

        results
    }
}

#[async_trait]
impl Actor for GeneralistAgent {
    type Message = AgentMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            AgentMessage::Query {
                query,
                user_id,
                session_id,
                intent,
                correlation_id,
                reply_to,
            } => {
                info!(
                    "GeneralistAgent[{}]: query ({} chars, intent={:?})",
                    self.agent_id,
                    query.len(),
                    intent
                );

                // 1. Build the caching-aware prompt
                let prompt = self.build_prompt(&query, &user_id, &session_id, &intent).await;

                // 2. LLM tool-call loop
                let mut current_prompt = prompt;
                let mut all_tool_results: Vec<String> = Vec::new();

                for iteration in 0..Self::MAX_TOOL_ITERATIONS {
                    // Call LLM
                    let (llm_tx_resp, llm_rx) = tokio::sync::oneshot::channel();
                    let _ = self.llm_tx
                        .send(LlmMessage::Complete {
                            prompt: current_prompt.clone(),
                            reply_to: llm_tx_resp,
                        })
                        .await;

                    let llm_response = match llm_rx.await {
                        Ok(Ok(content)) => content,
                        Ok(Err(e)) => {
                            let _ = reply_to.send(Err(e));
                            return;
                        }
                        Err(e) => {
                            let _ = reply_to.send(Err(ActorError::Internal(format!(
                                "LLM channel error: {}",
                                e
                            ))));
                            return;
                        }
                    };

                    // Check for tool calls
                    let tool_results = self.execute_tool_calls(&llm_response).await;

                    if tool_results.is_empty() {
                        // No more tool calls — this is the final response
                        let _ = reply_to.send(Ok(AgentResponse {
                            content: llm_response,
                            correlation_id,
                        }));
                        return;
                    }

                    // Feed tool results back to LLM
                    all_tool_results.extend(tool_results);
                    current_prompt = format!(
                        "{}\n\nTool results:\n{}\n\nContinue:",
                        current_prompt,
                        all_tool_results.join("\n")
                    );
                }

                // Fallback: max iterations reached
                let _ = reply_to.send(Ok(AgentResponse {
                    content: "Max tool call iterations reached.".to_string(),
                    correlation_id,
                }));
            }

            AgentMessage::ToolResult {
                tool_name,
                result,
                correlation_id: _,
            } => {
                info!(
                    "GeneralistAgent[{}]: tool result from '{}' ({} chars)",
                    self.agent_id,
                    tool_name,
                    result.len()
                );
                // Tool results for the current query are handled inline in the loop above.
                // This variant is for async tool results arriving after the fact.
            }

            AgentMessage::InvalidatePrefix { prefix_hash } => {
                info!(
                    "GeneralistAgent[{}]: invalidating prefix {}",
                    self.agent_id, prefix_hash
                );
                self.prefix_cache.remove(&prefix_hash);
            }
        }
    }
}
```

### Key points

- All GraphDB queries use `MemoryGraphMessage::QueryNodes` with `NodeFilter` — the results are hash-sorted by `MemoryGraphActor::query_nodes()`
- Tools are called via `McpHandlerActor` (the existing embedded tool dispatch actor)
- External MCP servers can be called by adding `McpClientActor` calls similarly (using `McpClientMessage::CallTool`)
- The prefix cache is keyed by a hash of all component version hashes — when a component changes, the hash changes, and the next request rebuilds the prefix

---

## 6. Phase 4: Build PlannerAgent Actor

**File:** `core/src/actors/agent/planner.rs`

The PlannerAgent handles complex queries by:
1. Decomposing the query into 2–5 subtasks (via LLM)
2. Routing each subtask to a GeneralistAgent
3. Synthesizing results into a final response

```rust
use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::info;

use crate::actors::agent::{AgentMessage, AgentResponse, AgentIntent};
use crate::actors::llm::LlmMessage;
use crate::actors::memory_graph::MemoryGraphMessage;
use crate::actors::{Actor, ActorError};

/// Planner agent — decomposes complex queries into subtasks.
pub struct PlannerAgent {
    agent_id: String,
    llm_tx: mpsc::Sender<LlmMessage>,
    memory_graph_tx: mpsc::Sender<MemoryGraphMessage>,
    /// Sender to the GeneralistAgent for subtask execution.
    generalist_tx: mpsc::Sender<AgentMessage>,
}

impl PlannerAgent {
    pub fn new(
        agent_id: String,
        llm_tx: mpsc::Sender<LlmMessage>,
        memory_graph_tx: mpsc::Sender<MemoryGraphMessage>,
        generalist_tx: mpsc::Sender<AgentMessage>,
    ) -> Self {
        Self {
            agent_id,
            llm_tx,
            memory_graph_tx,
            generalist_tx,
        }
    }

    /// Use LLM to decompose a complex query into numbered subtasks.
    async fn decompose_query(&self, query: &str) -> Result<Vec<String>, ActorError> {
        let prompt = format!(
            "Break this query into 2-5 independent subtasks that can be executed in parallel.\n\
             Return ONLY a numbered list, one subtask per line.\n\n\
             Query: {}\n\nSubtasks:",
            query
        );

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.llm_tx
            .send(LlmMessage::Complete {
                prompt,
                reply_to: tx,
            })
            .await
            .map_err(|_| ActorError::ChannelClosed)?;

        let response = rx.await.map_err(|_| ActorError::ChannelClosed)?;
        let response = response?;

        // Parse numbered lines
        let subtasks: Vec<String> = response
            .lines()
            .filter(|line| line.trim().starts_with(|c: char| c.is_ascii_digit()))
            .map(|line| {
                line.trim_start_matches(|c: char| c.is_ascii_digit())
                    .trim_start_matches('.')
                    .trim()
                    .to_string()
            })
            .collect();

        Ok(subtasks)
    }

    /// Synthesize multiple subtask results into a single coherent response.
    async fn synthesize_results(&self, query: &str, results: &[(usize, String)]) -> Result<String, ActorError> {
        let results_text: Vec<String> = results
            .iter()
            .map(|(i, r)| format!("Subtask {}:\n{}", i, r))
            .collect();

        let prompt = format!(
            "Original query: {}\n\n\
             Results from subtasks:\n{}\n\n\
             Synthesize these results into a single coherent response:",
            query,
            results_text.join("\n---\n")
        );

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.llm_tx
            .send(LlmMessage::Complete {
                prompt,
                reply_to: tx,
            })
            .await
            .map_err(|_| ActorError::ChannelClosed)?;

        rx.await.map_err(|_| ActorError::ChannelClosed)?
    }
}

#[async_trait]
impl Actor for PlannerAgent {
    type Message = AgentMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            AgentMessage::Query {
                query,
                user_id,
                session_id,
                intent: _,
                correlation_id,
                reply_to,
            } => {
                info!(
                    "PlannerAgent[{}]: planning query ({} chars)",
                    self.agent_id,
                    query.len()
                );

                // 1. Decompose
                let subtasks = match self.decompose_query(&query).await {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = reply_to.send(Err(e));
                        return;
                    }
                };

                if subtasks.is_empty() {
                    let _ = reply_to.send(Ok(AgentResponse {
                        content: "Could not decompose query into subtasks.".to_string(),
                        correlation_id,
                    }));
                    return;
                }

                // 2. Execute subtasks via GeneralistAgent
                let mut results: Vec<(usize, String)> = Vec::new();
                for (i, subtask) in subtasks.iter().enumerate() {
                    let (sub_tx, sub_rx) = tokio::sync::oneshot::channel();
                    let _ = self.generalist_tx
                        .send(AgentMessage::Query {
                            query: subtask.clone(),
                            user_id: user_id.clone(),
                            session_id: session_id.clone(),
                            intent: AgentIntent::Generalist,
                            correlation_id: format!("{}-subtask-{}", correlation_id, i),
                            reply_to: sub_tx,
                        })
                        .await;

                    match sub_rx.await {
                        Ok(Ok(response)) => {
                            results.push((i, response.content));
                        }
                        Ok(Err(e)) => {
                            results.push((i, format!("Error: {}", e)));
                        }
                        Err(_) => {
                            results.push((i, "Channel closed".to_string()));
                        }
                    }
                }

                // 3. Synthesize
                let final_response = self
                    .synthesize_results(&query, &results)
                    .await
                    .unwrap_or_else(|e| format!("Synthesis error: {}", e));

                let _ = reply_to.send(Ok(AgentResponse {
                    content: final_response,
                    correlation_id,
                }));
            }

            AgentMessage::ToolResult { .. } => {
                // Planner doesn't use tools directly — delegates to GeneralistAgent
            }

            AgentMessage::InvalidatePrefix { .. } => {
                // Planner doesn't cache prefixes — delegates to GeneralistAgent
            }
        }
    }
}
```

---

## 7. Phase 5: Upgrade CoordinatorActor to Orchestrator

**File:** `core/src/actors/coordinator.rs`

Upgrade the `CoordinatorActor` to classify user intents and route to the appropriate agent. Keep the existing `CoordinatorMessage` for backward compatibility, and add orchestration logic.

```rust
use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::info;

use crate::actors::agent::{AgentIntent, AgentMessage, AgentResponse};
use crate::actors::llm::LlmMessage;
use crate::actors::memory_graph::MemoryGraphMessage;
use crate::actors::progress::{ProgressMessage, ProgressStatus, ProgressUpdate};
use crate::actors::{Actor, ActorError};
use crate::models::analysis::{CodeAnalysis, CodeAnalysisRequest};

/// Messages for the coordinator actor.
pub enum CoordinatorMessage {
    // ── Existing messages (keep for backward compat) ───────────
    ExplainCode {
        code: String,
        language: String,
        reply_to: tokio::sync::oneshot::Sender<Result<String, ActorError>>,
    },
    SearchCodebase {
        query: String,
        max_results: usize,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<(String, f64)>, ActorError>>,
    },
    AnalyzeCode {
        request: CodeAnalysisRequest,
        reply_to: tokio::sync::oneshot::Sender<Result<CodeAnalysis, ActorError>>,
    },

    // ── New orchestration message ────────────────────────────
    /// Route a user query to the appropriate agent.
    UserQuery {
        query: String,
        user_id: String,
        session_id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<String, ActorError>>,
    },
}

/// Orchestrator — routes user queries to agents.
///
/// Wraps the existing CoordinatorActor functionality and adds
/// intent classification + agent routing.
pub struct CoordinatorActor {
    memory_graph_tx: mpsc::Sender<MemoryGraphMessage>,
    llm_tx: mpsc::Sender<LlmMessage>,
    progress_tx: mpsc::Sender<ProgressMessage>,
    /// Sender to the generalist agent.
    generalist_tx: Option<mpsc::Sender<AgentMessage>>,
    /// Sender to the planner agent.
    planner_tx: Option<mpsc::Sender<AgentMessage>>,
}

impl CoordinatorActor {
    pub fn new(
        memory_graph_tx: mpsc::Sender<MemoryGraphMessage>,
        llm_tx: mpsc::Sender<LlmMessage>,
        progress_tx: mpsc::Sender<ProgressMessage>,
    ) -> Self {
        Self {
            memory_graph_tx,
            llm_tx,
            progress_tx,
            generalist_tx: None,
            planner_tx: None,
        }
    }

    /// Register the generalist agent's sender.
    pub fn register_generalist(&mut self, tx: mpsc::Sender<AgentMessage>) {
        self.generalist_tx = Some(tx);
    }

    /// Register the planner agent's sender.
    pub fn register_planner(&mut self, tx: mpsc::Sender<AgentMessage>) {
        self.planner_tx = Some(tx);
    }

    // ─── Intent Classification ────────────────────────────────

    /// Classify a user query into an intent and select the appropriate agent.
    fn classify_intent(&self, query: &str) -> AgentIntent {
        let query_lower = query.to_lowercase();

        // Complex keywords → route to Planner
        let complex_keywords = [
            "compare", "report", "analyze", "overview", "plan",
            "design", "architecture", "refactor", "migrate",
        ];
        for keyword in &complex_keywords {
            if query_lower.contains(keyword) {
                return AgentIntent::Planner;
            }
        }

        // Detect domain-specific queries
        let domain_keywords: [(&str, &[&str]); 4] = [
            ("filesystem", &["file", "directory", "folder", "read", "write", "path"]),
            ("github", &["repo", "pr", "commit", "issue", "clone", "push"]),
            ("terminal", &["command", "run", "execute", "shell", "script"]),
            ("search", &["search", "find", "query", "locate"]),
        ];

        for (domain, keywords) in &domain_keywords {
            if keywords.iter().any(|k| query_lower.contains(k)) {
                return AgentIntent::DomainSpecific(domain.to_string());
            }
        }

        AgentIntent::Generalist
    }
}

#[async_trait]
impl Actor for CoordinatorActor {
    type Message = CoordinatorMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            // ── Route user queries ─────────────────────────────
            CoordinatorMessage::UserQuery {
                query,
                user_id,
                session_id,
                reply_to,
            } => {
                info!("Coordinator: route user query ({} chars)", query.len());

                let intent = self.classify_intent(&query);
                info!("Coordinator: classified as {:?}", intent);

                let agent_tx = match &intent {
                    AgentIntent::Planner => &self.planner_tx,
                    _ => &self.generalist_tx, // Generalist handles DomainSpecific too
                };

                let agent_tx = match agent_tx {
                    Some(tx) => tx.clone(),
                    None => {
                        let _ = reply_to.send(Err(ActorError::Internal(
                            "No agent available to handle this request".to_string(),
                        )));
                        return;
                    }
                };

                let correlation_id = uuid::Uuid::new_v4().to_string();

                let (agent_reply_tx, agent_reply_rx) = tokio::sync::oneshot::channel();
                let _ = agent_tx
                    .send(AgentMessage::Query {
                        query,
                        user_id,
                        session_id,
                        intent,
                        correlation_id,
                        reply_to: agent_reply_tx,
                    })
                    .await;

                match agent_reply_rx.await {
                    Ok(Ok(response)) => {
                        let _ = reply_to.send(Ok(response.content));
                    }
                    Ok(Err(e)) => {
                        let _ = reply_to.send(Err(e));
                    }
                    Err(e) => {
                        let _ = reply_to.send(Err(ActorError::Internal(format!(
                            "Agent channel error: {}",
                            e
                        ))));
                    }
                }
            }

            // ── Existing handlers (keep as-is) ─────────────────
            CoordinatorMessage::ExplainCode {
                code,
                language,
                reply_to,
            } => {
                info!(
                    "Coordinator: explain_code ({} chars, {})",
                    code.len(),
                    language
                );

                let _ = self
                    .progress_tx
                    .send(ProgressMessage::Publish(ProgressUpdate {
                        task_id: "explain".to_string(),
                        message: "Analyzing code...".to_string(),
                        percent: 30.0,
                        status: ProgressStatus::Running,
                    }))
                    .await;

                let llm_tx = self.llm_tx.clone();
                let progress_tx = self.progress_tx.clone();
                tokio::spawn(async move {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = llm_tx
                        .send(LlmMessage::Complete {
                            prompt: format!(
                                "Explain this {} code:\n```\n{}\n```",
                                language, code
                            ),
                            reply_to: tx,
                        })
                        .await;
                    match rx.await {
                        Ok(Ok(response)) => {
                            let _ = progress_tx
                                .send(ProgressMessage::Publish(ProgressUpdate {
                                    task_id: "explain".to_string(),
                                    message: "Explanation complete".to_string(),
                                    percent: 100.0,
                                    status: ProgressStatus::Completed,
                                }))
                                .await;
                            let _ = reply_to.send(Ok(response));
                        }
                        Ok(Err(e)) => {
                            let _ = reply_to.send(Err(e));
                        }
                        Err(e) => {
                            let _ = reply_to.send(Err(ActorError::Internal(format!(
                                "Actor error: {}",
                                e
                            ))));
                        }
                    }
                });
            }

            CoordinatorMessage::SearchCodebase {
                query,
                max_results,
                reply_to,
            } => {
                info!("Coordinator: search_codebase ({})", query);

                let memory_graph_tx = self.memory_graph_tx.clone();
                tokio::spawn(async move {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = memory_graph_tx
                        .send(MemoryGraphMessage::SearchContext {
                            query,
                            options: Some(crate::models::memory_graph::SearchOptions {
                                top_k: Some(max_results),
                                threshold: Some(0.5),
                                node_types: None,
                                max_depth: None,
                                include_structural: Some(true),
                                recency_weight: None,
                            }),
                            reply_to: tx,
                        })
                        .await;
                    match rx.await {
                        Ok(Ok(result)) => {
                            let items: Vec<(String, f64)> = result
                                .nodes
                                .into_iter()
                                .map(|sn| (sn.node.name, sn.score))
                                .collect();
                            let _ = reply_to.send(Ok(items));
                        }
                        Ok(Err(e)) => {
                            let _ = reply_to.send(Err(e));
                        }
                        Err(e) => {
                            let _ = reply_to.send(Err(ActorError::Internal(format!(
                                "Actor error: {}",
                                e
                            ))));
                        }
                    }
                });
            }

            CoordinatorMessage::AnalyzeCode {
                request,
                reply_to,
            } => {
                info!("Coordinator: analyze_code ({})", request.language);

                let analysis = CodeAnalysis {
                    summary: format!(
                        "Analysis of {} code ({} chars)",
                        request.language,
                        request.code.len()
                    ),
                    complexity: None,
                    symbols: vec![],
                    suggestions: vec![],
                };
                let _ = reply_to.send(Ok(analysis));
            }
        }
    }
}
```

---

## 8. Phase 6: Registration and Startup

**File:** `core/src/main.rs`

Wire everything together at startup.

```rust
mod actors;
mod embedder;
mod framework;
mod graph;
mod mcp;
mod mcp_server;
mod models;

use crate::actors::agent::{AgentMessage, GeneralistAgent, PlannerAgent};
use crate::actors::coordinator::{CoordinatorActor, CoordinatorMessage};
use crate::actors::llm::{LlmActor, LlmMessage};
use crate::actors::mcp_client::{McpClientActor, McpClientMessage};
use crate::actors::mcp_handler::{McpHandlerActor, McpHandlerMessage};
use crate::actors::memory_graph::{MemoryGraphActor, MemoryGraphMessage};
use crate::actors::progress::{ProgressActor, ProgressMessage};
use crate::actors::tools::{register_all_tools, ToolRegistry};
use crate::actors::{ActorSystem};
use crate::framework::Actor;
use crate::graph::GraphDb;
use crate::mcp_server::config::McpConfig;
use crate::mcp_server::server::MCPServer;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let system = ActorSystem::new();

    // ── 1. GraphDB + MemoryGraphActor ──────────────────────────
    let graph_db = GraphDb::new_in_memory()
        .expect("Failed to create in-memory graph");

    // Use default embedder (stub — can be upgraded later)
    let embedder = crate::embedder::create_default_embedder();

    let memory_graph = MemoryGraphActor::new(
        std::sync::Arc::new(graph_db),
        embedder,
    );
    let (memory_graph_tx, _memory_graph_handle) = system.spawn(memory_graph);

    // ── 2. Load MCP config ─────────────────────────────────────
    let mcp_config = McpConfig::load().unwrap_or_default();

    // ── 3. MCPServer + embedded tools ──────────────────────────
    let mut mcp_server = MCPServer::new();
    register_all_tools(&mut mcp_server, &mcp_config);
    let mcp_server = std::sync::Arc::new(mcp_server);

    let mcp_handler = McpHandlerActor::new(mcp_server);
    let (mcp_handler_tx, _mcp_handler_handle) = system.spawn(mcp_handler);

    // ── 4. External MCP client ─────────────────────────────────
    let mcp_client = McpClientActor::new();
    let (_mcp_client_tx, _mcp_client_handle) = system.spawn(mcp_client);

    // ── 5. Sync MCP servers with graph ─────────────────────────
    let sync_tx = memory_graph_tx.clone();
    let sync_config = mcp_config.clone();
    tokio::spawn(async move {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = sync_tx
            .send(MemoryGraphMessage::SyncMcpServers {
                config: sync_config,
                reply_to: tx,
            })
            .await;
        if let Ok(Ok(result)) = rx.await {
            tracing::info!("MCP sync complete: {:#?}", result);
        }
    });

    // ── 6. LlmActor (DeepSeek) ─────────────────────────────────
    let llm = LlmActor::new();
    let (llm_tx, _llm_handle) = system.spawn(llm);

    // ── 7. Progress actor ──────────────────────────────────────
    let progress = ProgressActor::new();
    let (progress_tx, _progress_handle) = system.spawn(progress);

    // ── 8. Agent actors ────────────────────────────────────────
    let generalist = GeneralistAgent::new(
        "generalist-1".to_string(),
        llm_tx.clone(),
        memory_graph_tx.clone(),
        mcp_handler_tx.clone(),
    );
    let (generalist_tx, _generalist_handle) = system.spawn(generalist);

    let planner = PlannerAgent::new(
        "planner-1".to_string(),
        llm_tx.clone(),
        memory_graph_tx.clone(),
        generalist_tx.clone(),
    );
    let (planner_tx, _planner_handle) = system.spawn(planner);

    // ── 9. Coordinator/Orchestrator ────────────────────────────
    let mut coordinator = CoordinatorActor::new(
        memory_graph_tx.clone(),
        llm_tx.clone(),
        progress_tx.clone(),
    );
    coordinator.register_generalist(generalist_tx);
    coordinator.register_planner(planner_tx);
    let (coordinator_tx, _coordinator_handle) = system.spawn(coordinator);

    // ── 10. Demo query ─────────────────────────────────────────
    let (tx, rx) = tokio::sync::oneshot::channel();
    coordinator_tx
        .send(CoordinatorMessage::UserQuery {
            query: "Read README.md and summarize it".to_string(),
            user_id: "user-1".to_string(),
            session_id: "session-1".to_string(),
            reply_to: tx,
        })
        .await
        .unwrap();

    match rx.await {
        Ok(Ok(response)) => println!("Response:\n{}", response),
        Ok(Err(e)) => eprintln!("Error: {}", e),
        Err(e) => eprintln!("Channel error: {}", e),
    }
}
```

### Update `core/src/actors/tools/mod.rs`

Ensure there's a `register_all_tools` function that registers tools based on config:

```rust
use crate::actors::tools::sample::EchoTool;
use crate::actors::tools::read_file::ReadFileTool;
use crate::actors::tools::write_file::WriteFileTool;
use crate::actors::tools::list_dir::ListDirTool;
use crate::actors::tools::explain_code::ExplainCodeTool;
use crate::actors::tools::search_codebase::SearchCodebaseTool;
use crate::actors::tools::analyze_deps::AnalyzeDepsTool;
use crate::actors::tools::code_metrics::CodeMetricsTool;
use crate::mcp_server::config::McpConfig;
use crate::mcp_server::server::MCPServer;
use crate::actors::ToolInfo;

/// Register all enabled embedded tools on the MCPServer.
pub fn register_all_tools(server: &mut MCPServer, config: &McpConfig) {
    if config.embedded_tools.echo.enabled {
        server.register_tool(EchoTool::tool_info(), EchoTool);
    }
    if config.embedded_tools.read_file.enabled {
        server.register_tool(ReadFileTool::tool_info(), ReadFileTool::new(
            config.embedded_tools.read_file.workspace_root.clone(),
        ));
    }
    if config.embedded_tools.write_file.enabled {
        server.register_tool(WriteFileTool::tool_info(), WriteFileTool::new(
            config.embedded_tools.write_file.workspace_root.clone(),
        ));
    }
    if config.embedded_tools.list_directory.enabled {
        server.register_tool(ListDirTool::tool_info(), ListDirTool::new(
            config.embedded_tools.list_directory.workspace_root.clone(),
        ));
    }
    if config.embedded_tools.explain_code.enabled {
        server.register_tool(ExplainCodeTool::tool_info(), ExplainCodeTool);
    }
    if config.embedded_tools.search_codebase.enabled {
        server.register_tool(SearchCodebaseTool::tool_info(), SearchCodebaseTool);
    }
    if config.embedded_tools.analyze_dependencies.enabled {
        server.register_tool(AnalyzeDepsTool::tool_info(), AnalyzeDepsTool);
    }
    if config.embedded_tools.get_code_metrics.enabled {
        server.register_tool(CodeMetricsTool::tool_info(), CodeMetricsTool);
    }
}
```

---

## 9. Key API Reference

### MemoryGraphActor — available `MemoryGraphMessage` variants

| Variant | What it does |
|---------|-------------|
| `StoreNode { node: NodeInput, reply_to }` | Create a new node (checks duplicate) |
| `QueryNodes { filter: NodeFilter, reply_to }` | Query nodes by type/name; hash-sorted |
| `GetNode { id, reply_to }` | Get single node by UUID |
| `UpdateNode { id, updates, reply_to }` | Partial update |
| `DeleteNode { id, reply_to }` | Delete node + cascade edges |
| `CreateRelationship { rel: RelationshipInput, reply_to }` | Create edge (checks acyclic DependsOn) |
| `GetRelationships { node_id, reply_to }` | Get edges for a node; hash-sorted |
| `SearchContext { query, options, reply_to }` | Semantic + text fallback search |
| `AddMemory { text, metadata, reply_to }` | Add a memory entry |
| `Recall { query, limit, reply_to }` | Text-based memory recall; hash-sorted |
| `GetProjectContext { reply_to }` | Get full project snapshot |
| `Traverse { start_node_id, options, reply_to }` | BFS graph traversal |

### McpHandlerActor — available `McpHandlerMessage` variants

| Variant | What it does |
|---------|-------------|
| `ListTools { reply_to }` | Get all registered tool metadata |
| `CallTool { name, args, reply_to }` | Call a tool by name with arguments |

### McpClientActor — available `McpClientMessage` variants

| Variant | What it does |
|---------|-------------|
| `Connect { server_name, reply_to }` | Connect to an external MCP server |
| `CallTool { server_name, tool_name, arguments, reply_to }` | Call tool on external server |
| `GetTools { server_name, reply_to }` | List tools from an external server |
| `Disconnect { server_name, reply_to }` | Disconnect from server |

### LlmActor — available `LlmMessage` variants

| Variant | What it does |
|---------|-------------|
| `Complete { prompt, reply_to }` | Call DeepSeek API, return full response |
| `Stream { prompt, reply_to }` | Stream response token-by-token |

### NodeFilter usage

```rust
// Query all Tool nodes
NodeFilter {
    node_type: Some(NodeType::Tool),
    ..Default::default()
}

// Query by name substring
NodeFilter {
    node_type: Some(NodeType::Entity),
    name: Some("auth".to_string()),
    ..Default::default()
}

// Query by subtype
NodeFilter {
    node_type: Some(NodeType::Entity),
    subtype: Some("knowledge".to_string()),
    ..Default::default()
}
```

---

## Implementation Checklist

### Phase 1: LlmActor upgrade
- [ ] Add `reqwest` dependency to `Cargo.toml`
- [ ] Replace `LlmActor` stub with real DeepSeek API client
- [ ] Verify `cargo build` compiles

### Phase 2: Agent types
- [ ] Create `core/src/actors/agent/mod.rs` with `AgentMessage`, `AgentResponse`, `AgentIntent`, `CacheMode`
- [ ] Update `core/src/actors/mod.rs` to include `pub mod agent` and re-exports

### Phase 3: GeneralistAgent
- [ ] Create `core/src/actors/agent/generalist.rs`
- [ ] Implement prefix caching (HashMap keyed by component hash)
- [ ] Implement prompt assembly from GraphDB queries
- [ ] Implement tool call loop via `McpHandlerMessage::CallTool`
- [ ] Verify `cargo build` compiles

### Phase 4: PlannerAgent
- [ ] Create `core/src/actors/agent/planner.rs`
- [ ] Implement query decomposition via LLM
- [ ] Implement subtask routing to GeneralistAgent
- [ ] Implement result synthesis via LLM

### Phase 5: Coordinator upgrade
- [ ] Add `UserQuery` variant to `CoordinatorMessage`
- [ ] Add intent classification logic
- [ ] Add agent registration methods
- [ ] Keep existing `CoordinatorMessage` variants for backward compat

### Phase 6: Startup wiring
- [ ] Create `register_all_tools` in `core/src/actors/tools/mod.rs`
- [ ] Wire all actors in `main.rs` or `main_new.rs`
- [ ] Add MCP config sync to graph at startup

### Tests
- [ ] Unit test: Intent classification (generalist vs planner vs domain)
- [ ] Unit test: Prefix cache invalidation
- [ ] Unit test: Tool call parsing from LLM response
- [ ] Integration test: Agent → LLM → tool call flow

---

## Appendix: File Modification Summary

| File | Action |
|------|--------|
| `core/Cargo.toml` | Add `reqwest = { version = "0.12", features = ["json"] }` |
| `core/src/actors/llm.rs` | **Replace** — stub → real DeepSeek API |
| `core/src/actors/agent/mod.rs` | **Create new** — `AgentMessage`, `AgentResponse`, etc. |
| `core/src/actors/agent/generalist.rs` | **Create new** — `GeneralistAgent` |
| `core/src/actors/agent/planner.rs` | **Create new** — `PlannerAgent` |
| `core/src/actors/mod.rs` | **Modify** — add `pub mod agent`, re-export agent types |
| `core/src/actors/tools/mod.rs` | **Modify** — add `register_all_tools()` |
| `core/src/actors/coordinator.rs` | **Modify** — add `UserQuery`, intent classification, agent registration |
| `core/src/main.rs` or `core/src/main_new.rs` | **Modify** — wire all actors together |
