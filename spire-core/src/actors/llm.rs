// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! LlmActor — handles LLM completion and streaming requests.
//!
//! This actor sends HTTP requests to the DeepSeek API (or compatible endpoint)
//! for text completions. It supports both single-shot and streaming responses.

use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;

use crate::actors::{Actor, ActorError};

/// Messages for the LLM actor.
pub enum LlmMessage {
    /// Complete a prompt (single-shot, non-streaming).
    Complete {
        prompt: String,
        reply_to: tokio::sync::oneshot::Sender<Result<String, ActorError>>,
    },
    /// Stream a response (returns a receiver for chunks).
    Stream {
        prompt: String,
        reply_to: tokio::sync::oneshot::Sender<Result<tokio::sync::mpsc::Receiver<String>, ActorError>>,
    },
    /// Update the LLM configuration at runtime (e.g. API key, model, URL).
    UpdateConfig {
        config: LlmConfig,
        reply_to: tokio::sync::oneshot::Sender<Result<(), ActorError>>,
    },
}

/// Configuration for the LLM API client.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub api_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_url: std::env::var("DEEPSEEK_API_URL")
                .unwrap_or_else(|_| "https://api.deepseek.com/v1/chat/completions".to_string()),
            api_key: std::env::var("DEEPSEEK_API_KEY")
                .unwrap_or_else(|_| "".to_string()),
            model: std::env::var("LLM_MODEL")
                .unwrap_or_else(|_| "deepseek-chat".to_string()),
            max_tokens: 4096,
            temperature: 0.7,
        }
    }
}

/// Actor that manages LLM completions.
pub struct LlmActor {
    config: LlmConfig,
    client: reqwest::Client,
}

impl LlmActor {
    pub fn new(config: LlmConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to create HTTP client");
        Self { config, client }
    }

    /// Send a completion request to the LLM API.
    async fn complete(&self, prompt: &str) -> Result<String, ActorError> {
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "max_tokens": self.config.max_tokens,
            "temperature": self.config.temperature,
            "stream": false,
        });

        let response = self
            .client
            .post(&self.config.api_url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ActorError::Internal(format!("LLM request failed: {}", e)))?;

        let status = response.status();
        let json: Value = response
            .json()
            .await
            .map_err(|e| ActorError::Internal(format!("LLM response parse failed: {}", e)))?;

        if !status.is_success() {
            return Err(ActorError::Internal(format!(
                "LLM API error ({}): {}",
                status,
                json.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("unknown")
            )));
        }

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| ActorError::Internal("LLM response missing content".to_string()))?
            .to_string();

        Ok(content)
    }

    /// Send a streaming completion request to the LLM API.
    async fn stream_complete(
        &self,
        prompt: &str,
    ) -> Result<tokio::sync::mpsc::Receiver<String>, ActorError> {
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "max_tokens": self.config.max_tokens,
            "temperature": self.config.temperature,
            "stream": true,
        });

        let response = self
            .client
            .post(&self.config.api_url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ActorError::Internal(format!("LLM stream request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown".to_string());
            return Err(ActorError::Internal(format!(
                "LLM API error ({}): {}",
                status, text
            )));
        }

        let (tx, rx) = tokio::sync::mpsc::channel(256);

        tokio::spawn(async move {
            let mut stream = response.bytes_stream();
            use futures::StreamExt;
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        // Parse SSE format: "data: {...}\n\n"
                        for line in text.lines() {
                            if let Some(data) = line.strip_prefix("data: ") {
                                if data == "[DONE]" {
                                    return;
                                }
                                if let Ok(json) = serde_json::from_str::<Value>(data) {
                                    if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
                                        let _ = tx.send(content.to_string()).await;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("LLM stream error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}

#[async_trait]
impl Actor for LlmActor {
    type Message = LlmMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            LlmMessage::Complete { prompt, reply_to } => {
                let result = self.complete(&prompt).await;
                let _ = reply_to.send(result);
            }
            LlmMessage::Stream { prompt, reply_to } => {
                let result = self.stream_complete(&prompt).await;
                let _ = reply_to.send(result);
            }
            LlmMessage::UpdateConfig { config, reply_to } => {
                tracing::info!("LlmActor: updating config (model={}, url={})", config.model, config.api_url);
                self.config = config;
                let _ = reply_to.send(Ok(()));
            }
        }
    }
}
