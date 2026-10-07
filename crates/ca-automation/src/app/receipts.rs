//! Machine-readable CLI receipts for Hermes / unattended supervisors.
//!
//! Owns typed `protocol_version: 1` cycle, status, disposition, and doctor
//! receipts. Emission and quiet-stdout policy stay in `app` entrypoints.
//!
//! Read when changing Hermes JSON contracts. Do not read for cycle routing.

use serde::{Deserialize, Serialize};

use crate::state::{HumanDispositionPause, PendingCommit, State};

pub(crate) const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PauseReceipt {
    pub(crate) reason: String,
    pub(crate) options: Vec<String>,
    pub(crate) source: String,
}

impl From<&HumanDispositionPause> for PauseReceipt {
    fn from(pause: &HumanDispositionPause) -> Self {
        Self {
            reason: pause.reason.clone(),
            options: pause.options.clone(),
            source: pause.source.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct QueueTaskReceipt {
    pub(crate) task_id: String,
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PendingCommitReceipt {
    pub(crate) id: String,
    pub(crate) digest: String,
    pub(crate) source_lane: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) import_digest: Option<String>,
    pub(crate) base_head: String,
    pub(crate) dirty_paths: Vec<String>,
    pub(crate) commit_message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) guard_evidence_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) verification_evidence_ids: Vec<String>,
    pub(crate) created_at: String,
}

impl From<&PendingCommit> for PendingCommitReceipt {
    fn from(pending: &PendingCommit) -> Self {
        Self {
            id: pending.id.clone(),
            digest: pending.digest.clone(),
            source_lane: pending.source_lane.clone(),
            task_id: pending.task_id.clone(),
            import_digest: pending.import_digest.clone(),
            base_head: pending.base_head.clone(),
            dirty_paths: pending.dirty_paths.clone(),
            commit_message: pending.commit_message.clone(),
            guard_evidence_ids: pending.guard_evidence_ids.clone(),
            verification_evidence_ids: pending.verification_evidence_ids.clone(),
            created_at: pending.created_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct CycleReceipt {
    pub(crate) protocol_version: u32,
    pub(crate) cycle_id: String,
    pub(crate) repo: String,
    pub(crate) outcome: String,
    pub(crate) mission: String,
    pub(crate) planner_due: bool,
    pub(crate) master_due: bool,
    pub(crate) queue_len: usize,
    pub(crate) human_disposition_required: bool,
    pub(crate) pause: Option<PauseReceipt>,
    pub(crate) events_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) commit_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) require_clean_start: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pause_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pending_commit: Option<PendingCommitReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) error_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) error_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) run_artifact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) provider_hint: Option<ProviderHintReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) learning: Option<LearningSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ProviderHintReceipt {
    pub(crate) recommended_provider: String,
    pub(crate) reason: String,
}

/// Advisory Rust memory exported to Hermes / supervisors (protocol_version 1 additive).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct LearningSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) recommended_lane: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) top_signals: Vec<LearningSignalReceipt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) cooldowns: Vec<LearningCooldownReceipt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) recent_episode_error_kinds: Vec<EpisodeErrorKindCount>,
    #[serde(default)]
    pub(crate) catalog_healthy: bool,
    #[serde(default)]
    pub(crate) proposal_count: usize,
    #[serde(default)]
    pub(crate) approved_matching_count: usize,
    #[serde(default)]
    pub(crate) quarantine_count: usize,
    #[serde(default)]
    pub(crate) execution_count: usize,
    #[serde(default)]
    pub(crate) ai_calls_avoided: u64,
    #[serde(default)]
    pub(crate) estimated_tokens_avoided: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) applicable_recipe_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) activation_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct LearningSignalReceipt {
    pub(crate) lane: String,
    pub(crate) weight: i32,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct LearningCooldownReceipt {
    pub(crate) role: String,
    pub(crate) failure_kind: String,
    pub(crate) recent_failures: usize,
    pub(crate) cooldown_advisory: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct EpisodeErrorKindCount {
    pub(crate) error_kind: String,
    pub(crate) count: usize,
}

impl CycleReceipt {
    pub(crate) fn from_state(
        cycle_id: String,
        repo: String,
        outcome: &str,
        state: &State,
        events_path: String,
    ) -> Self {
        let pause = state
            .human_disposition_required
            .as_ref()
            .map(PauseReceipt::from);
        Self {
            protocol_version: PROTOCOL_VERSION,
            cycle_id,
            repo,
            outcome: outcome.to_string(),
            mission: state.mission.clone(),
            planner_due: state.planner_due,
            master_due: state.master_due,
            queue_len: state.queue.len(),
            human_disposition_required: pause.is_some(),
            pause,
            events_path,
            commit_mode: None,
            require_clean_start: None,
            revision: None,
            pause_fingerprint: None,
            pending_commit: state
                .pending_commit
                .as_ref()
                .map(PendingCommitReceipt::from),
            error_kind: None,
            error_summary: None,
            role: None,
            run_artifact: None,
            provider_hint: None,
            learning: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn error(
        cycle_id: String,
        repo: String,
        state: Option<&State>,
        events_path: String,
        error_kind: &str,
        error_summary: &str,
        role: Option<String>,
        run_artifact: Option<String>,
    ) -> Self {
        let (mission, planner_due, master_due, queue_len, pause) = if let Some(state) = state {
            (
                state.mission.clone(),
                state.planner_due,
                state.master_due,
                state.queue.len(),
                state
                    .human_disposition_required
                    .as_ref()
                    .map(PauseReceipt::from),
            )
        } else {
            (String::new(), false, false, 0, None)
        };
        Self {
            protocol_version: PROTOCOL_VERSION,
            cycle_id,
            repo,
            outcome: "error".to_string(),
            mission,
            planner_due,
            master_due,
            queue_len,
            human_disposition_required: pause.is_some(),
            pause,
            events_path,
            commit_mode: None,
            require_clean_start: None,
            revision: None,
            pause_fingerprint: None,
            pending_commit: state
                .and_then(|state| state.pending_commit.as_ref())
                .map(PendingCommitReceipt::from),
            error_kind: Some(error_kind.to_string()),
            error_summary: Some(error_summary.to_string()),
            role,
            run_artifact,
            provider_hint: None,
            learning: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct StatusReceipt {
    pub(crate) protocol_version: u32,
    pub(crate) repo: String,
    pub(crate) mission: String,
    pub(crate) project_complete: bool,
    pub(crate) planner_due: bool,
    pub(crate) master_due: bool,
    pub(crate) queue: Vec<QueueTaskReceipt>,
    pub(crate) human_disposition_required: Option<PauseReceipt>,
    pub(crate) events_path: String,
    pub(crate) observe: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) commit_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) require_clean_start: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pause_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pending_commit: Option<PendingCommitReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) provider_hint: Option<ProviderHintReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) learning: Option<LearningSnapshot>,
}

impl StatusReceipt {
    pub(crate) fn from_state(
        repo: String,
        state: &State,
        events_path: String,
        observe: bool,
    ) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            repo,
            mission: state.mission.clone(),
            project_complete: state.project_complete,
            planner_due: state.planner_due,
            master_due: state.master_due,
            queue: state
                .queue
                .iter()
                .map(|task| QueueTaskReceipt {
                    task_id: task.task_id.clone(),
                    kind: task.kind.clone(),
                    title: task.title.clone(),
                    status: task.status.clone(),
                })
                .collect(),
            human_disposition_required: state
                .human_disposition_required
                .as_ref()
                .map(PauseReceipt::from),
            events_path,
            observe,
            commit_mode: None,
            require_clean_start: None,
            revision: None,
            pause_fingerprint: None,
            pending_commit: state
                .pending_commit
                .as_ref()
                .map(PendingCommitReceipt::from),
            provider_hint: None,
            learning: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct DispositionReceipt {
    pub(crate) protocol_version: u32,
    pub(crate) repo: String,
    pub(crate) outcome: String,
    pub(crate) decision: String,
    pub(crate) refresh_report: bool,
    pub(crate) human_disposition_required: bool,
    pub(crate) pause: Option<PauseReceipt>,
    pub(crate) events_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pause_fingerprint: Option<String>,
}

impl DispositionReceipt {
    pub(crate) fn from_state(
        repo: String,
        outcome: &str,
        decision: &str,
        refresh_report: bool,
        state: &State,
        events_path: String,
    ) -> Self {
        let pause = state
            .human_disposition_required
            .as_ref()
            .map(PauseReceipt::from);
        Self {
            protocol_version: PROTOCOL_VERSION,
            repo,
            outcome: outcome.to_string(),
            decision: decision.to_string(),
            refresh_report,
            human_disposition_required: pause.is_some(),
            pause,
            events_path,
            revision: None,
            pause_fingerprint: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct DoctorCheck {
    pub(crate) name: String,
    pub(crate) ok: bool,
    /// `ok` | `error` (protocol_version 1). Warnings are reserved for a future additive value.
    pub(crate) severity: String,
    pub(crate) detail: String,
}

impl DoctorCheck {
    pub(crate) fn new(name: impl Into<String>, ok: bool, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ok,
            severity: if ok {
                "ok".to_string()
            } else {
                "error".to_string()
            },
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct DoctorReceipt {
    pub(crate) protocol_version: u32,
    pub(crate) repo: String,
    pub(crate) healthy: bool,
    pub(crate) initialized: bool,
    pub(crate) checks: Vec<DoctorCheck>,
}

impl DoctorReceipt {
    pub(crate) fn new(repo: String, initialized: bool, checks: Vec<DoctorCheck>) -> Self {
        let healthy = checks.iter().all(|check| check.ok);
        Self {
            protocol_version: PROTOCOL_VERSION,
            repo,
            healthy,
            initialized,
            checks,
        }
    }
}

pub(crate) fn new_cycle_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("cycle-{nanos}")
}

pub(crate) fn emit_json_receipt<T: Serialize>(receipt: &T) -> anyhow::Result<()> {
    // Always write the machine receipt to real stdout, even under quiet JSON mode
    // where shadowed `println!` in `app` redirects progress to stderr.
    let text = serde_json::to_string_pretty(receipt)?;
    ::std::println!("{text}");
    Ok(())
}

#[cfg(test)]
mod schema_contract_tests {
    use super::*;
    use serde_json::{json, Value};

    fn pending_receipt() -> PendingCommitReceipt {
        PendingCommitReceipt {
            id: "pending-0123456789abcdef".to_string(),
            digest: "a".repeat(64),
            source_lane: "accepted_worker_task".to_string(),
            task_id: Some("INT-148".to_string()),
            import_digest: Some("b".repeat(64)),
            base_head: "c".repeat(40),
            dirty_paths: vec!["src/main.rs".to_string()],
            commit_message: "Complete INT-148".to_string(),
            guard_evidence_ids: vec!["guard:INT-148".to_string()],
            verification_evidence_ids: vec!["evidence-1".to_string()],
            created_at: "2026-08-20T00:00:00Z".to_string(),
        }
    }

    fn assert_closed_schema_covers_value(schema_text: &str, value: &Value) {
        let schema: Value = serde_json::from_str(schema_text).unwrap();
        assert_eq!(schema.get("additionalProperties"), Some(&json!(false)));
        let properties = schema["properties"].as_object().unwrap();
        let object = value.as_object().unwrap();
        for key in object.keys() {
            assert!(
                properties.contains_key(key),
                "closed receipt schema omitted emitted property {key}"
            );
        }
        for required in schema["required"].as_array().unwrap() {
            let required = required.as_str().unwrap();
            assert!(object.contains_key(required), "receipt omitted {required}");
        }

        let pending = object.get("pending_commit").unwrap().as_object().unwrap();
        let pending_schema = schema["properties"]["pending_commit"]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate.get("type") == Some(&json!("object")))
            .unwrap();
        assert_eq!(
            pending_schema.get("additionalProperties"),
            Some(&json!(false))
        );
        let pending_properties = pending_schema["properties"].as_object().unwrap();
        for key in pending.keys() {
            assert!(
                pending_properties.contains_key(key),
                "closed pending-commit schema omitted emitted property {key}"
            );
        }
    }

    #[test]
    fn strict_receipt_schemas_cover_every_additive_emitted_field() {
        let pending = pending_receipt();
        let cycle = CycleReceipt {
            protocol_version: PROTOCOL_VERSION,
            cycle_id: "cycle-1".to_string(),
            repo: "/tmp/repo".to_string(),
            outcome: "paused".to_string(),
            mission: "mission".to_string(),
            planner_due: false,
            master_due: false,
            queue_len: 1,
            human_disposition_required: false,
            pause: None,
            events_path: ".orchestrator/events.jsonl".to_string(),
            commit_mode: Some("manual".to_string()),
            require_clean_start: Some(true),
            revision: Some("d".repeat(64)),
            pause_fingerprint: Some("e".repeat(64)),
            pending_commit: Some(pending.clone()),
            error_kind: Some("none".to_string()),
            error_summary: Some("summary".to_string()),
            role: Some("worker".to_string()),
            run_artifact: Some("run.json".to_string()),
            provider_hint: Some(ProviderHintReceipt {
                recommended_provider: "ca".to_string(),
                reason: "configured".to_string(),
            }),
            learning: Some(LearningSnapshot::default()),
        };
        assert_closed_schema_covers_value(
            include_str!("../../schemas/cycle_receipt.schema.json"),
            &serde_json::to_value(cycle).unwrap(),
        );

        let status = json!({
            "protocol_version": PROTOCOL_VERSION,
            "repo": "/tmp/repo",
            "mission": "mission",
            "project_complete": false,
            "planner_due": false,
            "master_due": false,
            "queue": [],
            "human_disposition_required": null,
            "events_path": ".orchestrator/events.jsonl",
            "observe": true,
            "commit_mode": "manual",
            "require_clean_start": true,
            "revision": "f".repeat(64),
            "pause_fingerprint": "1".repeat(64),
            "pending_commit": pending,
        });
        assert_closed_schema_covers_value(
            include_str!("../../schemas/status_receipt.schema.json"),
            &status,
        );

        let disposition = DispositionReceipt {
            protocol_version: PROTOCOL_VERSION,
            repo: "/tmp/repo".to_string(),
            outcome: "recorded".to_string(),
            decision: "preserve".to_string(),
            refresh_report: false,
            human_disposition_required: false,
            pause: None,
            events_path: ".orchestrator/events.jsonl".to_string(),
            revision: Some("2".repeat(64)),
            pause_fingerprint: Some("3".repeat(64)),
        };
        let schema: Value = serde_json::from_str(include_str!(
            "../../schemas/disposition_receipt.schema.json"
        ))
        .unwrap();
        let value = serde_json::to_value(disposition).unwrap();
        let properties = schema["properties"].as_object().unwrap();
        for key in value.as_object().unwrap().keys() {
            assert!(properties.contains_key(key));
        }
    }
}
