# Multi-Step Execution Flows

This document catalogs all multi-step execution flows in the Spire system,
annotating each step with its resource type (Graph, Tool, LLM, etc.) and
calling out decisions and gaps.

---

## 1. Build-Fix Loop

**Entry point:** `RouteResult::Build { parameters }` (from IntentRouter)
**Primary actor:** `BuildOrchestratorActor`
**Duration:** Potentially many cycles (build → fail → fix → rebuild)

### 1.1 Start Build

```
Intent matched → RouteResult::Build
  ↓
[Coordinator] line 580
  │ sends BuildOrchestratorMessage::StartBuild
  ↓
[BuildOrchestratorActor] line 87
  │
  ├── 1. StoreNode("build_session", status="building")            ← Graph write
  ├── 2. current_state = "building"                                ← Local state
  │
  ├── 3. dispatch_build() via ToolRouter → project/build           ← REAL TOOL EXECUTION
  │     ├── Gets build config from ProjectQueryActor                ← Graph read
  │     ├── Dispatches per-system MCP builds (Cargo, npm, etc.)    ← Tool execution (MCP)
  │     ├── Collects results (SystemBuildResult[] with errors/warnings)
  │     └── Returns aggregated BuildResult with per-system details
  │
  ├── 4. IF success:
  │     ├── UpdateNode: build_session status → "completed"         ← Graph write
  │     ├── transition_state("build_completed", true)               ← State activated!
  │     ├── transition_state("build_failed", false)
  │     └── Return BuildStartResult { success: true, fix_plan: None }
  │
  └── 5. IF failure:
        ├── UpdateNode: build_session status → "failed"            ← Graph write
        ├── transition_state("build_completed", false)
        ├── transition_state("build_failed", true)                  ← State activated!
        │
        ├── 6. analyze_errors() → ErrorAnalyzer::AnalyzeErrors     ← Cross-actor call
        │     ↓
        │     [ErrorAnalyzerActor line 49]
        │       ├── Load error types from graph (NodeType::ErrorType) ← Graph read
        │       │     Uses regex detection_patterns from config/intents.json
        │       │
        │       ├── For each failed system:
        │       │     ├── QueryDiagnostic nodes (build_run_id)       ← Graph read
        │       │     │     Finds filed nodes stored by ProjectBuildActor
        │       │     │
        │       │     ├── For each error:
        │       │     │     ├── find_file_node_id() via HasDiagnostic ← Graph traversal
        │       │     │     │     OR direct file name query
        │       │     │     │
        │       │     │     ├── Match error_type patterns via regex ← Local logic
        │       │     │     │     Uses config/intents.json patterns:
        │       │     │     │     "error\\[E\\d{4}\\]" → "rustc-compile-error"
        │       │     │     │     "Cannot find module" → "node-module-not-found"
        │       │     │     │
        │       │     │     └── lookup_fix_strategies()              ← Graph read
        │       │     │           Returns matched strategies sorted by confidence
        │       │     │
        │       │     └── Build AnnotatedError with fix_options
        │       │
        │       ├── Merge and deduplicate fixes across all errors
        │       │     sorted by confidence descending
        │       │
        │       └── Return FixPlan { errors, ordered_fixes, max_iterations }
        │
        └── 7. Return BuildStartResult { success: false, fix_plan: Some(...) }
```

**Resource breakdown:**
- 3-4 Graph writes (session node, state transitions)
- 1-N Graph reads (error types, fix strategies, diagnostics)
- 1 ToolRouter call (which dispatches N MCP builds in parallel)
- 0 LLM calls
- 1 Cross-actor call (ErrorAnalyzer)

**Key changes from previous version:**
- ✅ `start_build()` now **actually dispatches the build** via ToolRouter → `project/build`
- ✅ State nodes are now properly transitioned (`build_completed`, `build_failed`)
- ✅ ErrorAnalyzer uses **diagnostic-driven regex matching** instead of vector search
- ✅ Fix plan uses **multiple error types** with per-error annotated options

### 1.2 Actual Build Execution

**Entry point:** `CallTool { tool: "project/build" }` (via ToolRouter or LLM tool call)
**Primary actor:** `ProjectBuildActor`

```
CallTool("project/build", {mode, scope, clean})
  ↓
[ProjectBuildActor::handle_build()] line 108
  │
  ├── 1. get_build_config() → ProjectQueryActor::CallTool        ← Graph query
  │       ("project/getBuildConfig", {})
  │
  ├── 2. Parse buildSystems array from config                     ← Local logic
  │
  ├── 3. For each build system (in parallel):
  │       │
  │       ├── "Cargo" → McpClientMessage::CallTool              ← Tool execution (MCP)
  │       │     { server: "mcp-cargo", tool: "build", args: {path, mode, scope, clean} }
  │       │     ⚠ If clean=true, also calls mcp-cargo/clean first
  │       │
  │       ├── "npm"|"pnpm"|"yarn" → McpClientMessage::CallTool  ← Tool execution (MCP)
  │       │     { server: "mcp-node", tool: "build", args: {path, package_manager} }
  │       │     ⚠ If clean=true, calls mcp-node/install first
  │       │
  │       └── Unknown → skip with note
  │
  ├── 4. Store diagnostics in graph                               ← Graph write
  │       ├── For each warning: StoreNode("diagnostic", severity="warning")
  │       ├── For each error: StoreNode("diagnostic", severity="error")
  │       └── Create HasDiagnostic edges from file → diagnostic
  │
  ├── 5. Collect aggregated results                              ← Local logic
  │       per-system { type, path, success, errors[], warnings[], exit_code }
  │
  └── 6. Return { success, duration_secs, systems: [...] }       ← Response
```

**Resource breakdown:**
- 1 Graph query (getBuildConfig)
- N MCP tool calls (one per build system, in parallel)
- M Graph writes (diagnostics + relationships)
- 0 LLM calls

**Note:** This is now called **from** BuildOrchestrator via ToolRouter, so BuildOrchestrator gets the result and can react to failures.

### 1.3 Apply Fix (with Auto-Rebuild)

```
User/LLM chooses fix strategy
  ↓
BuildOrchestratorMessage::ApplyFix { strategy_name, target_system }
  ↓
[BuildOrchestratorActor::apply_fix()] line 286
  │
  ├── 1. Check iteration limit                                     ← Loop guard
  │       iteration_count++ > max_iterations (default: 5)?
  │       → "Manual intervention required" error
  │
  ├── 2. QueryNodes("fix_strategy", name=strategy_name)            ← Graph read
  │       Extract execution_steps from node properties["steps"]
  │
  ├── 3. ExecuteToolChain(execution_steps) via ToolOrchestrator    ← Tool execution
  │       (steps: "identify_missing_symbol", "find_correct_module",
  │        "add_import_statement", etc.)
  │       Note: ToolOrchestrator currently uses stubs — needs MCP wiring
  │
  ├── 4. transition_state("fix_applied", true)                     ← State transition
  │
  ├── 5. Auto-rebuild (scoped to target_system)                   ← Recursive
  │       BuildContext.target = Some(target_system)
  │       start_build(rebuild_context)                              ← Build again!
  │         └── Only builds the affected system (not everything)
  │
  └── 6. Return BuildStartResult (new cycle result)                ← Response
```

**Resource breakdown:**
- 1 Graph read (fix strategy)
- N Tool executions (fix steps)
- 1 state transition
- 1 full rebuild cycle (recursive)

### 1.4 Rollback

```
Someone calls RollbackBuild { step }
  ↓
[BuildOrchestratorActor::rollback_build()] line 403
  │
  └── 1. ToolOrchestratorMessage::ExecuteTool                    ← Cross-actor call
        tool_name: "rollback_{step}"
        parameters: {}
        ↓ (stub implementation)
```

---

## 2. LLM Prompt → Tool Call Lifecycle (The Agent Loop)

**Entry point:** `PromptHandlerMessage::HandlePromptWithTools { query, tools }`
**Primary actor:** `PromptHandlerActor`
**Duration:** 2+ LLM round trips (LLM → tool call → tool results → LLM → final)

```
HandlePromptWithTools(query, intent_name, build_context, tools)
  ↓
[PromptHandlerActor::handle_prompt_with_tools()] line 148
  │
  ├── 1. gather_context()                                        ← Graph reads
  │       ├── GetProjectContext()                                ← Graph read
  │       ├── SearchMemories(query, top_k=5, threshold=0.3)      ← Graph semantic search
  │       │   Decision: similarity threshold 0.3
  │       ├── QueryRelatedNodes(intent, "build_error")           ← Graph read
  │       ├── QueryRelatedNodes(intent, "fix_strategy")          ← Graph read
  │       ├── QueryRelatedNodes(intent, "tool")                  ← Graph read
  │       └── GetBuildState(intent)                              ← Graph read
  │
  ├── 2. build_system_prompt(context)                            ← Local assembly
  │       ├── SystemPrompt::BuildPrefix(hash=0, tools=[])        ← Cross-actor call
  │       │     ↓
  │       │     [SystemPromptActor::build_prefix()] line 347
  │       │       ├── [0] Base system prompt (static string)
  │       │       ├── [1] Tool list (empty here — separate path)
  │       │       ├── [2] call_project_tool("project/getOverview")    ← Graph/ProjectQuery
  │       │       └── [3] call_project_tool("project/getArchitecture") ← Graph/ProjectQuery
  │       │     Cache: caches on tools_hash, invalidates on change
  │       │
  │       └── Append context sections (intent, project, memories,
  │           error_types, fix_strategies, relevant tools, build_state)
  │
  ├── 3. Build messages array [system + user]                    ← Local assembly
  │
  ├── 4. LlmMessage::CompleteWithTools(messages, tools)          ← LLM call (1st RTT)
  │       ↓
  │       [LlmActor::complete_with_tools()] line 255
  │         ├── Sanitize tool names (replace / with _)
  │         ├── Deduplicate tools by sanitized name
  │         ├── POST to DeepSeek /v1/chat/completions (or /beta/ for strict mode)
  │         └── Parse response:
  │               ├── Native JSON tool_calls? → translate names back
  │               ├── XML/Claude format? → parse <DSML> tags → synthetic tool_calls
  │               └── Plain text? → return as-is
  │
  ├── 5. Parse LLM response for tool_calls                       ← Decision point
  │       │
  │       ├── IF tool_calls found:                              ← Tool call lifecycle
  │       │   │
  │       │   ├── For each tool call:
  │       │   │     ├── ToolRouterMessage::CallTool              ← Tool execution (routed)
  │       │   │     │     ↓
  │       │   │     │     [ToolRouterActor]
  │       │   │     │       ├── "project/*" → ProjectQueryActor
  │       │   │     │       ├── "project/build" → ProjectBuildActor (see §1.2)
  │       │   │     │       ├── "workspace/*" → FileHandlerV2
  │       │   │     │       ├── "document/*" → DocumentHandler
  │       │   │     │       ├── "git/*" → GitHandler
  │       │   │     │       ├── "diagnostics/*" → DiagnosticsHandler
  │       │   │     │       ├── "symbols/*" → SymbolsHandler
  │       │   │     │       └── mcp-*/* → McpClient::CallTool (see §1.2 step 3)
  │       │   │     │
  │       │   │     └── Collect result { tool_call_id, tool_name, result|error }
  │       │   │
  │       │   ├── Build follow-up messages:
  │       │   │     [system, user query, "Tool execution results:\n{json}"]
  │       │   │
  │       │   └── LlmMessage::CompleteWithMessages(follow_up)   ← LLM call (2nd RTT)
  │       │         ↓ Returns final response
  │       │
  │       └── IF no tool_calls:
  │             └── Return content directly
  │
  └── 6. Return final response
```

**Resource breakdown (typical):**
- 5-8 Graph reads (context gathering)
- 2-3 Graph/ProjectQuery reads (system prompt)
- 1 LLM call (1st RTT)
- 1+ Tool executions (MCP or handler)
- 1 LLM call (2nd RTT, follow-up with tool results)
- 0 Graph writes

---

## 3. Intent Detection → Routing

**Entry point:** User message (JSON-RPC `handle_message`)
**Primary actor:** `IntentRouterActor` → `CoordinatorActor`
**Duration:** Graph queries + matching logic (no LLM call)

```
User: "build this project"
  ↓
[JSON-RPC handler] CoordinatorActor
  │
  ├── 1. IntentRouterMessage::RouteQuery(query)                    ← Cross-actor call
  │       ↓
  │       [IntentRouterActor::route_query()] (intent_router.rs line 118)
  │         │
  │         ├── a. QueryNodes(subtype="intent")                    ← Graph read
  │         │     Gets all configured intents from graph
  │         │     (Seeded from config/intents.json at startup)
  │         │
  │         ├── b. Keyword matching (lines 133-180)               ← Local logic
  │         │     For each intent node:
  │         │       For each pattern in properties["patterns"]:
  │         │         IF query_lower.contains(pattern_lower):
  │         │           Score = priority / 10.0
  │         │           Keep best match by priority
  │         │     Also checks intent name + description as weak match
  │         │     (confidence = priority/10.0 * 0.9)
  │         │
  │         ├── c. IF keyword match found:                        ← Decision
  │         │     ├── Check state_requirements from intent config ← Graph read
  │         │     │     QueryNodes(subtype="build_state"), filter active=true
  │         │     │     Compare against state_requirements array
  │         │     │     (States now properly transitioned by BuildOrchestrator!)
  │         │     │
  │         │     ├── Check requires_approval flag                 ← Properties read
  │         │     │
  │         │     └── route_to_handler(handler, action)            ← Config-driven dispatch
  │         │           ├── "BuildOrchestrator" → RouteResult::Build
  │         │           ├── "ErrorAnalyzer"     → RouteResult::AnalyzeError
  │         │           ├── "ProjectAnalyzer"   → RouteResult::AnalyzeProject
  │         │           ├── "ProjectQuery"      → RouteResult::QueryProject
  │         │           ├── "ToolOrchestrator"  → RouteResult::ExecuteTool
  │         │           └── (anything else)     → RouteResult::Chat (fallback)
  │         │
  │         └── d. IF no keyword match (line 248):                ← Semantic fallback
  │               ├── SearchContext(query, top_k=5, threshold=0.6) ← Graph vector search
  │               │   Post-filter for subtype="intent", similarity ≥ 0.6
  │               │
  │               ├── IF match found:
  │               │     └── Same state/approval/route flow as (c)
  │               │
  │               └── IF no match:
  │                     └── RouteResult::Chat (default)
  │
  └── 2. Coordinator handles RouteResult                          ← Hard-coded match
        │
        ├── RouteResult::Build → BuildOrchestrator::StartBuild     ← See §1.1 (NOW WORKS!)
        ├── RouteResult::Chat → PromptHandler::HandlePrompt        ← See §2 (LLM-agent path)
        ├── RouteResult::AnalyzeError → ErrorAnalyzer::AnalyzeErrors ← See §1.1 step 6
        ├── RouteResult::AnalyzeProject → ProjectAnalyzer::Analyze
        ├── RouteResult::QueryProject → ProjectQuery::CallTool("project/query")
        ├── RouteResult::ExecuteTool → ToolOrchestrator::ExecuteTool ← See §1.3 (stub)
        ├── RouteResult::StateBlocked → Return error message to user
        └── RouteResult::NeedsApproval → Return confirmation prompt to user
```

---

## 4. Startup Phases

**Entry point:** `CoordinatorActor::run_startup_phases()`
**Duration:** Sequential phases, ~1-10 seconds total

```
[Coordinator] run_startup_phases()
  │
  ├── Phase 1: Initialize Memory Graph                           ← Graph init
  │     └── MemoryGraphActor starts, connects to Dgraph/DB
  │
  ├── Phase 2: Bootstrap Intents + Tool Config (from startup_phases.rs)
  │     ├── bootstrap_intents_from_config(config/intents.json)    ← Config load → Graph write
  │     │     ├── For each intent: StoreNode("intent", ...)       ← Graph write (×N)
  │     │     ├── For each tool: StoreNode("tool", ...)           ← Graph write (×N)
  │     │     ├── For each error_type: StoreNode("error_type", ...) ← Graph write (×N)
  │     │     ├── For each fix_strategy: StoreNode("fix_strategy", ...) ← Graph write (×N)
  │     │     └── For each state: StoreNode("state", ...)         ← Graph write (×N)
  │     │
  │     ├── bootstrap_states(intents.json)                        ← Config load
  │     │     └── Sets active=true on state nodes that have been satisfied
  │     │         (Now also activated by BuildOrchestrator during build lifecycle)
  │     │
  │     ├── discover_mcp_tools(mcp-config.json)                   ← MCP discovery
  │     │     ├── GetConnectedServersWithTools                    ← MCP RPC call
  │     │     │     For each MCP server:
  │     │     │       └── StoreNode("tool", name=mcp-tool-name, ...)  ← Graph write
  │     │     │
  │     │     └── Load tool_capabilities.json                     ← Config load
  │     │           Maps MCP tools to capabilities
  │     │
  │     └── bootstrap_relationships(intents.json)                 ← Config load
  │           ├── intent → tool (lookup_tool)                     ← Graph edge write
  │           ├── error_type → fix_strategy                       ← Graph edge write
  │           └── intent → state_requirements                     ← Graph edge write
  │
  ├── Phase 3: Project Sync (ProjectSyncPhase)
  │     ├── Scan project directory                                ← File system scan
  │     ├── Parse build files (Cargo.toml, package.json, etc.)    ← Build parsers
  │     ├── Store parsed results in graph                         ← Graph writes
  │     └── Activate "state-project_synced" state node            ← Graph write
  │
  └── Phase 4: Start MCP Servers
        ├── For each MCP server in mcp-config.json:              ← Process spawning
        │     └── Spawn subprocess (e.g., mcp-cargo, mcp-node)
        │
        └── ToolRouterActor registers discovered MCP tools
```


## 5. Plan Mode Execution Flow (NEW)

**Entry point:** `RouteResult::Plan { parameters }` (from IntentRouter)
**Primary actor:** `PlanOrchestratorActor`
**Duration:** Single cycle plan → execute (no retry loop by default)

### 5.1 Create Plan

```
User: "plan how to fix the build"
  ↓
IntentRouter matches "plan" or "plan how to" pattern
  ↓
RouteResult::Plan { intent_name: "plan", confidence, parameters }
  ↓
[Coordinator] line ~605
  │ sends PlanOrchestratorMessage::CreatePlan
  ↓
[PlanOrchestratorActor]
  │
  ├── 1. gather_plan_context()                                    ← Graph read (optional)
  │     ├── Query ProjectSnapshot (name, node count)
  │     └── Query StepDefinitions (available step names)
  │
  ├── 2. generate_plan_steps(goal, context)                        ← LLM call
  │     └── Returns Vec<PlanStepData> (description, step_name,
  │         arg_template, depends_on, uses_error_context)
  │
  ├── 3. store_plan(plan_id, goal, steps)                          ← Graph write
  │     ├── StoreNode("plan", status="pending", ...)
  │     └── For each step:
  │           ├── StoreNode("planStep", order=N, ...)
  │           └── CreateRelationship("HAS_STEP", plan → step)
  │
  └── 4. chat_notify(plan_widget)                                 ← Chat/Transport notification
        └── Plan-list widget pushed to chat (approve/reject buttons)
```

### 5.2 Approve Plan (Execute Steps)

```
User clicks "Approve"
  ↓
PlanOrchestratorMessage::ApprovePlan { plan_id }
  ↓
[PlanOrchestratorActor]
  │
  ├── 1. update_plan_status("executing")                           ← Graph write
  ├── 2. Query PlanStep nodes for plan_id (sorted by order)        ← Graph read
  ├── 3. get_step_data() for dependency resolution                 ← Graph read
  ├── 4. resolve_execution_order() (topological sort via DFS)       ← Local logic
  │
  └── 5. For each step in order:
        ├── update_step_status("running")                          ← Graph write
        ├── emit_plan_widget() → real-time update                  ← Transport notification
        ├── ExecuteTool via ToolOrchestrator                        ← Cross-actor call
        ├── update_step_status("completed") / ("failed")            ← Graph write
        └── emit_plan_widget() → real-time update                  ← Transport notification
  │
  └── 6. IF all steps completed:
        ├── update_plan_status("completed")                        ← Graph write
        └── chat_notify("✅ Plan completed")
```

### 5.3 Reject / Pause / Retry / Skip

| Action | Handler | What Happens |
|--------|---------|-------------|
| Reject | `RejectPlan` | Plan status → `rejected`, chat notification |
| Pause | `PausePlan` | Plan status → `paused`, next step won't execute |
| Resume | `ResumePlan` | Calls `approve_plan()` to resume execution |
| Retry | `RetryStep` | Step status → `pending`, calls `approve_plan()` |
| Skip | `SkipStep` | Step status → `skipped`, calls `approve_plan()` |

**Resource breakdown (approve):**
- N Graph reads (steps for plan, dependencies)
- N Graph writes (step status updates)
- N ToolOrchestrator calls (one per step)
- N Transport notifications (widget updates)
- 0 LLM calls (plan was already generated in Create step)

---

## 6. Summary of All Actor Dependencies

| Actor | Graph R/W | Tool Exec | LLM Calls | MCP RPC | Cross-Actor Msgs |
|-------|-----------|-----------|-----------|---------|------------------|
| BuildOrchestrator | ✅ Yes | ✅ Via ToolRouter | ❌ No | ❌ No | ErrorAnalyzer, ToolRouter, ToolOrchestrator, MemoryGraph |
| ErrorAnalyzer | ✅ Yes | ❌ No | ❌ No | ❌ No | MemoryGraph |
| ToolOrchestrator | ✅ Yes | ❌ Stub only | ❌ No | ❌ No | MemoryGraph |
| ProjectBuildActor | ✅ Yes (diagnostics) | ✅ Yes (MCP) | ❌ No | ✅ Yes | ProjectQuery, McpClient |
| ProjectQueryActor | ✅ Yes | ❌ No | ❌ No | ❌ No | MemoryGraph |
| PromptHandlerActor | ✅ Yes | ✅ Yes (via ToolRouter) | ✅ Yes | ✅ Indirectly | SystemPrompt, LLM, ToolRouter, MemoryGraph |
| LlmActor | ❌ No | ❌ No | ✅ Yes (external API) | ❌ No | None |
| SystemPromptActor | ❌ No (via ProjectQuery) | ❌ No | ❌ No | ❌ No | ProjectQuery |
| IntentRouterActor | ✅ Yes | ❌ No | ✅ Yes | ❌ No | MemoryGraph, LLM |
| CoordinatorActor | ❌ No | ❌ No | ❌ No | ❌ No | All actors |
| McpClientActor | ❌ No | ✅ Yes (MCP) | ❌ No | ✅ Yes | MCP server subprocesses |

**Key observations (updated):**
- ✅ `BuildOrchestrator` now **actually executes builds** via ToolRouter — no longer a skeleton
- ✅ **States are transitioned** — `build_completed` and `build_failed` set active=true/false
- ✅ **ErrorAnalyzer uses graph diagnostics** instead of vector search — linked to files via `HasDiagnostic` edges
- ⚠️ `ToolOrchestrator` still uses stubs — fix strategies cannot actually execute tools yet
- ⚠️ No cycle detection in the LLM tool call loop (only one iteration)
- ⚠️ Tool results still injected as `user` messages instead of `tool` role

### Resolved Gaps from Previous Version

| # | Gap | Status |
|---|-----|--------|
| 1 | States never activated | ✅ **Fixed** — `transition_state()` called on build/fix lifecycle |
| 2 | BuildOrchestrator doesn't run builds | ✅ **Fixed** — dispatches via ToolRouter to `project/build` |
| 3 | Two separate build paths | ✅ **Fixed** — BuildOrchestrator owns lifecycle, calls ProjectBuildActor |
| 4 | ToolOrchestrator is a stub | ⚠️ **Partially fixed** — API defined, MCP wiring still needed |
| 5 | No cycle detection in LLM loop | ❌ Still open |
| 6 | No streaming in tool path | ❌ Still open |
| 7 | Tool results use wrong role | ❌ Still open |
| 8 | No error recovery for tool failures | ❌ Still open |
| 9 | No intent confidence threshold | ❌ Still open |
| 10 | No intent fallback | ❌ Still open |
| 11 | Capability-based filtering not wired | ❌ Still open |
| 12 | Approval has no resume | ❌ Still open |
| 13 | No state lifecycle management | ✅ **Fixed** — transition_state() in BuildOrchestrator |