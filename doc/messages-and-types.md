# Spire Rust: Data Types Quick Reference

> **Last updated:** 2026-07-26

This document is a quick reference for all shared data types used across actors. For message enum definitions, see [`actors-and-messages.md`](actors-and-messages.md).

---

## Graph Types (`models/memory_graph.rs`)

| Type | Fields | Purpose |
|------|--------|---------|
| `NodeType` | `Plan, PlanStep, BuildSystem, Diagnostic, StepDefinition, ConcreteTool, ...` | Graph node type discriminator |
| `GraphNode` | `id, node_type, subtype, name, description, properties: HashMap<String, Value>, embedding_id, created_at, updated_at, version` | A node in the knowledge graph |
| `NodeInput` | `node_type, subtype, name, description, properties, embedding_id` | Input for creating a new node |
| `NodeUpdate` | `node_type, subtype, name, description, properties, embedding_id` | Partial update (Option<Option<T>> for clear-vs-unset) |
| `NodeFilter` | `node_type, subtype, name, status, tags, properties, limit, offset` | Query filter for finding nodes |
| `RelationshipType` | `HasDiagnostic, DependsOn, HasStep, Custom(String), ...` | Edge type discriminator |
| `GraphEdge` | `id, edge_type, from_id, to_id, properties, created_at, weight` | A directed edge between nodes |
| `RelationshipInput` | `edge_type, from_id, to_id, properties, weight` | Input for creating a relationship |
| `TraversalOptions` | `max_depth, relationship_types, max_nodes, direction` | Options for graph traversal |
| `TraversalResult` | `nodes, edges, paths` | Result of a graph traversal |

---

## Build-Fix Loop Types (`models/memory_graph.rs`)

| Type | Fields | Purpose |
|------|--------|---------|
| `BuildContext` | `project_root, build_system, target, environment` | Parameters for a build operation |
| `SystemBuildResult` | `build_type, path, project_name, success, errors, warnings, exit_code, duration_ms` | Result from a single build system |
| `BuildResult` | `success, system_results, build_run_id, duration_secs` | Aggregated result across all systems |
| `BuildError` | `error_text, error_type, file, line, column, exit_code, build_type, diagnostic_node_id, file_node_id` | A single build error with context |
| `BuildStartResult` | `build_id, success, build_result, fix_plan, iteration_count, max_iterations` | Result returned from BuildOrchestrator |
| `FixPlan` | `errors: Vec<AnnotatedError>, ordered_fixes: Vec<ScoredFix>, max_iterations` | Complete fix plan with ordered strategies |
| `AnnotatedError` | `error, build_type, build_path, file_node_id, diagnostic_node_id, fix_options` | Error annotated with matched fix options |
| `ScoredFix` | `strategy: FixStrategy, confidence, required_tools, validation_tools` | A fix strategy with confidence score |
| `FixStrategy` | `id, name, description, category, confidence_threshold, success_rate, execution_steps, has_rollback` | A multi-step fix strategy |
| `ErrorType` | `id, name, description, severity, parent_id, detection_patterns, common_causes` | An error type in the build-fix hierarchy |
| `Intent` | `id, name, description, priority, requires_approval, state_requirements` | A detected intent from a user query |
| `BuildState` | `id, name, description, conditions, rollback_state` | A state in the build state machine |

---

## Plan Mode Types (`models/memory_graph.rs`)

| Type | Fields | Purpose |
|------|--------|---------|
| `PlanStatus` | `Pending, Approved, Executing, Paused, Completed, Rejected, Failed, Skipped` | Status of a plan or step |
| `PlanStepData` | `description, step_name, arg_template: Value, depends_on: Vec<u32>, uses_error_context` | A step within a plan (from LLM) |
| `PlanStatusResult` | `plan_id, goal, status, intent_name, steps, total_steps, completed_steps, failed_steps` | Result of a plan status query |
| `PlanStepEntry` | `id, order, description, step_name, status, result, error` | A single step in a plan status response |

---

## Actor Framework Types (`framework/`)

| Type | Fields | Purpose |
|------|--------|---------|
| `ActorError` | `ToolNotFound, Io, Serialization, ChannelClosed, Internal` | Typed errors for actor operations |
| `Actor` (trait) | `Message: Send + 'static`, `handle(&mut self, msg)` | Every actor implements this |
| `ActorSystem` | `DashMap<String, Box<dyn Any>>` | Registry mapping actor names to `mpsc::Sender<M>` |

---

## Transport Types (`transport/socket.rs`)

| Type | Fields | Purpose |
|------|--------|---------|
| `TransportMessage` | `Bind, Accept, SendRequest, SendNotification, SetSelfTx, SetRequestHandler, SetNotificationHandler` | Messages for the TCP transport actor |
| `IncomingRequestMessage` | `method, params, response_tx` | A JSON-RPC request from the extension |
| `IncomingNotification` | `method, params` | A JSON-RPC notification from the extension |

---

## Embedding Types (`embedder/`)

| Type | Fields | Purpose |
|------|--------|---------|
| `Embedder` (trait) | `embed(&self, text) -> Result<Vec<f32>>` | Trait for text embedding |
| `CandleEmbedder` | — | Real embedding via Candle + HuggingFace |
| `NoopEmbedder` | — | Fallback embedder (returns zero vector) |

---

## Diagnostic Types (`models/memory_graph.rs`)

| Type | Fields | Purpose |
|------|--------|---------|
| `DiagnosticInfo` | `message, file, line, column, severity, build_type, build_run_id` | A single build diagnostic stored as a graph node |

---

## MCP Config Types (`models/memory_graph.rs`)

| Type | Fields | Purpose |
|------|--------|---------|
| `McpServerConfigEntry` | `name, command, args, env, url, headers, autostart` | MCP server configuration |
| `McpConfigFile` | `servers: Vec<McpServerConfigEntry>` | The JSON file format for MCP config |

---

## Progress Types (`models/` or `actors/progress.rs`)

| Type | Fields | Purpose |
|------|--------|---------|
| `ProgressUpdate` | `task_id, message, percent, status, metadata` | A progress update for a background task |
| `ProgressStatus` | `Running, Completed, Failed` | Status of a background task |