use anyhow::Result;
use serde::Serialize;
use tonari_actor::{Actor, Context};
use tracing::info;

/// Status of a progress update.
#[derive(Debug, Clone, Serialize)]
pub enum ProgressStatus {
    Running,
    Completed,
    Failed,
}

/// A progress update payload.
#[derive(Debug, Clone, Serialize)]
pub struct ProgressUpdate {
    pub task_id: String,
    pub message: String,
    pub percent: f64,
    pub status: ProgressStatus,
}

/// Messages for the progress broadcaster actor.
pub enum ProgressMessage {
    Publish(ProgressUpdate),
    Subscribe {
        reply_to: tokio::sync::oneshot::Sender<tokio::sync::broadcast::Receiver<ProgressUpdate>>,
    },
}

/// Progress broadcaster actor.
///
/// Broadcasts progress updates to the MCP client.
pub struct ProgressActor {
    tx: tokio::sync::broadcast::Sender<ProgressUpdate>,
}

impl ProgressActor {
    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(256);
        Self { tx }
    }
}

impl Actor for ProgressActor {
    type Message = ProgressMessage;
    type Error = anyhow::Error;
    type Context = Context<Self::Message>;

    fn handle(
        &mut self,
        _ctx: &mut Self::Context,
        msg: Self::Message,
    ) -> Result<(), Self::Error> {
        match msg {
            ProgressMessage::Publish(update) => {
                info!("Progress: {} - {} ({:.0}%)", update.task_id, update.message, update.percent);
                let _ = self.tx.send(update);
            }
            ProgressMessage::Subscribe { reply_to } => {
                let _ = reply_to.send(self.tx.subscribe());
            }
        }
        Ok(())
    }
}
