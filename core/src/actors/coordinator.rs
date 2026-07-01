use anyhow::Result;
use tonari_actor::{Actor, Addr, Context};
use tracing::info;

use crate::actors::llm::LlmMessage;
use crate::actors::memory_graph::MemoryGraphMessage;
use crate::actors::progress::ProgressMessage;
use crate::models::analysis::{CodeAnalysis, CodeAnalysisRequest};

/// Messages for the coordinator actor.
pub enum CoordinatorMessage {
    ExplainCode {
        code: String,
        language: String,
        reply_to: tokio::sync::oneshot::Sender<Result<String>>,
    },
    SearchCodebase {
        query: String,
        max_results: usize,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<(String, f64)>>>,
    },
    AnalyzeCode {
        request: CodeAnalysisRequest,
        reply_to: tokio::sync::oneshot::Sender<Result<CodeAnalysis>>,
    },
}

/// The main workflow orchestrator.
///
/// Receives user requests and coordinates the other actors
/// (memory_graph, LLM, progress) to fulfill them.
pub struct CoordinatorActor {
    memory_graph_addr: Addr<MemoryGraphMessage>,
    llm_addr: Addr<LlmMessage>,
    progress_addr: Addr<ProgressMessage>,
}

impl CoordinatorActor {
    pub fn new(
        memory_graph_addr: Addr<MemoryGraphMessage>,
        llm_addr: Addr<LlmMessage>,
        progress_addr: Addr<ProgressMessage>,
    ) -> Self {
        Self {
            memory_graph_addr,
            llm_addr,
            progress_addr,
        }
    }
}

impl Actor for CoordinatorActor {
    type Message = CoordinatorMessage;
    type Error = anyhow::Error;
    type Context = Context<Self::Message>;

    fn handle(
        &mut self,
        _ctx: &mut Self::Context,
        msg: Self::Message,
    ) -> Result<(), Self::Error> {
        match msg {
            CoordinatorMessage::ExplainCode {
                code,
                language,
                reply_to,
            } => {
                info!("Coordinator: explain_code ({} chars, {})", code.len(), language);

                // Publish progress
                let _ = self.progress_addr.send(ProgressMessage::Publish(
                    crate::actors::progress::ProgressUpdate {
                        task_id: "explain".to_string(),
                        message: "Analyzing code...".to_string(),
                        percent: 30.0,
                        status: crate::actors::progress::ProgressStatus::Running,
                    },
                ));

                // Forward to LLM actor
                let llm_addr = self.llm_addr.clone();
                let progress_addr = self.progress_addr.clone();
                tokio::spawn(async move {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = llm_addr.send(LlmMessage::Complete {
                        prompt: format!("Explain this {} code:\n```\n{}\n```", language, code),
                        reply_to: tx,
                    });
                    match rx.await {
                        Ok(Ok(response)) => {
                            let _ = progress_addr.send(ProgressMessage::Publish(
                                crate::actors::progress::ProgressUpdate {
                                    task_id: "explain".to_string(),
                                    message: "Explanation complete".to_string(),
                                    percent: 100.0,
                                    status: crate::actors::progress::ProgressStatus::Completed,
                                },
                            ));
                            let _ = reply_to.send(Ok(response));
                        }
                        Ok(Err(e)) => {
                            let _ = reply_to.send(Err(e));
                        }
                        Err(e) => {
                            let _ = reply_to.send(Err(anyhow::anyhow!("Actor error: {}", e)));
                        }
                    }
                });
            }
            CoordinatorMessage::SearchCodebase {
                query,
                max_results,
                reply_to,
            } => {
                info!("Coordinator: search_codebase ({})", query);

                let memory_graph_addr = self.memory_graph_addr.clone();
                tokio::spawn(async move {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let _ = memory_graph_addr.send(MemoryGraphMessage::SearchContext {
                        query,
                        options: Some(crate::models::memory_graph::SearchOptions {
                            top_k: Some(max_results),
                            threshold: Some(0.5),
                            node_types: None,
                            max_depth: None,
                            include_structural: Some(true),
                            recency_weight: None,
                        }),
                        reply_to: tx,
                    });
                    match rx.await {
                        Ok(Ok(result)) => {
                            let items: Vec<(String, f64)> = result
                                .nodes
                                .into_iter()
                                .map(|sn| (sn.node.name, sn.score))
                                .collect();
                            let _ = reply_to.send(Ok(items));
                        }
                        Ok(Err(e)) => {
                            let _ = reply_to.send(Err(e));
                        }
                        Err(e) => {
                            let _ = reply_to.send(Err(anyhow::anyhow!("Actor error: {}", e)));
                        }
                    }
                });
            }
            CoordinatorMessage::AnalyzeCode {
                request,
                reply_to,
            } => {
                info!("Coordinator: analyze_code ({})", request.language);

                // Stub analysis
                let analysis = CodeAnalysis {
                    summary: format!("Analysis of {} code ({} chars)", request.language, request.code.len()),
                    complexity: None,
                    symbols: vec![],
                    suggestions: vec![],
                };
                let _ = reply_to.send(Ok(analysis));
            }
        }
        Ok(())
    }
}
