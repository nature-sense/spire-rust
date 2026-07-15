// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! ChatActor — manages chat dialogs and messages in-memory.
//!
//! This actor stores chat dialogs and messages, providing CRUD operations
//! for the chat system. It is the single source of truth for chat state.

use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;

use crate::actors::{Actor, ActorError};

/// A chat message.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessageData {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

/// A chat dialog.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatDialog {
    pub id: String,
    pub title: String,
    pub messages: Vec<ChatMessageData>,
    pub created_at: String,
    pub updated_at: String,
}

/// Messages for the Chat actor.
pub enum ChatMessage {
    /// Get the active chat dialog.
    GetActive {
        reply_to: tokio::sync::oneshot::Sender<Option<ChatDialog>>,
    },
    /// Get all chat dialogs.
    GetHistory {
        reply_to: tokio::sync::oneshot::Sender<Vec<ChatDialog>>,
    },
    /// Append a message to a chat dialog.
    Append {
        chat_id: String,
        content: String,
        role: String,
        reply_to: tokio::sync::oneshot::Sender<Result<ChatMessageData, ActorError>>,
    },
    /// Clear all messages in a chat dialog.
    Clear {
        chat_id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<(), ActorError>>,
    },
    /// Set the title of a chat dialog.
    SetTitle {
        chat_id: String,
        title: String,
        reply_to: tokio::sync::oneshot::Sender<Result<(), ActorError>>,
    },
}

/// Actor that manages chat dialogs and messages.
pub struct ChatActor {
    dialogs: HashMap<String, ChatDialog>,
    active_id: Option<String>,
}

impl ChatActor {
    pub fn new() -> Self {
        let mut dialogs = HashMap::new();
        let now = Utc::now().to_rfc3339();
        let default_id = "default".to_string();
        dialogs.insert(default_id.clone(), ChatDialog {
            id: default_id.clone(),
            title: "New Chat".to_string(),
            messages: vec![],
            created_at: now.clone(),
            updated_at: now,
        });
        Self {
            dialogs,
            active_id: Some(default_id),
        }
    }
}

impl Default for ChatActor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Actor for ChatActor {
    type Message = ChatMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            ChatMessage::GetActive { reply_to } => {
                let dialog = self.active_id.as_ref().and_then(|id| self.dialogs.get(id).cloned());
                let _ = reply_to.send(dialog);
            }
            ChatMessage::GetHistory { reply_to } => {
                let dialogs: Vec<ChatDialog> = self.dialogs.values().cloned().collect();
                let _ = reply_to.send(dialogs);
            }
            ChatMessage::Append {
                chat_id,
                content,
                role,
                reply_to,
            } => {
                let result = self.append_message(&chat_id, &content, &role);
                let _ = reply_to.send(result);
            }
            ChatMessage::Clear { chat_id, reply_to } => {
                if let Some(dialog) = self.dialogs.get_mut(&chat_id) {
                    dialog.messages.clear();
                    dialog.updated_at = Utc::now().to_rfc3339();
                    let _ = reply_to.send(Ok(()));
                } else {
                    let _ = reply_to.send(Err(ActorError::Internal(format!("Chat not found: {}", chat_id))));
                }
            }
            ChatMessage::SetTitle { chat_id, title, reply_to } => {
                if let Some(dialog) = self.dialogs.get_mut(&chat_id) {
                    dialog.title = title;
                    dialog.updated_at = Utc::now().to_rfc3339();
                    let _ = reply_to.send(Ok(()));
                } else {
                    let _ = reply_to.send(Err(ActorError::Internal(format!("Chat not found: {}", chat_id))));
                }
            }
        }
    }
}

impl ChatActor {
    fn append_message(
        &mut self,
        chat_id: &str,
        content: &str,
        role: &str,
    ) -> Result<ChatMessageData, ActorError> {
        let dialog = self.dialogs.get_mut(chat_id)
            .ok_or_else(|| ActorError::Internal(format!("Chat not found: {}", chat_id)))?;

        let message = ChatMessageData {
            id: format!("msg-{}", uuid::Uuid::new_v4()),
            role: role.to_string(),
            content: content.to_string(),
            timestamp: Utc::now().to_rfc3339(),
        };

        dialog.messages.push(message.clone());
        dialog.updated_at = Utc::now().to_rfc3339();

        Ok(message)
    }
}
