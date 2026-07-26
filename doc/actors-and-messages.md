# Actor System — Actors & Message Types

> **Last updated:** 2026-07-26

This document catalogs every actor in the system, its message enum variants, and how they connect.

---

## Architecture Overview

All 20+ actors communicate via `tokio::sync::mpsc` channels. The singleton `ActorSystem` (in `spire-core/src/framework/system.rs`) spawns each actor and returns an `mpsc::Sender<M>` handle.

```
ActorTrait: Send + 'static
  └─ Message: Send + 'static
  └─ handle(&mut self, msg: Self::Message)  ← called for each received message
  └─ spawn(self) -> (Sender<M>, JoinHandle)  ← default loop on rx.recv()
```

Messages use `tokio::sync::oneshot::Sender<R>` for reply channels. The `anyhow::Result<R>` pattern is standard — errors propagate up via `?`.

---

## Actor Catalog

### 1. CoordinatorActor (`actors/coordinator.rs`)

**Purpose:** Main workflow orchestrator. Receives user requests, routes to intent router or LLM.

```rust
pub enum CoordinatorMessage {
    HandleRequest { method: String, params: Value, response_tx: oneshot::Sender<Value> },
    Shutdown,
}
```

**Connections:** Holds `mpsc::Sender` for all sub-actors (chat, tools, mcp_client, llm, progress, system, memory_graph, project_query, intent_router, prompt_handler, build_orchestrator, tool_router, plan_orchestrator, transport).

---

### 2. IntentRouterActor (`actors/intent_router.rs`)

**Purpose:** Matches user queries to registered intents using config-driven pattern matching.

```rust
pub enum RouteResult {
    Build { intent_name, confidence, parameters },
    Plan { intent_name, confidence, parameters },
    StateBlocked { intent_name, confidence, missing_states },
    NeedsApproval { intent_name, confidence },
    Chat,
}
```

**Key method:** `route_query(query, states) -> RouteResult` — loads intents from graph, matches against patterns.

---

### 3. ChatActor (`actors/chat.rs`)

**Purpose:** Manages chat dialogs and message history.

```rust
pub enum ChatMessage {
    Send { chat_id, content, role, reply_to: Sender<Result<ChatMessageResponse>> },
    Append { chat_id, content, role, reply_to: Sender<Result<()>>, widget: Option<Value> },
    GetHistory { chat_id, reply_to: Sender<Result<Vec<ChatMessageResponse>>> },
}
```

---

### 4. LlmActor (`actors/llm.rs`)

**Purpose:** LLM gateway — sends prompts to DeepSeek API (or mock in dev mode).

```rust
pub enum LlmMessage {
    Complete { prompt: String, reply_to: Sender<Result<String>> },
    Stream { prompt: String, reply_to: Sender<Result<mpsc::Receiver<String>>> },
}
```

---

### 5. MemoryGraphActor (`actors/memory_graph.rs`)

**Purpose:** Sole data store actor. Owns the GraphDb (SeleneDB) instance for nodes, edges, and vector embeddings.

```rust
pub enum MemoryGraphMessage {
    // Node operations
    GetNode { id, reply_to },
    StoreNode { node: NodeInput, reply_to },
    UpdateNode { id, updates: NodeUpdate, reply_to },
    DeleteNode { id, reply_to },
    QueryNodes { filter: NodeFilter, reply_to },

    // Relationship operations
    CreateRelationship { rel: RelationshipInput, reply_to },
    DeleteRelationship { id, reply_to },
    QueryRelationships { filter, reply_to },
    Traverse { start_id, options, reply_to },

    // Config
    GetConfig { key, reply_to },
    SetConfig { key, value, reply_to },

    // Transaction stream (for atomic batches)
    OpenStream { reply_to },

    // Project
    GetProjectContext { reply_to },
    Sync { reply_to },
    Initialize { data_dir, reply_to },

    // Search
    SemanticSearch { query, threshold, reply_to },
}
```

---

### 6. ToolOrchestratorActor (`actors/tool_orchestrator.rs`)

**Purpose:** Executes tools and tool chains with step context (template resolution for `$error.*`, `$step.*` variables).

```rust
pub enum ToolOrchestratorMessage {
    ExecuteTool { tool_name, parameters, reply_to },
    ExecuteToolChain { tools, parameters, reply_to },
    ExecuteToolChainWithContext { tools, error: BuildError, project_root, reply_to },
}
```

---

### 7. ToolRouterActor (`actors/tool_providers/mod.rs`)

**Purpose:** Routes tool calls by prefix:
- `workspace/`, `document/`, `diagnostics/`, `git/`, `symbols/` → VS Code extension
- `project/`, `build/`, `test/`, `lint/`, `install/` → internal actors
- Everything else → MCP

---

### 8. BuildOrchestratorActor (`actors/build_orchestrator.rs`)

**Purpose:** Manages the build-fix loop lifecycle.

```rust
pub enum BuildOrchestratorMessage {
    StartBuild { parameters: BuildContext, reply_to },
    ApplyFix { strategy_name, target_system, reply_to },
    RollbackBuild { step, reply_to },
}
```

Key data types: `BuildContext`, `SystemBuildResult`, `BuildResult`, `BuildError`, `FixPlan`, `AnnotatedError`, `ScoredFix`.

---

### 9. ErrorAnalyzerActor (`actors/error_analyzer.rs`)

**Purpose:** Matches build errors to fix strategies via regex detection + graph lookup.

```rust
pub enum ErrorAnalyzerMessage {
    AnalyzeErrors { system_results, build_run_id, reply_to },
    ValidateFix { fix_name, context, reply_to },
}
```

---

### 10. PlanOrchestratorActor (`actors/plan_orchestrator.rs`) ★ NEW

**Purpose:** Creates, stores, and executes multi-step plans with approve/reject/pause/retry/skip flow.

```rust
pub enum PlanOrchestratorMessage {
    CreatePlan { goal, intent_name, parameters, reply_to },
    ApprovePlan { plan_id, reply_to },
    RejectPlan { plan_id, reason, reply_to },
    GetPlanStatus { plan_id, reply_to },
    PausePlan { plan_id, reply_to },
    ResumePlan { plan_id, reply_to },
    RetryStep { plan_id, step_order, reply_to },
    SkipStep { plan_id, step_order, reply_to },
}
```

Key data types: `PlanStatus` (Pending/Approved/Executing/Paused/Completed/Rejected/Failed/Skipped), `PlanStepData`, `PlanStatusResult`, `PlanStepEntry`.

---

### 11. ProjectBuildActor (`actors/project_build.rs`)

**Purpose:** Per-system build execution via MCP tools. Supports multi-system parallel builds.

### 12. ProjectTestActor (`actors/project_test.rs`)

**Purpose:** Test execution via MCP tools.

### 13. ProjectLintActor (`actors/project_lint.rs`)

**Purpose:** Lint execution via MCP tools.

### 14. ProjectInstallActor (`actors/project_install.rs`)

**Purpose:** Package installation via MCP tools (e.g., `cargo add`, `npm install`).

### 15. ProjectAnalyzerActor (`actors/project_analyzer.rs`)

**Purpose:** Semantic project analysis for LLM context injection.

### 16. ProjectQueryActor (`actors/project_query.rs`)

**Purpose:** Structured project queries (files, symbols, dependencies) for LLM context.

### 17. ProjectSyncActor (`actors/project_sync.rs`)

**Purpose:** Three-phase project structure sync (scan, analyze, store).

### 18. SystemPromptActor (`actors/system_prompt.rs`)

**Purpose:** Caches system prompt prefix for DeepSeek prompt caching.

### 19. McpClientActor (`actors/mcp_client.rs`)

**Purpose:** Manages external MCP server connections (stdio or HTTP transport).

### 20. ProgressActor (`actors/progress.rs`)

**Purpose:** Broadcasts progress updates via `tokio::sync::broadcast`.

```rust
pub enum ProgressMessage {
    Send { update: ProgressUpdate, reply_to },
    Subscribe { reply_to: Sender<Receiver<ProgressUpdate>> },
}
```

### 21. SystemActor (`actors/system.rs`)

**Purpose:** System state machine driving the startup phase chain.

---

## Actor Wiring Diagram

```
main.rs spawn order:
  ChatActor → ProgressActor → McpClientActor → LlmActor → SystemActor →
  MemoryGraphActor → ProjectSyncActor → ProjectAnalyzerActor → ProjectQueryActor →
  SystemPromptActor → IntentRouterActor → ErrorAnalyzerActor → TransportActor →
  ProjectBuildActor → ProjectTestActor → ProjectLintActor → ProjectInstallActor →
  ToolRouterActor → ToolsActor → PromptHandlerActor → ToolOrchestratorActor →
  BuildOrchestratorActor → PlanOrchestratorActor →
  CoordinatorActor (receives all sender handles)
```

## Reply Channel Pattern

All actors use the same reply pattern:

```rust
let (tx, rx) = tokio::sync::oneshot::channel();
actor_tx.send(SomeMessage { ..., reply_to: tx }).await?;
let result = rx.await??;  // Outer Result: channel closed; Inner Result: operation
```

Error types vary by actor:
- `MemoryGraphMessage` → `Result<String>` (SeleneDB error messages)
- `LlmMessage` → `Result<String>` (LLM error messages)
- `ProjectQueryMessage` → `Result<Vec<GraphNode>>` (empty if none)
- `BuildOrchestratorMessage` → `Result<BuildStartResult>`
- `PlanOrchestratorMessage` → `Result<PlanStatusResult>`