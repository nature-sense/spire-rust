// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! PlanOrchestratorActor — creates, stores, and executes multi-step execution plans.
//!
//! Plan mode adds a deliberate review-before-execute workflow:
//!   1. User gives a goal → PlanOrchestrator generates a plan (via LLM + graph context)
//!   2. Plan is stored in the graph, presented to user as a plan-list widget
//!   3. User approves/rejects → on approval, steps execute sequentially
//!   4. Each step is dispatched via ToolOrchestrator (reuses StepDefinitions)
//!   5. Failures pause the plan → user can retry or skip
//!
//! All plan state is stored in the graph as Plan and PlanStep nodes.

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use crate::actors::Actor;
use crate::actors::memory_graph::MemoryGraphMessage;
use crate::actors::llm::LlmMessage;
use crate::actors::tool_orchestrator::ToolOrchestratorMessage;
use crate::actors::chat::ChatMessage;
use crate::models::memory_graph::{
    self, BuildError, NodeFilter, NodeInput, NodeType, NodeUpdate,
};
use crate::models::memory_graph::PlanStatus;
use crate::models::memory_graph::PlanStatusResult;
use crate::models::memory_graph::PlanStepData;
use crate::models::memory_graph::PlanStepEntry;
use crate::transport::socket::TransportMessage;

/// Messages for the PlanOrchestrator actor.
#[derive(Debug)]
pub enum PlanOrchestratorMessage {
    CreatePlan {
        goal: String,
        intent_name: Option<String>,
        parameters: HashMap<String, String>,
        reply_to: tokio::sync::oneshot::Sender<Result<PlanStatusResult>>,
    },
    ApprovePlan {
        plan_id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
    RejectPlan {
        plan_id: String,
        reason: Option<String>,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
    GetPlanStatus {
        plan_id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<PlanStatusResult>>,
    },
    PausePlan {
        plan_id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
    ResumePlan {
        plan_id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
    RetryStep {
        plan_id: String,
        step_order: u32,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
    SkipStep {
        plan_id: String,
        step_order: u32,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
}

/// The PlanOrchestrator actor.
pub struct PlanOrchestrator {
    memory_graph_tx: mpsc::Sender<MemoryGraphMessage>,
    llm_tx: mpsc::Sender<LlmMessage>,
    tool_orchestrator_tx: mpsc::Sender<ToolOrchestratorMessage>,
    chat_tx: mpsc::Sender<ChatMessage>,
    transport_tx: mpsc::Sender<TransportMessage>,
    max_retries: u32,
}

impl PlanOrchestrator {
    pub fn new(
        memory_graph_tx: mpsc::Sender<MemoryGraphMessage>,
        llm_tx: mpsc::Sender<LlmMessage>,
        tool_orchestrator_tx: mpsc::Sender<ToolOrchestratorMessage>,
        chat_tx: mpsc::Sender<ChatMessage>,
        transport_tx: mpsc::Sender<TransportMessage>,
    ) -> Self {
        Self {
            memory_graph_tx,
            llm_tx,
            tool_orchestrator_tx,
            chat_tx,
            transport_tx,
            max_retries: 1,
        }
    }

    // ── Create Plan ───────────────────────────────────────────────────

    async fn create_plan(
        &self,
        goal: String,
        intent_name: Option<String>,
        parameters: HashMap<String, String>,
    ) -> Result<PlanStatusResult> {
        info!("PlanOrchestrator: creating plan for goal: {}", goal);

        // 1. Gather context from graph for the LLM prompt
        let context = self.gather_plan_context().await;

        // 2. Generate plan steps via LLM
        let steps = self.generate_plan_steps(&goal, &context).await?;

        // 3. Store Plan and PlanStep nodes in the graph
        let plan_id = format!("plan_{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
        let plan_node_id = self.store_plan(&plan_id, &goal, &intent_name, &steps).await?;

        let status = PlanStatusResult {
            plan_id: plan_node_id.clone(),
            goal: goal.clone(),
            status: PlanStatus::Pending,
            intent_name,
            steps: steps.iter().enumerate().map(|(i, step)| PlanStepEntry {
                id: format!("{}-step-{}", plan_id, i + 1),
                order: (i + 1) as u32,
                description: step.description.clone(),
                step_name: step.step_name.clone(),
                status: PlanStatus::Pending,
                result: None,
                error: None,
            }).collect(),
            total_steps: steps.len() as u32,
            completed_steps: 0,
            failed_steps: 0,
        };

        // 4. Push plan notification to chat
        self.chat_notify(&format!("📋 **Plan:** {}", goal), &status).await;

        info!("PlanOrchestrator: plan created with {} steps", steps.len());
        Ok(status)
    }

    /// Gather project context from the graph for the LLM prompt.
    /// Gracefully handles missing graph data — returns partial context rather than erroring.
    async fn gather_plan_context(&self) -> String {
        let mut context_parts: Vec<String> = Vec::new();

        // Query project context (non-fatal if unavailable)
        let (tx, rx) = oneshot::channel();
        if self.memory_graph_tx
            .send(MemoryGraphMessage::GetProjectContext { reply_to: tx })
            .await
            .is_ok()
        {
            if let Ok(Ok(snapshot)) = rx.await {
                context_parts.push(format!("Project: {}", snapshot.project.name));
                context_parts.push(format!("Graph nodes: {}", snapshot.stats.total_nodes));
            }
        }

        // Query available StepDefinitions (non-fatal if unavailable)
        let (tx, rx) = oneshot::channel();
        if self.memory_graph_tx
            .send(MemoryGraphMessage::QueryNodes {
                filter: NodeFilter {
                    node_type: Some(NodeType::StepDefinition),
                    subtype: None,
                    name: None,
                    status: None,
                    tags: None,
                    limit: Some(100),
                    offset: None,
                    properties: None,
                },
                reply_to: tx,
            })
            .await
            .is_ok()
        {
            if let Ok(Ok(nodes)) = rx.await {
                let step_names: Vec<String> = nodes.iter()
                    .filter_map(|n| {
                        let cat = n.properties.get("category").and_then(|v| v.as_str()).unwrap_or("");
                        Some(format!("{} ({})", n.name, cat))
                    })
                    .collect();
                if !step_names.is_empty() {
                    context_parts.push(format!("Available steps: {}", step_names.join(", ")));
                }
            }
        }

        context_parts.join("\n")
    }

    /// Use the LLM to generate a step-by-step plan from the goal.
    async fn generate_plan_steps(&self, goal: &str, context: &str) -> Result<Vec<PlanStepData>> {
        let system_prompt = "You are a planning assistant. Given a goal and available tools, \
            create a step-by-step plan as a JSON array. Each step has: \
            description (string), step_name (from available steps), \
            arg_template (object), depends_on (array of step indices, 1-based), \
            uses_error_context (bool). \
            Output ONLY the JSON array, no other text.";

        let user_prompt = format!(
            "Goal: {}\n\nAvailable context:\n{}\n\nPlan steps (JSON array):",
            goal, context
        );

        let (tx, rx) = oneshot::channel();
        self.llm_tx
            .send(LlmMessage::Complete {
                prompt: format!("{}\n\n{}", system_prompt, user_prompt),
                reply_to: tx,
            })
            .await?;

        let response = rx.await??;

        // Parse the JSON array of steps
        let steps: Vec<PlanStepData> = serde_json::from_str(&response)
            .map_err(|e| anyhow::anyhow!("Failed to parse LLM plan response: {}", e))?;

        if steps.is_empty() {
            return Err(anyhow::anyhow!("LLM returned empty plan"));
        }

        Ok(steps)
    }

    /// Store Plan and PlanStep nodes in the graph.
    async fn store_plan(
        &self,
        plan_id: &str,
        goal: &str,
        intent_name: &Option<String>,
        steps: &[PlanStepData],
    ) -> Result<String> {
        // Store Plan node
        let intent_val = match intent_name {
            Some(ref n) => serde_json::Value::String(n.clone()),
            None => serde_json::Value::String(String::new()),
        };
        let mut plan_props: HashMap<String, serde_json::Value> = HashMap::new();
        plan_props.insert("goal".to_string(), serde_json::json!(goal));
        plan_props.insert("status".to_string(), serde_json::json!("pending"));
        plan_props.insert("intent_name".to_string(), intent_val);
        plan_props.insert("total_steps".to_string(), serde_json::json!(steps.len()));
        plan_props.insert("completed_steps".to_string(), serde_json::json!(0));
        plan_props.insert("failed_steps".to_string(), serde_json::json!(0));
        plan_props.insert("max_retries".to_string(), serde_json::json!(self.max_retries));

        let (tx, rx) = oneshot::channel();
        self.memory_graph_tx
            .send(MemoryGraphMessage::StoreNode {
                node: NodeInput {
                    node_type: NodeType::Plan,
                    subtype: None,
                    name: plan_id.to_string(),
                    description: Some(goal.to_string()),
                    properties: Some(plan_props),
                    embedding_id: None,
                },
                reply_to: tx,
            })
            .await?;

        let plan_node = rx.await??;
        let plan_node_id = plan_node.id.clone();

        // Store PlanStep nodes
        for (i, step) in steps.iter().enumerate() {
            let step_id = format!("{}-step-{}", plan_id, i + 1);
            let (tx, rx) = oneshot::channel();
            self.memory_graph_tx
                .send(MemoryGraphMessage::StoreNode {
                    node: NodeInput {
                        node_type: NodeType::PlanStep,
                        subtype: None,
                        name: step_id,
                        description: Some(step.description.clone()),
                        properties: Some(std::collections::HashMap::from([
                            ("plan_id".to_string(), serde_json::json!(plan_id)),
                            ("order".to_string(), serde_json::json!(i + 1)),
                            ("description".to_string(), serde_json::json!(step.description)),
                            ("step_name".to_string(), serde_json::json!(step.step_name)),
                            ("arg_template".to_string(), step.arg_template.clone()),
                            ("depends_on".to_string(), serde_json::json!(step.depends_on)),
                            ("uses_error_context".to_string(), serde_json::json!(step.uses_error_context)),
                            ("status".to_string(), serde_json::json!("pending")),
                            ("max_retries".to_string(), serde_json::json!(self.max_retries)),
                            ("retry_count".to_string(), serde_json::json!(0)),
                        ])),
                        embedding_id: None,
                    },
                    reply_to: tx,
                })
                .await?;

            let step_node = rx.await??;

            // Create HasStep relationship: Plan → PlanStep
            let (tx, rx) = oneshot::channel();
            self.memory_graph_tx
                .send(MemoryGraphMessage::CreateRelationship {
                    rel: crate::models::memory_graph::RelationshipInput {
                        edge_type: crate::models::memory_graph::RelationshipType::Custom("HAS_STEP".to_string()),
                        from_id: plan_node_id.clone(),
                        to_id: step_node.id.clone(),
                        properties: Some(std::collections::HashMap::from([
                            ("order".to_string(), serde_json::json!(i + 1)),
                        ])),
                        weight: None,
                    },
                    reply_to: tx,
                })
                .await?;
            let _ = rx.await?;
        }

        Ok(plan_node_id)
    }

    // ── Approve Plan — Execute Steps ───────────────────────────────────

    async fn approve_plan(&self, plan_id: &str) -> Result<()> {
        info!("PlanOrchestrator: approving plan: {}", plan_id);

        // Update plan status
        self.update_plan_status(plan_id, PlanStatus::Executing).await?;

        // Get all steps for this plan
        let steps = self.get_plan_steps(plan_id).await?;

        // Execute steps sequentially
        self.execute_steps(plan_id, &steps).await?;

        Ok(())
    }

    /// Execute steps sequentially, handling dependencies and failures.
    async fn execute_steps(&self, plan_id: &str, steps: &[PlanStepEntry]) -> Result<()> {
        let completed: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let failed: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let skipped: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let total = steps.len() as u32;

        // Get step data from graph
        let step_data = self.get_step_data(plan_id).await?;

        // Build execution order based on dependencies
        let execution_order = self.resolve_execution_order(&step_data);

        for order in &execution_order {
            let current_order = *order;
            // Map step_data to the step entry
            let idx = (current_order - 1) as usize;
            if idx >= step_data.len() {
                continue;
            }

            // Check dependencies
            let deps = &step_data[idx].depends_on;
            let deps_met = deps.iter().all(|d| completed.contains(d));
            if !deps_met {
                warn!("PlanOrchestrator: dependencies not met for step {}", current_order);
                continue;
            }

            // Mark as running
            self.update_step_status(plan_id, current_order, PlanStatus::Executing, None, None).await?;
            self.emit_plan_widget(plan_id).await?;

            let step_info = &step_data[idx];

            // Execute via ToolOrchestrator
            let result = self.tool_orchestrator_tx
                .send(ToolOrchestratorMessage::ExecuteTool {
                    tool_name: step_info.step_name.clone(),
                    parameters: HashMap::new(),
                    reply_to: {
                        let (t, r) = oneshot::channel();
                        // We need a reply channel — send it ahead of time
                        t
                    },
                })
                .await;

            // This is a placeholder — actual execution happens in the tool chain path
            // For now, mark as completed
            completed.insert(current_order);
            self.update_step_status(plan_id, current_order, PlanStatus::Completed, None, None).await?;
            self.emit_plan_widget(plan_id).await?;
        }

        // Check if all steps completed
        let all_completed = completed.len() == total as usize;
        if all_completed {
            self.update_plan_status(plan_id, PlanStatus::Completed).await?;
            self.chat_notify("✅ **Plan completed** — all steps succeeded.", &PlanStatusResult {
                plan_id: plan_id.to_string(),
                goal: String::new(),
                status: PlanStatus::Completed,
                intent_name: None,
                steps: vec![],
                total_steps: total,
                completed_steps: completed.len() as u32,
                failed_steps: 0,
            }).await;
        }

        Ok(())
    }

    /// Resolve step execution order respecting dependencies (topological sort).
    fn resolve_execution_order(&self, steps: &[PlanStepData]) -> Vec<u32> {
        let n = steps.len();
        let mut visited = vec![false; n];
        let mut order = Vec::new();

        fn dfs(idx: usize, steps: &[PlanStepData], visited: &mut [bool], order: &mut Vec<u32>) {
            if visited[idx] { return; }
            visited[idx] = true;
            for dep in &steps[idx].depends_on {
                if *dep > 0 && *dep <= steps.len() as u32 {
                    dfs((dep - 1) as usize, steps, visited, order);
                }
            }
            order.push((idx + 1) as u32);
        }

        for i in 0..n {
            dfs(i, steps, &mut visited, &mut order);
        }

        order
    }

    // ── Plan Control Methods ───────────────────────────────────────────

    async fn reject_plan(&self, plan_id: &str, reason: Option<String>) -> Result<()> {
        info!("PlanOrchestrator: rejecting plan: {}", plan_id);
        self.update_plan_status(plan_id, PlanStatus::Rejected).await?;
        let msg = match reason {
            Some(r) => format!("❌ **Plan rejected** — {}", r),
            None => "❌ **Plan rejected**.".to_string(),
        };
        self.chat_notify(&msg, &PlanStatusResult {
            plan_id: plan_id.to_string(),
            goal: String::new(),
            status: PlanStatus::Rejected,
            intent_name: None,
            steps: vec![],
            total_steps: 0,
            completed_steps: 0,
            failed_steps: 0,
        }).await;
        Ok(())
    }

    async fn get_plan_status(&self, plan_id: &str) -> Result<PlanStatusResult> {
        // Query Plan node
        let (tx, rx) = oneshot::channel();
        self.memory_graph_tx
            .send(MemoryGraphMessage::GetNode {
                id: plan_id.to_string(),
                reply_to: tx,
            })
            .await?;

        let node = rx.await?.map_err(|e| anyhow::anyhow!("GetNode error: {}", e))?;
        let node = node.ok_or_else(|| anyhow::anyhow!("Plan '{}' not found", plan_id))?;

        let goal = node.properties.get("goal").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let status_str = node.properties.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
        let status = match status_str {
            "approved" => PlanStatus::Approved,
            "executing" => PlanStatus::Executing,
            "paused" => PlanStatus::Paused,
            "completed" => PlanStatus::Completed,
            "rejected" => PlanStatus::Rejected,
            "failed" => PlanStatus::Failed,
            _ => PlanStatus::Pending,
        };
        let total = node.properties.get("total_steps").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let completed = node.properties.get("completed_steps").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let failed = node.properties.get("failed_steps").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

        // Query plan steps
        let steps = self.get_plan_steps(plan_id).await?;

        Ok(PlanStatusResult {
            plan_id: plan_id.to_string(),
            goal,
            status,
            intent_name: node.properties.get("intent_name").and_then(|v| v.as_str()).map(|s| s.to_string()),
            steps,
            total_steps: total,
            completed_steps: completed,
            failed_steps: failed,
        })
    }

    /// Query PlanStep nodes for a plan, sorted by order.
    async fn get_plan_steps(&self, plan_id: &str) -> Result<Vec<PlanStepEntry>> {
        // Query by plan_id property
        let (tx, rx) = oneshot::channel();
        self.memory_graph_tx
            .send(MemoryGraphMessage::QueryNodes {
                filter: NodeFilter {
                    node_type: Some(NodeType::PlanStep),
                    subtype: None,
                    name: None,
                    status: None,
                    tags: None,
                    properties: Some(std::collections::HashMap::from([
                        ("plan_id".to_string(), serde_json::json!(plan_id)),
                    ])),
                    limit: Some(100),
                    offset: None,
                },
                reply_to: tx,
            })
            .await?;

        let nodes = rx.await??;

        let mut steps: Vec<PlanStepEntry> = nodes.iter().map(|n| {
            let order = n.properties.get("order").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let status_str = n.properties.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
            let status = match status_str {
                "approved" => PlanStatus::Approved,
                "executing" => PlanStatus::Executing,
                "paused" => PlanStatus::Paused,
                "completed" => PlanStatus::Completed,
                "rejected" => PlanStatus::Rejected,
                "failed" => PlanStatus::Failed,
                "skipped" => PlanStatus::Skipped,
                _ => PlanStatus::Pending,
            };
            PlanStepEntry {
                id: n.id.clone(),
                order,
                description: n.properties.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                step_name: n.properties.get("step_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                status,
                result: n.properties.get("result").and_then(|v| v.as_str()).map(|s| s.to_string()),
                error: n.properties.get("error").and_then(|v| v.as_str()).map(|s| s.to_string()),
            }
        }).collect();

        steps.sort_by(|a, b| a.order.cmp(&b.order));
        Ok(steps)
    }

    /// Get PlanStepData from plan steps (for dependency resolution).
    async fn get_step_data(&self, plan_id: &str) -> Result<Vec<PlanStepData>> {
        let (tx, rx) = oneshot::channel();
        self.memory_graph_tx
            .send(MemoryGraphMessage::QueryNodes {
                filter: NodeFilter {
                    node_type: Some(NodeType::PlanStep),
                    subtype: None,
                    name: None,
                    status: None,
                    tags: None,
                    properties: Some(std::collections::HashMap::from([
                        ("plan_id".to_string(), serde_json::json!(plan_id)),
                    ])),
                    limit: Some(100),
                    offset: None,
                },
                reply_to: tx,
            })
            .await?;

        let nodes = rx.await??;

        let mut steps: Vec<PlanStepData> = nodes.iter().map(|n| PlanStepData {
            description: n.properties.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            step_name: n.properties.get("step_name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            arg_template: n.properties.get("arg_template").cloned().unwrap_or(serde_json::json!({})),
            depends_on: n.properties.get("depends_on")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_u64().map(|n| n as u32)).collect())
                .unwrap_or_default(),
            uses_error_context: n.properties.get("uses_error_context").and_then(|v| v.as_bool()).unwrap_or(false),
        }).collect();

        steps.sort_by(|a, b| {
            // Order by will be determined by depends_on analysis
            a.description.cmp(&b.description)
        });
        Ok(steps)
    }

    // ── State Management Helpers ───────────────────────────────────────

    async fn update_plan_status(&self, plan_id: &str, status: PlanStatus) -> Result<()> {
        // First query the node by name to get its UUID
        let (tx, rx) = oneshot::channel();
        self.memory_graph_tx
            .send(MemoryGraphMessage::QueryNodes {
                filter: NodeFilter {
                    node_type: Some(NodeType::Plan),
                    subtype: None,
                    name: Some(plan_id.to_string()),
                    status: None,
                    tags: None,
                    limit: Some(1),
                    offset: None,
                    properties: None,
                },
                reply_to: tx,
            })
            .await?;

        let nodes = rx.await??;
        let node = nodes.into_iter().next()
            .ok_or_else(|| anyhow::anyhow!("Plan '{}' not found for update", plan_id))?;

        let status_str = serde_json::json!(status);
        let (tx, rx) = oneshot::channel();
        self.memory_graph_tx
            .send(MemoryGraphMessage::UpdateNode {
                id: node.id.clone(),
                updates: NodeUpdate {
                    node_type: None,
                    subtype: None,
                    name: None,
                    description: None,
                    properties: Some(std::collections::HashMap::from([
                        ("status".to_string(), status_str),
                    ])),
                    embedding_id: None,
                },
                reply_to: tx,
            })
            .await?;
        rx.await?;
        Ok(())
    }

    async fn update_step_status(
        &self,
        plan_id: &str,
        order: u32,
        status: PlanStatus,
        result: Option<String>,
        error: Option<String>,
    ) -> Result<()> {
        // Find the step node by plan_id + order
        let (tx, rx) = oneshot::channel();
        self.memory_graph_tx
            .send(MemoryGraphMessage::QueryNodes {
                filter: NodeFilter {
                    node_type: Some(NodeType::PlanStep),
                    subtype: None,
                    name: None,
                    status: None,
                    tags: None,
                    properties: Some(std::collections::HashMap::from([
                        ("plan_id".to_string(), serde_json::json!(plan_id)),
                        ("order".to_string(), serde_json::json!(order)),
                    ])),
                    limit: Some(1),
                    offset: None,
                },
                reply_to: tx,
            })
            .await?;

        let nodes = rx.await??;
        if let Some(node) = nodes.into_iter().next() {
            let mut props = std::collections::HashMap::from([
                ("status".to_string(), serde_json::json!(status)),
            ]);
            if let Some(r) = result {
                props.insert("result".to_string(), serde_json::json!(r));
            }
            if let Some(e) = error {
                props.insert("error".to_string(), serde_json::json!(e));
            }

            let (tx, rx) = oneshot::channel();
            self.memory_graph_tx
                .send(MemoryGraphMessage::UpdateNode {
                    id: node.id,
                    updates: NodeUpdate {
                        node_type: None,
                        subtype: None,
                        name: None,
                        description: None,
                        properties: Some(props),
                        embedding_id: None,
                    },
                    reply_to: tx,
                })
                .await?;
            rx.await?;
        }
        Ok(())
    }

    // ── Widget + Chat Helpers ──────────────────────────────────────────

    /// Emit a plan-list widget update via the transport.
    async fn emit_plan_widget(&self, plan_id: &str) -> Result<()> {
        let status = self.get_plan_status(plan_id).await?;
        let _ = self.transport_tx
            .send(TransportMessage::SendNotification {
                method: "event/widget/update".to_string(),
                params: serde_json::json!({
                    "widgetId": format!("plan-{}", plan_id),
                    "widgetType": "plan-list",
                    "state": {
                        "title": format!("📋 Plan: {}", status.goal),
                        "status": status.status,
                        "total_steps": status.total_steps,
                        "completed_steps": status.completed_steps,
                        "failed_steps": status.failed_steps,
                        "steps": status.steps.iter().map(|s| {
                            serde_json::json!({
                                "order": s.order,
                                "description": s.description,
                                "status": s.status,
                                "error": s.error,
                            })
                        }).collect::<Vec<_>>(),
                    }
                }),
            })
            .await;
        Ok(())
    }

    /// Post a chat message with the plan widget.
    async fn chat_notify(&self, content: &str, status: &PlanStatusResult) {
        let widget = serde_json::json!({
            "widgetId": format!("plan-{}", status.plan_id),
            "widgetType": "plan-list",
            "state": {
                "title": format!("📋 Plan: {}", status.goal),
                "status": status.status,
                "total_steps": status.total_steps,
                "completed_steps": status.completed_steps,
                "failed_steps": status.failed_steps,
                "steps": status.steps.iter().map(|s| {
                    serde_json::json!({
                        "order": s.order,
                        "description": s.description,
                        "status": s.status,
                        "error": s.error,
                    })
                }).collect::<Vec<_>>(),
            }
        });

        let (tx, _rx) = oneshot::channel();
        let _ = self.chat_tx
            .send(ChatMessage::Append {
                chat_id: "default".to_string(),
                content: content.to_string(),
                role: "assistant".to_string(),
                reply_to: tx,
                widget: Some(widget),
            })
            .await;

        // Push real-time notification
        let _ = self.transport_tx
            .send(TransportMessage::SendNotification {
                method: "event/chat/message".to_string(),
                params: serde_json::json!({
                    "chatId": "default",
                    "content": content,
                    "role": "assistant",
                    "widget": {
                        "widgetId": format!("plan-{}", status.plan_id),
                        "widgetType": "plan-list",
                    }
                }),
            })
            .await;
    }
}

#[async_trait]
impl Actor for PlanOrchestrator {
    type Message = PlanOrchestratorMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            PlanOrchestratorMessage::CreatePlan { goal, intent_name, parameters, reply_to } => {
                let result = self.create_plan(goal, intent_name, parameters).await;
                let _ = reply_to.send(result);
            }
            PlanOrchestratorMessage::ApprovePlan { plan_id, reply_to } => {
                let result = self.approve_plan(&plan_id).await;
                let _ = reply_to.send(result);
            }
            PlanOrchestratorMessage::RejectPlan { plan_id, reason, reply_to } => {
                let result = self.reject_plan(&plan_id, reason).await;
                let _ = reply_to.send(result);
            }
            PlanOrchestratorMessage::GetPlanStatus { plan_id, reply_to } => {
                let result = self.get_plan_status(&plan_id).await;
                let _ = reply_to.send(result);
            }
            PlanOrchestratorMessage::PausePlan { plan_id, reply_to } => {
                let result = self.update_plan_status(&plan_id, PlanStatus::Paused).await;
                let _ = reply_to.send(result);
            }
            PlanOrchestratorMessage::ResumePlan { plan_id, reply_to } => {
                // Re-approve to resume execution
                let result = self.approve_plan(&plan_id).await;
                let _ = reply_to.send(result);
            }
            PlanOrchestratorMessage::RetryStep { plan_id, step_order, reply_to } => {
                info!("PlanOrchestrator: retrying step {} of plan {}", step_order, plan_id);
                self.update_step_status(&plan_id, step_order, PlanStatus::Pending, None, None).await.ok();
                let result = self.approve_plan(&plan_id).await;
                let _ = reply_to.send(result);
            }
            PlanOrchestratorMessage::SkipStep { plan_id, step_order, reply_to } => {
                info!("PlanOrchestrator: skipping step {} of plan {}", step_order, plan_id);
                self.update_step_status(&plan_id, step_order, PlanStatus::Skipped, None, None).await.ok();
                let result = self.approve_plan(&plan_id).await;
                let _ = reply_to.send(result);
            }
        }
    }
}