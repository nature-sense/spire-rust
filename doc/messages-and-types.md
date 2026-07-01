# Spire-Rust: Actor Messages & Data Types

> Reference document for the `core/` actor system — message enums, reply channels, and shared data structures.
>
> **Last updated:** 2026-06-30 (after GraphActor/VectorActor consolidation into MemoryGraphActor)

---

## 1. Actor System Overview

Four actors communicate via `tonari_actor` message passing. Each actor owns an `Addr<M>` handle and processes messages through its `Actor::handle` method.

```
                    ┌──────────────────┐
                    │  CoordinatorActor │──→ ProgressActor (broadcast progress)
                    │  (orchestrator)   │──→ LlmActor (LLM calls)
                    └──────┬───────────┘
                           │
                           ▼
                    MemoryGraphActor
                    (sole data store:
                     nodes, edges,
                     embeddings)
                           │
                      Embedder (trait)
```

| Actor | File | Role |
|-------|------|------|
| `CoordinatorActor` | `actors/coordinator.rs` | Top-level orchestrator; receives user requests, delegates to other actors |
| `MemoryGraphActor` | `actors/memory_graph.rs` | Sole data store — owns graph nodes, edges, and vector embeddings directly |
| `ProgressActor` | `actors/progress.rs` | Broadcasts progress updates via `tokio::sync::broadcast` |
| `LlmActor` | `actors/llm.rs` | LLM gateway (stub — echoes prompt) |

> **Note:** `GraphActor` and `VectorActor` were consolidated into `MemoryGraphActor` in June 2026. The old separate actors no longer exist. `MemoryGraphActor` now owns all data inline (in-memory `HashMap`s) rather than delegating to sub-actors.

---

## 2. Actor Message Definitions

### 2.1 `CoordinatorMessage` — `actors/coordinator.rs`

```rust
pub enum CoordinatorMessage {
    ExplainCode {
        code: String,
        language: String,
        reply_to: tokio::sync::oneshot::Sender<Result<String>>,
    },
    SearchCodebase {
        query: String,
        max_results: usize,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<(String, f64)>>>,
    },
    AnalyzeCode {
        request: CodeAnalysisRequest,
        reply_to: tokio::sync::oneshot::Sender<Result<CodeAnalysis>>,
    },
}
```

| Variant | Reply Type | Description |
|---------|-----------|-------------|
| `ExplainCode` | `Result<String>` | Explain a code snippet |
| `SearchCodebase` | `Result<Vec<(String, f64)>>` | Semantic search; returns `(node_id, score)` pairs |
| `AnalyzeCode` | `Result<CodeAnalysis>` | Static analysis of code |

---

### 2.2 `MemoryGraphMessage` — `actors/memory_graph.rs`

The sole data store actor. 14 variants covering all graph, memory, and maintenance operations:

```rust
pub enum MemoryGraphMessage {
    // ── Node Operations ──
    GetNode {
        id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<Option<GraphNode>>>,
    },
    QueryNodes {
        filter: NodeFilter,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<GraphNode>>>,
    },
    StoreNode {
        node: NodeInput,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    UpdateNode {
        id: String,
        updates: NodeUpdate,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    DeleteNode {
        id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
    // ── Relationship Operations ──
    CreateRelationship {
        rel: RelationshipInput,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphEdge>>,
    },
    GetRelationships {
        node_id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<GraphEdge>>>,
    },
    DeleteRelationship {
        id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
    // ── Traversal ──
    Traverse {
        start_node_id: String,
        options: TraversalOptions,
        reply_to: tokio::sync::oneshot::Sender<Result<TraversalResult>>,
    },
    // ── Context & Memory ──
    GetProjectContext {
        reply_to: tokio::sync::oneshot::Sender<Result<ProjectSnapshot>>,
    },
    SearchContext {
        query: String,
        options: Option<SearchOptions>,
        reply_to: tokio::sync::oneshot::Sender<Result<ContextSearchResult>>,
    },
    AddMemory {
        text: String,
        metadata: Option<MemoryMetadata>,
        reply_to: tokio::sync::oneshot::Sender<Result<MemoryEntry>>,
    },
    Recall {
        query: String,
        limit: Option<usize>,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<MemoryEntry>>>,
    },
    // ── Maintenance ──
    Sync {
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
}
```

| Variant | Reply Type | Description |
|---------|-----------|-------------|
| `GetNode` | `Result<Option<GraphNode>>` | Get single node by ID |
| `QueryNodes` | `Result<Vec<GraphNode>>` | Filtered query (by type, subtype, name) |
| `StoreNode` | `Result<GraphNode>` | Create node from `NodeInput` + spawn async embedding |
| `UpdateNode` | `Result<GraphNode>` | Partial update via `NodeUpdate` + re-embed if description changed |
| `DeleteNode` | `Result<()>` | Remove node + its associated edges |
| `CreateRelationship` | `Result<GraphEdge>` | Create edge from `RelationshipInput` |
| `GetRelationships` | `Result<Vec<GraphEdge>>` | Get edges for a node (both outgoing and incoming) |
| `DeleteRelationship` | `Result<()>` | Remove edge by ID |
| `Traverse` | `Result<TraversalResult>` | BFS traversal from start node (max depth, max nodes) |
| `GetProjectContext` | `Result<ProjectSnapshot>` | Project snapshot with stats |
| `SearchContext` | `Result<ContextSearchResult>` | Text-based search (fallback; no vector index yet) |
| `AddMemory` | `Result<MemoryEntry>` | Store conversation memory + spawn async embedding |
| `Recall` | `Result<Vec<MemoryEntry>>` | Text-based memory recall |
| `Sync` | `Result<()>` | Flush/persist (no-op stub) |

---

### 2.3 `ProgressMessage` — `actors/progress.rs`

```rust
pub enum ProgressMessage {
    Publish(ProgressUpdate),
    Subscribe {
        reply_to: tokio::sync::oneshot::Sender<tokio::sync::broadcast::Receiver<ProgressUpdate>>,
    },
}
```

| Variant | Reply Type | Description |
|---------|-----------|-------------|
| `Publish` | (fire-and-forget) | Broadcast a progress update |
| `Subscribe` | `broadcast::Receiver<ProgressUpdate>` | Get a receiver for progress events |

---

### 2.4 `LlmMessage` — `actors/llm.rs`

```rust
pub enum LlmMessage {
    Complete {
        prompt: String,
        reply_to: tokio::sync::oneshot::Sender<Result<String>>,
    },
    Stream {
        prompt: String,
        reply_to: tokio::sync::oneshot::Sender<tokio::sync::mpsc::Receiver<String>>,
    },
}
```

| Variant | Reply Type | Description |
|---------|-----------|-------------|
| `Complete` | `Result<String>` | Single response (stub: echoes first 100 chars) |
| `Stream` | `mpsc::Receiver<String>` | Streaming token-by-token (stub: sends one token) |

---

## 3. Data Model Types

### 3.1 `models/memory_graph.rs` — Core Graph Types

#### `GraphNode`

```rust
pub struct GraphNode {
    pub id: String,
    pub node_type: NodeType,
    pub subtype: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub properties: HashMap<String, serde_json::Value>,
    pub embedding_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u32,
}
```

#### `NodeType`

```rust
pub enum NodeType {
    Project,
    Entity,
    Decision,
    #[serde(rename = "activeContext")]
    ActiveContext,
    Blocker,
    Milestone,
    Standard,
    Conversation,
    Session,
    #[serde(other)]
    Unknown,
}
```

#### `NodeInput`

```rust
pub struct NodeInput {
    pub node_type: NodeType,
    pub subtype: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub properties: Option<HashMap<String, serde_json::Value>>,
    pub embedding_id: Option<String>,
}
```

#### `NodeUpdate`

```rust
pub struct NodeUpdate {
    pub node_type: Option<NodeType>,
    pub subtype: Option<Option<String>>,       // None = no change, Some(None) = clear
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub properties: Option<HashMap<String, serde_json::Value>>,
    pub embedding_id: Option<Option<String>>,
}
```

#### `NodeFilter`

```rust
pub struct NodeFilter {
    pub node_type: Option<NodeType>,
    pub subtype: Option<String>,
    pub name: Option<String>,
    pub status: Option<String>,
    pub tags: Option<Vec<String>>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}
```

#### `GraphEdge`

```rust
pub struct GraphEdge {
    pub id: String,
    pub edge_type: RelationshipType,
    pub from_id: String,
    pub to_id: String,
    pub properties: HashMap<String, serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub weight: Option<f64>,
}
```

#### `RelationshipType`

```rust
pub enum RelationshipType {
    #[serde(rename = "active_context")]
    ActiveContext,
    #[serde(rename = "has_decision")]
    HasDecision,
    #[serde(rename = "has_blocker")]
    HasBlocker,
    #[serde(rename = "has_milestone")]
    HasMilestone,
    #[serde(rename = "follows_standard")]
    FollowsStandard,
    #[serde(rename = "belongs_to")]
    BelongsTo,
    #[serde(rename = "depends_on")]
    DependsOn,
    #[serde(rename = "called_by")]
    CalledBy,
    Resolves,
    Supersedes,
    #[serde(rename = "semantically_related")]
    SemanticallyRelated,
    #[serde(rename = "conversation_context")]
    ConversationContext,
    #[serde(rename = "learned_from")]
    LearnedFrom,
    #[serde(rename = "session_worked_on")]
    SessionWorkedOn,
    #[serde(rename = "informed_by")]
    InformedBy,
    #[serde(other)]
    Unknown,
}
```

#### `RelationshipInput`

```rust
pub struct RelationshipInput {
    pub edge_type: RelationshipType,
    pub from_id: String,
    pub to_id: String,
    pub properties: Option<HashMap<String, serde_json::Value>>,
    pub weight: Option<f64>,
}
```

#### `TraversalOptions`

```rust
pub struct TraversalOptions {
    pub max_depth: u8,
    pub relationship_types: Option<Vec<RelationshipType>>,
    pub max_nodes: Option<usize>,
    pub direction: Option<TraversalDirection>,
}
```

#### `TraversalDirection`

```rust
pub enum TraversalDirection {
    #[serde(rename = "out")]
    Out,
    #[serde(rename = "in")]
    In,
    #[serde(rename = "both")]
    Both,
}
```

#### `TraversalResult`

```rust
pub struct TraversalResult {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub paths: Vec<TraversalPath>,
}
```

#### `TraversalPath`

```rust
pub struct TraversalPath {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}
```

#### `ProjectSnapshot`

```rust
pub struct ProjectSnapshot {
    pub project: GraphNode,
    pub active_context: Option<GraphNode>,
    pub milestones: Vec<GraphNode>,
    pub blockers: Vec<GraphNode>,
    pub recent_decisions: Vec<GraphNode>,
    pub recent_entities: Vec<GraphNode>,
    pub standards: Vec<GraphNode>,
    pub stats: ProjectStats,
}
```

#### `ProjectStats`

```rust
pub struct ProjectStats {
    pub total_nodes: usize,
    pub total_relationships: usize,
    pub last_updated: DateTime<Utc>,
}
```

#### `SearchOptions`

```rust
pub struct SearchOptions {
    pub top_k: Option<usize>,
    pub threshold: Option<f64>,
    pub node_types: Option<Vec<NodeType>>,
    pub max_depth: Option<u8>,
    pub include_structural: Option<bool>,
    pub recency_weight: Option<f64>,
}
```

#### `ContextSearchResult`

```rust
pub struct ContextSearchResult {
    pub nodes: Vec<ScoredNode>,
    pub relationships: Vec<GraphEdge>,
    pub total_results: usize,
    pub search_time_ms: u64,
    pub truncated: bool,
}
```

#### `ScoredNode`

```rust
pub struct ScoredNode {
    pub node: GraphNode,
    pub similarity: f64,
    pub source: RetrievalSource,
    pub score: f64,
}
```

#### `RetrievalSource`

```rust
pub enum RetrievalSource {
    Semantic,
    Structural,
    Ambient,
    Hybrid,
}
```

#### `MemoryMetadata`

```rust
pub struct MemoryMetadata {
    pub mem_type: Option<NodeType>,
    pub tags: Option<Vec<String>>,
    pub source: Option<String>,
    pub confidence: Option<f64>,
}
```

#### `MemoryEntry`

```rust
pub struct MemoryEntry {
    pub id: String,
    pub text: String,
    pub embedding_id: String,
    pub metadata: MemoryMetadata,
    pub node_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

---

### 3.2 `models/graph.rs` — Query & Result Types

```rust
pub struct GraphQuery {
    pub query_type: GraphQueryType,
    pub node_id: Option<String>,
    pub node_label: Option<String>,
    pub max_depth: Option<usize>,
    pub limit: Option<usize>,
}

pub enum GraphQueryType {
    Neighbors,
    Path,
    Search,
    Subgraph,
}

pub struct GraphResult {
    pub nodes: Vec<GraphNode>,    // re-exports memory_graph::GraphNode
    pub edges: Vec<GraphEdge>,    // re-exports memory_graph::GraphEdge
    pub total_count: usize,
}
```

> **Note:** `GraphNode` and `GraphEdge` in this module are re-exports from `models::memory_graph`. The old standalone definitions were removed to avoid duplication.

---

### 3.3 `models/analysis.rs` — Code Analysis Types

```rust
pub struct CodeAnalysisRequest {
    pub code: String,
    pub language: String,
    pub file_path: Option<String>,
}

pub struct CodeAnalysis {
    pub summary: String,
    pub complexity: Option<ComplexityScore>,
    pub symbols: Vec<SymbolInfo>,
    pub suggestions: Vec<String>,
}

pub struct ComplexityScore {
    pub cyclomatic: u32,
    pub cognitive: u32,
    pub lines_of_code: u32,
}

pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub line: u32,
    pub column: u32,
}

pub enum SymbolKind {
    Function, Class, Variable, Method, Interface,
    Enum, Struct, Trait, Module, Unknown,
}

pub struct SearchResult {
    pub file_path: String,
    pub line: u32,
    pub column: u32,
    pub snippet: String,
    pub score: f64,
    pub context: Option<String>,
}

pub struct SearchRequest {
    pub query: String,
    pub max_results: Option<usize>,
    pub file_pattern: Option<String>,
}
```

---

### 3.4 `models/embedding.rs` — Embedder Contract

```rust
pub struct Embedding {
    pub vector: Vec<f32>,           // 384-dimensional, L2-normalized
    pub text: String,
    pub text_hash: String,          // MD5 hex digest
    pub token_count: usize,
    pub dimensions: usize,          // always 384
    pub model_name: String,         // e.g. "sentence-transformers/all-MiniLM-L6-v2"
    pub generated_at: DateTime<Utc>,
}

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> anyhow::Result<Embedding>;
    async fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Embedding>>;
    fn dimensions(&self) -> usize;
}
```

---

## 4. MCP Layer

### 4.1 `mcp/server.rs` — Server Handler

```rust
pub struct SpireMcpHandler {
    coordinator: Option<CoordinatorActor>,  // TODO: wire up actor system
}
```

Implements `ServerHandler` from `rust-mcp-sdk`:
- `handle_list_tools_request` → returns tools from `mcp/tools.rs`
- `handle_call_tool_request` → dispatches to `handle_tool_call(name, args)`

### 4.2 `mcp/tools.rs` — Tool Definitions

Four tools exposed via MCP:

| Tool Name | Description | Required Params |
|-----------|-------------|-----------------|
| `explain_code` | Explain a code snippet | `code: string` |
| `search_codebase` | Regex or semantic search | `query: string` |
| `analyze_dependencies` | Dependency graph analysis | `path: string` |
| `get_code_metrics` | Code quality metrics | `path: string` |

### 4.3 `mcp/client.rs` — External MCP Client

```rust
pub struct McpClient {
    /// Connected MCP server name
    pub server_name: String,
    /// Underlying transport (stdio, WebSocket, etc.)
    pub transport: Box<dyn McpTransport>,
}
```

Planned to connect to 8 external MCP servers (not yet wired).

---

## 5. Progress Types

### `ProgressUpdate` — `actors/progress.rs`

```rust
pub struct ProgressUpdate {
    pub task_id: String,
    pub message: String,
    pub percent: f64,
    pub status: ProgressStatus,
}

pub enum ProgressStatus {
    Running,
    Completed,
    Failed,
}
```

---

## 6. Actor Wiring Diagram

```
main.rs
  │
  ├── spawns MemoryGraphActor  → Addr<MemoryGraphMessage>
  │     (owns nodes, edges, embeddings directly;
  │      receives Arc<dyn Embedder> for text→vector)
  ├── spawns LlmActor          → Addr<LlmMessage>
  ├── spawns ProgressActor     → Addr<ProgressMessage>
  └── spawns CoordinatorActor  → Addr<CoordinatorMessage>
        (receives memory_graph, llm, progress addrs)
```

The `MemoryGraphActor` is the sole data store. All graph, vector, and memory operations are handled inline within its `handle` method — no delegation to sub-actors. The `Embedder` trait (implemented by `CandleEmbedder`) is used for async text-to-vector generation spawned via `tokio::spawn`.
