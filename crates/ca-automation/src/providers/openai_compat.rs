//! Shared OpenAI-compatible HTTP helpers for local providers (Ollama, MLX LM).
//!
//! Pure URL/body helpers stay here; provider-specific request knobs live in
//! each provider module.

use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub(crate) struct ChatMessage {
    pub(crate) role: String,
    pub(crate) content: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ResponseFormat {
    #[serde(rename = "type")]
    pub(crate) type_: &'static str,
}

pub(crate) fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

pub(crate) fn chat_completions_url(base_url: &str) -> String {
    format!("{}/v1/chat/completions", normalize_base_url(base_url))
}

pub(crate) fn models_url(base_url: &str) -> String {
    format!("{}/v1/models", normalize_base_url(base_url))
}

pub(crate) fn structured_system_preamble(schema_text: &str) -> String {
    format!(
        "You are a structured output agent for codex-team. Respond with a single JSON object that validates against this schema. Do not wrap the JSON in markdown fences.\n\n{}",
        schema_text.trim()
    )
}

pub(crate) fn chat_messages(system: String, user_prompt: &str) -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            role: "system".to_string(),
            content: system,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user_prompt.to_string(),
        },
    ]
}

/// Extract assistant text from an OpenAI-compatible chat completions response.
pub(crate) fn extract_chat_completion_content(body: &Value) -> Result<String> {
    if let Some(content) = body
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
    {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if let Some(content) = body.get("message").and_then(|m| m.get("content")) {
        if let Some(text) = content.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
    }
    if let Some(response) = body.get("response").and_then(Value::as_str) {
        let trimmed = response.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    bail!("OpenAI-compatible response did not contain assistant content")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_openai_style_content() -> Result<()> {
        let body = json!({
            "choices": [{ "message": { "content": "{\"ok\":true}" } }]
        });
        assert_eq!(extract_chat_completion_content(&body)?, "{\"ok\":true}");
        Ok(())
    }

    #[test]
    fn urls_normalize_trailing_slash() {
        assert_eq!(
            chat_completions_url("http://127.0.0.1:8080/"),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
        assert_eq!(
            models_url("http://127.0.0.1:8080/"),
            "http://127.0.0.1:8080/v1/models"
        );
    }
}
