# Graph Schema Reference

> The Spire knowledge graph schema — node types, relationship types, constraints, and physical storage mapping.
>
> **Last updated:** 2026-07-01

---

## 1. Architecture Overview

The graph has **two layers**:

| Layer | Module | Description |
|-------|--------|-------------|
| **Physical** | `graph::GraphDb` (SeleneDB) | Labeled property graph with `u64` IDs, string labels, key-value properties, and optional vector indexes |
| **Application** | `models::memory_graph` | Typed schema with enums, UUID-based IDs, and constraint enforcement via `MemoryGraphActor` |

The `MemoryGraphActor` maintains a **bidirectional ID mapping** between external UUID strings and SeleneDB's compact `u64` IDs (`NodeId`/`EdgeId`).

```
External API (UUID strings)          SeleneDB (u64 IDs)
┌──────────────────────┐            ┌──────────────────┐
│  GraphNode { id:     │  ──────→  │  NodeId(42)      │
│    "a1b2..." }       │  ←──────  │  + labels        │
│  GraphEdge { id:     │  ──────→  │  + properties    │
│    "c3d4..." }       │  ←──────  │                  │
└──────────────────────┘            │  EdgeId(99)      │
                                    │  + predicate     │
                                    │  + properties    │
                                    └──────────────────┘
```

---

## 2. Node Types

### 2.1 `enum NodeType`

| Variant | Serde alias | Purpose |
|---------|-------------|---------|
| `Project` | — | Top-level project root node |
| `Entity` | — | Named entity discovered in the codebase |
| `Decision` | — | Architectural or design decision |
| `ActiveContext` | `activeContext` | Current active context for the session |
| `Blocker` | — | Blocking issue or impediment |
| `Milestone` | — | Project milestone or goal |
| `Standard` | — | Coding standard or convention being followed |
| `Conversation` | — | A chat conversation |
| `Session` | — | A development session |
| `McpServer` | `mcp_server` | An MCP server (embedded or external) |
| `Unknown` | `#[serde(other)]` | Fallback for code-analysis types (File, Function, Class, etc.) |

### 2.2 `struct GraphNode`

```rust
pub struct GraphNode {
    pub id: String,                              // UUID v4
    pub node_type: NodeType,                     // typed enum
    pub subtype: Option<String>,                 // e.g. "Function", "File", "Class"
    pub name: String,                            // human-readable name
    pub description: Option<String>,             // long-form text (used for embedding)
    pub properties: HashMap<String, Value>,      // arbitrary JSON metadata
    pub embedding_id: Option<String>,            // links to a generated vector embedding
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u32,                            // incremented on each update
}
```

### 2.3 `struct NodeInput` (creation)

```rust
pub struct NodeInput {
    pub node_type: NodeType,
    pub subtype: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub properties: Option<HashMap<String, Value>>,
    pub embedding_id: Option<String>,
}
```

### 2.4 `struct NodeUpdate` (partial update)

Uses `Option<Option<T>>` to distinguish:
- `None` — don't change this field
- `Some(None)` — explicitly clear/set to null
- `Some(Some(v))` — set to value `v`

```rust
pub struct NodeUpdate {
    pub node_type: Option<NodeType>,
    pub subtype: Option<Option<String>>,
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub properties: Option<HashMap<String, Value>>,
    pub embedding_id: Option<Option<String>>,
}
```

### 2.5 `struct NodeFilter` (query)

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

---

## 3. Relationship Types

### 3.1 `enum RelationshipType`

| Variant | Serde alias | Meaning |
|---------|-------------|---------|
| `ActiveContext` | `active_context` | Node → its active context |
| `HasDecision` | `has_decision` | Node → a decision made about it |
| `HasBlocker` | `has_blocker` | Node → a blocker affecting it |
| `HasMilestone` | `has_milestone` | Node → a milestone it relates to |
| `FollowsStandard` | `follows_standard` | Node → a standard it follows |
| `BelongsTo` | `belongs_to` | Child → parent containment |
| `DependsOn` | `depends_on` | Node → a dependency (acyclic enforced) |
| `CalledBy` | `called_by` | Caller → callee (code analysis) |
| `Resolves` | — | Edge resolves something |
| `Supersedes` | — | Edge supersedes another |
| `SemanticallyRelated` | `semantically_related` | Semantic similarity link |
| `ConversationContext` | `conversation_context` | Conversation → context node |
| `LearnedFrom` | `learned_from` | Knowledge → source of learning |
| `SessionWorkedOn` | `session_worked_on` | Session → node worked on |
| `InformedBy` | `informed_by` | Decision → source that informed it |
| `Unknown` | `#[serde(other)]` | Fallback for code-analysis types (Calls, Imports, etc.) |

### 3.2 `struct GraphEdge`

```rust
pub struct GraphEdge {
    pub id: String,                              // UUID v4
    pub edge_type: RelationshipType,             // typed enum
    pub from_id: String,                         // source node UUID
    pub to_id: String,                           // target node UUID
    pub properties: HashMap<String, Value>,      // arbitrary JSON metadata
    pub created_at: DateTime<Utc>,
    pub weight: Option<f64>,                     // importance or certainty score
}
```

### 3.3 `struct RelationshipInput` (creation)

```rust
pub struct RelationshipInput {
    pub edge_type: RelationshipType,
    pub from_id: String,
    pub to_id: String,
    pub properties: Option<HashMap<String, Value>>,
    pub weight: Option<f64>,
}
```

---

## 4. Traversal Types

### 4.1 `struct TraversalOptions`

```rust
pub struct TraversalOptions {
    pub max_depth: u8,                                    // max BFS depth
    pub relationship_types: Option<Vec<RelationshipType>>, // filter by type
    pub max_nodes: Option<usize>,                          // max nodes to return
    pub direction: Option<TraversalDirection>,             // in/out/both
}
```

### 4.2 `enum TraversalDirection`

| Variant | Meaning |
|---------|---------|
| `Out` | Follow outgoing edges only |
| `In` | Follow incoming edges only |
| `Both` | Follow edges in both directions |

### 4.3 `struct TraversalResult`

```rust
pub struct TraversalResult {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub paths: Vec<TraversalPath>,   // individual paths (currently empty)
}
```

---

## 5. Context & Memory Types

### 5.1 `struct ProjectSnapshot`

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

### 5.2 `struct ProjectStats`

```rust
pub struct ProjectStats {
    pub total_nodes: usize,
    pub total_relationships: usize,
    pub last_updated: DateTime<Utc>,
}
```

### 5.3 `struct SearchOptions`

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

### 5.4 `struct ContextSearchResult`

```rust
pub struct ContextSearchResult {
    pub nodes: Vec<ScoredNode>,
    pub relationships: Vec<GraphEdge>,
    pub total_results: usize,
    pub search_time_ms: u64,
    pub truncated: bool,
}
```

### 5.5 `struct ScoredNode`

```rust
pub struct ScoredNode {
    pub node: GraphNode,
    pub similarity: f64,
    pub source: RetrievalSource,
    pub score: f64,
}
```

### 5.6 `enum RetrievalSource`

| Variant | Meaning |
|---------|---------|
| `Semantic` | Vector similarity search |
| `Structural` | Graph traversal / text match |
| `Ambient` | Ambient context |
| `Hybrid` | Combined semantic + structural |

### 5.7 `struct MemoryEntry`

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

### 5.8 `struct MemoryMetadata`

```rust
pub struct MemoryMetadata {
    pub mem_type: Option<NodeType>,
    pub tags: Option<Vec<String>>,
    pub source: Option<String>,
    pub confidence: Option<f64>,
}
```

---

## 6. Query Types

### 6.1 `struct GraphQuery`

```rust
pub struct GraphQuery {
    pub query_type: GraphQueryType,
    pub node_id: Option<String>,
    pub node_label: Option<String>,
    pub max_depth: Option<usize>,
    pub limit: Option<usize>,
}
```

### 6.2 `enum GraphQueryType`

| Variant | Meaning |
|---------|---------|
| `Neighbors` | Find neighbors of a node |
| `Path` | Find paths between two nodes |
| `Search` | Search nodes by label |
| `Subgraph` | Get subgraph around a node |

### 6.3 `struct GraphResult`

```rust
pub struct GraphResult {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub total_count: usize,
}
```

---

## 7. Schema Constraints

The `MemoryGraphActor` enforces the following constraints:

### 7.1 Unique `(type, name)` per node

No two nodes may share the same `(NodeType, name)` pair. Attempting to create or update a node to a duplicate pair returns `SchemaError::DuplicateNode`.

### 7.2 Referential integrity

Relationships must reference existing node UUIDs. Creating a relationship with a non-existent `from_id` or `to_id` returns `SchemaError::NodeNotFound`.

### 7.3 Acyclic `DependsOn`

Adding a `DependsOn` edge triggers a DFS cycle check from the target node following outgoing `DependsOn` edges. If the source node is reachable, the operation is rejected with `SchemaError::AcyclicDependencyViolation`.

### 7.4 Bidirectional ID mapping

The actor maintains four `HashMap`s for the UUID ↔ SeleneDB ID mapping:

| Map | Key | Value |
|-----|-----|-------|
| `uuid_to_node` | UUID `String` | SeleneDB `NodeId` |
| `node_to_uuid` | SeleneDB `NodeId` | UUID `String` |
| `uuid_to_edge` | UUID `String` | SeleneDB `EdgeId` |
| `edge_to_uuid` | SeleneDB `EdgeId` | UUID `String` |

---

## 8. Physical Storage Mapping (SeleneDB)

### 8.1 Node storage

When a `GraphNode` is stored in SeleneDB via `GraphDb::create_node()`:

| SeleneDB concept | Value |
|-----------------|-------|
| **Labels** | `[Debug_repr_of_NodeType, optional_subtype]` e.g. `["Entity", "Function"]` |
| **Properties** | `name` (String), `description` (String), `embedding_id` (String), `created_at` (ISO 8601 String), `updated_at` (ISO 8601 String), `version` (Int), plus all entries from `properties` HashMap converted via `json_value_to_selene()` |

### 8.2 Edge storage

When a `GraphEdge` is stored in SeleneDB via `GraphDb::create_edge()`:

| SeleneDB concept | Value |
|-----------------|-------|
| **Predicate** | `Debug_repr_of_RelationshipType` e.g. `"DependsOn"` |
| **Properties** | Same conversion as nodes |

### 8.3 JSON → SeleneDB value conversion

```rust
serde_json::Value::Null     → None (skipped)
serde_json::Value::Bool(b)  → Value::Bool(b)
serde_json::Value::Number   → Value::Int(i64) or Value::Float(f64)
serde_json::Value::String(s) → Value::String(DbString)
serde_json::Value::Array    → Value::List([converted items])
serde_json::Value::Object   → Value::List([(key, value) pairs as lists])
```

### 8.4 Vector indexes

Vector indexes can be created on any `(label, property)` combination. The `MemoryGraphActor` uses `GraphDb::vector_search()` with `VectorMetric::Cosine` for semantic search, falling back to text-based search if the vector index is unavailable or returns no results.

---

## 9. MCP Server Sync

The `MemoryGraphActor::sync_mcp_servers()` method synchronises the graph with the current MCP configuration at startup.

### Sync algorithm

1. **Index existing state** — query all `McpServer` and `Tool` nodes, plus `uses_tool` edges
2. **Build desired state** — from `McpConfig`:
   - An `"embedded"` server node if any embedded tools are enabled
   - One `McpServer` node per external server config
   - `Tool` nodes for each enabled embedded tool (named after the tool)
   - `Tool` nodes for each external server (named `"{server_name}:*"`)
3. **Diff servers** — create missing, update type property, remove stale (cascade-deleting tools)
4. **Diff tools per server** — create missing, remove stale
5. **Create/remove `uses_tool` edges** — server → tool

### `SyncResult`

```rust
pub struct SyncResult {
    pub servers_added: usize,
    pub servers_removed: usize,
    pub tools_added: usize,
    pub tools_removed: usize,
    pub tools_updated: usize,
}
```

### McpServer node properties

| Property | Type | Description |
|----------|------|-------------|
| `type` | `"embedded"` \| `"external"` | Server type |
| `command` | `String` | (external only) Launch command |
| `args` | `Vec<String>` | (external only) Command arguments |
| `env` | `HashMap<String, String>` | (external only) Environment variables |

### Tool node properties

| Property | Type | Description |
|----------|------|-------------|
| `server` | `String` | Name of the owning MCP server |
| `enabled` | `bool` | Whether the tool is enabled |

---

## 10. MCP Config Storage

The graph stores MCP server configuration in a dedicated section of the graph, enabling persistence across restarts and import/export via JSON files.

### 10.1 Node Types

| Node Type | Label | Purpose |
|-----------|-------|---------|
| `McpConfig` | `mcp_config` | Root config node (singleton) |
| `McpServer` | `mcp_server` | An MCP server definition |
| `McpTool` | `mcp_tool` | A tool provided by an MCP server |

### 10.2 Relationship Types

| Relationship | Label | Purpose |
|-------------|-------|---------|
| `HasMcpServer` | `has_mcp_server` | McpConfig → McpServer |
| `HasMcpTool` | `has_mcp_tool` | McpServer → McpTool |

### 10.3 McpConfig node properties

| Property | Type | Description |
|----------|------|-------------|
| `name` | `String` | Always `"mcp-config"` |
| `version` | `u32` | Config version (incremented on import) |
| `imported_at` | `String` | ISO 8601 timestamp of last import |
| `source_file` | `String` | Path to the source JSON file (if imported) |

### 10.4 McpServer node properties

| Property | Type | Description |
|----------|------|-------------|
| `name` | `String` | Server name (e.g. `"filesystem"`, `"github.com/..."`) |
| `server_type` | `"embedded"` \| `"external"` | Server type |
| `enabled` | `bool` | Whether the server is enabled |
| `command` | `String` | (external only) Launch command |
| `args` | `Vec<String>` | (external only) Command arguments |
| `env` | `HashMap<String, String>` | (external only) Environment variables |
| `description` | `String` | Human-readable description |

### 10.5 McpTool node properties

| Property | Type | Description |
|----------|------|-------------|
| `name` | `String` | Tool name |
| `enabled` | `bool` | Whether the tool is enabled |
| `description` | `String` | Tool description |
| `input_schema` | `Value` (JSON) | JSON Schema for tool parameters |

### 10.6 Bootstrap / Sync

On startup, the `MemoryGraphActor` bootstraps the MCP config from the `mcp-config.json` file:

1. **Check if McpConfig node exists** — if not, create it
2. **Read `mcp-config.json`** — parse the JSON file
3. **Diff servers** — compare graph state with file state:
   - Create missing `McpServer` nodes
   - Update changed server properties
   - Remove stale servers (cascade-delete tools)
4. **Diff tools per server** — create missing, remove stale
5. **Create/remove `has_mcp_server` and `has_mcp_tool` edges**

The bootstrap runs as part of the startup progress sequence (before `percent=100`).

### 10.7 Import via UI

The MCP tab in the webview has an **Import** button (⚙) that:

1. Sends a `mcpImportConfig` message to the extension host
2. Extension host opens a VS Code file dialog (filtered to `*.json`)
3. Reads the selected JSON file
4. Validates it as valid JSON
5. Calls `mcp/config/import` on the Rust subprocess
6. The subprocess stores the config in the graph (same diff algorithm as bootstrap)
7. Sends `event/mcp/config/imported` notification to refresh the UI

### 10.8 JSON Format

The imported JSON file follows the standard MCP configuration format:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "node",
      "args": ["/path/to/server.js"],
      "env": {
        "KEY": "value"
      }
    },
    "github.com/org/repo": {
      "command": "uvx",
      "args": ["mcp-server"],
      "env": {}
    }
  }
}
```

---

## 11. Example Graph

```
Project "my-app"
  │
  ├── has_decision ──→ Decision "use-rust"
  │                       │
  │                       └── informed_by ──→ Entity "performance-benchmarks"
  │
  ├── has_blocker ──→ Blocker "license-issue"
  │
  ├── has_milestone ──→ Milestone "v1.0"
  │
  ├── follows_standard ──→ Standard "rustfmt"
  │
  ├── belongs_to ──→ Entity "auth-module"
  │                     │
  │                     └── depends_on ──→ Entity "crypto-lib"
  │
  └── active_context ──→ ActiveContext "working-on-auth"
                            │
                            └── conversation_context ──→ Conversation "chat-123"
                                                           │
                                                           └── session_worked_on ──→ Session "sess-456"
```
