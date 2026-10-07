//! Ollama OpenAI-compatible role runner.
//!
//! Owns optional local-model role calls via HTTP. Policy (which role uses
//! Ollama) stays in config/app; guard/verify/commit stay Rust-owned.
//!
//! Read when changing Ollama request shape or health probes. Do not read for
//! Codex process spawning or queue/guard policy.

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::config::{Config, OllamaConfig, RoleConfig};
use crate::domain::{
    classify_tool_failure, compact_tool_excerpt, OrchestratorEvent, OrchestratorEventKind,
    ToolFailureKind, ToolHealthRecord, ToolHealthStatus,
};
use crate::fs_util::write_text_atomic;
use crate::paths::Paths;

#[derive(Debug, Clone)]
pub(crate) struct OllamaRoleRequest<'a> {
    pub(crate) role_name: &'a str,
    pub(crate) role: &'a RoleConfig,
    pub(crate) task_id: Option<&'a str>,
    pub(crate) prompt: &'a str,
    pub(crate) schema_path: &'a Path,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct OllamaHealth {
    pub(crate) enabled: bool,
    pub(crate) reachable: bool,
    pub(crate) base_url: String,
    pub(crate) model_count: Option<usize>,
    pub(crate) detail: String,
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    stream: bool,
    /// Ollama native JSON mode.
    format: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    type_: &'static str,
}

/// Build the JSON body for Ollama `/v1/chat/completions` (testable without HTTP).
pub(crate) fn build_chat_completion_body(
    model: &str,
    prompt: &str,
    schema_text: &str,
) -> Result<Value> {
    let system = format!(
        "You are a structured output agent for codex-team. Respond with a single JSON object that validates against this schema. Do not wrap the JSON in markdown fences.\n\n{}",
        schema_text.trim()
    );
    let request = ChatCompletionRequest {
        model,
        messages: vec![
            ChatMessage {
                role: "system",
                content: system,
            },
            ChatMessage {
                role: "user",
                content: prompt.to_string(),
            },
        ],
        stream: false,
        format: "json",
        response_format: Some(ResponseFormat {
            type_: "json_object",
        }),
    };
    Ok(serde_json::to_value(request)?)
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
    bail!("ollama response did not contain assistant content")
}

pub(crate) fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

pub(crate) fn chat_completions_url(base_url: &str) -> String {
    format!("{}/v1/chat/completions", normalize_base_url(base_url))
}

pub(crate) fn tags_url(base_url: &str) -> String {
    format!("{}/api/tags", normalize_base_url(base_url))
}

pub(crate) fn ensure_ollama_enabled(cfg: &Config) -> Result<()> {
    if !cfg.ollama.enabled {
        bail!(
            "role provider is ollama but [ollama].enabled is false; enable it in .orchestrator/config.toml or set provider = \"codex\""
        );
    }
    Ok(())
}

pub(crate) fn probe_ollama_health(cfg: &OllamaConfig) -> OllamaHealth {
    let base_url = normalize_base_url(&cfg.base_url);
    if !cfg.enabled {
        return OllamaHealth {
            enabled: false,
            reachable: false,
            base_url,
            model_count: None,
            detail: "ollama.enabled is false".to_string(),
        };
    }
    match fetch_tags_model_count(&base_url, Duration::from_secs(5)) {
        Ok(count) => OllamaHealth {
            enabled: true,
            reachable: true,
            base_url,
            model_count: Some(count),
            detail: format!("{count} model(s) listed by /api/tags"),
        },
        Err(err) => OllamaHealth {
            enabled: true,
            reachable: false,
            base_url,
            model_count: None,
            detail: format!("unreachable: {err:#}"),
        },
    }
}

fn fetch_tags_model_count(base_url: &str, timeout: Duration) -> Result<usize> {
    Ok(fetch_tag_model_ids(base_url, timeout)?.len())
}

pub(crate) fn list_ollama_model_ids(cfg: &OllamaConfig) -> Result<Vec<String>> {
    if !cfg.enabled {
        bail!("ollama.enabled is false");
    }
    let base_url = normalize_base_url(&cfg.base_url);
    fetch_tag_model_ids(&base_url, Duration::from_secs(5))
}

fn fetch_tag_model_ids(base_url: &str, timeout: Duration) -> Result<Vec<String>> {
    let url = tags_url(base_url);
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    let response = agent
        .get(&url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let body: Value = response
        .into_json()
        .with_context(|| format!("decode Ollama tags from {url}"))?;
    let mut ids = Vec::new();
    if let Some(models) = body.get("models").and_then(|m| m.as_array()) {
        for item in models {
            if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                ids.push(name.to_string());
            }
        }
    }
    Ok(ids)
}

pub(crate) fn run_ollama_role(
    paths: &Paths,
    cfg: &Config,
    request: OllamaRoleRequest<'_>,
) -> Result<String> {
    ensure_ollama_enabled(cfg)?;
    let ollama = &cfg.ollama;
    let base_url = normalize_base_url(&ollama.base_url);
    let schema_text = fs::read_to_string(request.schema_path)
        .with_context(|| format!("read Ollama schema {}", request.schema_path.display()))?;

    let call_reason = format!("ollama role call for {}", request.role_name);
    let _ = append_event(
        paths,
        OrchestratorEventKind::AiEscalated,
        request.task_id,
        json!({
            "role": request.role_name,
            "reason": call_reason,
            "model": request.role.model,
            "provider": "ollama",
            "effort": request.role.effort,
        }),
    );

    fs::create_dir_all(&paths.runs)?;
    let run_id = format!(
        "{}-{}-ollama",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        request.role_name
    );
    let prompt_path = paths.runs.join(format!("{run_id}.prompt.md"));
    let response_path = paths.runs.join(format!("{run_id}.ollama.json"));
    let error_path = paths.runs.join(format!("{run_id}.stderr.log"));
    write_text_atomic(&prompt_path, request.prompt)?;

    let body = build_chat_completion_body(&request.role.model, request.prompt, &schema_text)?;
    let prompt_tokens = estimate_tokens(request.prompt);
    let url = chat_completions_url(&base_url);
    let timeout = Duration::from_secs(request.role.timeout_seconds.max(1));
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    let started = Instant::now();

    let response = match agent.post(&url).send_json(body) {
        Ok(response) => response,
        Err(err) => {
            let detail = err.to_string();
            let _ = write_text_atomic(&error_path, &detail);
            let timed_out = detail.to_lowercase().contains("timed out")
                || detail.to_lowercase().contains("timeout");
            let failure_kind = if timed_out {
                ToolFailureKind::Timeout
            } else {
                classify_tool_failure(None, timed_out, &detail)
            };
            let _ = append_model_call_record(
                paths,
                &ModelCallRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    role: request.role_name.to_string(),
                    task_id: request.task_id.map(str::to_string),
                    model: request.role.model.clone(),
                    effort: request.role.effort.clone(),
                    provider: Some("ollama".to_string()),
                    prompt_token_estimate: prompt_tokens,
                    output_char_count: 0,
                    skipped_by_rust: false,
                    skip_or_call_reason: format!("ollama request failed: {detail}"),
                    tool_failure_kind: Some(failure_kind),
                    tool_failure_excerpt: Some(compact_tool_excerpt(&detail, 500)),
                    exit_code: None,
                    timed_out,
                },
            );
            let _ = append_tool_health_record(
                paths,
                &ToolHealthRecord {
                    observed_at: Utc::now().to_rfc3339(),
                    role: request.role_name.to_string(),
                    task_id: request.task_id.map(str::to_string),
                    model: request.role.model.clone(),
                    execution_engine: Some("ollama".to_string()),
                    route_provider: None,
                    status: ToolHealthStatus::Failure,
                    failure_kind: Some(failure_kind),
                    exit_code: None,
                    timed_out,
                    excerpt: compact_tool_excerpt(&detail, 500),
                },
            );
            return Err(anyhow!("ollama {} failed: {detail}", request.role_name));
        }
    };

    let status = response.status();
    let raw = response
        .into_string()
        .with_context(|| format!("read Ollama response body from {url}"))?;
    write_text_atomic(&response_path, &raw)?;

    if !(200..300).contains(&status) {
        let excerpt = compact_tool_excerpt(&raw, 500);
        let failure_kind = classify_tool_failure(Some(status as i32), false, &raw);
        let _ = append_model_call_record(
            paths,
            &ModelCallRecord {
                timestamp: Utc::now().to_rfc3339(),
                role: request.role_name.to_string(),
                task_id: request.task_id.map(str::to_string),
                model: request.role.model.clone(),
                effort: request.role.effort.clone(),
                provider: Some("ollama".to_string()),
                prompt_token_estimate: prompt_tokens,
                output_char_count: raw.chars().count(),
                skipped_by_rust: false,
                skip_or_call_reason: format!("ollama HTTP {status}"),
                tool_failure_kind: Some(failure_kind),
                tool_failure_excerpt: Some(excerpt.clone()),
                exit_code: Some(status as i32),
                timed_out: false,
            },
        );
        let _ = append_tool_health_record(
            paths,
            &ToolHealthRecord {
                observed_at: Utc::now().to_rfc3339(),
                role: request.role_name.to_string(),
                task_id: request.task_id.map(str::to_string),
                model: request.role.model.clone(),
                execution_engine: Some("ollama".to_string()),
                route_provider: None,
                status: ToolHealthStatus::Failure,
                failure_kind: Some(failure_kind),
                exit_code: Some(status as i32),
                timed_out: false,
                excerpt,
            },
        );
        bail!("ollama {} returned HTTP {status}", request.role_name);
    }

    let parsed: Value =
        serde_json::from_str(&raw).with_context(|| format!("parse Ollama JSON from {url}"))?;
    let content = extract_chat_completion_content(&parsed)?;
    let elapsed = started.elapsed();
    let _ = append_model_call_record(
        paths,
        &ModelCallRecord {
            timestamp: Utc::now().to_rfc3339(),
            role: request.role_name.to_string(),
            task_id: request.task_id.map(str::to_string),
            model: request.role.model.clone(),
            effort: request.role.effort.clone(),
            provider: Some("ollama".to_string()),
            prompt_token_estimate: prompt_tokens,
            output_char_count: content.chars().count(),
            skipped_by_rust: false,
            skip_or_call_reason: format!("ollama completed in {}ms", elapsed.as_millis()),
            tool_failure_kind: None,
            tool_failure_excerpt: None,
            exit_code: Some(0),
            timed_out: false,
        },
    );
    Ok(content)
}

#[derive(Debug, Clone, Serialize)]
struct ModelCallRecord {
    timestamp: String,
    role: String,
    task_id: Option<String>,
    model: String,
    effort: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    prompt_token_estimate: usize,
    output_char_count: usize,
    skipped_by_rust: bool,
    skip_or_call_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_failure_kind: Option<ToolFailureKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_failure_excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "is_false")]
    timed_out: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn append_event(
    paths: &Paths,
    kind: OrchestratorEventKind,
    task_id: Option<&str>,
    payload: Value,
) -> Result<()> {
    fs::create_dir_all(&paths.orch)?;
    let event = OrchestratorEvent {
        timestamp: Utc::now().to_rfc3339(),
        kind,
        task_id: task_id.map(str::to_string),
        payload,
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.events)
        .with_context(|| format!("open event stream {}", paths.events.display()))?;
    writeln!(file, "{}", serde_json::to_string(&event)?)?;
    Ok(())
}

fn append_model_call_record(paths: &Paths, record: &ModelCallRecord) -> Result<()> {
    fs::create_dir_all(&paths.orch)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.cost)
        .with_context(|| format!("open cost ledger {}", paths.cost.display()))?;
    writeln!(file, "{}", serde_json::to_string(record)?)?;
    Ok(())
}

fn append_tool_health_record(paths: &Paths, record: &ToolHealthRecord) -> Result<()> {
    fs::create_dir_all(&paths.observations)?;
    let path = paths.observations.join("tool_health.jsonl");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open tool health ledger {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(record)?)?;
    Ok(())
}

fn estimate_tokens(text: &str) -> usize {
    (text.chars().count() / 4).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{OllamaConfig, RoleProvider};

    #[test]
    fn ollama_chat_body_includes_schema_and_json_mode() -> Result<()> {
        let body = build_chat_completion_body(
            "qwen2.5-coder:32b",
            "do the work",
            r#"{"type":"object","required":["ok"]}"#,
        )?;
        assert_eq!(body["model"], "qwen2.5-coder:32b");
        assert_eq!(body["format"], "json");
        assert_eq!(body["response_format"]["type"], "json_object");
        assert_eq!(body["stream"], false);
        let system = body["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains(r#""required":["ok"]"#));
        assert_eq!(body["messages"][1]["content"], "do the work");
        Ok(())
    }

    #[test]
    fn ollama_extracts_openai_style_content() -> Result<()> {
        let body = json!({
            "choices": [{ "message": { "content": "{\n  \"ok\": true\n}" } }]
        });
        assert_eq!(
            extract_chat_completion_content(&body)?,
            "{\n  \"ok\": true\n}"
        );
        Ok(())
    }

    #[test]
    fn ollama_extracts_native_response_field() -> Result<()> {
        let body = json!({ "response": "{\"ok\":true}" });
        assert_eq!(extract_chat_completion_content(&body)?, "{\"ok\":true}");
        Ok(())
    }

    #[test]
    fn ollama_extract_rejects_missing_content() {
        let err = extract_chat_completion_content(&json!({ "choices": [] }))
            .expect_err("missing content");
        assert!(err.to_string().contains("assistant content"));
    }

    #[test]
    fn ollama_disabled_fails_closed() {
        let cfg = Config {
            ollama: OllamaConfig {
                enabled: false,
                base_url: "http://127.0.0.1:11434".to_string(),
            },
            ..Config::default()
        };
        let err = ensure_ollama_enabled(&cfg).expect_err("disabled");
        assert!(err.to_string().contains("enabled is false"));
    }

    #[test]
    fn ollama_health_reports_disabled_without_http() {
        let health = probe_ollama_health(&OllamaConfig {
            enabled: false,
            base_url: "http://127.0.0.1:9".to_string(),
        });
        assert!(!health.enabled);
        assert!(!health.reachable);
        assert!(health.detail.contains("enabled is false"));
    }

    #[test]
    fn role_provider_defaults_to_codex() {
        assert_eq!(RoleProvider::default(), RoleProvider::Codex);
        assert_eq!(RoleProvider::Ollama.as_str(), "ollama");
    }

    #[test]
    fn urls_normalize_trailing_slash() {
        assert_eq!(
            chat_completions_url("http://127.0.0.1:11434/"),
            "http://127.0.0.1:11434/v1/chat/completions"
        );
        assert_eq!(
            tags_url("http://127.0.0.1:11434/"),
            "http://127.0.0.1:11434/api/tags"
        );
    }
}
