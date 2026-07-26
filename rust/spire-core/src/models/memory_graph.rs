use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tokio::sync::oneshot;

// ============================================================================
// Transaction Stream Types
// ============================================================================

/// A single operation within an atomic transaction stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamOp {
    /// Store a new node.
    StoreNode(NodeInput),
    /// Store a new node with an optional pre-computed embedding vector.
    /// The vector is stored as the `embedding` property on the node for vector search.
    StoreNodeWithEmbedding {
        node: NodeInput,
        embedding_vector: Vec<f32>,
    },
    /// Update an existing node.
    UpdateNode {
        id: String,
        updates: NodeUpdate,
    },
    /// Delete a node by UUID.
    DeleteNode(String),
    /// Create a relationship between two nodes.
    CreateRelationship(RelationshipInput),
    /// Delete a relationship by UUID.
    DeleteRelationship(String),
    /// Set a config key-value pair.
    SetConfig {
        key: String,
        value: serde_json::Value,
    },
    /// Execute a raw GQL statement.
    RawGql(String),
    /// Merge (upsert) a node by (node_type, name) uniqueness constraint.
    /// Creates the node if it doesn't exist, or updates its properties if it does.
    MergeNode(NodeInput),
    /// Merge (upsert) a relationship by (edge_type, from_id, to_id) uniqueness constraint.
    /// Creates the relationship if it doesn't exist, or updates its properties if it does.
    MergeRelationship(RelationshipInput),
    /// Commit the transaction and close the stream.
    Commit,
    /// Roll back the transaction and close the stream.
    Rollback,
}

/// The result of a single stream operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamOpResult {
    NodeStored(GraphNode),
    NodeUpdated(GraphNode),
    NodeDeleted,
    RelationshipCreated(GraphEdge),
    RelationshipDeleted,
    ConfigSet,
    RawGql(Option<serde_json::Value>),
}

/// A request sent through a transaction stream.
#[derive(Debug)]
pub struct TransactionRequest {
    /// The operation to execute.
    pub operation: StreamOp,
    /// Channel to send the result back.
    pub reply_to: oneshot::Sender<Result<StreamOpResult, String>>,
}


// ============================================================================
// Schema Errors
// ============================================================================

/// Typed errors for schema constraint violations.
#[derive(Debug, Clone, Serialize, Deserialize, Error)]
pub enum SchemaError {
    #[error("Duplicate node: ({type_name}, {name}) already exists")]
    DuplicateNode {
        type_name: String,
        name: String,
    },
    #[error("Node not found: {id}")]
    NodeNotFound {
        id: String,
    },
    #[error("Acyclic dependency violation: adding depends_on from {from} to {to} would create a cycle")]
    AcyclicDependencyViolation {
        from: String,
        to: String,
    },
}

// ============================================================================
// MCP Server Config (stored in graph as SpireMcpConfig nodes)
// ============================================================================

/// A single MCP server configuration entry, matching the JSON file format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfigEntry {
    /// Unique server name (e.g. "filesystem").
    pub name: String,
    /// The command to execute (stdio transport).
    pub command: Option<String>,
    /// Arguments for the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables (key → value).
    pub env: Option<HashMap<String, String>>,
    /// HTTP URL for HTTP transport.
    pub url: Option<String>,
    /// HTTP headers for HTTP transport.
    pub headers: Option<HashMap<String, String>>,
    /// Whether to auto-connect on startup.
    #[serde(default = "default_autostart")]
    pub autostart: bool,
}

fn default_autostart() -> bool {
    true
}

/// The JSON file format for MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfigFile {
    pub servers: Vec<McpServerConfigEntry>,
}


// ============================================================================
// Node Types
// ============================================================================

/// The type of a graph node, mirroring the TypeScript `NodeType` union.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    /// Build system type (e.g. Cargo, Node, CMake)
    #[serde(rename = "buildSystem")]
    BuildSystem,
    /// User intent type
    Intent,
    /// Error type for build/analysis errors
    #[serde(rename = "errorType")]
    ErrorType,
    /// Fix strategy type
    #[serde(rename = "fixStrategy")]
    FixStrategy,
    /// Tool type (MCP or internal)
    Tool,
    /// Build state type
    #[serde(rename = "buildState")]
    BuildState,
    /// Build/analysis diagnostic (warning or error) linked to a file node
    #[serde(rename = "diagnostic")]
    Diagnostic,
    /// A step definition that maps an abstract step name to a concrete tool call
    #[serde(rename = "stepDefinition")]
    StepDefinition,
    /// A concrete tool with its provider and input schema
    #[serde(rename = "concreteTool")]
    ConcreteTool,
    /// A tool provider (e.g. vscode-extension, mcp-cargo, llm)
    #[serde(rename = "toolProvider")]
    ToolProvider,
    /// A multi-step execution plan
    #[serde(rename = "plan")]
    Plan,
    /// A single step within an execution plan
    #[serde(rename = "planStep")]
    PlanStep,
    /// Fallback for code-analysis node types (File, Function, Class, etc.)
    #[serde(other)]
    Unknown,
}

/// A node in the knowledge graph, mirroring the TypeScript `Node` interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Input for creating a new node, mirroring the TypeScript `NodeInput` interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInput {
    pub node_type: NodeType,
    pub subtype: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub properties: Option<HashMap<String, serde_json::Value>>,
    pub embedding_id: Option<String>,
}

/// Partial update for a node, mirroring `Partial<Node>` in TypeScript.
///
/// Each field uses `Option<Option<T>>` to distinguish between:
/// - `None` — don't change this field
/// - `Some(None)` — explicitly clear/set to null
/// - `Some(Some(v))` — set to value `v`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeUpdate {
    pub node_type: Option<NodeType>,
    pub subtype: Option<Option<String>>,
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub properties: Option<HashMap<String, serde_json::Value>>,
    pub embedding_id: Option<Option<String>>,
}

/// Filter for querying nodes, mirroring the TypeScript `NodeFilter` interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeFilter {
    pub node_type: Option<NodeType>,
    pub subtype: Option<String>,
    pub name: Option<String>,
    pub status: Option<String>,
    pub tags: Option<Vec<String>>,
    /// Optional property key-value filter. When set, only nodes whose
    /// properties contain the specified key with the matching value are returned.
    pub properties: Option<HashMap<String, serde_json::Value>>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

// ============================================================================
// Relationship Types
// ============================================================================

/// The type of relationship between nodes, mirroring the TypeScript `RelationshipType` union.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    /// Relationship from a file node to a build diagnostic (warning or error)
    #[serde(rename = "has_diagnostic")]
    HasDiagnostic,
    /// Custom relationship type for dynamic/arbitrary edge labels.
    Custom(String),
    /// Fallback for code-analysis relationship types (Calls, Imports, etc.)
    #[serde(other)]
    Unknown,
}

/// A directed edge between two graph nodes, mirroring the TypeScript `Relationship` interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub edge_type: RelationshipType,
    pub from_id: String,
    pub to_id: String,
    pub properties: HashMap<String, serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub weight: Option<f64>,
}

/// Input for creating a new relationship, mirroring the TypeScript `RelationshipInput` interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipInput {
    pub edge_type: RelationshipType,
    pub from_id: String,
    pub to_id: String,
    pub properties: Option<HashMap<String, serde_json::Value>>,
    pub weight: Option<f64>,
}

/// A single diagnostic (warning or error) from a build tool output.
/// Stored as a graph node of type `Diagnostic` linked to a file node
/// via a `HasDiagnostic` relationship.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticInfo {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    /// "warning" or "error"
    pub severity: String,
    /// Build system that produced this diagnostic (e.g. "Cargo", "npm")
    pub build_type: String,
    /// Build run identifier (UUID string) for grouping diagnostics from the same build
    pub build_run_id: String,
}

// ============================================================================
// Traversal Types
// ============================================================================

/// Options for graph traversal, mirroring the TypeScript `TraversalOptions` interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalOptions {
    pub max_depth: u8,
    pub relationship_types: Option<Vec<RelationshipType>>,
    pub max_nodes: Option<usize>,
    pub direction: Option<TraversalDirection>,
}

/// Direction of traversal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TraversalDirection {
    #[serde(rename = "out")]
    Out,
    #[serde(rename = "in")]
    In,
    #[serde(rename = "both")]
    Both,
}

/// Result of a graph traversal, mirroring the TypeScript `TraversalResult` interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalResult {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub paths: Vec<TraversalPath>,
}

/// A single path through the graph during traversal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalPath {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

// ============================================================================
// Context & Memory Types
// ============================================================================

/// A snapshot of the project context, mirroring the TypeScript `ProjectSnapshot` interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Statistics about the project graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStats {
    pub total_nodes: usize,
    pub total_relationships: usize,
    pub last_updated: DateTime<Utc>,
}

/// Options for context search, mirroring the TypeScript `SearchOptions` interface.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchOptions {
    pub top_k: Option<usize>,
    pub threshold: Option<f64>,
    pub node_types: Option<Vec<NodeType>>,
    pub max_depth: Option<u8>,
    pub include_structural: Option<bool>,
    pub recency_weight: Option<f64>,
}

/// Result of a context search, mirroring the TypeScript `ContextSearchResult` interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSearchResult {
    pub nodes: Vec<ScoredNode>,
    pub relationships: Vec<GraphEdge>,
    pub total_results: usize,
    pub search_time_ms: u64,
    pub truncated: bool,
}

/// A node with a relevance score from a search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredNode {
    pub node: GraphNode,
    pub similarity: f64,
    pub source: RetrievalSource,
    pub score: f64,
}

/// The source of a retrieval result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetrievalSource {
    Semantic,
    Structural,
    Ambient,
    Hybrid,
}

/// Metadata for a memory entry, mirroring the TypeScript `MemoryMetadata` interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetadata {
    pub mem_type: Option<NodeType>,
    pub tags: Option<Vec<String>>,
    pub source: Option<String>,
    pub confidence: Option<f64>,
}

/// A memory entry, mirroring the TypeScript `MemoryEntry` interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub text: String,
    pub embedding_id: String,
    pub metadata: MemoryMetadata,
    pub node_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ============================================================================
// Build-Fix Loop Intent Detection Types
// ============================================================================

/// Graph label constants for the build-fix loop schema.
pub const LABEL_INTENT: &str = "SpireIntent";
pub const LABEL_ERROR_TYPE: &str = "SpireError";
pub const LABEL_FIX_STRATEGY: &str = "SpireFixStrategy";
pub const LABEL_TOOL: &str = "SpireTool";
pub const LABEL_STATE: &str = "SpireState";
pub const LABEL_PATTERN: &str = "SpirePattern";
pub const LABEL_CAPABILITY: &str = "SpireCapability";

/// Relationship type constants for the build-fix loop schema.
pub const REL_MAPS_TO: &str = "MAPS_TO";
pub const REL_USES: &str = "USES";
pub const REL_REQUIRES: &str = "REQUIRES";
pub const REL_FIXED_BY: &str = "FIXED_BY";
pub const REL_HAS_PATTERN: &str = "HAS_PATTERN";
pub const REL_IS_SUBTYPE_OF: &str = "IS_SUBTYPE_OF";
pub const REL_TRIGGERS: &str = "TRIGGERS";
pub const REL_USES_TOOL: &str = "USES_TOOL";
pub const REL_VALIDATES_WITH: &str = "VALIDATES_WITH";
pub const REL_DEPENDS_ON: &str = "DEPENDS_ON";
pub const REL_PRECEDES: &str = "PRECEDES";
pub const REL_RESOLVES: &str = "RESOLVES";
pub const REL_TRANSITIONS_TO: &str = "TRANSITIONS_TO";
pub const REL_CAN_ROLLBACK_TO: &str = "CAN_ROLLBACK_TO";
pub const REL_PROVIDES: &str = "PROVIDES";
pub const REL_REQUIRES_TOOL: &str = "REQUIRES_TOOL";

/// A detected intent from a user query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub id: String,
    pub name: String,
    pub description: String,
    pub priority: u32,
    pub requires_approval: bool,
    pub state_requirements: Vec<String>,
}

/// An error type in the build-fix hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorType {
    pub id: String,
    pub name: String,
    pub description: String,
    pub severity: String,
    pub parent_id: Option<String>,
    pub detection_patterns: Vec<String>,
    pub common_causes: Vec<String>,
}

/// A fix strategy for resolving build errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixStrategy {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub confidence_threshold: f64,
    pub success_rate: f64,
    pub execution_steps: Vec<String>,
    pub has_rollback: bool,
}

/// A tool that can be used in the build-fix loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: ToolCategory,
    pub capabilities: Vec<String>,
    pub approval_required: bool,
}

/// Category of a tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ToolCategory {
    Analysis,
    Fix,
    Monitoring,
    Verification,
    Recovery,
}

/// A build state in the state machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildState {
    pub id: String,
    pub name: String,
    pub description: String,
    pub conditions: Vec<String>,
    pub rollback_state: Option<String>,
}

/// Context for a build operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildContext {
    pub project_root: String,
    pub build_system: String,
    pub target: Option<String>,
    pub environment: HashMap<String, String>,
}

// ── NEW: Per-system build result ──────────────────────────────────────────

/// Result from a single build system within a multi-system build.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemBuildResult {
    /// Build system type (e.g. "Cargo", "npm")
    pub build_type: String,
    /// Project root path for this build system
    pub path: String,
    /// Logical project name
    pub project_name: String,
    /// Whether the build succeeded
    pub success: bool,
    /// Errors produced by this build system
    pub errors: Vec<BuildError>,
    /// Warnings produced by this build system
    pub warnings: Vec<BuildError>,
    /// Optional exit code from the build process
    pub exit_code: Option<i32>,
    /// Duration of the build in milliseconds
    pub duration_ms: u64,
}

// ── MODIFIED: BuildResult — now carries per-system results ────────────────

/// Result of a multi-system build operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    /// Overall success (true only if ALL systems succeeded)
    pub success: bool,
    /// Per-system results
    pub system_results: Vec<SystemBuildResult>,
    /// Build run identifier (UUID) for grouping diagnostics
    pub build_run_id: String,
    /// Total duration across all systems in seconds
    pub duration_secs: f64,
}

// ── MODIFIED: BuildError — now carries build_type + diagnostic_node_id ────

/// A build error with context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildError {
    pub error_text: String,
    pub error_type: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub exit_code: Option<i32>,
    /// Which build system produced this error (e.g. "Cargo", "npm")
    pub build_type: Option<String>,
    /// Graph node ID for the diagnostic node, if stored
    pub diagnostic_node_id: Option<String>,
    /// Graph node ID for the associated file node, if known
    pub file_node_id: Option<String>,
}

/// A scored fix strategy with confidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredFix {
    pub strategy: FixStrategy,
    pub confidence: f64,
    pub required_tools: Vec<String>,
    pub validation_tools: Vec<String>,
}

// ── NEW: AnnotatedError — wraps an error with its matched fix options ──────

/// A build error annotated with its matched fix options and graph metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedError {
    /// The original build error
    pub error: BuildError,
    /// Which build system produced this error
    pub build_type: String,
    /// The build system's path
    pub build_path: String,
    /// File node ID in the graph (if linked via HasDiagnostic)
    pub file_node_id: Option<String>,
    /// Diagnostic node ID in the graph
    pub diagnostic_node_id: Option<String>,
    /// Fix options matched to this error, sorted by confidence descending
    pub fix_options: Vec<ScoredFix>,
}

// ── MODIFIED: FixPlan — now holds annotated errors across all systems ─────

/// A complete fix plan with ordered strategies across all failed build systems.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixPlan {
    /// All errors from all failed build systems, each annotated with fix options
    pub errors: Vec<AnnotatedError>,
    /// Merged, sorted, deduplicated fix list across ALL errors
    pub ordered_fixes: Vec<ScoredFix>,
    /// Maximum fix iterations allowed before manual intervention is required
    pub max_iterations: u32,
}

// ============================================================================
// Build/Fix Lifecycle Result Types
// ============================================================================

/// Result returned from a build lifecycle (success or failure with fix plan).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildStartResult {
    /// The build session ID
    pub build_id: String,
    /// Whether the build succeeded
    pub success: bool,
    /// The full build result (with per-system details)
    pub build_result: BuildResult,
    /// If build failed, the fix plan (None if build succeeded)
    pub fix_plan: Option<FixPlan>,
    /// Number of fix iterations applied so far in this session
    pub iteration_count: u32,
    /// Maximum iterations allowed
    pub max_iterations: u32,
}

// ============================================================================
// Plan Mode Types
// ============================================================================

/// Status of a plan or step in the execution lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlanStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "approved")]
    Approved,
    #[serde(rename = "executing")]
    Executing,
    #[serde(rename = "paused")]
    Paused,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "skipped")]
    Skipped,
}

/// A step within a plan, as stored in PlanStep graph nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStepData {
    /// Human-readable description of the step
    pub description: String,
    /// References a StepDefinition name in the graph
    pub step_name: String,
    /// Arguments template for this step's execution
    pub arg_template: serde_json::Value,
    /// Step order indices that must complete before this one
    pub depends_on: Vec<u32>,
    /// Whether this step uses the build error context
    pub uses_error_context: bool,
}

/// Result of a plan status query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStatusResult {
    /// The plan's unique ID (also the graph node ID)
    pub plan_id: String,
    /// The user's original goal
    pub goal: String,
    /// Current plan status
    pub status: PlanStatus,
    /// Matched intent name, if any
    pub intent_name: Option<String>,
    /// Steps in execution order
    pub steps: Vec<PlanStepEntry>,
    /// Total step count
    pub total_steps: u32,
    /// Completed step count
    pub completed_steps: u32,
    /// Failed step count
    pub failed_steps: u32,
}

/// A single step entry in a plan status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStepEntry {
    pub id: String,
    pub order: u32,
    pub description: String,
    pub step_name: String,
    pub status: PlanStatus,
    pub result: Option<String>,
    pub error: Option<String>,
}

// ============================================================================
// Actor Message Types for Build-Fix Loop
// ============================================================================

