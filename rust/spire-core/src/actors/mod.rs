// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Actor system — message-passing orchestration framework for spire-core.
//!
//! This module provides all actors for the standalone spire-core binary.
//! Communication with the VS Code extension is via JSON-RPC 2.0 over stdin/stdout.

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
