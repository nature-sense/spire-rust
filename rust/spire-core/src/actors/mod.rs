// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Actor system — message-passing orchestration framework for spire-core.
//!
//! This module provides all actors for the standalone spire-core binary.
//! Communication with the VS Code extension is via JSON-RPC 2.0 over TCP socket.

pub mod chat;
pub mod coordinator;
pub mod mcp_client;
pub mod tools;
pub mod vscode_tools;
pub mod progress;
pub mod llm;
pub mod system;
pub mod memory_graph;
pub mod project_sync;
pub mod project_analyzer;
pub mod startup_phases;
pub mod system_prompt;
pub mod tool_providers;

// Re-export from the framework module
pub use crate::framework::{Actor, ActorSystem, ActorError, ToolMessage, ToolInfo};

// Re-export actor types
pub use chat::{ChatActor, ChatMessage};
pub use coordinator::{CoordinatorActor, CoordinatorMessage};
pub use mcp_client::{McpClientActor, McpClientMessage};
pub use tools::{ToolsActor, ToolsMessage};
pub use vscode_tools::vscode_tool_definitions;
pub use progress::{ProgressActor, ProgressMessage, ProgressStatus, ProgressUpdate};
pub use llm::{LlmActor, LlmConfig, LlmMessage};
pub use system::{SystemActor, SystemMessage};
pub use memory_graph::{MemoryGraphActor, MemoryGraphMessage};
pub use project_sync::{ProjectSyncActor, ProjectSyncMessage, ChangeType, SyncResult};
pub use project_analyzer::{ProjectAnalyzerActor, ProjectAnalyzerMessage, ProjectAnalysis, LanguageBreakdown, RoleBreakdown};
pub mod project_query;
pub use project_query::{ProjectQueryActor, ProjectQueryMessage};
pub use system_prompt::{SystemPromptActor, SystemPromptMessage};

pub mod project_build;
pub use project_build::{ProjectBuildActor, ProjectBuildMessage};

pub mod project_test;
pub use project_test::{ProjectTestActor, ProjectTestMessage};

pub mod project_lint;
pub use project_lint::{ProjectLintActor, ProjectLintMessage};

pub mod project_install;
pub use project_install::{ProjectInstallActor, ProjectInstallMessage};

// Build-fix loop actors (all graph access via MemoryGraphMessage → GQL)
pub mod error_analyzer;
pub use error_analyzer::{ErrorAnalyzer, ErrorAnalyzerMessage};

pub mod build_orchestrator;
pub use build_orchestrator::{BuildOrchestrator, BuildOrchestratorMessage};

pub mod tool_orchestrator;
pub use tool_orchestrator::{ToolOrchestrator, ToolOrchestratorMessage};

// Plan mode actors
pub mod plan_orchestrator;
pub use plan_orchestrator::{PlanOrchestrator, PlanOrchestratorMessage};

// Re-export ToolRouterActor (replaces the old ToolDispatcher + ToolProvider pattern)
pub use tool_providers::{ToolRouterActor, ToolRouterMessage};

// Intent routing + prompt handling actors
pub mod intent_router;
pub use intent_router::{IntentRouterActor, IntentRouterMessage, RouteResult};

pub mod prompt_handler;
pub use prompt_handler::{PromptHandlerActor, PromptHandlerMessage, PromptContext};
