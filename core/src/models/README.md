# Spire Models — Shared Data Structures

[![Rust](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)

The models module defines all shared data structures used across the Spire actor system — graph nodes and edges, code analysis types, embedding data, and query/result types. These types are the contract between actors and the foundation of the knowledge graph.

---

## Architecture

```
models/
├── mod.rs           # Re-exports all model modules
├── memory_graph.rs  # Core graph types (nodes, edges, memory, traversal)
├── graph.rs         # Query & result types
├── analysis.rs      # Code analysis types
└── embedding.rs     # Embedding types & Embedder trait
```

---

## `memory_graph.rs` — Core Graph Types

The central data model for the knowledge graph. These types are used by `MemoryGraphActor` and serialized for the TypeScript extension.

### GraphNode

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

### NodeType

```rust
pub enum NodeType {
    Project,         // Top-level project node
    Entity,          // Code entity (function, class, module)
    Decision,        // Architectural or design decision
    ActiveContext,   // Current working context
    Blocker,         // Issue or blocker
    Milestone,       // Project milestone
    Standard,        // Coding standard or convention
    Conversation,    // LLM conversation record
    Session,         // Development session
    Unknown,         // Fallback for unknown types
}
```

### NodeInput

Used when creating a new node:

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

### NodeUpdate

Used for partial updates. `Option<Option<T>>` semantics: `None` = no change, `Some(None)` = clear, `Some(Some(v))` = set.

```rust
pub struct NodeUpdate {
    pub node_type: Option<NodeType>,
    pub subtype: Option<Option<String>>,
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub properties: Option<HashMap<String, serde_json::Value>>,
    pub embedding_id: Option<Option<String>>,
}
```

### NodeFilter

Used for querying nodes:

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

### GraphEdge

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

### RelationshipType

```rust
pub enum RelationshipType {
    ActiveContext,       // Node is active context for project
    HasDecision,         // Node has a decision
    HasBlocker,          // Node has a blocker
    HasMilestone,        // Node has a milestone
    FollowsStandard,     // Node follows a standard
    BelongsTo,           // Node belongs to a parent
    DependsOn,           // Dependency relationship (acyclic enforced)
    CalledBy,            // Call graph relationship
    Resolves,            // Node resolves an issue
    Supersedes,          // Node supersedes another
    SemanticallyRelated, // Semantic similarity
    ConversationContext, // Conversation context link
    LearnedFrom,         // Knowledge learned from source
    SessionWorkedOn,     // Session worked on entity
    InformedBy,          // Decision informed by context
    Unknown,             // Fallback
}
```

### RelationshipInput

```rust
pub struct RelationshipInput {
    pub edge_type: RelationshipType,
    pub from_id: String,
    pub to_id: String,
    pub properties: Option<HashMap<String, serde_json::Value>>,
    pub weight: Option<f64>,
}
```

### Traversal Types

```rust
pub struct TraversalOptions {
    pub max_depth: u8,
    pub relationship_types: Option<Vec<RelationshipType>>,
    pub max_nodes: Option<usize>,
    pub direction: Option<TraversalDirection>,
}

pub enum TraversalDirection { Out, In, Both }

pub struct TraversalResult {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub paths: Vec<TraversalPath>,
}

pub struct TraversalPath {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}
```

### Context & Memory Types

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

pub struct ProjectStats {
    pub total_nodes: usize,
    pub total_relationships: usize,
    pub last_updated: DateTime<Utc>,
}

pub struct SearchOptions {
    pub top_k: Option<usize>,
    pub threshold: Option<f64>,
    pub node_types: Option<Vec<NodeType>>,
    pub max_depth: Option<u8>,
    pub include_structural: Option<bool>,
    pub recency_weight: Option<f64>,
}

pub struct ContextSearchResult {
    pub nodes: Vec<ScoredNode>,
    pub relationships: Vec<GraphEdge>,
    pub total_results: usize,
    pub search_time_ms: u64,
    pub truncated: bool,
}

pub struct ScoredNode {
    pub node: GraphNode,
    pub similarity: f64,
    pub source: RetrievalSource,
    pub score: f64,
}

pub enum RetrievalSource { Semantic, Structural, Ambient, Hybrid }

pub struct MemoryMetadata {
    pub mem_type: Option<NodeType>,
    pub tags: Option<Vec<String>>,
    pub source: Option<String>,
    pub confidence: Option<f64>,
}

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

## `graph.rs` — Query & Result Types

```rust
pub struct GraphQuery {
    pub query_type: GraphQueryType,
    pub node_id: Option<String>,
    pub node_label: Option<String>,
    pub max_depth: Option<usize>,
    pub limit: Option<usize>,
}

pub enum GraphQueryType {
    Neighbors,  // Get immediate neighbors
    Path,       // Find path between nodes
    Search,     // Search by label/property
    Subgraph,   // Get subgraph around node
}

pub struct GraphResult {
    pub nodes: Vec<GraphNode>,    // Re-exports from memory_graph
    pub edges: Vec<GraphEdge>,    // Re-exports from memory_graph
    pub total_count: usize,
}
```

> **Note:** `GraphNode` and `GraphEdge` in this module are re-exports from `models::memory_graph`. The old standalone definitions were removed to avoid duplication.

---

## `analysis.rs` — Code Analysis Types

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

## `embedding.rs` — Embedding Types

```rust
pub struct Embedding {
    pub vector: Vec<f32>,           // 384-dimensional, L2-normalized
    pub text: String,               // Original input text
    pub text_hash: String,          // MD5 hex digest for deduplication
    pub token_count: usize,         // Number of tokens after tokenization
    pub dimensions: usize,          // Always 384
    pub model_name: String,         // "sentence-transformers/all-MiniLM-L6-v2"
    pub generated_at: DateTime<Utc>, // Timestamp of embedding generation
}

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> anyhow::Result<Embedding>;
    async fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Embedding>>;
    fn dimensions(&self) -> usize;
}
```

---

## Related

- [Actors README](../actors/README.md) — How these types are used in message passing
- [Graph README](../graph/README.md) — How `GraphNode`/`GraphEdge` map to SeleneDB
- [Embedder README](../embedder/README.md) — The `Embedder` trait implementation
- [doc/messages-and-types.md](../../../doc/messages-and-types.md) — Complete type reference
- [Core README](../../README.md) — Project overview
