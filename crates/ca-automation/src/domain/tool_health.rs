//! Tool and Codex-call health vocabulary.
//!
//! Owns failure-kind classification and advisory cooldown summaries. Pure: does
//! not read/write files or spawn tools. Persistence and watcher reporting live
//! in `app` / operations docs.
//!
//! Read when changing tool failure labels or cooldown aggregation. Do not read
//! for Codex spawn policy.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Cheapest-first provider ordering for automatic fallback routing.
/// Local inference backends are free; cloud (codex) costs money per token.
const PROVIDER_COST_ORDER: &[&str] = &["mlx_lm", "ollama", "codex"];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolFailureKind {
    ContextTooLarge,
    RateLimited,
    QuotaExceeded,
    UsageLimit,
    AuthError,
    InvalidModel,
    ModelUnavailable,
    ToolNotFound,
    PermissionDenied,
    Timeout,
    Cancelled,
    ProcessFailed,
    MalformedOutput,
    Unknown,
}

impl ToolFailureKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ContextTooLarge => "context_too_large",
            Self::RateLimited => "rate_limited",
            Self::QuotaExceeded => "quota_exceeded",
            Self::UsageLimit => "usage_limit",
            Self::AuthError => "auth_error",
            Self::InvalidModel => "invalid_model",
            Self::ModelUnavailable => "model_unavailable",
            Self::ToolNotFound => "tool_not_found",
            Self::PermissionDenied => "permission_denied",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::ProcessFailed => "process_failed",
            Self::MalformedOutput => "malformed_output",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) fn cooldown_advisory(self) -> bool {
        matches!(
            self,
            Self::ContextTooLarge
                | Self::RateLimited
                | Self::QuotaExceeded
                | Self::UsageLimit
                | Self::AuthError
                | Self::InvalidModel
                | Self::ModelUnavailable
                | Self::ToolNotFound
                | Self::PermissionDenied
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolHealthStatus {
    Success,
    Failure,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct ToolHealthRecord {
    pub(crate) observed_at: String,
    pub(crate) role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) task_id: Option<String>,
    pub(crate) model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) execution_engine: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) route_provider: Option<String>,
    pub(crate) status: ToolHealthStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) failure_kind: Option<ToolFailureKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) exit_code: Option<i32>,
    #[serde(default)]
    pub(crate) timed_out: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct ToolCooldownSummary {
    pub(crate) role: String,
    pub(crate) model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) execution_engine: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) route_provider: Option<String>,
    pub(crate) failure_kind: ToolFailureKind,
    pub(crate) recent_failures: usize,
    pub(crate) cooldown_advisory: bool,
    pub(crate) last_observed_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) excerpt: String,
}

pub(crate) fn classify_tool_failure(
    exit_code: Option<i32>,
    timed_out: bool,
    output: &str,
) -> ToolFailureKind {
    if timed_out {
        return ToolFailureKind::Timeout;
    }
    let lower = output.to_ascii_lowercase();
    if lower.contains("context length")
        || lower.contains("context window")
        || lower.contains("maximum context")
        || lower.contains("too many tokens")
        || lower.contains("prompt is too long")
    {
        ToolFailureKind::ContextTooLarge
    } else if lower.contains("rate limit") || lower.contains("429") {
        ToolFailureKind::RateLimited
    } else if lower.contains("quota") || lower.contains("insufficient_quota") {
        ToolFailureKind::QuotaExceeded
    } else if lower.contains("usage limit") || lower.contains("limit reached") {
        ToolFailureKind::UsageLimit
    } else if lower.contains("unauthorized")
        || lower.contains("authentication")
        || lower.contains("api key")
        || lower.contains("invalid token")
    {
        ToolFailureKind::AuthError
    } else if lower.contains("invalid model")
        || lower.contains("model does not exist")
        || lower.contains("unknown model")
    {
        ToolFailureKind::InvalidModel
    } else if lower.contains("model unavailable")
        || lower.contains("overloaded")
        || lower.contains("temporarily unavailable")
        || lower.contains("server error")
    {
        ToolFailureKind::ModelUnavailable
    } else if lower.contains("command not found")
        || lower.contains("no such file or directory")
        || lower.contains("failed to launch")
    {
        ToolFailureKind::ToolNotFound
    } else if lower.contains("permission denied") || lower.contains("operation not permitted") {
        ToolFailureKind::PermissionDenied
    } else if lower.contains("missing output-last-message")
        || lower.contains("failed to parse")
        || lower.contains("invalid json")
        || lower.contains("malformed")
    {
        ToolFailureKind::MalformedOutput
    } else if exit_code.is_some_and(|code| code != 0) {
        ToolFailureKind::ProcessFailed
    } else {
        ToolFailureKind::Unknown
    }
}

pub(crate) fn compact_tool_excerpt(text: &str, max_chars: usize) -> String {
    let clean = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if clean.chars().count() <= max_chars {
        return clean;
    }
    let mut out = clean
        .chars()
        .take(max_chars.saturating_sub(14))
        .collect::<String>();
    out.push_str(" [truncated]");
    out
}

type CooldownKey = (
    String,
    Option<String>,
    Option<String>,
    String,
    ToolFailureKind,
);

pub(crate) fn summarize_tool_cooldowns(records: &[ToolHealthRecord]) -> Vec<ToolCooldownSummary> {
    let mut summaries: BTreeMap<CooldownKey, ToolCooldownSummary> = BTreeMap::new();
    let mut successes = BTreeMap::new();
    for record in records {
        let route_key = (
            record.role.clone(),
            record.execution_engine.clone(),
            record.route_provider.clone(),
            record.model.clone(),
        );
        if record.status == ToolHealthStatus::Success {
            successes.insert(route_key, record.observed_at.clone());
            continue;
        }
        let Some(kind) = record.failure_kind else {
            continue;
        };
        let key = (
            record.role.clone(),
            record.execution_engine.clone(),
            record.route_provider.clone(),
            record.model.clone(),
            kind,
        );
        let success_after_failure = successes
            .get(&route_key)
            .is_some_and(|success_at| success_at >= &record.observed_at);
        if success_after_failure {
            continue;
        }
        let entry = summaries.entry(key).or_insert_with(|| ToolCooldownSummary {
            role: record.role.clone(),
            model: record.model.clone(),
            execution_engine: record.execution_engine.clone(),
            route_provider: record.route_provider.clone(),
            failure_kind: kind,
            recent_failures: 0,
            cooldown_advisory: false,
            last_observed_at: record.observed_at.clone(),
            excerpt: record.excerpt.clone(),
        });
        entry.recent_failures += 1;
        if record.observed_at >= entry.last_observed_at {
            entry.last_observed_at = record.observed_at.clone();
            entry.excerpt = record.excerpt.clone();
        }
        entry.cooldown_advisory = kind.cooldown_advisory()
            || (entry.recent_failures >= 2 && kind == ToolFailureKind::Timeout);
    }
    let mut values = summaries.into_values().collect::<Vec<_>>();
    values.sort_by(|left, right| {
        right
            .cooldown_advisory
            .cmp(&left.cooldown_advisory)
            .then_with(|| right.recent_failures.cmp(&left.recent_failures))
            .then_with(|| left.role.cmp(&right.role))
            .then_with(|| left.execution_engine.cmp(&right.execution_engine))
            .then_with(|| left.route_provider.cmp(&right.route_provider))
            .then_with(|| left.model.cmp(&right.model))
            .then_with(|| left.failure_kind.cmp(&right.failure_kind))
    });
    values
}

/// Suggest a fallback provider when the current one is on cooldown.
///
/// Returns the cheapest available alternative provider (by [`PROVIDER_COST_ORDER`])
/// when the current provider's model has an active cooldown advisory.  Returns
/// `None` when there is no cooldown or no alternative is available.
pub(crate) fn suggest_fallback_provider(
    current_provider: &str,
    current_model: &str,
    cooldowns: &[ToolCooldownSummary],
    available_providers: &[&str],
) -> Option<String> {
    let on_cooldown = cooldowns.iter().any(|c| {
        c.model == current_model
            && c.cooldown_advisory
            && c.execution_engine
                .as_deref()
                .is_none_or(|engine| engine == current_provider)
    });
    if !on_cooldown {
        return None;
    }
    PROVIDER_COST_ORDER
        .iter()
        .find(|p| **p != current_provider && available_providers.contains(p))
        .map(|p| (*p).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_health_classifies_common_provider_failures() {
        assert_eq!(
            classify_tool_failure(Some(1), false, "429 rate limit exceeded"),
            ToolFailureKind::RateLimited
        );
        assert_eq!(
            classify_tool_failure(None, true, ""),
            ToolFailureKind::Timeout
        );
        assert_eq!(
            classify_tool_failure(Some(2), false, "missing output-last-message"),
            ToolFailureKind::MalformedOutput
        );
    }

    #[test]
    fn tool_health_summarizes_advisory_cooldowns() {
        let records = vec![
            ToolHealthRecord {
                observed_at: "2026-07-06T00:00:00Z".to_string(),
                role: "worker".to_string(),
                task_id: Some("T1".to_string()),
                model: "gpt-x".to_string(),
                execution_engine: None,
                route_provider: None,
                status: ToolHealthStatus::Failure,
                failure_kind: Some(ToolFailureKind::RateLimited),
                exit_code: Some(1),
                timed_out: false,
                excerpt: "rate limit".to_string(),
            },
            ToolHealthRecord {
                observed_at: "2026-07-06T00:01:00Z".to_string(),
                role: "planner".to_string(),
                task_id: None,
                model: "gpt-x".to_string(),
                execution_engine: None,
                route_provider: None,
                status: ToolHealthStatus::Failure,
                failure_kind: Some(ToolFailureKind::ProcessFailed),
                exit_code: Some(1),
                timed_out: false,
                excerpt: "exit 1".to_string(),
            },
        ];

        let summaries = summarize_tool_cooldowns(&records);

        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].role, "worker");
        assert!(summaries[0].cooldown_advisory);
        assert!(!summaries[1].cooldown_advisory);
    }

    #[test]
    fn suggest_fallback_routes_away_from_cooldown() {
        let cooldowns = vec![ToolCooldownSummary {
            role: "worker".to_string(),
            model: "gpt-4".to_string(),
            execution_engine: None,
            route_provider: None,
            failure_kind: ToolFailureKind::RateLimited,
            recent_failures: 3,
            cooldown_advisory: true,
            last_observed_at: "2026-07-20T00:00:00Z".to_string(),
            excerpt: "429".to_string(),
        }];
        let result = suggest_fallback_provider("codex", "gpt-4", &cooldowns, &["codex", "ollama"]);
        assert_eq!(result.as_deref(), Some("ollama"));
    }

    #[test]
    fn suggest_fallback_noop_when_no_cooldown() {
        let cooldowns = vec![ToolCooldownSummary {
            role: "worker".to_string(),
            model: "gpt-4".to_string(),
            execution_engine: None,
            route_provider: None,
            failure_kind: ToolFailureKind::ProcessFailed,
            recent_failures: 1,
            cooldown_advisory: false,
            last_observed_at: "2026-07-20T00:00:00Z".to_string(),
            excerpt: "exit 1".to_string(),
        }];
        let result = suggest_fallback_provider("codex", "gpt-4", &cooldowns, &["codex", "ollama"]);
        assert!(result.is_none());
    }

    #[test]
    fn suggest_fallback_noop_when_no_alternative_available() {
        let cooldowns = vec![ToolCooldownSummary {
            role: "worker".to_string(),
            model: "mlx-community/test".to_string(),
            execution_engine: None,
            route_provider: None,
            failure_kind: ToolFailureKind::RateLimited,
            recent_failures: 2,
            cooldown_advisory: true,
            last_observed_at: "2026-07-20T00:00:00Z".to_string(),
            excerpt: "rate limit".to_string(),
        }];
        // Only mlx_lm is available — no alternative to switch to.
        let result =
            suggest_fallback_provider("mlx_lm", "mlx-community/test", &cooldowns, &["mlx_lm"]);
        assert!(result.is_none());
    }

    #[test]
    fn cooldown_keys_separate_execution_engine_and_internal_route() {
        let legacy_json = r#"{"observed_at":"2026-08-20T00:00:00Z","role":"worker","task_id":null,"model":"gpt-test","status":"failure","failure_kind":"rate_limited","exit_code":1,"timed_out":false,"excerpt":"legacy"}"#;
        let legacy: ToolHealthRecord =
            serde_json::from_str(legacy_json).expect("legacy tool health row");
        assert!(legacy.execution_engine.is_none());
        assert!(legacy.route_provider.is_none());

        let mut ca_openai = legacy.clone();
        ca_openai.execution_engine = Some("ca".to_string());
        ca_openai.route_provider = Some("openai".to_string());
        let mut ca_loopback = ca_openai.clone();
        ca_loopback.route_provider = Some("loopback".to_string());
        let summaries = summarize_tool_cooldowns(&[legacy, ca_openai, ca_loopback]);
        assert_eq!(summaries.len(), 3);
        assert!(summaries.iter().any(|summary| {
            summary.execution_engine.as_deref() == Some("ca")
                && summary.route_provider.as_deref() == Some("openai")
        }));
        assert!(summaries.iter().any(|summary| {
            summary.execution_engine.as_deref() == Some("ca")
                && summary.route_provider.as_deref() == Some("loopback")
        }));

        let ca_cooldowns = summaries
            .into_iter()
            .filter(|summary| summary.execution_engine.as_deref() == Some("ca"))
            .collect::<Vec<_>>();
        assert!(suggest_fallback_provider(
            "codex",
            "gpt-test",
            &ca_cooldowns,
            &["codex", "ollama"]
        )
        .is_none());
    }
}
