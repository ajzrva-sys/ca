use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const TRACE_SCHEMA: &str = "codex-team.learning_trace.v1";
pub const CAPSULE_SCHEMA: &str = "codex-team.learning_capsule.v1";
pub const OBJECTIVE_SCHEMA: &str = "codex-team.improvement_objective.v1";
pub const ARTIFACT_SCHEMA: &str = "codex-team.artifact_revision.v1";
pub const ARTIFACT_CONTENT_SCHEMA: &str = "codex-team.artifact_content.v1";
pub const OVERLAY_SCHEMA: &str = "codex-team.provider_overlay.v1";
pub const EVALUATION_PLAN_SCHEMA: &str = "codex-team.evaluation_plan.v1";
pub const EVALUATION_RECEIPT_SCHEMA: &str = "codex-team.evaluation_receipt.v1";
pub const DEPLOYMENT_CANDIDATE_SCHEMA: &str = "codex-team.deployment_candidate.v1";
pub const DEPLOYMENT_RECEIPT_SCHEMA: &str = "codex-team.deployment_receipt.v1";
pub const REGRESSION_SCHEMA: &str = "codex-team.regression_receipt.v1";
pub const ARTIFACT_CANDIDATE_SCHEMA: &str = "codex-team.artifact_candidate.v1";
pub const WORKFLOW_TRANSITION_SCHEMA: &str = "codex-team.learning_workflow_transition.v1";
pub const REPLAY_RECEIPT_SCHEMA: &str = "codex-team.replay_receipt.v1";
pub const REPLAY_CASE_INPUT_SCHEMA: &str = "codex-team.replay_case_input.v1";

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UsageV1 {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub estimated_microusd: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TimingV1 {
    pub started_at: String,
    pub finished_at: String,
    pub elapsed_millis: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub enum ToolEventV1 {
    Known {
        name: String,
        safe_arguments: BTreeMap<String, Value>,
        result_summary: String,
        success: bool,
    },
    Opaque {
        name: String,
        arguments_sha256: String,
        result_sha256: String,
        success: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LearningTraceV1 {
    pub schema: String,
    pub trace_id: String,
    pub provider: String,
    pub model: String,
    pub role: String,
    pub prompt: String,
    pub response: String,
    pub tool_events: Vec<ToolEventV1>,
    pub task_identity: String,
    pub repository_fingerprint: String,
    pub acceptance_evidence_ids: Vec<String>,
    pub timing: TimingV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageV1>,
    pub outcome: String,
    pub core_revision: String,
    pub overlay_revision: String,
    pub created_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LearningCapsuleV1 {
    pub schema: String,
    pub capsule_id: String,
    pub reusable_problem: String,
    pub decision: String,
    pub result: String,
    pub applicability: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub confidence_basis_points: u16,
    pub supersedes: Option<String>,
    pub created_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveSourceV1 {
    RepeatedFailure,
    MeasuredInefficiency,
    HumanFeedback,
    Roadmap,
    OracleGap,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ImprovementObjectiveV1 {
    pub schema: String,
    pub objective_id: String,
    pub source: ObjectiveSourceV1,
    pub summary: String,
    pub evidence_ids: Vec<String>,
    pub improvement_class: String,
    pub frozen_evaluation_plan_id: String,
    pub repository_fingerprint: Option<String>,
    pub state: String,
    pub cooldown_until: Option<String>,
    pub created_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKindV1 {
    Memory,
    Skill,
    RolePrompt,
    Routing,
    Recipe,
    Source,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactEncodingV1 {
    Utf8,
    Hex,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactFileV1 {
    /// Portable slash-separated path inside the immutable package.
    pub path: String,
    pub encoding: ArtifactEncodingV1,
    pub content: String,
    /// Only explicitly injectable UTF-8 files may enter a later model prompt.
    /// Scripts and binary assets remain inert evidence/package material.
    pub inject_into_prompt: bool,
    pub executable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactContentV1 {
    pub schema: String,
    pub content_id: String,
    pub files: Vec<ArtifactFileV1>,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRevisionV1 {
    pub schema: String,
    pub artifact_id: String,
    pub revision: String,
    pub kind: ArtifactKindV1,
    pub content_tree_sha256: String,
    pub parent_revision: Option<String>,
    pub objective_id: String,
    pub evaluation_receipt_id: String,
    pub applicability: Vec<String>,
    pub retired: bool,
    pub created_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactCandidateLimitsV1 {
    pub maximum_files: u16,
    pub maximum_package_bytes: u64,
    pub maximum_prompt_bytes: u64,
    pub timeout_seconds: u64,
    pub network_allowed: bool,
    pub source_changes_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactCandidateV1 {
    pub schema: String,
    pub candidate_id: String,
    pub objective_id: String,
    pub evaluation_plan_id: String,
    pub author_provider: String,
    pub author_model: String,
    pub kind: ArtifactKindV1,
    pub content_tree_sha256: String,
    pub baseline_revisions: Vec<String>,
    pub source_evidence_ids: Vec<String>,
    pub applicability: Vec<String>,
    pub limits: ArtifactCandidateLimitsV1,
    pub created_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderOverlayV1 {
    pub schema: String,
    pub overlay_id: String,
    pub provider: String,
    pub model: String,
    pub triggering: Vec<String>,
    pub prompt_framing_revision: String,
    pub reasoning_effort: String,
    pub context_packaging_revision: String,
    pub tool_order: Vec<String>,
    pub suitable_roles: Vec<String>,
    pub evaluation_receipt_id: String,
    pub created_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvaluationCaseV1 {
    pub case_id: String,
    pub input_evidence_ids: Vec<String>,
    pub repository_fingerprint: String,
    pub route: String,
    pub grants_digest: String,
    pub acceptance_contract_digest: String,
    pub adversarial: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvaluationPlanV1 {
    pub schema: String,
    pub plan_id: String,
    pub objective_id: String,
    pub baseline_revisions: Vec<String>,
    pub cases: Vec<EvaluationCaseV1>,
    pub deterministic_acceptance: Vec<String>,
    pub available_non_author_judges: Vec<String>,
    pub frozen_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvaluationCaseResultV1 {
    pub case_id: String,
    pub deterministic_pass: bool,
    pub acceptance_properties: BTreeSet<String>,
    pub output_equivalence_digest: String,
    pub elapsed_millis: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_microusd: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct JudgeVoteV1 {
    pub judge_route: String,
    pub author_route: bool,
    pub approves: bool,
    pub rationale_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvaluationReceiptV1 {
    pub schema: String,
    pub receipt_id: String,
    pub plan_id: String,
    pub plan_digest: String,
    pub baseline_results: Vec<EvaluationCaseResultV1>,
    pub candidate_results: Vec<EvaluationCaseResultV1>,
    pub judge_votes: Vec<JudgeVoteV1>,
    pub human_disposition: Option<String>,
    pub promoted: bool,
    pub reasons: Vec<String>,
    pub created_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayLaneV1 {
    Baseline,
    Candidate,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReplayCaseInputV1 {
    pub schema: String,
    pub input_id: String,
    pub source_evidence_ids: Vec<String>,
    pub prompt: String,
    pub expected_properties: Vec<String>,
    pub adversarial: bool,
    pub frozen_by_route: Option<String>,
    pub created_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReplayReceiptV1 {
    pub schema: String,
    pub replay_id: String,
    pub plan_id: String,
    pub candidate_id: String,
    pub case_id: String,
    pub lane: ReplayLaneV1,
    pub route: String,
    pub result: EvaluationCaseResultV1,
    pub bounded_output: String,
    pub evidence_hashes: Vec<String>,
    pub created_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LearningWorkflowStageV1 {
    ObjectiveCreated,
    EvaluationFrozen,
    CandidateReady,
    ReplayingBaseline,
    ReplayingCandidate,
    AwaitingJudgment,
    NeedsReview,
    Promoted,
    Rejected,
    Quarantined,
    CoolingDown,
    WaitingForEvidence,
    BudgetPaused,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LearningWorkflowTransitionV1 {
    pub schema: String,
    pub transition_id: String,
    pub objective_id: String,
    pub from: Option<LearningWorkflowStageV1>,
    pub to: LearningWorkflowStageV1,
    pub candidate_id: Option<String>,
    pub evaluation_plan_id: Option<String>,
    pub evidence_ids: Vec<String>,
    pub failure_fingerprint: Option<String>,
    pub reason: String,
    pub created_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ComponentV1 {
    Ca,
    CodexTeam,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DeploymentCandidateV1 {
    pub schema: String,
    pub candidate_id: String,
    pub component: ComponentV1,
    pub source_commit: String,
    pub binary_sha256: String,
    pub evaluation_receipt_id: String,
    pub protocol_matrix_digest: String,
    pub inactive_slot: String,
    pub created_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DeploymentReceiptV1 {
    pub schema: String,
    pub deployment_id: String,
    pub candidate_id: String,
    pub component: ComponentV1,
    pub previous_slot: String,
    pub active_slot: String,
    pub binary_sha256: String,
    pub source_commit: String,
    pub health_evidence_ids: Vec<String>,
    pub known_good_pair_digest: String,
    pub disposition: String,
    pub created_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RegressionReceiptV1 {
    pub schema: String,
    pub regression_id: String,
    pub component: ComponentV1,
    pub deployment_id: String,
    pub failure_fingerprint: String,
    pub restored_slot: String,
    pub source_revert_commit: Option<String>,
    pub repository_paused: bool,
    pub created_at: String,
    pub digest: String,
}

pub fn canonical_digest<T: Serialize>(value: &T) -> Result<String> {
    let value = serde_json::to_value(value)?;
    let canonical = canonical_json(&value);
    Ok(format!("{:x}", Sha256::digest(canonical.as_bytes())))
}

pub fn verify_digest<T: Serialize>(value: &T, expected: &str) -> Result<()> {
    let actual = canonical_digest(value)?;
    if actual != expected {
        bail!("record digest mismatch: expected {expected}, computed {actual}");
    }
    Ok(())
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value).expect("serialize JSON string"),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Object(values) => {
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(left, _)| *left);
            let body = entries
                .into_iter()
                .map(|(key, value)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).expect("serialize JSON key"),
                        canonical_json(value)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
    }
}

pub fn validate_common_identifier(value: &str, field: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 160
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        bail!("{field} is not a safe bounded identifier");
    }
    Ok(())
}

pub trait VaultRecord: Serialize + Clone {
    fn record_directory(&self) -> &'static str;
    fn record_id(&self) -> &str;
    fn digest(&self) -> &str;
    fn set_digest(&mut self, digest: String);
}

pub fn finalize_record<T: VaultRecord>(record: &mut T) -> Result<()> {
    validate_common_identifier(record.record_id(), "record id")?;
    record.set_digest(String::new());
    record.set_digest(canonical_digest(record)?);
    Ok(())
}

pub fn verify_record<T: VaultRecord>(record: &T) -> Result<()> {
    validate_common_identifier(record.record_id(), "record id")?;
    let expected = record.digest().to_string();
    let mut unsigned = record.clone();
    unsigned.set_digest(String::new());
    verify_digest(&unsigned, &expected)
}

macro_rules! impl_vault_record {
    ($type:ty, $directory:literal, $id:ident) => {
        impl VaultRecord for $type {
            fn record_directory(&self) -> &'static str {
                $directory
            }
            fn record_id(&self) -> &str {
                &self.$id
            }
            fn digest(&self) -> &str {
                &self.digest
            }
            fn set_digest(&mut self, digest: String) {
                self.digest = digest;
            }
        }
    };
}

impl_vault_record!(LearningTraceV1, "traces", trace_id);
impl_vault_record!(LearningCapsuleV1, "capsules", capsule_id);
impl_vault_record!(ImprovementObjectiveV1, "objectives", objective_id);
impl_vault_record!(ArtifactRevisionV1, "artifacts", revision);
impl_vault_record!(ArtifactContentV1, "artifacts/content", content_id);
impl_vault_record!(ArtifactCandidateV1, "candidates", candidate_id);
impl_vault_record!(ProviderOverlayV1, "overlays", overlay_id);
impl_vault_record!(EvaluationPlanV1, "evaluations/plans", plan_id);
impl_vault_record!(EvaluationReceiptV1, "evaluations/receipts", receipt_id);
impl_vault_record!(ReplayReceiptV1, "evaluations/replays", replay_id);
impl_vault_record!(ReplayCaseInputV1, "evaluations/inputs", input_id);
impl_vault_record!(
    LearningWorkflowTransitionV1,
    "workflows/transitions",
    transition_id
);
impl_vault_record!(
    DeploymentCandidateV1,
    "deployments/candidates",
    candidate_id
);
impl_vault_record!(DeploymentReceiptV1, "deployments/receipts", deployment_id);
impl_vault_record!(
    RegressionReceiptV1,
    "deployments/regressions",
    regression_id
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_object_key_order_independent() -> Result<()> {
        let left = serde_json::json!({"b": 2, "a": 1});
        let right = serde_json::json!({"a": 1, "b": 2});
        assert_eq!(canonical_digest(&left)?, canonical_digest(&right)?);
        Ok(())
    }

    #[test]
    fn records_deny_unknown_fields() {
        let value = serde_json::json!({
            "schema": EVALUATION_PLAN_SCHEMA,
            "plan_id": "p1",
            "objective_id": "o1",
            "baseline_revisions": [],
            "cases": [],
            "deterministic_acceptance": [],
            "available_non_author_judges": [],
            "frozen_at": "now",
            "digest": "x",
            "unexpected": true
        });
        assert!(serde_json::from_value::<EvaluationPlanV1>(value).is_err());
    }
}
