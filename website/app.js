// ============================================================
// Spire Rust Documentation — Application
// ============================================================

// ── Actor Catalog Data ──
const ACTORS = [
  {
    id: "coordinator",
    name: "CoordinatorActor",
    file: "rust/spire-core/src/actors/coordinator.rs",
    category: "core",
    purpose: "Central message router and workflow orchestrator. The CoordinatorActor receives all incoming JSON-RPC requests from the transport layer and dispatches them to the appropriate sub-actor by matching the method name against registered handlers. It holds mpsc::Sender channels to 15 different actors — chat, tools, MCP client, LLM, progress, system, memory graph, project query, transport, tool router, system prompt, intent detector, error analyzer, build orchestrator, and tool orchestrator. Tool calls are further routed via the ToolRouterActor, which uses prefix matching to determine the correct backend (extension tools → VS Code, project tools → ProjectQueryActor, catch-all → MCP servers). The coordinator also handles the Shutdown message, gracefully stopping all sub-actors in the correct order.",
    state: "Holds mpsc::Sender for: chat, tools, mcp_client, llm, progress, system, memory_graph, project_query, transport, tool_router, system_prompt, intent_detector, error_analyzer, build_orchestrator, tool_orchestrator",
    messages: [
      { variant: "HandleRequest", reply: "Value", fields: [
        { name: "method", type: "String" },
        { name: "params", type: "Value (serde_json)" },
        { name: "response_tx", type: "oneshot::Sender<Value>" }
      ]},
      { variant: "Shutdown", reply: "()", fields: [] }
    ],
    connections: [
      { actor: "ChatActor", msg: "ChatMessage", via: "direct mpsc channel" },
      { actor: "ToolsActor", msg: "ToolsMessage", via: "direct mpsc channel" },
      { actor: "McpClientActor", msg: "McpClientMessage", via: "direct mpsc channel" },
      { actor: "LlmActor", msg: "LlmMessage", via: "direct mpsc channel" },
      { actor: "ProgressActor", msg: "ProgressMessage", via: "direct mpsc channel" },
      { actor: "SystemActor", msg: "SystemMessage", via: "direct mpsc channel" },
      { actor: "MemoryGraphActor", msg: "MemoryGraphMessage", via: "direct mpsc channel" },
      { actor: "ProjectQueryActor", msg: "ProjectQueryMessage", via: "direct mpsc channel" },
      { actor: "TransportActor", msg: "TransportMessage", via: "direct mpsc channel" },
      { actor: "ToolRouterActor", msg: "ToolRouterMessage", via: "direct mpsc channel" },
      { actor: "SystemPromptActor", msg: "SystemPromptMessage", via: "direct mpsc channel" },
      { actor: "IntentDetector", msg: "IntentDetectorMessage", via: "direct mpsc channel" },
      { actor: "ErrorAnalyzer", msg: "ErrorAnalyzerMessage", via: "direct mpsc channel" },
      { actor: "BuildOrchestrator", msg: "BuildOrchestratorMessage", via: "direct mpsc channel" },
      { actor: "ToolOrchestrator", msg: "ToolOrchestratorMessage", via: "direct mpsc channel" }
    ],
    init: "CoordinatorActor::new(chat_tx, tools_tx, mcp_client_tx, llm_tx, progress_tx, system_tx, memory_graph_tx, project_query_tx, transport_tx, tool_router_tx, system_prompt_tx, intent_detector_tx, error_analyzer_tx, build_orchestrator_tx, tool_orchestrator_tx)"
  },
  {
    id: "chat",
    name: "ChatActor",
    file: "rust/spire-core/src/actors/chat.rs",
    category: "core",
    purpose: "In-memory chat dialog store and message history manager. The ChatActor is the single source of truth for all chat state in the system. It stores dialogs as a HashMap<String, Dialog> where each dialog contains a title, creation timestamp, and ordered list of messages. It supports 6 CRUD operations: GetActive (returns the currently active dialog), Create (creates a new dialog with optional title), SendMessage (appends a message with role and content to a dialog), GetHistory (retrieves all messages for a dialog), ListDialogs (returns summaries of all dialogs), and DeleteDialog (removes a dialog by ID). Each message is timestamped with chrono::Utc::now() and stored with a unique ID. The actor is stateless beyond its in-memory HashMap — no persistence layer is used.",
    state: "HashMap<String, Dialog> — dialog ID → dialog state",
    messages: [
      { variant: "GetActive", reply: "Option<Dialog>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Option<Dialog>>" }
      ]},
      { variant: "Create", reply: "Dialog", fields: [
        { name: "title", type: "Option<String>" },
        { name: "reply_to", type: "oneshot::Sender<Dialog>" }
      ]},
      { variant: "SendMessage", reply: "Result<Message, ActorError>", fields: [
        { name: "dialog_id", type: "String" },
        { name: "content", type: "String" },
        { name: "role", type: "String" },
        { name: "reply_to", type: "oneshot::Sender<Result<Message, ActorError>>" }
      ]},
      { variant: "GetHistory", reply: "Option<Vec<Message>>", fields: [
        { name: "dialog_id", type: "String" },
        { name: "reply_to", type: "oneshot::Sender<Option<Vec<Message>>>" }
      ]},
      { variant: "ListDialogs", reply: "Vec<DialogSummary>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Vec<DialogSummary>>" }
      ]},
      { variant: "DeleteDialog", reply: "bool", fields: [
        { name: "dialog_id", type: "String" },
        { name: "reply_to", type: "oneshot::Sender<bool>" }
      ]}
    ],
    connections: [
      { actor: "CoordinatorActor", msg: "receives ChatMessage from", via: "mpsc channel" }
    ],
    init: "ChatActor::new()"
  },
  {
    id: "tools",
    name: "ToolsActor",
    file: "rust/spire-core/src/actors/tools.rs",
    category: "core",
    purpose: "Tool registration hub and execution dispatcher. The ToolsActor maintains a Vec<Box<dyn Tool>> of all registered tool implementations in the system. When the coordinator receives a ListTools request, it aggregates tool definitions from two sources: embedded tools (defined directly in Rust code) and VS Code extension tools (defined in vscode_tools.rs). For CallTool requests, it forwards the invocation to the ToolRouterActor, which uses prefix matching to determine the correct backend — extension tools (workspace/, document/, diagnostics/, git/, symbols/) go to the TransportActor for VS Code execution, project tools (project/) go to ProjectQueryActor, and all other tools go to the McpClientActor for MCP server dispatch. This two-layer architecture (ToolsActor → ToolRouterActor → backends) cleanly separates tool metadata management from routing logic.",
    state: "Vec<Box<dyn Tool>> — registered tool implementations",
    messages: [
      { variant: "ListTools", reply: "Vec<ToolInfo>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Vec<ToolInfo>>" }
      ]},
      { variant: "CallTool", reply: "Result<Value, ActorError>", fields: [
        { name: "tool", type: "String" },
        { name: "args", type: "Value" },
        { name: "reply_to", type: "oneshot::Sender<Result<Value, ActorError>>" }
      ]}
    ],
    connections: [
      { actor: "ToolRouterActor", msg: "forwards tool calls to", via: "ToolRouterMessage" },
      { actor: "CoordinatorActor", msg: "receives ToolsMessage from", via: "mpsc channel" }
    ],
    init: "ToolsActor::new(tool_router_tx)"
  },
  {
    id: "mcp-client",
    name: "McpClientActor",
    file: "rust/spire-core/src/actors/mcp_client.rs",
    category: "infra",
    purpose: "External MCP server connection manager and tool dispatch gateway. The McpClientActor owns the McpClientManager, which manages the full lifecycle of connections to external MCP servers — mcp-filesystem, mcp-git, mcp-search, mcp-process, mcp-terminal, mcp-cargo, and mcp-node. It supports loading server configurations from the graph database (replacing the old file-based config), connecting/disconnecting individual servers or all at once, listing available tools per server, and dispatching tool calls to the correct server. It also maintains a set of internal tools that appear under the pseudo-MCP server name \"spire\". The actor is the bridge between the actor system and the external MCP ecosystem — every tool call that reaches an MCP server flows through this actor's CallTool message, which translates actor messages into MCP JSON-RPC calls and returns structured CallToolResult responses.",
    state: "McpClientManager",
    messages: [
      { variant: "LoadConfig", reply: "Result<Option<PathBuf>, ActorError>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Result<Option<PathBuf>, ActorError>>" }
      ]},
      { variant: "ConnectAll", reply: "Result<(), ActorError>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Result<(), ActorError>>" }
      ]},
      { variant: "Connect", reply: "Result<(), ActorError>", fields: [
        { name: "server_name", type: "String" },
        { name: "reply_to", type: "oneshot::Sender<Result<(), ActorError>>" }
      ]},
      { variant: "DisconnectAll", reply: "Result<(), ActorError>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Result<(), ActorError>>" }
      ]},
      { variant: "Disconnect", reply: "Result<(), ActorError>", fields: [
        { name: "server_name", type: "String" },
        { name: "reply_to", type: "oneshot::Sender<Result<(), ActorError>>" }
      ]},
      { variant: "GetTools", reply: "Option<Vec<Tool>>", fields: [
        { name: "server_name", type: "String" },
        { name: "reply_to", type: "oneshot::Sender<Option<Vec<Tool>>>" }
      ]},
      { variant: "ConnectedServers", reply: "Vec<String>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Vec<String>>" }
      ]},
      { variant: "CallTool", reply: "Result<CallToolResult, ActorError>", fields: [
        { name: "server_name", type: "String" },
        { name: "tool_name", type: "String" },
        { name: "arguments", type: "Option<Map<String, Value>>" },
        { name: "reply_to", type: "oneshot::Sender<Result<CallToolResult, ActorError>>" }
      ]}
    ],
    connections: [
      { actor: "External MCP Servers", msg: "manages connections to", via: "mcp-git, mcp-search, mcp-process, mcp-terminal, mcp-filesystem, mcp-cargo, mcp-node" },
      { actor: "CoordinatorActor", msg: "receives McpClientMessage from", via: "mpsc channel" },
      { actor: "ToolRouterActor", msg: "routes MCP tool calls to", via: "McpClientMessage::CallTool" },
      { actor: "ProjectBuildActor", msg: "dispatches build tools to", via: "McpClientMessage::CallTool" }
    ],
    init: "McpClientActor::new()"
  },
  {
    id: "llm",
    name: "LlmActor",
    file: "rust/spire-core/src/actors/llm.rs",
    category: "core",
    purpose: "LLM inference gateway for the actor system. The LlmActor is designed as the single point of contact for all LLM interactions — both completion requests and streaming responses. Currently it operates as a stub that echoes back the input prompt, serving as a placeholder for the future DeepSeek provider integration. It holds an LlmConfig that will eventually contain model selection, temperature, max tokens, and API endpoint configuration. The actor supports two message variants: Complete (returns the full response as a single String) and Stream (returns a tokio::sync::mpsc::Receiver<String> for real-time token-by-token streaming). Once wired to DeepSeek, this actor will handle prompt caching, context window management, and retry logic for API failures.",
    state: "Stateless (LlmConfig)",
    messages: [
      { variant: "Complete", reply: "Result<String, ActorError>", fields: [
        { name: "prompt", type: "String" },
        { name: "reply_to", type: "oneshot::Sender<Result<String, ActorError>>" }
      ]},
      { variant: "Stream", reply: "Result<Receiver<String>, ActorError>", fields: [
        { name: "prompt", type: "String" },
        { name: "reply_to", type: "oneshot::Sender<Result<Receiver<String>, ActorError>>" }
      ]}
    ],
    connections: [
      { actor: "CoordinatorActor", msg: "receives LlmMessage from", via: "mpsc channel" }
    ],
    init: "LlmActor::new(LlmConfig::default())"
  },
  {
    id: "progress",
    name: "ProgressActor",
    file: "rust/spire-core/src/actors/progress.rs",
    category: "infra",
    purpose: "Progress event broadcast hub for long-running operations. The ProgressActor uses a tokio::sync::broadcast channel (buffer size 256) to fan out progress updates to multiple subscribers simultaneously. Each ProgressUpdate contains a task_id, human-readable message, completion percentage (0.0–100.0), and a status enum (Running, Completed, Failed). The SystemActor publishes startup phase progress through this actor during initialization, and other actors use it for long-running operations like project sync and analysis. The TransportActor subscribes to progress updates and forwards them as JSON-RPC notifications to the VS Code extension, enabling real-time progress bars in the UI. The broadcast channel design means any number of subscribers can listen without affecting the publisher.",
    state: "broadcast::Sender<ProgressUpdate> (buffer: 256)",
    messages: [
      { variant: "Publish", reply: "fire-and-forget", fields: [
        { name: "update", type: "ProgressUpdate" }
      ]},
      { variant: "Subscribe", reply: "Result<Receiver<ProgressUpdate>, ActorError>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Result<Receiver<ProgressUpdate>, ActorError>>" }
      ]}
    ],
    connections: [
      { actor: "CoordinatorActor", msg: "receives ProgressMessage from", via: "mpsc channel" },
      { actor: "SystemActor", msg: "publishes startup progress to", via: "ProgressMessage::Publish" },
      { actor: "TransportActor", msg: "forwards progress as JSON-RPC notifications", via: "tokio::spawn task" }
    ],
    init: "ProgressActor::new()"
  },
  {
    id: "system",
    name: "SystemActor",
    file: "rust/spire-core/src/actors/system.rs",
    category: "infra",
    purpose: "System lifecycle state machine and startup orchestrator. The SystemActor owns the entire system lifecycle as a state machine, driving the startup sequence through 4 modular phases defined in startup_phases.rs: GraphInitPhase (initializes SeleneDB), ParallelInitPhase (starts embedder download, project sync, project analysis concurrently), ParallelConnectPhase (connects all MCP servers), and RegisterToolsPhase (registers all tools with the coordinator). Unlike the previous monolithic run_initialize() approach, the SystemActor now holds a current_phase and delegates incoming messages to it, keeping the mailbox responsive during long-running startup operations. Background work like embedder model download and project analysis runs concurrently via the phase system — they don't block the fast path to Ready. The actor also handles Shutdown (graceful teardown of all sub-actors) and Health (returns system status as JSON).",
    state: "Phase state machine (StartupPhase trait implementations)",
    messages: [
      { variant: "Shutdown", reply: "Result<(), ActorError>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Result<(), ActorError>>" }
      ]},
      { variant: "Health", reply: "Value", fields: [
        { name: "reply_to", type: "oneshot::Sender<Value>" }
      ]},
      { variant: "Initialize", reply: "Result<(), ActorError>", fields: [
        { name: "coordinator_tx", type: "mpsc::Sender<CoordinatorMessage>" },
        { name: "memory_graph_tx", type: "mpsc::Sender<MemoryGraphMessage>" },
        { name: "mcp_client_tx", type: "mpsc::Sender<McpClientMessage>" },
        { name: "project_sync_tx", type: "mpsc::Sender<ProjectSyncMessage>" },
        { name: "project_analyzer_tx", type: "mpsc::Sender<ProjectAnalyzerMessage>" },
        { name: "project_query_tx", type: "mpsc::Sender<ProjectQueryMessage>" },
        { name: "llm_tx", type: "mpsc::Sender<LlmMessage>" },
        { name: "progress_tx", type: "mpsc::Sender<ProgressMessage>" },
        { name: "embedder", type: "Arc<dyn Embedder>" },
        { name: "data_dir", type: "PathBuf" },
        { name: "project_root", type: "PathBuf" },
        { name: "reply_to", type: "oneshot::Sender<Result<(), ActorError>>" }
      ]},
      { variant: "PhaseEvent", reply: "internal", fields: [
        { name: "phase", type: "String" },
        { name: "result", type: "PhaseResult" }
      ]},
      { variant: "SetSystemTx", reply: "internal", fields: [
        { name: "system_tx", type: "mpsc::Sender<SystemMessage>" }
      ]}
    ],
    connections: [
      { actor: "CoordinatorActor", msg: "receives SystemMessage from", via: "mpsc channel" },
      { actor: "MemoryGraphActor", msg: "initializes graph during startup", via: "MemoryGraphMessage" },
      { actor: "McpClientActor", msg: "connects MCP servers during startup", via: "McpClientMessage" },
      { actor: "ProjectSyncActor", msg: "triggers project sync during startup", via: "ProjectSyncMessage" },
      { actor: "ProjectAnalyzerActor", msg: "triggers analysis during startup", via: "ProjectAnalyzerMessage" },
      { actor: "ProgressActor", msg: "publishes startup progress to", via: "ProgressMessage::Publish" }
    ],
    init: "SystemActor::new()"
  },
  {
    id: "memory-graph",
    name: "MemoryGraphActor",
    file: "rust/spire-core/src/actors/memory_graph.rs",
    category: "graph",
    purpose: "Central knowledge graph database — the single source of truth for all persistent state. The MemoryGraphActor is backed by SeleneDB's GraphDb, a labeled property graph engine with u64 node/edge IDs, string labels, key-value properties, and vector indexes for semantic search. It handles 16+ operations including CRUD for nodes and edges, graph traversal, context-aware search (using CandleEmbedder for vector similarity), memory storage and recall (for LLM conversation memory), and streaming transactions via OpenTransactionStream for atomic multi-step writes. The actor maintains a bidirectional UUID↔u64 ID mapping so the external API can use UUID-based String IDs while SeleneDB uses compact u64 IDs internally. All data access is through GQL statements via execute_gql_write and execute_gql_query — no low-level SharedGraph API is used. This actor is the backbone of the entire system: project sync writes structure here, intent detector and error analyzer query patterns here, build orchestrator tracks build state here, and tool orchestrator reads tool configurations here.",
    state: "GraphDb (SeleneDB) — labeled property graph with u64 IDs, string labels, key-value properties, and vector indexes",
    messages: [
      { variant: "GetNode", reply: "Result<Option<GraphNode>>", fields: [
        { name: "id", type: "String" },
        { name: "reply_to", type: "oneshot::Sender<Result<Option<GraphNode>>>" }
      ]},
      { variant: "QueryNodes", reply: "Result<Vec<GraphNode>>", fields: [
        { name: "filter", type: "NodeFilter" },
        { name: "reply_to", type: "oneshot::Sender<Result<Vec<GraphNode>>>" }
      ]},
      { variant: "StoreNode", reply: "Result<GraphNode>", fields: [
        { name: "node", type: "NodeInput" },
        { name: "reply_to", type: "oneshot::Sender<Result<GraphNode>>" }
      ]},
      { variant: "UpdateNode", reply: "Result<GraphNode>", fields: [
        { name: "id", type: "String" },
        { name: "updates", type: "NodeUpdate" },
        { name: "reply_to", type: "oneshot::Sender<Result<GraphNode>>" }
      ]},
      { variant: "DeleteNode", reply: "Result<()>", fields: [
        { name: "id", type: "String" },
        { name: "reply_to", type: "oneshot::Sender<Result<()>>" }
      ]},
      { variant: "CreateRelationship", reply: "Result<GraphEdge>", fields: [
        { name: "rel", type: "RelationshipInput" },
        { name: "reply_to", type: "oneshot::Sender<Result<GraphEdge>>" }
      ]},
      { variant: "GetRelationships", reply: "Result<Vec<GraphEdge>>", fields: [
        { name: "node_id", type: "String" },
        { name: "reply_to", type: "oneshot::Sender<Result<Vec<GraphEdge>>>" }
      ]},
      { variant: "DeleteRelationship", reply: "Result<()>", fields: [
        { name: "id", type: "String" },
        { name: "reply_to", type: "oneshot::Sender<Result<()>>" }
      ]},
      { variant: "Traverse", reply: "Result<TraversalResult>", fields: [
        { name: "start_node_id", type: "String" },
        { name: "options", type: "TraversalOptions" },
        { name: "reply_to", type: "oneshot::Sender<Result<TraversalResult>>" }
      ]},
      { variant: "GetProjectContext", reply: "Result<ProjectSnapshot>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Result<ProjectSnapshot>>" }
      ]},
      { variant: "SearchContext", reply: "Result<ContextSearchResult>", fields: [
        { name: "query", type: "String" },
        { name: "options", type: "Option<SearchOptions>" },
        { name: "reply_to", type: "oneshot::Sender<Result<ContextSearchResult>>" }
      ]},
      { variant: "AddMemory", reply: "Result<MemoryEntry>", fields: [
        { name: "text", type: "String" },
        { name: "metadata", type: "Option<MemoryMetadata>" },
        { name: "reply_to", type: "oneshot::Sender<Result<MemoryEntry>>" }
      ]},
      { variant: "Recall", reply: "Result<Vec<MemoryEntry>>", fields: [
        { name: "query", type: "String" },
        { name: "limit", type: "Option<usize>" },
        { name: "reply_to", type: "oneshot::Sender<Result<Vec<MemoryEntry>>>" }
      ]},
      { variant: "Sync", reply: "Result<()>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Result<()>>" }
      ]},
      { variant: "OpenTransactionStream", reply: "Result<mpsc::Receiver<StreamOpResult>>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Result<mpsc::Receiver<StreamOpResult>>>" }
      ]},
      { variant: "ExecuteGql", reply: "Result<StatementOutput>", fields: [
        { name: "gql", type: "String" },
        { name: "reply_to", type: "oneshot::Sender<Result<StatementOutput>>" }
      ]}
    ],
    connections: [
      { actor: "CoordinatorActor", msg: "receives MemoryGraphMessage from", via: "mpsc channel" },
      { actor: "ProjectSyncActor", msg: "writes project structure to", via: "MemoryGraphMessage" },
      { actor: "IntentDetector", msg: "queries graph for intent patterns", via: "MemoryGraphMessage" },
      { actor: "ErrorAnalyzer", msg: "queries graph for error patterns", via: "MemoryGraphMessage" },
      { actor: "BuildOrchestrator", msg: "reads/writes build state to", via: "MemoryGraphMessage" },
      { actor: "ToolOrchestrator", msg: "reads tool config from", via: "MemoryGraphMessage" },
      { actor: "SystemActor", msg: "initializes graph during startup", via: "MemoryGraphMessage" }
    ],
    init: "MemoryGraphActor::new()"
  },
  {
    id: "project-sync",
    name: "ProjectSyncActor",
    file: "rust/spire-core/src/actors/project_sync.rs",
    category: "graph",
    purpose: "Filesystem-to-graph synchronisation engine with three distinct phases. The ProjectSyncActor keeps the knowledge graph in perfect sync with the actual filesystem state using a content-hash manifest approach (Sha256 hashes of file contents). Phase 1 — Bootstrap: cold start where no Project node exists, performs a full filesystem scan and creates all graph nodes. Phase 2 — Startup sync: warm start where a Project node exists, uses content-hash manifest diffing to detect only changed files. Phase 3 — Continuous sync: real-time file change events from the VS Code file watcher, processing individual file additions, modifications, and deletions. All multi-step write operations (bootstrap, force-resync) use the OpenTransactionStream API to group all graph mutations into a single SeleneDB transaction — if any operation fails, the entire transaction is rolled back with no orphan nodes or dangling relationships. The analyzer module provides semantic classification for each file (role, language, build system) so every graph node is enriched with full metadata.",
    state: "HashMap<String, String> — file path → content hash manifest",
    messages: [
      { variant: "Bootstrap", reply: "Result<SyncResult>", fields: [
        { name: "project_root", type: "PathBuf" },
        { name: "reply_to", type: "oneshot::Sender<Result<SyncResult>>" }
      ]},
      { variant: "StartupSync", reply: "Result<SyncResult>", fields: [
        { name: "project_root", type: "PathBuf" },
        { name: "reply_to", type: "oneshot::Sender<Result<SyncResult>>" }
      ]},
      { variant: "SyncChanges", reply: "Result<SyncResult>", fields: [
        { name: "changes", type: "Vec<FileChange>" },
        { name: "reply_to", type: "oneshot::Sender<Result<SyncResult>>" }
      ]},
      { variant: "ForceResync", reply: "Result<SyncResult>", fields: [
        { name: "project_root", type: "PathBuf" },
        { name: "reply_to", type: "oneshot::Sender<Result<SyncResult>>" }
      ]}
    ],
    connections: [
      { actor: "MemoryGraphActor", msg: "writes project structure to", via: "MemoryGraphMessage" },
      { actor: "SystemActor", msg: "triggered during startup phases", via: "SystemMessage::Initialize" }
    ],
    init: "ProjectSyncActor::new()"
  },
  {
    id: "project-analyzer",
    name: "ProjectAnalyzerActor",
    file: "rust/spire-core/src/actors/project_analyzer.rs",
    category: "graph",
    purpose: "MCP-delegated semantic project analyzer for LLM consumption. The ProjectAnalyzerActor produces a rich, structured representation of a project's directory structure, build systems, languages, and architecture — designed to give an LLM deep understanding without reading every file. It operates through a pure actor-based design: it scans the filesystem for build config files (Cargo.toml, package.json, CMakeLists.txt, etc.), discovers which MCP servers can analyze each build file by calling describe_analysis_capabilities on each connected MCP server, delegates the actual analysis to the matching MCP server via its analyze tool, and falls back to the local analyzer module for file tree, language detection, and architecture summary. No direct function calls to build parsers — all build analysis flows through the actor message system via McpClientMessage::CallTool.",
    state: "Stateless",
    messages: [
      { variant: "Analyze", reply: "Result<ProjectAnalysis>", fields: [
        { name: "project_root", type: "PathBuf" },
        { name: "reply_to", type: "oneshot::Sender<Result<ProjectAnalysis>>" }
      ]},
      { variant: "GetAnalysis", reply: "Result<Option<ProjectAnalysis>>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Result<Option<ProjectAnalysis>>>" }
      ]}
    ],
    connections: [
      { actor: "McpClientActor", msg: "delegates build analysis to", via: "McpClientMessage::CallTool" },
      { actor: "SystemActor", msg: "triggered during startup phases", via: "SystemMessage::Initialize" }
    ],
    init: "ProjectAnalyzerActor::new()"
  },
  {
    id: "project-query",
    name: "ProjectQueryActor",
    file: "rust/spire-core/src/actors/project_query.rs",
    category: "graph",
    purpose: "LLM-facing semantic project query engine with 12 rich tools. The ProjectQueryActor provides a comprehensive set of tools that sit on top of the knowledge graph, giving the LLM deep semantic understanding of the project without needing to read raw files. Tools include: getOverview (high-level project summary), getFileTree (directory/file tree with semantic annotations), getFileDetails (detailed metadata about a specific file), searchFiles (search by name, language, role, or pattern), getBuildConfig (parsed build configuration), getDependencies (dependency graph — external + internal), getEntryPoints (main entry points), getArchitecture (high-level architectural overview), getEntities (functions, classes, types), getRelationships (relationships between project elements), queryGraph (flexible graph query), and getChanges (recent file changes since last sync). Each tool translates the LLM's request into MemoryGraphMessage queries against SeleneDB, then formats the results into LLM-friendly JSON responses. The actor is initialized with a memory_graph_tx sender and project_root path.",
    state: "Holds memory_graph_tx sender + project_root",
    messages: [
      { variant: "Initialize", reply: "Result<()>", fields: [
        { name: "memory_graph_tx", type: "mpsc::Sender<MemoryGraphMessage>" },
        { name: "project_root", type: "PathBuf" },
        { name: "reply_to", type: "oneshot::Sender<anyhow::Result<()>>" }
      ]},
      { variant: "CallTool", reply: "Value", fields: [
        { name: "tool", type: "String" },
        { name: "args", type: "Value" },
        { name: "reply_to", type: "oneshot::Sender<Value>" }
      ]},
      { variant: "ListTools", reply: "Vec<ToolInfo>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Vec<ToolInfo>>" }
      ]}
    ],
    connections: [
      { actor: "MemoryGraphActor", msg: "queries graph for project data", via: "MemoryGraphMessage" },
      { actor: "ToolRouterActor", msg: "routes project/ tools to", via: "ProjectQueryMessage::CallTool" },
      { actor: "ProjectBuildActor", msg: "queries build config from", via: "ProjectQueryMessage::CallTool" },
      { actor: "SystemPromptActor", msg: "fetches project overview for", via: "ProjectQueryMessage::CallTool" }
    ],
    init: "ProjectQueryActor::new()"
  },
  {
    id: "project-build",
    name: "ProjectBuildActor",
    file: "rust/spire-core/src/actors/project_build.rs",
    category: "infra",
    purpose: "Multi-system parallel build orchestrator. The ProjectBuildActor provides the project/build meta-tool that discovers all build systems in a project by calling project/getBuildConfig, then dispatches the appropriate build tool for each system in parallel. For Cargo projects, it calls mcp-cargo/build; for npm/pnpm/yarn projects, it calls mcp-node/build. Results from all build systems are aggregated into a single structured JSON response with per-system status, output, and error information. The actor holds senders to both ProjectQueryActor (for build config discovery) and McpClientActor (for dispatching build commands to MCP servers). Future support is planned for meson, cmake, gradle, make, and maven build systems.",
    state: "Holds project_query_tx + mcp_client_tx",
    messages: [
      { variant: "ListTools", reply: "Vec<ToolInfo>", fields: [
        { name: "reply_to", type: "oneshot::Sender<Vec<ToolInfo>>" }
      ]},
      { variant: "CallTool", reply: "Result<Value, String>", fields: [
        { name: "tool", type: "String" },
        { name: "args", type: "Value" },
        { name: "reply_to", type: "oneshot::Sender<Result<Value, String>>" }
      ]}
    ],
    connections: [
      { actor: "ProjectQueryActor", msg: "queries build config from", via: "ProjectQueryMessage::CallTool" },
      { actor: "McpClientActor", msg: "dispatches build commands to", via: "McpClientMessage::CallTool" }
    ],
    init: "ProjectBuildActor::new(project_query_tx, mcp_client_tx)"
  }
];

// ── Mermaid Architecture Diagram ──
const ARCHITECTURE_DIAGRAM = `graph TB
    subgraph Transport["Transport Layer"]
        TE[TransportActor<br/><i>transport/socket.rs</i>]
    end

    subgraph Core["Core Actors"]
        CA[CoordinatorActor<br/><i>actors/coordinator.rs</i>]
        CH[ChatActor<br/><i>actors/chat.rs</i>]
        TA[ToolsActor<br/><i>actors/tools.rs</i>]
        LA[LlmActor<br/><i>actors/llm.rs</i>]
    end

    subgraph Graph["Knowledge Graph"]
        MG[MemoryGraphActor<br/><i>actors/memory_graph.rs</i>]
        PS[ProjectSyncActor<br/><i>actors/project_sync.rs</i>]
        PA[ProjectAnalyzerActor<br/><i>actors/project_analyzer.rs</i>]
        PQ[ProjectQueryActor<br/><i>actors/project_query.rs</i>]
    end

    subgraph BuildFix["Build-Fix Loop"]
        BO[BuildOrchestrator<br/><i>actors/build_orchestrator.rs</i>]
        ID[IntentDetector<br/><i>actors/intent_detector.rs</i>]
        EA[ErrorAnalyzer<br/><i>actors/error_analyzer.rs</i>]
        TO[ToolOrchestrator<br/><i>actors/tool_orchestrator.rs</i>]
    end

    subgraph Infrastructure["Infrastructure"]
        SA[SystemActor<br/><i>actors/system.rs</i>]
        PR[ProgressActor<br/><i>actors/progress.rs</i>]
        MC[McpClientActor<br/><i>actors/mcp_client.rs</i>]
        TR[ToolRouterActor<br/><i>actors/tool_providers/mod.rs</i>]
        SP[SystemPromptActor<br/><i>actors/system_prompt.rs</i>]
        PB[ProjectBuildActor<br/><i>actors/project_build.rs</i>]
        VT[VSCodeToolsActor<br/><i>actors/vscode_tools.rs</i>]
    end

    subgraph External["External"]
        VSC[VS Code Extension<br/><i>JSON-RPC over TCP</i>]
        MCP_SERVERS[MCP Servers<br/><i>cargo, node, filesystem, git, search, process, terminal</i>]
    end

    %% Transport connections
    VSC -- "JSON-RPC 2.0" --> TE
    TE -- "HandleRequest" --> CA

    %% Core connections
    CA --> CH
    CA --> TA
    CA --> LA
    CA --> SA
    CA --> PR
    CA --> MC
    CA --> MG
    CA --> PQ
    CA --> TR
    CA --> SP
    CA --> ID
    CA --> EA
    CA --> BO
    CA --> TO

    %% Tool routing
    TA --> TR
    TR -- "extension tools" --> TE
    TR -- "project/ tools" --> PQ
    TR -- "project/build" --> PB
    TR -- "catch-all" --> MC
    MC --> MCP_SERVERS
    PB --> PQ
    PB --> MC

    %% Graph connections
    PS --> MG
    PA --> MC
    PQ --> MG
    SP --> PQ

    %% Build-fix loop
    BO --> ID
    BO --> EA
    BO --> TO
    ID --> MG
    EA --> MG
    TO --> MG

    %% System startup
    SA --> MG
    SA --> MC
    SA --> PS
    SA --> PA
    SA --> PR

    style TE fill:#1a1a2e,stroke:#e94560,color:#fff
    style CA fill:#16213e,stroke:#0f3460,color:#fff
    style MG fill:#1b4332,stroke:#40916c,color:#fff
    style BO fill:#4a3000,stroke:#d29922,color:#fff
    style SA fill:#3c096c,stroke:#9d4edd,color:#fff`;

// ============================================================
// Application State
// ============================================================

let currentRoute = "home";
let currentRouteParam = null;

// ============================================================
// Navigation
// ============================================================

function navigate(route, param) {
  currentRoute = route;
  currentRouteParam = param || null;
  updateSidebarActive();
  render();
}

function updateSidebarActive() {
  document.querySelectorAll("#sidebar-nav a").forEach(a => a.classList.remove("active"));
  if (currentRoute === "home") {
    const homeLink = document.querySelector('.sidebar-header a');
    if (homeLink) homeLink.style.color = "var(--accent)";
  } else if (currentRoute === "actor") {
    const link = document.querySelector(`a[data-actor="${currentRouteParam}"]`);
    if (link) link.classList.add("active");
  } else if (currentRoute === "diagram") {
    const link = document.querySelector('a[onclick*="diagram"]');
    if (link) link.classList.add("active");
  }
}

// ============================================================
// Sidebar Population
// ============================================================

function populateSidebar() {
  const actorList = document.getElementById("actor-list");
  actorList.innerHTML = ACTORS.map(a =>
    `<li><a href="#" data-actor="${a.id}" onclick="navigate('actor', '${a.id}')">${a.name}</a></li>`
  ).join("");
}

// ============================================================
// Sidebar Filter
// ============================================================

function filterSidebar(query) {
  const q = query.toLowerCase().trim();
  document.querySelectorAll("#sidebar-nav .nav-section ul li").forEach(li => {
    const a = li.querySelector("a");
    if (!q || a.textContent.toLowerCase().includes(q)) {
      li.style.display = "";
    } else {
      li.style.display = "none";
    }
  });
}

// ============================================================
// Theme Toggle
// ============================================================

function toggleTheme() {
  const html = document.documentElement;
  const btn = document.getElementById("theme-toggle");
  if (html.getAttribute("data-theme") === "dark") {
    html.setAttribute("data-theme", "light");
    btn.textContent = "🌙 Dark";
  } else {
    html.setAttribute("data-theme", "dark");
    btn.textContent = "☀️ Light";
  }
}

// ============================================================
// Rendering
// ============================================================

function render() {
  const container = document.getElementById("content-inner");
  switch (currentRoute) {
    case "home": renderHome(container); break;
    case "actor": renderActor(container, currentRouteParam); break;
    case "diagram": renderDiagram(container); break;
    default: renderHome(container);
  }
}

// ── Home Page ──

function renderHome(container) {
  const categories = {
    core: { label: "Core Actors", icon: "⚙️" },
    graph: { label: "Knowledge Graph", icon: "🔗" },
    build: { label: "Build-Fix Loop", icon: "🔄" },
    infra: { label: "Infrastructure", icon: "🏗️" }
  };

  let cards = "";
  for (const [cat, info] of Object.entries(categories)) {
    const actors = ACTORS.filter(a => a.category === cat);
    cards += actors.map(a => `
      <div class="home-card" onclick="navigate('actor', '${a.id}')">
        <h3>${a.name}</h3>
        <p>${a.purpose}</p>
        <div class="actor-tags" style="margin-top:8px">
          <span class="actor-tag ${a.category}">${info.label}</span>
        </div>
      </div>
    `).join("");
  }

  container.innerHTML = `
    <div class="home-hero">
      <h1>Spire Rust</h1>
      <p>An AI-powered build-fix loop agent for multi-language projects. Built with the actor model in Rust.</p>
    </div>
    <h2>Actor Catalog</h2>
    <p style="color:var(--text-secondary);margin-bottom:16px">
      ${ACTORS.length} actors across 4 categories. Click any card for full details.
    </p>
    <div class="home-cards">${cards}</div>
  `;
}

// ── Actor Detail Page ──

function renderActor(container, actorId) {
  const actor = ACTORS.find(a => a.id === actorId);
  if (!actor) {
    container.innerHTML = "<h1>Actor not found</h1>";
    return;
  }

  // Messages table
  let msgRows = actor.messages.map(m => {
    let fieldsHtml = "";
    if (m.fields.length > 0) {
      fieldsHtml = "<table class='message-table'><thead><tr><th>Field</th><th>Type</th></tr></thead><tbody>";
      fieldsHtml += m.fields.map(f =>
        `<tr><td><span class="field-name">${f.name}</span></td><td><span class="field-type">${f.type}</span></td></tr>`
      ).join("");
      fieldsHtml += "</tbody></table>";
    } else {
      fieldsHtml = "<span style='color:var(--text-muted);font-size:0.85em'>—</span>";
    }
    return `<tr><td><span class="variant-name">${m.variant}</span></td><td><span class="field-type">${m.reply}</span></td><td>${fieldsHtml}</td></tr>`;
  }).join("");

  // Connections list
  let connHtml = actor.connections.map(c =>
    `<li><span class="conn-actor">${c.actor}</span> — ${c.msg} <span style="color:var(--text-muted)">(${c.via})</span></li>`
  ).join("");

  container.innerHTML = `
    <div class="actor-detail-header">
      <h1>${actor.name}</h1>
      <div class="actor-source">${actor.file}</div>
      <div style="margin-top:8px"><span class="actor-tag ${actor.category}">${actor.category}</span></div>
    </div>
    <div class="actor-detail-purpose">${actor.purpose}</div>

    <h2>State</h2>
    <div class="actor-detail-state">
      <pre>${actor.state}</pre>
    </div>

    <h2>Inbox (Messages Consumed)</h2>
    <table class="message-table">
      <thead><tr><th>Variant</th><th>Reply</th><th>Fields</th></tr></thead>
      <tbody>${msgRows}</tbody>
    </table>

    <h2>Connections</h2>
    <ul class="connections-list">${connHtml}</ul>

    <h2>Initialization</h2>
    <div class="actor-detail-state">
      <pre>${actor.init}</pre>
    </div>
  `;
}

// ── Architecture Diagram Page ──

function renderDiagram(container) {
  container.innerHTML = `
    <h1>Architecture Diagram</h1>
    <p style="color:var(--text-secondary);margin-bottom:16px">
      Actor system architecture showing message flow between all components.
      Color-coded by category: <span style="color:#0f3460">Core</span>,
      <span style="color:#40916c">Graph</span>,
      <span style="color:#d29922">Build-Fix</span>,
      <span style="color:#9d4edd">Infrastructure</span>.
    </p>
    <div class="mermaid-wrapper">
      <pre class="mermaid">${ARCHITECTURE_DIAGRAM}</pre>
    </div>
  `;
  mermaid.run({ nodes: [document.querySelector(".mermaid")] });
}

// ============================================================
// Init
// ============================================================

document.addEventListener("DOMContentLoaded", () => {
  populateSidebar();
  render();
});
