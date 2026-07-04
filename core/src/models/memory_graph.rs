// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

// ============================================================================
// Schema Errors
// ============================================================================

/// Typed errors for schema constraint violations.
#[allow(dead_code)]
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
    // === AGENT INFRASTRUCTURE ERRORS ===
    #[error("Duplicate error pattern: fingerprint {fingerprint} already exists")]
    DuplicateError {
        fingerprint: String,
    },
    #[error("Duplicate step order {order} in plan {plan_id}")]
    DuplicateStepOrder {
        plan_id: String,
        order: u32,
    },
    #[error("Acyclic precedes violation: adding precedes from {from} to {to} would create a cycle")]
    AcyclicPrecedesViolation {
        from: String,
        to: String,
    },
}


// ============================================================================
// Node Types
// ============================================================================


/// The type of a graph node, mirroring the TypeScript `NodeType` union.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum NodeType {
    #[default]

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
    // === AGENT INFRASTRUCTURE ===
    #[serde(rename = "agent")]
    Agent,
    #[serde(rename = "tool")]
    Tool,
    #[serde(rename = "plan")]
    Plan,
    #[serde(rename = "plan_step")]
    PlanStep,
    #[serde(rename = "execution")]
    Execution,
    #[serde(rename = "task_result")]
    TaskResult,
    #[serde(rename = "artifact")]
    Artifact,
    #[serde(rename = "error_pattern")]
    ErrorPattern,
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeUpdate {

    pub node_type: Option<NodeType>,
    pub subtype: Option<Option<String>>,
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub properties: Option<HashMap<String, serde_json::Value>>,
    pub embedding_id: Option<Option<String>>,
}

/// Filter for querying nodes, mirroring the TypeScript `NodeFilter` interface.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeFilter {

    pub node_type: Option<NodeType>,
    pub subtype: Option<String>,
    pub name: Option<String>,
    pub status: Option<String>,
    pub tags: Option<Vec<String>>,
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
    // === AGENT INFRASTRUCTURE ===
    #[serde(rename = "uses_tool")]
    UsesTool,
    #[serde(rename = "follows_plan")]
    FollowsPlan,
    #[serde(rename = "contains_step")]
    ContainsStep,
    #[serde(rename = "precedes")]
    Precedes,
    #[serde(rename = "produced")]
    Produced,
    #[serde(rename = "encountered_error")]
    EncounteredError,
    #[serde(rename = "resolved_by")]
    ResolvedBy,
    #[serde(rename = "part_of_execution")]
    PartOfExecution,
    #[serde(rename = "executed_by")]
    ExecutedBy,
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
// Agent Infrastructure Types
// ============================================================================

/// Agent context returned by hybrid retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    pub agent: GraphNode,
    pub tools: Vec<GraphNode>,
    pub plan: Option<GraphNode>,
    pub steps: Vec<GraphNode>,
    pub recent_successes: Vec<GraphNode>,
    pub similar_successes: Vec<GraphNode>,
    pub similar_errors: Vec<GraphNode>,
    pub artifacts: Vec<GraphNode>,
    pub current_goal: String,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Agent execution status.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
}

/// Input for executing a tool.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecution {
    pub tool_name: String,
    pub arguments: HashMap<String, serde_json::Value>,
    pub timeout_ms: Option<u64>,
    pub retry_count: Option<u8>,
}

/// Error fingerprint for deduplication.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorFingerprint {
    pub error_type: String,
    pub message_hash: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl ErrorFingerprint {
    #[allow(dead_code)]
    pub fn generate(&self) -> String {
        use sha2::{Digest, Sha256};
        let input = format!(
            "{}:{}:{:?}:{:?}:{:?}",
            self.error_type, self.message_hash, self.file, self.line, self.column
        );
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        format!("sha256:{}", hex::encode(hasher.finalize()))
    }
}

/// Plan step ordering helper.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStepOrder {
    pub order: u32,
    pub parallel_group: Option<String>,
}

/// Plan step dependency helper.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStepDependency {
    pub depends_on: Vec<String>,
    pub condition: Option<String>,
}

/// Suggested fix for an error pattern.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedFix {
    pub before: String,
    pub after: String,
    pub line: u32,
    pub file: String,
    pub description: Option<String>,
}

