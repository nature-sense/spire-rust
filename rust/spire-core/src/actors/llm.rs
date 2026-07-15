// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! LlmActor — handles LLM completion and streaming requests.
//!
//! This actor sends HTTP requests to the DeepSeek API (or compatible endpoint)
//! for text completions. It supports both single-shot and streaming responses.
//!
//! # DeepSeek Workarounds
//!
//! DeepSeek sometimes returns tool calls in XML/Claude format (in `content`)
//! instead of the native JSON `tool_calls` field. This module implements two
//! workarounds:
//!
//! 1. **XML/Claude format parser** — detects `<｜DSML｜function_calls>` blocks
//!    in the response content and converts them to synthetic OpenAI-format
//!    `tool_calls` so the existing dispatch pipeline works unchanged.
//! 2. **Strict mode** — when enabled, uses the `/beta` API endpoint and sets
//!    `"strict": true` on each tool definition to enforce schema adherence.

use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

use crate::actors::{Actor, ActorError, ToolInfo};

/// Messages for the LLM actor.
pub enum LlmMessage {
    /// Complete a prompt (single-shot, non-streaming).
    Complete {
        prompt: String,
        reply_to: tokio::sync::oneshot::Sender<Result<String, ActorError>>,
    },
    /// Complete with a full messages array (system + history + user).
    CompleteWithMessages {
        messages: Vec<crate::actors::chat::ChatMessageData>,
        reply_to: tokio::sync::oneshot::Sender<Result<String, ActorError>>,
    },
    /// Complete with messages AND tool definitions (OpenAI-compatible tools array).
    CompleteWithTools {
        messages: Vec<crate::actors::chat::ChatMessageData>,
        tools: Vec<ToolInfo>,
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
    /// When true, use the DeepSeek beta API endpoint and set `strict: true`
    /// on each tool definition to enforce schema adherence.
    pub strict_mode: bool,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_url: "https://api.deepseek.com/v1/chat/completions".to_string(),
            api_key: String::new(),
            model: "deepseek-chat".to_string(),
            max_tokens: 4096,
            temperature: 0.7,
            strict_mode: false,
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
        tracing::info!("LLM sending prompt: {}", prompt);

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

        tracing::info!("LLM received response: {}", content);
        Ok(content)
    }

    /// Send a completion request with a full messages array (system + history + user).
    async fn complete_with_messages(
        &self,
        messages: &[crate::actors::chat::ChatMessageData],
    ) -> Result<String, ActorError> {
        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                })
            })
            .collect();

        tracing::info!("LLM sending {} messages", api_messages.len());
        if let Some(last) = messages.last() {
            tracing::info!("LLM last message (role={}): {}", last.role, last.content);
        }

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": api_messages,
            "max_tokens": self.config.max_tokens,
            "temperature": self.config.temperature,
            "stream": false,
        });

        tracing::info!("LLM request body: {}", serde_json::to_string_pretty(&body).unwrap_or_default());

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

        tracing::info!("LLM received response: {}", content);
        Ok(content)
    }

    /// Send a completion request with messages AND tool definitions.
    /// Uses the OpenAI-compatible `tools` array that DeepSeek supports.
    /// Returns the raw JSON response so the caller can inspect `tool_calls`.
    async fn complete_with_tools(
        &self,
        messages: &[crate::actors::chat::ChatMessageData],
        tools: &[ToolInfo],
    ) -> Result<String, ActorError> {
        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                })
            })
            .collect();

        // Sanitize tool names for the OpenAI/DeepSeek API:
        // The API requires function names to match `^[a-zA-Z0-9_-]+$`,
        // but our tool names use slashes (e.g. "workspace/getFolders").
        // We replace invalid characters with underscores and maintain
        // a reverse map so we can translate tool_calls back to original names.
        let mut name_map: HashMap<String, String> = HashMap::new();
        let api_tools: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                let sanitized: String = t.name
                    .chars()
                    .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
                    .collect();
                name_map.insert(sanitized.clone(), t.name.clone());

                let mut tool_def = serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": sanitized,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                });

                // When strict mode is enabled, add "strict": true to each tool
                // definition and use the beta API endpoint.
                if self.config.strict_mode {
                    if let Some(func) = tool_def.get_mut("function") {
                        if let Some(obj) = func.as_object_mut() {
                            obj.insert("strict".to_string(), serde_json::json!(true));
                        }
                    }
                }

                tool_def
            })
            .collect();

        tracing::info!(
            "LLM sending {} messages with {} tool definitions (strict_mode={})",
            api_messages.len(),
            api_tools.len(),
            self.config.strict_mode,
        );
        if let Some(last) = messages.last() {
            tracing::info!("LLM last message (role={}): {}", last.role, last.content);
        }

        // Determine the API URL — use beta endpoint when strict mode is enabled
        let api_url = if self.config.strict_mode {
            // Replace /v1/ with /beta/ in the URL path
            self.config.api_url.replace("/v1/", "/beta/")
        } else {
            self.config.api_url.clone()
        };

        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": api_messages,
            "max_tokens": self.config.max_tokens,
            "temperature": self.config.temperature,
            "stream": false,
        });

        if !api_tools.is_empty() {
            body["tools"] = serde_json::json!(api_tools);
        }

        tracing::info!("LLM request body: {}", serde_json::to_string_pretty(&body).unwrap_or_default());

        let response = self
            .client
            .post(&api_url)
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
            let err_msg = json
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");
            tracing::error!("LLM API error ({}): {}", status, err_msg);
            return Err(ActorError::Internal(format!(
                "LLM API error ({}): {}",
                status, err_msg
            )));
        }

        // ── Step 1: Check for native JSON tool_calls ──
        if let Some(tool_calls) = json["choices"][0]["message"]["tool_calls"].as_array() {
            if !tool_calls.is_empty() {
                return self.build_tool_calls_response(&json, &name_map);
            }
        }

        // ── Step 2: Check for XML/Claude-format tool calls in content ──
        if let Some(content) = json["choices"][0]["message"]["content"].as_str() {
            if let Some(xml_tool_calls) = Self::parse_xml_tool_calls(content) {
                tracing::info!(
                    "LLM returned {} XML-format tool call(s) in content, converting to synthetic tool_calls",
                    xml_tool_calls.len()
                );

                // Build a synthetic message with tool_calls
                let mut msg = json["choices"][0]["message"].clone();
                msg["tool_calls"] = serde_json::json!(xml_tool_calls);
                // Clear the content since we're treating this as a tool call response
                msg["content"] = serde_json::Value::Null;

                // Translate sanitized function names back to original names
                if let Some(calls) = msg["tool_calls"].as_array_mut() {
                    for tc in calls.iter_mut() {
                        if let Some(sanitized) = tc["function"]["name"].as_str() {
                            if let Some(original) = name_map.get(sanitized) {
                                tc["function"]["name"] = serde_json::json!(original);
                            }
                        }
                    }
                }

                let msg_with_tools = serde_json::to_string(&msg)
                    .map_err(|e| ActorError::Internal(format!("Failed to serialize synthetic tool_calls: {}", e)))?;
                return Ok(msg_with_tools);
            }
        }

        // ── Step 3: Normal text response ──
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| ActorError::Internal("LLM response missing content".to_string()))?
            .to_string();

        tracing::info!("LLM received response: {}", content);
        Ok(content)
    }

    /// Build a tool_calls response string from the JSON response, translating
    /// sanitized function names back to original names.
    fn build_tool_calls_response(
        &self,
        json: &Value,
        name_map: &HashMap<String, String>,
    ) -> Result<String, ActorError> {
        let mut msg = json["choices"][0]["message"].clone();
        if let Some(calls) = msg["tool_calls"].as_array_mut() {
            for tc in calls.iter_mut() {
                if let Some(sanitized) = tc["function"]["name"].as_str() {
                    if let Some(original) = name_map.get(sanitized) {
                        tc["function"]["name"] = serde_json::json!(original);
                    }
                }
            }
        }
        let msg_with_tools = serde_json::to_string(&msg)
            .map_err(|e| ActorError::Internal(format!("Failed to serialize tool_calls response: {}", e)))?;
        tracing::info!("LLM returned {} tool call(s)", msg["tool_calls"].as_array().map(|a| a.len()).unwrap_or(0));
        Ok(msg_with_tools)
    }

    /// Parse XML/Claude-format tool calls from a response content string.
    ///
    /// DeepSeek sometimes returns tool calls in this format instead of the
    /// native JSON `tool_calls` field:
    ///
    /// ```xml
    /// <｜DSML｜function_calls>
    ///   <｜DSML｜invoke name="get_weather">
    ///     <｜DSML｜parameter name="location" string="true">San Francisco</｜DSML｜parameter>
    ///   </｜DSML｜invoke>
    /// </｜DSML｜function_calls>
    /// ```
    ///
    /// Returns `None` if no XML tool calls are found.
    fn parse_xml_tool_calls(content: &str) -> Option<Vec<Value>> {
        // The full-width vertical line character (U+FF5C) used by DeepSeek
        // We also accept standard ASCII variants for robustness.
        let tag_prefix = "｜DSML｜";
        let tag_prefix_alt = "function_calls";

        // Check if the content contains function_calls markup
        if !content.contains(tag_prefix_alt) && !content.contains("function_calls") {
            return None;
        }

        // Build a regex that matches either the full-width or ASCII variant
        // Pattern: <(?:｜DSML｜)?function_calls> ... <(?:｜DSML｜)?invoke name="..."> ...
        let _re = Regex::new(
            r#"(?s)<(?:｜DSML｜)?function_calls\s*>.*?(?:<(?:｜DSML｜)?invoke\s+name\s*=\s*"([^"]+)">)"#
        ).ok()?;

        // Find all invoke blocks
        let mut tool_calls = Vec::new();
        let mut call_id_counter = 0u64;

        // Split on invoke tags to extract each tool call
        let invoke_re = Regex::new(
            r#"(?s)<(?:｜DSML｜)?invoke\s+name\s*=\s*"([^"]+)">(.*?)</(?:｜DSML｜)?invoke>"#
        ).ok()?;

        for cap in invoke_re.captures_iter(content) {
            let function_name = cap.get(1)?.as_str().to_string();
            let params_body = cap.get(2)?.as_str();

            // Parse parameters
            let param_re = Regex::new(
                r#"<(?:｜DSML｜)?parameter\s+name\s*=\s*"([^"]+)"(?:\s+string\s*=\s*"(true|false)")?\s*>(.*?)</(?:｜DSML｜)?parameter>"#
            ).ok()?;

            let mut args = serde_json::Map::new();
            for param_cap in param_re.captures_iter(params_body) {
                let param_name = param_cap.get(1)?.as_str().to_string();
                let param_value = param_cap.get(3)?.as_str().to_string();
                args.insert(param_name, serde_json::json!(param_value));
            }

            call_id_counter += 1;
            tool_calls.push(serde_json::json!({
                "id": format!("call_xml_{}", call_id_counter),
                "type": "function",
                "function": {
                    "name": function_name,
                    "arguments": serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string()),
                }
            }));
        }

        if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        }
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
            LlmMessage::CompleteWithMessages { messages, reply_to } => {
                let result = self.complete_with_messages(&messages).await;
                let _ = reply_to.send(result);
            }
            LlmMessage::CompleteWithTools { messages, tools, reply_to } => {
                let result = self.complete_with_tools(&messages, &tools).await;
                let _ = reply_to.send(result);
            }
            LlmMessage::Stream { prompt, reply_to } => {
                let result = self.stream_complete(&prompt).await;
                let _ = reply_to.send(result);
            }
            LlmMessage::UpdateConfig { config, reply_to } => {
                tracing::info!("LlmActor: updating config (model={}, url={}, strict_mode={})", config.model, config.api_url, config.strict_mode);
                self.config = config;
                let _ = reply_to.send(Ok(()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_xml_tool_calls_simple() {
        let content = r#"I'll look up the weather for you.

<｜DSML｜function_calls>
  <｜DSML｜invoke name="get_weather">
    <｜DSML｜parameter name="location" string="true">San Francisco</｜DSML｜parameter>
  </｜DSML｜invoke>
</｜DSML｜function_calls>"#;

        let result = LlmActor::parse_xml_tool_calls(content);
        assert!(result.is_some(), "Should parse XML tool calls");
        let calls = result.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["function"]["name"], "get_weather");
        let args: Value = serde_json::from_str(calls[0]["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args["location"], "San Francisco");
    }

    #[test]
    fn test_parse_xml_tool_calls_multiple() {
        let content = r#"Let me check both.

<｜DSML｜function_calls>
  <｜DSML｜invoke name="get_weather">
    <｜DSML｜parameter name="location" string="true">Tokyo</｜DSML｜parameter>
  </｜DSML｜invoke>
  <｜DSML｜invoke name="get_time">
    <｜DSML｜parameter name="timezone" string="true">Asia/Tokyo</｜DSML｜parameter>
  </｜DSML｜invoke>
</｜DSML｜function_calls>"#;

        let result = LlmActor::parse_xml_tool_calls(content);
        assert!(result.is_some());
        let calls = result.unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["function"]["name"], "get_weather");
        assert_eq!(calls[1]["function"]["name"], "get_time");
    }

    #[test]
    fn test_parse_xml_tool_calls_no_match() {
        let content = "Hello, how can I help you today?";
        let result = LlmActor::parse_xml_tool_calls(content);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_xml_tool_calls_multiple_params() {
        let content = r#"<｜DSML｜function_calls>
  <｜DSML｜invoke name="search_files">
    <｜DSML｜parameter name="path" string="true">/src</｜DSML｜parameter>
    <｜DSML｜parameter name="regex" string="true">fn main</｜DSML｜parameter>
    <｜DSML｜parameter name="file_pattern" string="true">*.rs</｜DSML｜parameter>
  </｜DSML｜invoke>
</｜DSML｜function_calls>"#;

        let result = LlmActor::parse_xml_tool_calls(content);
        assert!(result.is_some());
        let calls = result.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["function"]["name"], "search_files");
        let args: Value = serde_json::from_str(calls[0]["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args["path"], "/src");
        assert_eq!(args["regex"], "fn main");
        assert_eq!(args["file_pattern"], "*.rs");
    }
}
