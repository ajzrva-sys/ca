//! Apple MLX LM (`mlx_lm.server`) OpenAI-compatible role runner.
//!
//! Owns optional local-model role calls via HTTP against `mlx_lm.server`
//! (default `http://127.0.0.1:8080`). Policy stays in config/app; guard/verify/
//! commit stay Rust-owned.
//!
//! Read when changing MLX LM request shape or health probes. Do not read for
//! Codex spawning or queue/guard policy.

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::config::{Config, MlxLmConfig, RoleConfig};
use crate::domain::{
    classify_tool_failure, compact_tool_excerpt, OrchestratorEvent, OrchestratorEventKind,
    ToolFailureKind, ToolHealthRecord, ToolHealthStatus,
};
use crate::fs_util::write_text_atomic;
use crate::paths::Paths;
use crate::providers::openai_compat::{
    chat_completions_url, chat_messages, extract_chat_completion_content, models_url,
    normalize_base_url, structured_system_preamble, ResponseFormat,
};

#[derive(Debug, Clone)]
pub(crate) struct MlxLmRoleRequest<'a> {
    pub(crate) role_name: &'a str,
    pub(crate) role: &'a RoleConfig,
    pub(crate) task_id: Option<&'a str>,
    pub(crate) prompt: &'a str,
    pub(crate) schema_path: &'a Path,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct MlxLmHealth {
    pub(crate) enabled: bool,
    pub(crate) reachable: bool,
    pub(crate) base_url: String,
    pub(crate) model_count: Option<usize>,
    pub(crate) detail: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<crate::providers::openai_compat::ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

/// Build the JSON body for MLX LM `/v1/chat/completions` (testable without HTTP).
pub(crate) fn build_chat_completion_body(
    model: &str,
    prompt: &str,
    schema_text: &str,
) -> Result<Value> {
    let system = structured_system_preamble(schema_text);
    let request = ChatCompletionRequest {
        model: model.to_string(),
        messages: chat_messages(system, prompt),
        stream: false,
        response_format: Some(ResponseFormat {
            type_: "json_object",
        }),
    };
    Ok(serde_json::to_value(request)?)
}

pub(crate) fn ensure_mlx_lm_enabled(cfg: &Config) -> Result<()> {
    if !cfg.mlx_lm.enabled {
        bail!(
            "role provider is mlx_lm but [mlx_lm].enabled is false; enable it in .orchestrator/config.toml or set provider = \"codex\""
        );
    }
    Ok(())
}

pub(crate) fn probe_mlx_lm_health(cfg: &MlxLmConfig) -> MlxLmHealth {
    let base_url = normalize_base_url(&cfg.base_url);
    if !cfg.enabled {
        return MlxLmHealth {
            enabled: false,
            reachable: false,
            base_url,
            model_count: None,
            detail: "mlx_lm.enabled is false".to_string(),
        };
    }
    match fetch_models_count(&base_url, Duration::from_secs(5)) {
        Ok(count) => MlxLmHealth {
            enabled: true,
            reachable: true,
            base_url,
            model_count: Some(count),
            detail: format!("{count} model(s) listed by /v1/models"),
        },
        Err(err) => MlxLmHealth {
            enabled: true,
            reachable: false,
            base_url,
            model_count: None,
            detail: format!("unreachable: {err:#}"),
        },
    }
}

fn fetch_models_count(base_url: &str, timeout: Duration) -> Result<usize> {
    Ok(fetch_model_ids(base_url, timeout)?.len())
}

pub(crate) fn list_mlx_lm_model_ids(cfg: &MlxLmConfig) -> Result<Vec<String>> {
    if !cfg.enabled {
        bail!("mlx_lm.enabled is false");
    }
    let base_url = normalize_base_url(&cfg.base_url);
    fetch_model_ids(&base_url, Duration::from_secs(5))
}

fn fetch_model_ids(base_url: &str, timeout: Duration) -> Result<Vec<String>> {
    let url = models_url(base_url);
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    let response = agent
        .get(&url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let body: Value = response
        .into_json()
        .with_context(|| format!("decode MLX LM models from {url}"))?;
    let mut ids = Vec::new();
    if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
        for item in data {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                ids.push(id.to_string());
            }
        }
    } else if let Some(models) = body.get("models").and_then(|d| d.as_array()) {
        for item in models {
            if let Some(id) = item
                .get("id")
                .or_else(|| item.get("name"))
                .and_then(|v| v.as_str())
            {
                ids.push(id.to_string());
            }
        }
    }
    Ok(ids)
}

pub(crate) fn run_mlx_lm_role(
    paths: &Paths,
    cfg: &Config,
    request: MlxLmRoleRequest<'_>,
) -> Result<String> {
    ensure_mlx_lm_enabled(cfg)?;
    let mlx = &cfg.mlx_lm;
    let base_url = normalize_base_url(&mlx.base_url);
    let schema_text = fs::read_to_string(request.schema_path)
        .with_context(|| format!("read MLX LM schema {}", request.schema_path.display()))?;

    let call_reason = format!("mlx_lm role call for {}", request.role_name);
    let _ = append_event(
        paths,
        OrchestratorEventKind::AiEscalated,
        request.task_id,
        json!({
            "role": request.role_name,
            "reason": call_reason,
            "model": request.role.model,
            "provider": "mlx_lm",
            "effort": request.role.effort,
        }),
    );

    fs::create_dir_all(&paths.runs)?;
    let run_id = format!(
        "{}-{}-mlx_lm",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        request.role_name
    );
    let prompt_path = paths.runs.join(format!("{run_id}.prompt.md"));
    let response_path = paths.runs.join(format!("{run_id}.mlx_lm.json"));
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
            record_failure(
                paths,
                &request,
                prompt_tokens,
                failure_kind,
                &detail,
                None,
                timed_out,
                format!("mlx_lm request failed: {detail}"),
            );
            return Err(anyhow!("mlx_lm {} failed: {detail}", request.role_name));
        }
    };

    let status = response.status();
    let raw = response
        .into_string()
        .with_context(|| format!("read MLX LM response body from {url}"))?;
    write_text_atomic(&response_path, &raw)?;

    if !(200..300).contains(&status) {
        let excerpt = compact_tool_excerpt(&raw, 500);
        let failure_kind = classify_tool_failure(Some(status as i32), false, &raw);
        record_failure(
            paths,
            &request,
            prompt_tokens,
            failure_kind,
            &excerpt,
            Some(status as i32),
            false,
            format!("mlx_lm HTTP {status}"),
        );
        bail!("mlx_lm {} returned HTTP {status}", request.role_name);
    }

    let parsed: Value =
        serde_json::from_str(&raw).with_context(|| format!("parse MLX LM JSON from {url}"))?;
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
            provider: Some("mlx_lm".to_string()),
            prompt_token_estimate: prompt_tokens,
            output_char_count: content.chars().count(),
            skipped_by_rust: false,
            skip_or_call_reason: format!("mlx_lm completed in {}ms", elapsed.as_millis()),
            tool_failure_kind: None,
            tool_failure_excerpt: None,
            exit_code: Some(0),
            timed_out: false,
        },
    );
    Ok(content)
}

#[allow(clippy::too_many_arguments)]
fn record_failure(
    paths: &Paths,
    request: &MlxLmRoleRequest<'_>,
    prompt_tokens: usize,
    failure_kind: ToolFailureKind,
    excerpt: &str,
    exit_code: Option<i32>,
    timed_out: bool,
    reason: String,
) {
    let _ = append_model_call_record(
        paths,
        &ModelCallRecord {
            timestamp: Utc::now().to_rfc3339(),
            role: request.role_name.to_string(),
            task_id: request.task_id.map(str::to_string),
            model: request.role.model.clone(),
            effort: request.role.effort.clone(),
            provider: Some("mlx_lm".to_string()),
            prompt_token_estimate: prompt_tokens,
            output_char_count: 0,
            skipped_by_rust: false,
            skip_or_call_reason: reason,
            tool_failure_kind: Some(failure_kind),
            tool_failure_excerpt: Some(compact_tool_excerpt(excerpt, 500)),
            exit_code,
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
            execution_engine: Some("mlx_lm".to_string()),
            route_provider: None,
            status: ToolHealthStatus::Failure,
            failure_kind: Some(failure_kind),
            exit_code,
            timed_out,
            excerpt: compact_tool_excerpt(excerpt, 500),
        },
    );
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
    use crate::config::{MlxLmConfig, RoleProvider};
    use serde_json::json;

    #[test]
    fn mlx_lm_chat_body_is_openai_style() -> Result<()> {
        let body = build_chat_completion_body(
            "mlx-community/Qwen3-8B-4bit",
            "do the work",
            r#"{"type":"object","required":["ok"]}"#,
        )?;
        assert_eq!(body["model"], "mlx-community/Qwen3-8B-4bit");
        assert_eq!(body["stream"], false);
        assert_eq!(body["response_format"]["type"], "json_object");
        assert!(body.get("format").is_none());
        assert!(body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains(r#""required":["ok"]"#));
        Ok(())
    }

    #[test]
    fn mlx_lm_disabled_fails_closed() {
        let cfg = Config {
            mlx_lm: MlxLmConfig {
                enabled: false,
                base_url: "http://127.0.0.1:8080".to_string(),
            },
            ..Config::default()
        };
        let err = ensure_mlx_lm_enabled(&cfg).expect_err("disabled");
        assert!(err.to_string().contains("enabled is false"));
    }

    #[test]
    fn mlx_lm_health_reports_disabled_without_http() {
        let health = probe_mlx_lm_health(&MlxLmConfig {
            enabled: false,
            base_url: "http://127.0.0.1:9".to_string(),
        });
        assert!(!health.enabled);
        assert!(!health.reachable);
        assert!(health.detail.contains("enabled is false"));
    }

    #[test]
    fn mlx_lm_provider_label() {
        assert_eq!(RoleProvider::MlxLm.as_str(), "mlx_lm");
        assert!(RoleProvider::MlxLm.is_local_http());
    }

    #[test]
    fn mlx_lm_uses_shared_content_extract() -> Result<()> {
        let body = json!({
            "choices": [{ "message": { "content": "{\"ok\":true}" } }]
        });
        assert_eq!(extract_chat_completion_content(&body)?, "{\"ok\":true}");
        Ok(())
    }
}
