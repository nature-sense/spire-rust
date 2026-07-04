# Agent Infrastructure Extension for Spire Knowledge Graph

**Document Purpose**

This document defines the agent infrastructure extensions for the Spire knowledge graph. It provides the complete schema updates, node/relationship definitions, and implementation guidance needed to extend the existing graph system to support agentic workflows.

**Target Audience:** AI assistant implementing the MemoryGraphActor extensions in Rust.

**Current Schema Version:** 2026-07-01

---

## Part 1: Overview

### 1.1 What This Extension Adds

The current Spire graph schema supports:

- Project structure and entities
- Decisions, blockers, and milestones
- Conversations and sessions
- Semantic relationships and vector embeddings

This extension adds agent infrastructure:

- **Agent definitions** (fixed-purpose agents with system prompts)
- **Tool definitions** (schemas for agent-accessible functions)
- **Plan definitions** (deterministic DAG workflows)
- **Execution tracking** (full audit trail of agent runs)
- **Artifact storage** (build outputs, test reports, deployments)
- **Error pattern learning** (deduplicated error tracking with fixes)

### 1.2 Key Design Principles

- **Agents are data, not code** — Agent configurations live in the graph, not hardcoded in Rust
- **Plans are explicit DAGs** — Every step, dependency, and condition is stored as graph relationships
- **Everything is auditable** — Every execution, tool call, and error is recorded
- **Learning is built-in** — Vector embeddings enable semantic retrieval of past executions
- **Hybrid retrieval** — Combine graph traversal (structure) with vector search (semantics)

### 1.3 Relationship to Existing Schema

These extensions are **additive**. They do not modify existing node types or relationships. The new types coexist with:

- `Project`, `Entity`, `Decision`, `Milestone`, `Standard`
- `ActiveContext`, `Blocker`, `Conversation`, `Session`
- Existing relationships (`BelongsTo`, `DependsOn`, `SemanticallyRelated`, etc.)
- Existing constraints (unique `(type, name)`, acyclic `DependsOn`, referential integrity)

---

## Part 2: New Node Types

### 2.1 NodeType Enum Extension

Add these variants to the existing `NodeType` enum:

```rust
pub enum NodeType {
    // ... existing variants ...

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
}
```

### 2.2 Agent Node

**Purpose:** Defines a fixed-purpose agent with its system prompt, model, and operational parameters.

```rust
pub struct AgentNode {
    pub id: String,                              // UUID v4
    pub node_type: NodeType,                     // = Agent
    pub subtype: Option<String>,                 // "build" | "refund" | "travel" | "router"
    pub name: String,                            // "CompilerAgent"
    pub description: Option<String>,             // System prompt for the agent
    pub properties: HashMap<String, Value>,      // See Properties below
    pub embedding_id: Option<String>,            // For retrieving similar agent contexts
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u32,
}
```

**Properties Schema:**

```json
{
    "model": "gpt-4o",
    "max_steps": 10,
    "max_retries": 3,
    "temperature": 0.7,
    "version": "1.0.0",
    "is_active": true,
    "tags": ["rust", "build"],
    "cost_estimate": 0.05
}
```

### 2.3 Tool Node

**Purpose:** Defines a tool that agents can call. Stores the tool's JSON schema and Rust function mapping.

```rust
pub struct ToolNode {
    pub id: String,
    pub node_type: NodeType,                     // = Tool
    pub subtype: Option<String>,                 // "filesystem" | "process" | "network" | "database"
    pub name: String,                            // "read_file"
    pub description: Option<String>,             // Human-readable description for LLM
    pub properties: HashMap<String, Value>,      // See Properties below
    pub embedding_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u32,
}
```

**Properties Schema:**

```json
{
    "schema": {
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "File path to read"}
        },
        "required": ["path"]
    },
    "rust_function": "read_file",
    "is_async": true,
    "timeout_ms": 5000,
    "retry_count": 2,
    "cost_estimate": 0.001,
    "description": "Reads a file from the filesystem"
}
```

### 2.4 Plan Node

**Purpose:** Defines a deterministic workflow (DAG) that an agent follows.

```rust
pub struct PlanNode {
    pub id: String,
    pub node_type: NodeType,                     // = Plan
    pub subtype: Option<String>,                 // "build_pipeline" | "refund_workflow" | "test_suite"
    pub name: String,                            // "RustBuildPipeline"
    pub description: Option<String>,
    pub properties: HashMap<String, Value>,      // See Properties below
    pub embedding_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u32,
}
```

**Properties Schema:**

```json
{
    "version": "2.0.0",
    "is_active": true,
    "max_parallel_steps": 3,
    "timeout_seconds": 300,
    "tags": ["rust", "build", "ci"]
}
```

### 2.5 PlanStep Node

**Purpose:** A single step within a plan. Steps are nodes in a DAG and can be agents, tools, parallel groups, or decision points.

```rust
pub struct PlanStepNode {
    pub id: String,
    pub node_type: NodeType,                     // = PlanStep
    pub subtype: Option<String>,                 // "agent" | "tool" | "parallel" | "decision"
    pub name: String,                            // "compile"
    pub description: Option<String>,
    pub properties: HashMap<String, Value>,      // See Properties below
    pub embedding_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u32,
}
```

**Properties Schema:**

```json
{
    "step_order": 3,
    "parallel_group": null,
    "agent_id": "compiler_agent",
    "tool_id": "run_command",
    "condition": "compile_fails -> fix",
    "retry_count": 2,
    "timeout_ms": 30000,
    "max_parallel": null,
    "depends_on": ["step_2"],
    "is_required": true
}
```

### 2.6 Execution Node

**Purpose:** Represents a single run of an agent or plan. Tracks status, timing, and high-level results.

```rust
pub struct ExecutionNode {
    pub id: String,
    pub node_type: NodeType,                     // = Execution
    pub subtype: Option<String>,                 // "build" | "refund" | "test"
    pub name: String,                            // "Build CLI tool - attempt 3"
    pub description: Option<String>,
    pub properties: HashMap<String, Value>,      // See Properties below
    pub embedding_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u32,
}
```

**Properties Schema:**

```json
{
    "status": "success",
    "start_time": "2026-07-01T10:00:00Z",
    "end_time": "2026-07-01T10:05:00Z",
    "goal": "Build my CLI tool",
    "step_count": 7,
    "failed_step": null,
    "token_usage": 25000,
    "cost": 0.05,
    "metadata": {
        "project": "my_cli_tool",
        "user": "alice",
        "session_id": "sess_123"
    },
    "summary": "Successfully built CLI tool with optimizations",
    "status_message": null
}
```

### 2.7 TaskResult Node

**Purpose:** Records the outcome of a single step within an execution. This is the fine-grained audit trail.

```rust
pub struct TaskResultNode {
    pub id: String,
    pub node_type: NodeType,                     // = TaskResult
    pub subtype: Option<String>,                 // "agent_decision" | "tool_call" | "error"
    pub name: String,                            // "compile_step"
    pub description: Option<String>,
    pub properties: HashMap<String, Value>,
    pub embedding_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u32,
}
```

**Properties Schema:**

```json
{
    "status": "success",
    "step_name": "compile",
    "input": {
        "command": "cargo build",
        "args": ["--release"]
    },
    "output": {
        "stdout": "Finished release [optimized] target",
        "stderr": "",
        "exit_code": 0
    },
    "error": null,
    "duration_ms": 4500,
    "token_usage": 5000,
    "timestamp": "2026-07-01T10:02:00Z",
    "attempt": 1,
    "is_retry": false
}
```

### 2.8 Artifact Node

**Purpose:** Records build outputs, test reports, deployments, or any other file-like output.

```rust
pub struct ArtifactNode {
    pub id: String,
    pub node_type: NodeType,                     // = Artifact
    pub subtype: Option<String>,                 // "source_code" | "binary" | "test_report" | "deployment" | "log"
    pub name: String,
    pub description: Option<String>,
    pub properties: HashMap<String, Value>,
    pub embedding_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u32,
}
```

**Properties Schema:**

```json
{
    "path": "/build/target/release/my_cli_tool",
    "checksum": "sha256:abc123...",
    "size_bytes": 5120000,
    "mime_type": "application/x-executable",
    "metadata": {
        "language": "rust",
        "target": "x86_64-unknown-linux-gnu",
        "optimization_level": "release",
        "build_id": "build_456"
    },
    "storage": "filesystem",
    "storage_key": "builds/abc123/my_cli_tool"
}
```

### 2.9 ErrorPattern Node

**Purpose:** Deduplicated error tracking with fix strategies. Uses a fingerprint hash to identify recurring errors.

```rust
pub struct ErrorPatternNode {
    pub id: String,
    pub node_type: NodeType,                     // = ErrorPattern
    pub subtype: Option<String>,                 // "compiler_error" | "test_failure" | "deployment_error" | "runtime_error"
    pub name: String,                            // "E0382: use of moved value"
    pub description: Option<String>,
    pub properties: HashMap<String, Value>,
    pub embedding_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u32,
}
```

**Properties Schema:**

```json
{
    "fingerprint": "sha256:def456...",
    "error_type": "compiler_error",
    "message": "use of moved value `x`",
    "detailed_message": "error[E0382]: ...",
    "occurrence_count": 12,
    "first_seen": "2026-06-15T10:00:00Z",
    "last_seen": "2026-07-01T09:00:00Z",
    "fix_strategy": "Add .clone() at line 42",
    "fix_description": "The value `x` is moved in the previous line. Clone it before moving.",
    "fix_success_rate": 0.95,
    "suggested_fix_code": {
        "before": "let y = x;",
        "after": "let y = x.clone();",
        "line": 42,
        "file": "src/main.rs"
    },
    "tags": ["ownership", "borrowing"]
}
```

---

## Part 3: New Relationship Types

### 3.1 RelationshipType Enum Extension

Add these variants to the existing `RelationshipType` enum:

```rust
pub enum RelationshipType {
    // ... existing variants ...

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
    #[serde(rename = "learned_from")]
    LearnedFrom,
}
```

### 3.2 Relationship Semantics

#### UsesTool

- **Direction:** Agent → Tool
- **from_id:** Agent ID
- **to_id:** Tool ID
- **Properties:**
  - `priority` (integer): 0 = required, 1 = preferred, 2 = optional
  - `last_used` (string, ISO 8601)
  - `usage_count` (integer)
  - `success_rate` (float, 0.0–1.0)

```
(a:Agent)-[:USES_TOOL {priority: 0, usage_count: 25, success_rate: 0.92}]->(t:Tool)
```

#### FollowsPlan

- **Direction:** Agent → Plan
- **from_id:** Agent ID
- **to_id:** Plan ID
- **Properties:**
  - `activated_at` (string, ISO 8601)
  - `is_active` (boolean)
  - `priority` (integer): For multiple plans

```
(a:Agent)-[:FOLLOWS_PLAN {activated_at: "2026-07-01T09:00:00Z", is_active: true}]->(p:Plan)
```

#### ContainsStep

- **Direction:** Plan → PlanStep
- **from_id:** Plan ID
- **to_id:** PlanStep ID
- **Properties:**
  - `order` (integer): Step number for linear plans
  - `parallel_group` (string, optional): Group ID for parallel steps
  - `is_entry` (boolean): Is this an entry point?
  - `is_exit` (boolean): Is this an exit point?

```
(p:Plan)-[:CONTAINS_STEP {order: 1, is_entry: true}]->(ps:PlanStep)
```

#### Precedes

- **Direction:** PlanStep → PlanStep
- **from_id:** Source step ID
- **to_id:** Target step ID
- **Properties:**
  - `condition` (string, optional): Condition for traversing this edge
  - `is_default` (boolean): Default path if no condition

```
(ps1:PlanStep)-[:PRECEDES {condition: null}]->(ps2:PlanStep)
(ps2:PlanStep)-[:PRECEDES {condition: "IF compile_fails THEN fix"}]->(ps3:PlanStep)
```

#### Produced

- **Direction:** Execution → Artifact
- **from_id:** Execution ID
- **to_id:** Artifact ID
- **Properties:**
  - `artifact_type` (string): "binary", "test_report", etc.
  - `timestamp` (string, ISO 8601)
  - `size_bytes` (integer, optional)
  - `is_primary` (boolean): Is this the main output?

```
(e:Execution)-[:PRODUCED {artifact_type: "binary", is_primary: true}]->(art:Artifact)
```

#### EncounteredError

- **Direction:** Execution → ErrorPattern
- **from_id:** Execution ID
- **to_id:** ErrorPattern ID
- **Properties:**
  - `step_name` (string): Which step encountered the error
  - `timestamp` (string, ISO 8601)
  - `attempt_number` (integer): Which attempt this was
  - `was_resolved` (boolean)
  - `resolution_time_ms` (integer, optional)

```
(e:Execution)-[:ENCOUNTERED_ERROR {step_name: "compile", attempt_number: 1, was_resolved: true}]->(err:ErrorPattern)
```

#### ResolvedBy

- **Direction:** ErrorPattern → Execution
- **from_id:** ErrorPattern ID
- **to_id:** Execution ID
- **Properties:**
  - `resolution_time_ms` (integer)
  - `success` (boolean)
  - `applied_at` (string, ISO 8601)
  - `fix_attempt_number` (integer)

```
(err:ErrorPattern)-[:RESOLVED_BY {resolution_time_ms: 5000, success: true, fix_attempt_number: 2}]->(e:Execution)
```

#### PartOfExecution

- **Direction:** TaskResult → Execution
- **from_id:** TaskResult ID
- **to_id:** Execution ID
- **Properties:**
  - `step_order` (integer): Position in execution
  - `timestamp` (string, ISO 8601)

```
(tr:TaskResult)-[:PART_OF_EXECUTION {step_order: 3}]->(e:Execution)
```

#### ExecutedBy

- **Direction:** Execution → Agent
- **from_id:** Execution ID
- **to_id:** Agent ID
- **Properties:**
  - `timestamp` (string, ISO 8601)
  - `runtime_ms` (integer)

```
(e:Execution)-[:EXECUTED_BY {timestamp: "2026-07-01T10:00:00Z", runtime_ms: 300000}]->(a:Agent)
```

#### LearnedFrom

- **Direction:** Agent → Execution
- **from_id:** Agent ID
- **to_id:** Execution ID
- **Properties:**
  - `learned_at` (string, ISO 8601)
  - `weight` (float): Importance of this learning (0.0–1.0)
  - `success` (boolean): Was the learning outcome positive?

```
(a:Agent)-[:LEARNED_FROM {learned_at: "2026-07-01T10:05:00Z", weight: 0.85, success: true}]->(e:Execution)
```

---

## Part 4: Graph Schema Example

### 4.1 Complete Build Agent Example

```
// === AGENT DEFINITION ===
(a:Agent {
    id: "agent_compiler_001",
    node_type: "Agent",
    subtype: "build",
    name: "CompilerAgent",
    description: "You are a Rust compiler agent...",
    properties: {
        "model": "gpt-4o",
        "max_steps": 10,
        "max_retries": 3,
        "temperature": 0.7,
        "version": "1.0.0",
        "is_active": true,
        "tags": ["rust", "build"],
        "cost_estimate": 0.05
    },
    embedding_id: "embed_agent_001"
})

// === TOOLS ===
(t1:Tool {name: "read_file", ...})
(t2:Tool {name: "write_file", ...})
(t3:Tool {name: "run_command", ...})

// === AGENT USES TOOLS ===
(a)-[:USES_TOOL {priority: 0, usage_count: 0, success_rate: 0.0}]->(t1)
(a)-[:USES_TOOL {priority: 0}]->(t2)
(a)-[:USES_TOOL {priority: 0}]->(t3)

// === PLAN ===
(p:Plan {
    name: "RustBuildPipeline",
    subtype: "build_pipeline",
    properties: {
        "version": "1.0.0",
        "is_active": true,
        "max_parallel_steps": 2,
        "timeout_seconds": 300
    }
})

// === PLAN STEPS ===
(ps1:PlanStep {name: "scaffold", subtype: "tool", step_order: 1, is_entry: true})
(ps2:PlanStep {name: "compile", subtype: "tool", step_order: 2})
(ps3:PlanStep {name: "fix_errors", subtype: "agent", step_order: 3})
(ps4:PlanStep {name: "test", subtype: "tool", step_order: 4})
(ps5:PlanStep {name: "deploy", subtype: "tool", step_order: 5, is_exit: true})

// === PLAN CONTAINS STEPS ===
(p)-[:CONTAINS_STEP {order: 1, is_entry: true}]->(ps1)
(p)-[:CONTAINS_STEP {order: 2}]->(ps2)
(p)-[:CONTAINS_STEP {order: 3}]->(ps3)
(p)-[:CONTAINS_STEP {order: 4}]->(ps4)
(p)-[:CONTAINS_STEP {order: 5, is_exit: true}]->(ps5)

// === STEP DEPENDENCIES (DAG) ===
(ps1)-[:PRECEDES {condition: null}]->(ps2)
(ps2)-[:PRECEDES {condition: "IF compile_fails THEN fix_errors"}]->(ps3)
(ps3)-[:PRECEDES {condition: "IF fix_succeeds THEN test"}]->(ps4)
(ps4)-[:PRECEDES {condition: "IF test_succeeds THEN deploy"}]->(ps5)

// === AGENT FOLLOWS PLAN ===
(a)-[:FOLLOWS_PLAN {is_active: true}]->(p)

// === EXECUTION ===
(e:Execution {
    name: "Build CLI tool - attempt 1",
    properties: {
        "status": "success",
        "start_time": "2026-07-01T10:00:00Z",
        "end_time": "2026-07-01T10:05:00Z",
        "goal": "Build my CLI tool",
        "step_count": 5,
        "token_usage": 25000,
        "cost": 0.05
    }
})
(e)-[:EXECUTED_BY {runtime_ms: 300000}]->(a)

// === ARTIFACT ===
(art:Artifact {name: "my_cli_tool", subtype: "binary", ...})
(e)-[:PRODUCED {artifact_type: "binary", is_primary: true}]->(art)

// === TASK RESULTS ===
(tr1:TaskResult {name: "scaffold_step", status: "success", ...})
(tr1)-[:PART_OF_EXECUTION {step_order: 1}]->(e)

(tr2:TaskResult {name: "compile_step", status: "success", ...})
(tr2)-[:PART_OF_EXECUTION {step_order: 2}]->(e)

// === ERROR PATTERN ===
(err:ErrorPattern {
    name: "E0382: use of moved value",
    fingerprint: "sha256:def456...",
    occurrence_count: 12,
    fix_success_rate: 0.95,
    ...
})

// === ERROR IN PREVIOUS FAILED EXECUTION ===
(e_failed:Execution {name: "Build CLI tool - attempt 0", status: "failed", ...})
(e_failed)-[:ENCOUNTERED_ERROR {step_name: "compile", was_resolved: false}]->(err)

// === ERROR WAS RESOLVED BY SUBSEQUENT EXECUTION ===
(err)-[:RESOLVED_BY {resolution_time_ms: 180000, success: true}]->(e)

// === AGENT LEARNED FROM THE RESOLUTION ===
(a)-[:LEARNED_FROM {learned_at: "2026-07-01T10:05:00Z", weight: 0.85, success: true}]->(e)
```

---

## Part 5: Key Queries

### 5.1 Get Agent Configuration

Retrieve an agent with its tools and plan.

```
MATCH (a:Agent {id: $agent_id, is_active: true})
OPTIONAL MATCH (a)-[:USES_TOOL]->(t:Tool)
OPTIONAL MATCH (a)-[:FOLLOWS_PLAN]->(p:Plan {is_active: true})
OPTIONAL MATCH (p)-[:CONTAINS_STEP]->(ps:PlanStep)
OPTIONAL MATCH (ps)-[:PRECEDES]->(next:PlanStep)
RETURN a, collect(DISTINCT t) as tools, p, collect(DISTINCT ps) as steps, collect(DISTINCT next) as next_steps
```

### 5.2 Get Next Step in Plan

Find the next executable step based on execution state.

```
MATCH (p:Plan {id: $plan_id})-[:CONTAINS_STEP]->(step:PlanStep)
WHERE step.properties.step_order = $current_order + 1
OPTIONAL MATCH (step)-[:PRECEDES {condition: $condition}]->(conditional_step)
RETURN step, conditional_step
```

### 5.3 Get Similar Past Executions (Vector Search)

Retrieve semantically similar executions for learning.

```
CALL db.index.vector.queryNodes('execution_embeddings', $limit, $embedding)
YIELD node, score
WHERE node:Execution AND node.properties.status = "success"
MATCH (node)-[:PRODUCED]->(art:Artifact)
OPTIONAL MATCH (node)-[:ENCOUNTERED_ERROR]->(err:ErrorPattern)
RETURN node, score, art, err
ORDER BY score DESC
LIMIT $limit
```

### 5.4 Get Error Pattern by Fingerprint

Retrieve a specific error pattern with its resolution history.

```
MATCH (e:ErrorPattern {fingerprint: $fingerprint})
OPTIONAL MATCH (e)-[:RESOLVED_BY]->(exec:Execution)
OPTIONAL MATCH (exec)-[:PRODUCED]->(art:Artifact)
RETURN e, collect(DISTINCT exec) as resolutions, collect(DISTINCT art) as artifacts
```

### 5.5 Find Similar Errors (Vector Search)

Find error patterns similar to a given embedding.

```
CALL db.index.vector.queryNodes('error_embeddings', $limit, $embedding)
YIELD node, score
WHERE node:ErrorPattern
MATCH (node)-[:RESOLVED_BY]->(exec:Execution)
WHERE exec.properties.status = "success"
RETURN node, score, exec.properties.fix_strategy as fix_strategy
ORDER BY score DESC
LIMIT $limit
```

### 5.6 Get Execution History for an Agent

Get recent executions for an agent with their outcomes.

```
MATCH (a:Agent {id: $agent_id})
OPTIONAL MATCH (a)-[:EXECUTED_BY]-(e:Execution)
OPTIONAL MATCH (e)-[:PRODUCED]->(art:Artifact)
OPTIONAL MATCH (e)-[:ENCOUNTERED_ERROR]->(err:ErrorPattern)
RETURN a, e, collect(DISTINCT art) as artifacts, collect(DISTINCT err) as errors
ORDER BY e.properties.start_time DESC
LIMIT $limit
```

### 5.7 Get Agent Context (Hybrid Retrieval)

Combine structural and semantic retrieval for agent context.

```
// Structural: Recent successful executions
MATCH (a:Agent {id: $agent_id})-[:EXECUTED_BY]-(e:Execution {status: "success"})
WITH a, e
ORDER BY e.properties.start_time DESC
LIMIT 5
WITH a, collect(e) as recent_executions

// Semantic: Similar executions via vector search
CALL db.index.vector.queryNodes('execution_embeddings', 10, $embedding)
YIELD node, score
WHERE node:Execution AND node.properties.status = "success"
WITH a, recent_executions, collect(node) as similar_executions

// Semantic: Similar error patterns
CALL db.index.vector.queryNodes('error_embeddings', 5, $embedding)
YIELD node, score
WHERE node:ErrorPattern
WITH a, recent_executions, similar_executions, collect(node) as similar_errors

RETURN a, recent_executions, similar_executions, similar_errors
```

---

## Part 6: Implementation Requirements

### 6.1 MemoryGraphActor Extensions

Add these new methods to the `MemoryGraphActor`:

```rust
impl MemoryGraphActor {
    // === Agent Management ===
    pub async fn create_agent(&self, input: NodeInput) -> Result<GraphNode, SchemaError>;
    pub async fn get_agent(&self, id: &str) -> Result<GraphNode, SchemaError>;
    pub async fn get_agent_by_name(&self, name: &str) -> Result<GraphNode, SchemaError>;
    pub async fn get_active_agents(&self) -> Result<Vec<GraphNode>, SchemaError>;
    pub async fn get_agent_context(&self, agent_id: &str, goal: &str) -> Result<AgentContext, SchemaError>;

    // === Tool Management ===
    pub async fn create_tool(&self, input: NodeInput) -> Result<GraphNode, SchemaError>;
    pub async fn get_tool(&self, id: &str) -> Result<GraphNode, SchemaError>;
    pub async fn get_tools_for_agent(&self, agent_id: &str) -> Result<Vec<GraphNode>, SchemaError>;
    pub async fn get_tool_by_name(&self, name: &str) -> Result<GraphNode, SchemaError>;

    // === Plan Management ===
    pub async fn create_plan(&self, input: NodeInput) -> Result<GraphNode, SchemaError>;
    pub async fn get_plan(&self, id: &str) -> Result<GraphNode, SchemaError>;
    pub async fn get_plan_steps(&self, plan_id: &str) -> Result<Vec<GraphNode>, SchemaError>;
    pub async fn get_next_step(&self, plan_id: &str, current_order: u32) -> Result<Option<GraphNode>, SchemaError>;

    // === Execution Management ===
    pub async fn start_execution(&self, input: NodeInput) -> Result<GraphNode, SchemaError>;
    pub async fn update_execution_status(&self, id: &str, status: &str, result: Option<&str>) -> Result<(), SchemaError>;
    pub async fn get_execution(&self, id: &str) -> Result<GraphNode, SchemaError>;
    pub async fn get_execution_history(&self, agent_id: &str, limit: usize) -> Result<Vec<GraphNode>, SchemaError>;
    pub async fn get_successful_executions(&self, agent_id: &str, limit: usize) -> Result<Vec<GraphNode>, SchemaError>;
    pub async fn get_failed_executions(&self, agent_id: &str, limit: usize) -> Result<Vec<GraphNode>, SchemaError>;

    // === Task Result Management ===
    pub async fn record_task_result(&self, input: NodeInput) -> Result<GraphNode, SchemaError>;
    pub async fn get_task_results(&self, execution_id: &str) -> Result<Vec<GraphNode>, SchemaError>;

    // === Artifact Management ===
    pub async fn record_artifact(&self, input: NodeInput) -> Result<GraphNode, SchemaError>;
    pub async fn get_artifacts(&self, execution_id: &str) -> Result<Vec<GraphNode>, SchemaError>;
    pub async fn get_latest_artifact(&self, agent_id: &str, artifact_type: &str) -> Result<Option<GraphNode>, SchemaError>;

    // === Error Pattern Management ===
    pub async fn record_error(&self, input: NodeInput) -> Result<GraphNode, SchemaError>;
    pub async fn get_error_by_fingerprint(&self, fingerprint: &str) -> Result<Option<GraphNode>, SchemaError>;
    pub async fn get_similar_errors(&self, embedding_id: &str, limit: usize) -> Result<Vec<GraphNode>, SchemaError>;
    pub async fn link_error_to_fix(&self, error_id: &str, execution_id: &str, properties: HashMap<String, Value>) -> Result<(), SchemaError>;

    // === Relationship Management (New) ===
    pub async fn create_uses_tool(&self, agent_id: &str, tool_id: &str, properties: HashMap<String, Value>) -> Result<(), SchemaError>;
    pub async fn create_follows_plan(&self, agent_id: &str, plan_id: &str, properties: HashMap<String, Value>) -> Result<(), SchemaError>;
    pub async fn create_contains_step(&self, plan_id: &str, step_id: &str, properties: HashMap<String, Value>) -> Result<(), SchemaError>;
    pub async fn create_precedes(&self, from_step_id: &str, to_step_id: &str, properties: HashMap<String, Value>) -> Result<(), SchemaError>;
    pub async fn create_produced(&self, execution_id: &str, artifact_id: &str, properties: HashMap<String, Value>) -> Result<(), SchemaError>;
    pub async fn create_encountered_error(&self, execution_id: &str, error_id: &str, properties: HashMap<String, Value>) -> Result<(), SchemaError>;
    pub async fn create_resolved_by(&self, error_id: &str, execution_id: &str, properties: HashMap<String, Value>) -> Result<(), SchemaError>;
    pub async fn create_part_of_execution(&self, task_result_id: &str, execution_id: &str, properties: HashMap<String, Value>) -> Result<(), SchemaError>;
    pub async fn create_executed_by(&self, execution_id: &str, agent_id: &str, properties: HashMap<String, Value>) -> Result<(), SchemaError>;
    pub async fn create_learned_from(&self, agent_id: &str, execution_id: &str, properties: HashMap<String, Value>) -> Result<(), SchemaError>;
}

/// Agent context returned by hybrid retrieval
pub struct AgentContext {
    pub agent: GraphNode,
    pub tools: Vec<GraphNode>,
    pub plan: Option<GraphNode>,
    pub recent_successes: Vec<GraphNode>,
    pub similar_successes: Vec<GraphNode>,
    pub similar_errors: Vec<GraphNode>,
    pub artifacts: Vec<GraphNode>,
}
```

### 6.2 Schema Constraints (New)

Add these constraints for agent-specific nodes:

| Constraint | Description | Error Type |
|---|---|---|
| Unique `(name, node_type)` | No two agents/tools/plans with same name | `DuplicateNode` |
| Unique fingerprint on ErrorPattern | No duplicate error fingerprints | `DuplicateError` |
| Referential integrity for agent_id | Agent referenced in plans/steps must exist | `NodeNotFound` |
| Referential integrity for tool_id | Tool referenced must exist | `NodeNotFound` |
| Plan step order uniqueness | No duplicate step orders within a plan | `DuplicateStepOrder` |
| Acyclic Precedes | No cycles in plan DAG | `AcyclicDependencyViolation` |

### 6.3 Vector Indexes (New)

Create these vector indexes for semantic search:

```sql
CREATE VECTOR INDEX execution_embeddings
FOR (n:Execution)
ON (n.embedding)
OPTIONS {
    indexConfig: {
        `vector.dimensions`: 1536,
        `vector.similarity_function`: 'cosine'
    }
};

CREATE VECTOR INDEX error_embeddings
FOR (n:ErrorPattern)
ON (n.embedding)
OPTIONS {
    indexConfig: {
        `vector.dimensions`: 1536,
        `vector.similarity_function`: 'cosine'
    }
};

CREATE VECTOR INDEX artifact_embeddings
FOR (n:Artifact)
ON (n.embedding)
OPTIONS {
    indexConfig: {
        `vector.dimensions`: 1536,
        `vector.similarity_function`: 'cosine'
    }
};

CREATE VECTOR INDEX agent_embeddings
FOR (n:Agent)
ON (n.embedding)
OPTIONS {
    indexConfig: {
        `vector.dimensions`: 1536,
        `vector.similarity_function`: 'cosine'
    }
};
```

### 6.4 File Structure Updates

Add these files to your Rust project:

```
src/
├── models/
│   ├── memory_graph/
│   │   ├── mod.rs
│   │   ├── node.rs          # Existing
│   │   ├── edge.rs          # Existing
│   │   ├── traversal.rs     # Existing
│   │   ├── query.rs         # Existing
│   │   ├── context.rs       # Existing
│   │   └── agent.rs         # NEW: Agent-specific models
│   └── ...
├── graph/
│   ├── mod.rs
│   ├── graph_db.rs          # Existing
│   ├── selene_db.rs         # Existing
│   ├── schema.rs            # Existing
│   └── constraints.rs       # NEW: Agent constraints
└── actors/
    ├── memory_graph_actor.rs  # Existing (extend with agent methods)
    └── ...
```

### 6.5 Agent Model File

Create `src/models/memory_graph/agent.rs`:

```rust
// src/models/memory_graph/agent.rs

use serde::{Deserialize, Serialize};
use serde_json::Value;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

use super::node::{GraphNode, NodeInput, NodeUpdate};
use super::edge::RelationshipInput;

// === Agent Context ===
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
    pub metadata: HashMap<String, Value>,
}

// === Agent Execution Status ===
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionStatus {
    Pending,
    Running,
    Success,
    Failed,
    Cancelled,
}

// === Tool Input ===
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecution {
    pub tool_name: String,
    pub arguments: HashMap<String, Value>,
    pub timeout_ms: Option<u64>,
    pub retry_count: Option<u8>,
}

// === Error Fingerprint ===
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorFingerprint {
    pub error_type: String,
    pub message_hash: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl ErrorFingerprint {
    pub fn generate(&self) -> String {
        use sha2::{Sha256, Digest};
        let input = format!(
            "{}:{}:{:?}:{:?}:{:?}",
            self.error_type, self.message_hash, self.file, self.line, self.column
        );
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        format!("sha256:{}", hex::encode(hasher.finalize()))
    }
}

// === Helper Types for Plan Steps ===
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStepOrder {
    pub order: u32,
    pub parallel_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStepDependency {
    pub depends_on: Vec<String>,
    pub condition: Option<String>,
}

// === Suggested Fix ===
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedFix {
    pub before: String,
    pub after: String,
    pub line: u32,
    pub file: String,
    pub description: Option<String>,
}
```

---

## Part 7: Migration Path

### 7.1 Step 1: Update NodeType Enum
Add the new node type variants to `models/memory_graph/node.rs`.

### 7.2 Step 2: Update RelationshipType Enum
Add the new relationship variants to `models/memory_graph/edge.rs`.

### 7.3 Step 3: Create Agent Model File
Create `src/models/memory_graph/agent.rs` with the models defined above.

### 7.4 Step 4: Update Schema Constraints
Add the new constraints to `graph/constraints.rs`.

### 7.5 Step 5: Implement MemoryGraphActor Methods
Add the agent-related methods to `actors/memory_graph_actor.rs`.

### 7.6 Step 6: Create Vector Indexes
Execute the vector index creation queries in your database.

### 7.7 Step 7: Test with Example Data
Insert the example graph from Part 4 and verify all queries work.

---

## Part 8: Testing Checklist

- [ ] Create an Agent node
- [ ] Create Tool nodes
- [ ] Create a Plan node with PlanStep nodes
- [ ] Create USES_TOOL, FOLLOWS_PLAN, CONTAINS_STEP, PRECEDES relationships
- [ ] Start an Execution
- [ ] Record TaskResult nodes
- [ ] Record Artifact nodes
- [ ] Record ErrorPattern nodes
- [ ] Create ENCOUNTERED_ERROR, RESOLVED_BY, PRODUCED relationships
- [ ] Create LEARNED_FROM relationship
- [ ] Verify unique constraints
- [ ] Verify referential integrity
- [ ] Verify acyclic PRECEDES constraint
- [ ] Test vector search queries
- [ ] Test hybrid context retrieval
- [ ] Test execution history queries
- [ ] Test agent configuration queries

---

## Part 9: Integration with Existing System

These extensions integrate seamlessly with your existing schema:

- **Node types are additive** — no existing nodes are modified
- **Relationship types are additive** — no existing relationships are modified
- **Constraints are additive** — existing constraints remain unchanged
- **Vector indexes are additive** — existing indexes remain unchanged
- **API layer** extends the existing `MemoryGraphActor` with new methods
- The bidirectional ID mapping (UUID ↔ u64) continues to work as before. All new nodes and relationships use the same mapping system.

---

## Summary

This extension transforms the Spire knowledge graph into a complete agent infrastructure:

- **Agent definitions** are stored as nodes with system prompts and parameters
- **Tools** are stored as nodes with JSON schemas and Rust function mappings
- **Plans** are stored as DAGs of steps with explicit dependencies
- **Executions** are fully audited with task-level results
- **Artifacts** store build outputs and other results
- **Error patterns** enable learning from past mistakes
- **Vector embeddings** enable semantic retrieval of similar contexts
- **Hybrid queries** combine structural and semantic search

The graph remains the **single source of truth** for your entire agent system.
