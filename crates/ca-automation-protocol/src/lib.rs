//! Versioned, runner-free wire contracts for mechanistic delegation.
//!
//! This crate deliberately contains no repository access, process execution,
//! networking, isolation, or learning authority. Consumers can deserialize and
//! structurally validate requests; ca owns execution through `ca-automation`.

pub mod headless_contract;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};
use thiserror::Error;

pub const MECHANISTIC_REQUEST_SCHEMA: &str = "codex-team.mechanistic_request.v1";
pub const MECHANISTIC_RECEIPT_SCHEMA: &str = "codex-team.mechanistic_receipt.v1";
pub const DELEGATION_MANIFEST_SCHEMA: &str = "codex-team.delegation_manifest.v1";
pub const CA_MECHANISTIC_DELEGATION_SCHEMA: &str = "ca.mechanistic_delegation.v1";
pub const REMOTE_AUTOMATION_REQUEST_SCHEMA: &str = "codex-team.remote_automation_request.v1";
pub const REMOTE_AUTOMATION_RECEIPT_SCHEMA: &str = "codex-team.remote_automation_receipt.v1";
pub const AUTOMATION_SETUP_REQUEST_SCHEMA: &str = "codex-team.automation_setup_request.v1";
pub const AUTOMATION_SETUP_RECEIPT_SCHEMA: &str = "codex-team.automation_setup_receipt.v1";
pub const WORKFLOW_REQUEST_SCHEMA: &str = "codex-team.workflow_request.v1";
pub const WORKFLOW_DRAFT_SCHEMA: &str = "codex-team.workflow_draft.v1";
pub const WORKFLOW_CORRECTION_SCHEMA: &str = "codex-team.workflow_correction.v1";
pub const WORKFLOW_PLAN_RECEIPT_SCHEMA: &str = "codex-team.workflow_plan_receipt.v1";
pub const WORKFLOW_EXECUTION_RECEIPT_SCHEMA: &str = "codex-team.workflow_execution_receipt.v1";
pub const WORKFLOW_CORRECTION_RECEIPT_SCHEMA: &str = "codex-team.workflow_correction_receipt.v1";
pub const MAX_MECHANISTIC_ACTIONS: usize = 8;
pub const MAX_MECHANISTIC_OUTPUT_BYTES: usize = 64 * 1024;
pub const MAX_REMOTE_AUTOMATION_ACTIONS: usize = 4;
pub const MAX_WORKFLOW_ACTIONS: usize = 16;
pub const MAX_WORKFLOW_DEPTH: usize = 8;
pub const MAX_TORRENT_FILES: usize = 10_000;
pub const MAX_TORRENT_LABELS: usize = 32;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TorrentClientKindV1 {
    Transmission,
    Rtorrent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TorrentProtocolV1 {
    TransmissionLegacy,
    TransmissionJsonRpc2,
    RtorrentXmlRpc,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TransmissionProtocolPreferenceV1 {
    #[default]
    Auto,
    Legacy,
    JsonRpc2,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TorrentStateV1 {
    Stopped,
    Checking,
    Downloading,
    Seeding,
    Queued,
    Error,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TorrentPriorityV1 {
    Off,
    Low,
    Normal,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TorrentClientIdentityV1 {
    pub kind: TorrentClientKindV1,
    pub protocol: TorrentProtocolV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TorrentFileV1 {
    pub path: String,
    pub length_bytes: u64,
    pub completed_bytes: u64,
    pub priority: TorrentPriorityV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TorrentSnapshotV1 {
    pub info_hash: String,
    pub name: String,
    pub state: TorrentStateV1,
    pub completed: bool,
    /// Completion in thousandths, from 0 through 1000.
    pub completion_millis: u16,
    pub total_bytes: u64,
    pub left_bytes: u64,
    /// Ratio in thousandths. This avoids floating-point wire ambiguity.
    pub ratio_millis: u64,
    pub priority: TorrentPriorityV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracker_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location_name: Option<String>,
    /// Private-path-independent digest used to detect location drift.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<TorrentFileV1>,
    pub files_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TorrentInventoryV1 {
    pub endpoint: String,
    pub identity: TorrentClientIdentityV1,
    pub torrents: Vec<TorrentSnapshotV1>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TorrentControlActionV1 {
    Start,
    Stop,
    Verify,
    Reannounce,
    SetAutomationLabel {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    SetPriority {
        priority: TorrentPriorityV1,
    },
    AdoptLocation {
        location: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TorrentMutationReceiptV1 {
    pub endpoint: String,
    pub identity: TorrentClientIdentityV1,
    pub action: TorrentControlActionV1,
    pub target_hashes: Vec<String>,
    pub changed_hashes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_hashes_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_hashes_digest: Option<String>,
    pub post_state_confirmed: bool,
    pub ambiguous_transport_resolved: bool,
    #[serde(default)]
    pub truncated: bool,
}

pub fn mechanistic_request_json_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(MechanisticRequestV1))
        .expect("mechanistic request schema must serialize")
}

pub fn mechanistic_action_json_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(MechanisticActionV1))
        .expect("mechanistic action schema must serialize")
}

pub fn remote_automation_request_json_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(RemoteAutomationRequestV1))
        .expect("remote automation request schema must serialize")
}

pub fn automation_setup_request_json_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(AutomationSetupRequestV1))
        .expect("automation setup request schema must serialize")
}

pub fn workflow_request_json_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(WorkflowRequestV1))
        .expect("workflow request schema must serialize")
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowRequestV1 {
    pub schema: String,
    pub operation: WorkflowOperationV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowResponseV1 {
    List {
        workflows: Vec<WorkflowSummaryV1>,
    },
    Plan {
        receipt: WorkflowPlanReceiptV1,
    },
    Correction {
        receipt: WorkflowCorrectionReceiptV1,
        corrected_draft: WorkflowDraftV1,
    },
    Execution {
        receipt: WorkflowExecutionReceiptV1,
    },
    Disabled {
        workflow_id: String,
        workflow_digest: String,
    },
    FeedbackRecorded {
        workflow_digest: String,
        disabled: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowSummaryV1 {
    pub workflow_id: String,
    pub workflow_digest: String,
    pub policy_digest: String,
    pub resource_binding_digest: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_status: Option<WorkflowExecutionStatusV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowOperationV1 {
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workflow_id: Option<String>,
    },
    Preview {
        draft: WorkflowDraftV1,
        depth: WorkflowDryRunDepthV1,
        #[serde(default)]
        correction_round: u8,
    },
    Correct {
        draft: WorkflowDraftV1,
        correction: WorkflowCorrectionV1,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        accepted_correction_digest: Option<String>,
    },
    Run {
        workflow_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        draft: Option<WorkflowDraftV1>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        accepted_plan_digest: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_workflow_digest: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_resource_binding_digest: Option<String>,
    },
    Disable {
        workflow_id: String,
        accepted_workflow_digest: String,
    },
    Feedback {
        workflow_digest: String,
        receipt_digest: String,
        category: WorkflowFeedbackCategoryV1,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowFeedbackCategoryV1 {
    Accepted,
    IncorrectResult,
    Incomplete,
    WrongMatch,
    Unsafe,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDraftV1 {
    pub schema: String,
    pub workflow_id: String,
    pub title: String,
    pub actions: Vec<WorkflowNodeV1>,
    pub limits: WorkflowLimitsV1,
    pub rollback: WorkflowRollbackV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowRollbackV1 {
    Unavailable { limitation: String },
    ResumeOnly { limitation: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowNodeV1 {
    pub id: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub max_retries: u8,
    pub action: WorkflowActionV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowActionV1 {
    ProbeSsh {
        host: String,
    },
    QueryTransmission {
        endpoint: String,
        operation: RemoteRpcOperationV1,
    },
    QueryTorrents {
        endpoint: String,
    },
    SelectTorrents {
        source_action: String,
        #[serde(default = "true_value")]
        completed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        info_hashes: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        states: Vec<TorrentStateV1>,
        maximum_items: u32,
    },
    ControlTorrents {
        endpoint: String,
        selection_action: String,
        control: TorrentControlActionV1,
    },
    CheckFreeSpace {
        endpoint: String,
        minimum_bytes: u64,
    },
    CompareInventory {
        profile: String,
    },
    PlanSync {
        profile: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selection_action: Option<String>,
    },
    ApplySync {
        profile: String,
        plan_action: String,
    },
    VerifySha256 {
        profile: String,
        maximum_files: u32,
    },
    Assert {
        source_action: String,
        condition: WorkflowAssertionV1,
    },
}

impl WorkflowActionV1 {
    pub fn is_mutating(&self) -> bool {
        matches!(self, Self::ApplySync { .. } | Self::ControlTorrents { .. })
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::ProbeSsh { .. } => "probe_ssh",
            Self::QueryTransmission { .. } => "query_transmission",
            Self::QueryTorrents { .. } => "query_torrents",
            Self::SelectTorrents { .. } => "select_torrents",
            Self::ControlTorrents { .. } => "control_torrents",
            Self::CheckFreeSpace { .. } => "check_free_space",
            Self::CompareInventory { .. } => "compare_inventory",
            Self::PlanSync { .. } => "plan_sync",
            Self::ApplySync { .. } => "apply_sync",
            Self::VerifySha256 { .. } => "verify_sha256",
            Self::Assert { .. } => "assert",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowAssertionV1 {
    Empty,
    NonEmpty,
    Completed,
    AtMostItems { maximum: u32 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowDryRunDepthV1 {
    Offline,
    ReadOnlyProbe,
    MutationPreview,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowConflictPolicyV1 {
    Block,
    KeepBoth,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowVerificationV1 {
    EmptySyncPlan,
    Inventory,
    Sha256,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowLimitsV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_transfer_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_files: Option<u32>,
    pub max_runtime_seconds: u64,
    pub min_free_space_bytes: u64,
    pub deletion_allowed: bool,
    pub conflict_policy: WorkflowConflictPolicyV1,
    pub verification: Vec<WorkflowVerificationV1>,
}

impl WorkflowLimitsV1 {
    pub fn authorizes_mutation(&self) -> bool {
        self.max_items.is_some()
            && self.max_transfer_bytes.is_some()
            && !self.verification.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowCorrectionV1 {
    pub schema: String,
    pub source_plan_digest: String,
    pub reason_code: WorkflowCorrectionReasonV1,
    pub scope: WorkflowCorrectionScopeV1,
    pub changes: Vec<WorkflowCorrectionChangeV1>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub correction_round: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowCorrectionReasonV1 {
    WrongDestination,
    SelectionTooBroad,
    UnsafeDeletion,
    MissingVerification,
    LimitTooBroad,
    MissingPrecondition,
    HumanCorrection,
    RepairOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowCorrectionScopeV1 {
    Adapter { adapter: WorkflowAdapterV1 },
    Workflow { workflow_id: String },
    Resource { resource_binding_digest: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowAdapterV1 {
    Ssh,
    Transmission,
    Rtorrent,
    Rsync,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowCorrectionChangeV1 {
    DenyDeletion,
    NarrowTorrentLabel {
        label: String,
    },
    NarrowDestinationPrefix {
        prefix: String,
    },
    LowerMaxItems {
        maximum: u32,
    },
    LowerMaxTransferBytes {
        maximum: u64,
    },
    LowerMaxFiles {
        maximum: u32,
    },
    RequireVerification {
        verification: WorkflowVerificationV1,
    },
    IncreaseMinFreeSpace {
        minimum_bytes: u64,
    },
    AddPrecondition {
        action_id: String,
    },
    PermitDeletion,
    RaiseMaxItems {
        maximum: u32,
    },
    RaiseMaxTransferBytes {
        maximum: u64,
    },
    RaiseMaxFiles {
        maximum: u32,
    },
    RemoveVerification {
        verification: WorkflowVerificationV1,
    },
    ChangeDestination {
        destination: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowCorrectionClassificationV1 {
    Tightening,
    Widening,
    Conflicting,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowPlanStatusV1 {
    Previewed,
    ApprovalRequired,
    Ready,
    Stale,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowActionPlanV1 {
    pub action_id: String,
    pub kind: String,
    pub summary: String,
    #[serde(default)]
    pub predicted_items: u32,
    #[serde(default)]
    pub predicted_bytes: u64,
    #[serde(default)]
    pub uncertainties: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_hashes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torrent_control: Option<TorrentControlActionV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPlanReceiptV1 {
    pub schema: String,
    pub workflow_id: String,
    pub depth: WorkflowDryRunDepthV1,
    pub status: WorkflowPlanStatusV1,
    pub workflow_digest: String,
    pub policy_digest: String,
    pub resource_binding_digest: String,
    pub run_plan_digest: String,
    pub action_plans: Vec<WorkflowActionPlanV1>,
    pub predicted_items: u32,
    pub predicted_bytes: u64,
    pub mutation_present: bool,
    pub limits_authorize_mutation: bool,
    pub approval_required: bool,
    pub rollback: WorkflowRollbackV1,
    pub correction_round: u8,
    #[serde(default)]
    pub reason_codes: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowCorrectionReceiptV1 {
    pub schema: String,
    pub workflow_id: String,
    pub classification: WorkflowCorrectionClassificationV1,
    pub correction_digest: String,
    pub policy_digest: String,
    pub persisted: bool,
    pub approval_required: bool,
    pub correction_round: u8,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowExecutionStatusV1 {
    Completed,
    Failed,
    Blocked,
    Stale,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRepairCauseV1 {
    InterruptedTransfer,
    TransientTransport,
    ChecksumMismatch,
    InsufficientSpace,
    Authentication,
    IdentityChanged,
    Conflict,
    PolicyViolation,
    Indeterminate,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowActionExecutionV1 {
    pub action_id: String,
    pub status: WorkflowExecutionStatusV1,
    pub summary: String,
    pub attempts: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair_cause: Option<WorkflowRepairCauseV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torrent_mutation: Option<TorrentMutationReceiptV1>,
    pub duration_millis: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowExecutionReceiptV1 {
    pub schema: String,
    pub workflow_id: String,
    pub workflow_digest: String,
    pub policy_digest: String,
    pub resource_binding_digest: String,
    pub run_plan_digest: String,
    pub status: WorkflowExecutionStatusV1,
    pub action_results: Vec<WorkflowActionExecutionV1>,
    pub deterministic_repairs: u8,
    pub verification_passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback: Option<WorkflowRollbackV1>,
    #[serde(default)]
    pub deviations: Vec<String>,
    #[serde(default)]
    pub reason_codes: Vec<String>,
    pub started_at: String,
    pub finished_at: String,
    pub elapsed_millis: u64,
}

fn true_value() -> bool {
    true
}

pub fn validate_workflow_request(
    request: &WorkflowRequestV1,
) -> Result<(), StructuralValidationError> {
    if request.schema != WORKFLOW_REQUEST_SCHEMA {
        return Err(StructuralValidationError::new(
            "unsupported workflow request schema",
        ));
    }
    match &request.operation {
        WorkflowOperationV1::List { workflow_id } => {
            if let Some(workflow_id) = workflow_id {
                validate_remote_name("workflow ID", workflow_id)?;
            }
        }
        WorkflowOperationV1::Preview {
            draft,
            correction_round,
            ..
        } => {
            validate_workflow_draft(draft)?;
            validate_correction_round(*correction_round)?;
        }
        WorkflowOperationV1::Correct {
            draft,
            correction,
            accepted_correction_digest,
        } => {
            validate_workflow_draft(draft)?;
            validate_workflow_correction(correction)?;
            if let Some(digest) = accepted_correction_digest {
                validate_lower_sha256("accepted_correction_digest", digest)?;
            }
        }
        WorkflowOperationV1::Run {
            workflow_id,
            draft,
            accepted_plan_digest,
            expected_workflow_digest,
            expected_resource_binding_digest,
        } => {
            validate_remote_name("workflow ID", workflow_id)?;
            if let Some(draft) = draft {
                validate_workflow_draft(draft)?;
                if draft.workflow_id != *workflow_id {
                    return Err(StructuralValidationError::new(
                        "run workflow ID does not match its draft",
                    ));
                }
            }
            if let Some(digest) = accepted_plan_digest {
                validate_lower_sha256("accepted_plan_digest", digest)?;
            }
            if let Some(digest) = expected_workflow_digest {
                validate_lower_sha256("expected_workflow_digest", digest)?;
            }
            if let Some(digest) = expected_resource_binding_digest {
                validate_lower_sha256("expected_resource_binding_digest", digest)?;
            }
        }
        WorkflowOperationV1::Disable {
            workflow_id,
            accepted_workflow_digest,
        } => {
            validate_remote_name("workflow ID", workflow_id)?;
            validate_lower_sha256("accepted_workflow_digest", accepted_workflow_digest)?;
        }
        WorkflowOperationV1::Feedback {
            workflow_digest,
            receipt_digest,
            ..
        } => {
            validate_lower_sha256("workflow_digest", workflow_digest)?;
            validate_lower_sha256("receipt_digest", receipt_digest)?;
        }
    }
    Ok(())
}

pub fn validate_workflow_draft(draft: &WorkflowDraftV1) -> Result<(), StructuralValidationError> {
    if draft.schema != WORKFLOW_DRAFT_SCHEMA {
        return Err(StructuralValidationError::new(
            "unsupported workflow draft schema",
        ));
    }
    validate_remote_name("workflow ID", &draft.workflow_id)?;
    validate_safe_setup_text("workflow title", &draft.title, 256)?;
    if draft.actions.is_empty() || draft.actions.len() > MAX_WORKFLOW_ACTIONS {
        return Err(StructuralValidationError::new(format!(
            "workflow must contain 1-{MAX_WORKFLOW_ACTIONS} actions"
        )));
    }
    let rollback_limitation = match &draft.rollback {
        WorkflowRollbackV1::Unavailable { limitation }
        | WorkflowRollbackV1::ResumeOnly { limitation } => limitation,
    };
    validate_safe_setup_text("workflow rollback limitation", rollback_limitation, 512)?;
    if draft.limits.max_runtime_seconds == 0 || draft.limits.max_runtime_seconds > 7_200 {
        return Err(StructuralValidationError::new(
            "workflow runtime must be between 1 and 7200 seconds",
        ));
    }
    if draft
        .limits
        .max_items
        .is_some_and(|value| value == 0 || value > 10_000)
    {
        return Err(StructuralValidationError::new(
            "workflow max_items must be between 1 and 10000",
        ));
    }
    if draft.limits.max_transfer_bytes == Some(0) {
        return Err(StructuralValidationError::new(
            "workflow max_transfer_bytes must be non-zero",
        ));
    }
    if draft
        .limits
        .max_files
        .is_some_and(|value| value == 0 || value as usize > MAX_TORRENT_FILES)
    {
        return Err(StructuralValidationError::new(
            "workflow max_files must be between 1 and 10000",
        ));
    }
    if draft.limits.verification.len() > 3 {
        return Err(StructuralValidationError::new(
            "workflow accepts at most three verification requirements",
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    for node in &draft.actions {
        validate_remote_name("workflow action ID", &node.id)?;
        if !ids.insert(node.id.as_str()) {
            return Err(StructuralValidationError::new(
                "workflow action IDs must be unique",
            ));
        }
        if node.max_retries > 2 {
            return Err(StructuralValidationError::new(
                "workflow action retries must be between 0 and 2",
            ));
        }
        let mut dependencies = std::collections::BTreeSet::new();
        for dependency in &node.depends_on {
            validate_remote_name("workflow dependency", dependency)?;
            if dependency == &node.id || !dependencies.insert(dependency) {
                return Err(StructuralValidationError::new(
                    "workflow dependencies must be unique and cannot reference themselves",
                ));
            }
        }
        validate_workflow_action(&node.action)?;
    }
    let by_id = draft
        .actions
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<std::collections::BTreeMap<_, _>>();
    for node in &draft.actions {
        for dependency in &node.depends_on {
            if !by_id.contains_key(dependency.as_str()) {
                return Err(StructuralValidationError::new(
                    "workflow dependency references an unknown action",
                ));
            }
        }
        for reference in workflow_action_references(&node.action) {
            if !by_id.contains_key(reference)
                || !node.depends_on.iter().any(|item| item == reference)
            {
                return Err(StructuralValidationError::new(
                    "workflow action references must name an explicit dependency",
                ));
            }
        }
    }
    let mut memo = std::collections::BTreeMap::new();
    let mut visiting = std::collections::BTreeSet::new();
    for id in by_id.keys() {
        let depth = workflow_depth(id, &by_id, &mut memo, &mut visiting)?;
        if depth > MAX_WORKFLOW_DEPTH {
            return Err(StructuralValidationError::new(format!(
                "workflow depth exceeds {MAX_WORKFLOW_DEPTH}"
            )));
        }
    }
    Ok(())
}

fn validate_workflow_action(action: &WorkflowActionV1) -> Result<(), StructuralValidationError> {
    match action {
        WorkflowActionV1::ProbeSsh { host } => validate_remote_name("SSH host", host),
        WorkflowActionV1::QueryTransmission { endpoint, .. }
        | WorkflowActionV1::QueryTorrents { endpoint }
        | WorkflowActionV1::CheckFreeSpace { endpoint, .. } => {
            validate_remote_name("torrent endpoint", endpoint)
        }
        WorkflowActionV1::SelectTorrents {
            source_action,
            label,
            info_hashes,
            states,
            maximum_items,
            ..
        } => {
            validate_remote_name("source action", source_action)?;
            if *maximum_items == 0 || *maximum_items > 10_000 {
                return Err(StructuralValidationError::new(
                    "torrent selection must contain 1-10000 items",
                ));
            }
            if let Some(label) = label {
                validate_safe_setup_text("torrent label", label, 128)?;
            }
            if info_hashes.len() > *maximum_items as usize {
                return Err(StructuralValidationError::new(
                    "explicit torrent hashes exceed the selector item bound",
                ));
            }
            let mut hashes = std::collections::BTreeSet::new();
            for hash in info_hashes {
                validate_info_hash(hash)?;
                if !hashes.insert(hash) {
                    return Err(StructuralValidationError::new(
                        "explicit torrent hashes must be unique",
                    ));
                }
            }
            let mut selected_states = std::collections::BTreeSet::new();
            for state in states {
                if !selected_states.insert(format!("{state:?}")) {
                    return Err(StructuralValidationError::new(
                        "torrent selector states must be unique",
                    ));
                }
            }
            Ok(())
        }
        WorkflowActionV1::ControlTorrents {
            endpoint,
            selection_action,
            control,
        } => {
            validate_remote_name("torrent endpoint", endpoint)?;
            validate_remote_name("torrent selection action", selection_action)?;
            match control {
                TorrentControlActionV1::SetAutomationLabel { label: Some(label) } => {
                    validate_safe_setup_text("torrent automation label", label, 128)?
                }
                TorrentControlActionV1::AdoptLocation { location } => {
                    validate_remote_name("torrent location", location)?
                }
                _ => {}
            }
            Ok(())
        }
        WorkflowActionV1::CompareInventory { profile }
        | WorkflowActionV1::PlanSync { profile, .. } => {
            validate_remote_name("sync profile", profile)
        }
        WorkflowActionV1::ApplySync {
            profile,
            plan_action,
        } => {
            validate_remote_name("sync profile", profile)?;
            validate_remote_name("plan action", plan_action)
        }
        WorkflowActionV1::VerifySha256 {
            profile,
            maximum_files,
        } => {
            validate_remote_name("sync profile", profile)?;
            if *maximum_files == 0 || *maximum_files > 10_000 {
                return Err(StructuralValidationError::new(
                    "SHA-256 verification must contain 1-10000 files",
                ));
            }
            Ok(())
        }
        WorkflowActionV1::Assert {
            source_action,
            condition,
        } => {
            validate_remote_name("source action", source_action)?;
            if matches!(condition, WorkflowAssertionV1::AtMostItems { maximum: 0 }) {
                return Err(StructuralValidationError::new(
                    "assertion maximum must be non-zero",
                ));
            }
            Ok(())
        }
    }
}

fn workflow_action_references(action: &WorkflowActionV1) -> Vec<&str> {
    match action {
        WorkflowActionV1::SelectTorrents { source_action, .. }
        | WorkflowActionV1::Assert { source_action, .. } => vec![source_action],
        WorkflowActionV1::ControlTorrents {
            selection_action, ..
        } => vec![selection_action],
        WorkflowActionV1::PlanSync {
            selection_action: Some(selection_action),
            ..
        } => vec![selection_action],
        WorkflowActionV1::ApplySync { plan_action, .. } => vec![plan_action],
        _ => Vec::new(),
    }
}

fn workflow_depth<'a>(
    id: &'a str,
    actions: &std::collections::BTreeMap<&'a str, &'a WorkflowNodeV1>,
    memo: &mut std::collections::BTreeMap<&'a str, usize>,
    visiting: &mut std::collections::BTreeSet<&'a str>,
) -> Result<usize, StructuralValidationError> {
    if let Some(depth) = memo.get(id) {
        return Ok(*depth);
    }
    if !visiting.insert(id) {
        return Err(StructuralValidationError::new(
            "workflow dependency graph contains a cycle",
        ));
    }
    let node = actions
        .get(id)
        .ok_or_else(|| StructuralValidationError::new("workflow action is missing"))?;
    let mut depth = 1;
    for dependency in &node.depends_on {
        depth = depth.max(workflow_depth(dependency, actions, memo, visiting)?.saturating_add(1));
    }
    visiting.remove(id);
    memo.insert(id, depth);
    Ok(depth)
}

pub fn validate_workflow_correction(
    correction: &WorkflowCorrectionV1,
) -> Result<(), StructuralValidationError> {
    if correction.schema != WORKFLOW_CORRECTION_SCHEMA {
        return Err(StructuralValidationError::new(
            "unsupported workflow correction schema",
        ));
    }
    validate_lower_sha256("source_plan_digest", &correction.source_plan_digest)?;
    validate_correction_round(correction.correction_round)?;
    if correction.correction_round == 0 {
        return Err(StructuralValidationError::new(
            "workflow correction round must be 1 or 2",
        ));
    }
    if correction.changes.is_empty() || correction.changes.len() > 8 {
        return Err(StructuralValidationError::new(
            "workflow correction must contain 1-8 changes",
        ));
    }
    if correction.evidence_refs.len() > 8 {
        return Err(StructuralValidationError::new(
            "workflow correction accepts at most eight evidence references",
        ));
    }
    for evidence in &correction.evidence_refs {
        validate_lower_sha256("evidence reference", evidence)?;
    }
    match &correction.scope {
        WorkflowCorrectionScopeV1::Adapter { .. } => {
            if correction.changes.iter().any(|change| {
                matches!(
                    change,
                    WorkflowCorrectionChangeV1::NarrowTorrentLabel { .. }
                        | WorkflowCorrectionChangeV1::NarrowDestinationPrefix { .. }
                        | WorkflowCorrectionChangeV1::ChangeDestination { .. }
                        | WorkflowCorrectionChangeV1::AddPrecondition { .. }
                )
            }) {
                return Err(StructuralValidationError::new(
                    "target-specific workflow corrections require workflow or resource scope",
                ));
            }
        }
        WorkflowCorrectionScopeV1::Workflow { workflow_id } => {
            validate_remote_name("workflow ID", workflow_id)?
        }
        WorkflowCorrectionScopeV1::Resource {
            resource_binding_digest,
        } => validate_lower_sha256("resource binding digest", resource_binding_digest)?,
    }
    for change in &correction.changes {
        match change {
            WorkflowCorrectionChangeV1::NarrowTorrentLabel { label } => {
                validate_safe_setup_text("torrent label", label, 128)?
            }
            WorkflowCorrectionChangeV1::NarrowDestinationPrefix { prefix } => {
                validate_absolute_constraint_path(prefix)?
            }
            WorkflowCorrectionChangeV1::LowerMaxItems { maximum }
            | WorkflowCorrectionChangeV1::RaiseMaxItems { maximum }
            | WorkflowCorrectionChangeV1::LowerMaxFiles { maximum }
            | WorkflowCorrectionChangeV1::RaiseMaxFiles { maximum }
                if *maximum == 0 || *maximum > 10_000 =>
            {
                return Err(StructuralValidationError::new(
                    "corrected item limit must be between 1 and 10000",
                ));
            }
            WorkflowCorrectionChangeV1::LowerMaxTransferBytes { maximum }
            | WorkflowCorrectionChangeV1::RaiseMaxTransferBytes { maximum }
                if *maximum == 0 =>
            {
                return Err(StructuralValidationError::new(
                    "corrected transfer limit must be non-zero",
                ));
            }
            WorkflowCorrectionChangeV1::IncreaseMinFreeSpace { minimum_bytes }
                if *minimum_bytes == 0 =>
            {
                return Err(StructuralValidationError::new(
                    "corrected free-space minimum must be non-zero",
                ));
            }
            WorkflowCorrectionChangeV1::AddPrecondition { action_id } => {
                validate_remote_name("precondition action", action_id)?
            }
            WorkflowCorrectionChangeV1::ChangeDestination { destination } => {
                validate_safe_setup_text("destination", destination, 1_024)?
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn classify_workflow_correction(
    correction: &WorkflowCorrectionV1,
) -> WorkflowCorrectionClassificationV1 {
    let tightening = correction.changes.iter().all(|change| {
        matches!(
            change,
            WorkflowCorrectionChangeV1::DenyDeletion
                | WorkflowCorrectionChangeV1::NarrowTorrentLabel { .. }
                | WorkflowCorrectionChangeV1::NarrowDestinationPrefix { .. }
                | WorkflowCorrectionChangeV1::LowerMaxItems { .. }
                | WorkflowCorrectionChangeV1::LowerMaxTransferBytes { .. }
                | WorkflowCorrectionChangeV1::LowerMaxFiles { .. }
                | WorkflowCorrectionChangeV1::RequireVerification { .. }
                | WorkflowCorrectionChangeV1::IncreaseMinFreeSpace { .. }
                | WorkflowCorrectionChangeV1::AddPrecondition { .. }
        )
    });
    if tightening {
        WorkflowCorrectionClassificationV1::Tightening
    } else if correction.changes.iter().any(|change| {
        matches!(
            change,
            WorkflowCorrectionChangeV1::DenyDeletion
                | WorkflowCorrectionChangeV1::NarrowTorrentLabel { .. }
                | WorkflowCorrectionChangeV1::NarrowDestinationPrefix { .. }
                | WorkflowCorrectionChangeV1::LowerMaxItems { .. }
                | WorkflowCorrectionChangeV1::LowerMaxTransferBytes { .. }
                | WorkflowCorrectionChangeV1::LowerMaxFiles { .. }
                | WorkflowCorrectionChangeV1::RequireVerification { .. }
                | WorkflowCorrectionChangeV1::IncreaseMinFreeSpace { .. }
                | WorkflowCorrectionChangeV1::AddPrecondition { .. }
        )
    }) {
        WorkflowCorrectionClassificationV1::Conflicting
    } else {
        WorkflowCorrectionClassificationV1::Widening
    }
}

fn validate_correction_round(round: u8) -> Result<(), StructuralValidationError> {
    if round > 2 {
        return Err(StructuralValidationError::new(
            "workflow correction round must be between 0 and 2",
        ));
    }
    Ok(())
}

fn validate_absolute_constraint_path(value: &str) -> Result<(), StructuralValidationError> {
    validate_safe_setup_text("destination prefix", value, 1_024)?;
    let path = Path::new(value);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(StructuralValidationError::new(
            "destination prefix must be one normalized absolute path",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AutomationSetupRequestV1 {
    pub schema: String,
    pub operation: AutomationSetupOperationV1,
    pub draft: AutomationSetupDraftV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationSetupOperationV1 {
    Preview,
    Apply { accepted_plan_digest: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "template", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationSetupDraftV1 {
    RemoteAutomation {
        manifest_name: String,
        #[serde(default)]
        ssh_hosts: Vec<AutomationSshHostV1>,
        #[serde(default)]
        transmission: Vec<AutomationTransmissionEndpointV1>,
        #[serde(default)]
        rtorrent: Vec<AutomationRtorrentEndpointV1>,
        #[serde(default)]
        torrent_locations: Vec<AutomationTorrentLocationV1>,
        #[serde(default)]
        syncs: Vec<AutomationSyncProfileV1>,
    },
}

impl AutomationSetupDraftV1 {
    pub fn manifest_name(&self) -> &str {
        match self {
            Self::RemoteAutomation { manifest_name, .. } => manifest_name,
        }
    }

    pub fn template_name(&self) -> &'static str {
        match self {
            Self::RemoteAutomation { .. } => "remote_automation",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AutomationSshHostV1 {
    pub name: String,
    pub destination: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AutomationTransmissionEndpointV1 {
    pub name: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_env: Option<String>,
    #[serde(default)]
    pub protocol: TransmissionProtocolPreferenceV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RtorrentCustomSlotV1 {
    Custom1,
    Custom2,
    Custom3,
    Custom4,
    Custom5,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationRtorrentTransportV1 {
    HttpXmlRpc {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        username_env: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password_env: Option<String>,
    },
    UnixScgi {
        socket: String,
    },
    LoopbackTcpScgi {
        port: u16,
    },
    SshLoopbackScgi {
        ssh_host: String,
        port: u16,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AutomationRtorrentEndpointV1 {
    pub name: String,
    pub transport: AutomationRtorrentTransportV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_label_slot: Option<RtorrentCustomSlotV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AutomationTorrentLocationV1 {
    pub name: String,
    pub endpoint: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AutomationSyncProfileV1 {
    pub name: String,
    /// `local` or an SSH host name declared in this draft.
    pub controller: String,
    pub source: String,
    pub destination: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torrent_endpoint: Option<String>,
    #[serde(default)]
    pub delete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_changes: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationSetupStatusV1 {
    Previewed,
    Applied,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AutomationSetupReceiptV1 {
    pub schema: String,
    pub status: AutomationSetupStatusV1,
    pub template: String,
    pub manifest_name: String,
    pub plan_digest: String,
    pub effects: Vec<String>,
    pub environment_names: Vec<String>,
    pub profile_reload_required: bool,
}

pub fn validate_automation_setup_request(
    request: &AutomationSetupRequestV1,
) -> Result<(), StructuralValidationError> {
    if request.schema != AUTOMATION_SETUP_REQUEST_SCHEMA {
        return Err(StructuralValidationError::new(
            "unsupported automation setup request schema",
        ));
    }
    if let AutomationSetupOperationV1::Apply {
        accepted_plan_digest,
    } = &request.operation
    {
        validate_lower_sha256("accepted_plan_digest", accepted_plan_digest)?;
    }
    match &request.draft {
        AutomationSetupDraftV1::RemoteAutomation {
            manifest_name,
            ssh_hosts,
            transmission,
            rtorrent,
            torrent_locations,
            syncs,
        } => {
            validate_remote_name("manifest name", manifest_name)?;
            if ssh_hosts.len() > 8
                || transmission.len() > 8
                || rtorrent.len() > 8
                || torrent_locations.len() > 16
                || syncs.len() > 8
            {
                return Err(StructuralValidationError::new(
                    "automation setup bounds SSH/endpoint/sync entries to eight and locations to sixteen",
                ));
            }
            if ssh_hosts.is_empty()
                && transmission.is_empty()
                && rtorrent.is_empty()
                && torrent_locations.is_empty()
                && syncs.is_empty()
            {
                return Err(StructuralValidationError::new(
                    "remote automation setup requires at least one capability",
                ));
            }
            let mut names = std::collections::BTreeSet::new();
            for host in ssh_hosts {
                validate_remote_name("SSH host name", &host.name)?;
                if !names.insert(host.name.as_str()) {
                    return Err(StructuralValidationError::new("duplicate SSH host name"));
                }
                validate_safe_setup_text("SSH destination", &host.destination, 255)?;
                if host.destination.bytes().any(|byte| {
                    !(byte.is_ascii_alphanumeric()
                        || matches!(byte, b'@' | b'.' | b'_' | b'-' | b':' | b'[' | b']'))
                }) {
                    return Err(StructuralValidationError::new(
                        "SSH destination contains unsupported bytes",
                    ));
                }
                if host.port == Some(0) {
                    return Err(StructuralValidationError::new("SSH port must be non-zero"));
                }
            }
            names.clear();
            for endpoint in transmission {
                validate_remote_name("Transmission endpoint name", &endpoint.name)?;
                if !names.insert(endpoint.name.as_str()) {
                    return Err(StructuralValidationError::new(
                        "torrent endpoint names must be unique across clients",
                    ));
                }
                validate_safe_setup_text("Transmission URL", &endpoint.url, 2_048)?;
                if let Some(name) = &endpoint.username_env {
                    validate_environment_name(name)?;
                }
                if let Some(name) = &endpoint.password_env {
                    validate_environment_name(name)?;
                }
                if endpoint
                    .timeout_seconds
                    .is_some_and(|value| value == 0 || value > 7_200)
                {
                    return Err(StructuralValidationError::new(
                        "Transmission timeout must be between 1 and 7200 seconds",
                    ));
                }
            }
            for endpoint in rtorrent {
                validate_remote_name("rTorrent endpoint name", &endpoint.name)?;
                if !names.insert(endpoint.name.as_str()) {
                    return Err(StructuralValidationError::new(
                        "torrent endpoint names must be unique across clients",
                    ));
                }
                if endpoint
                    .timeout_seconds
                    .is_some_and(|value| value == 0 || value > 7_200)
                {
                    return Err(StructuralValidationError::new(
                        "rTorrent timeout must be between 1 and 7200 seconds",
                    ));
                }
                match &endpoint.transport {
                    AutomationRtorrentTransportV1::HttpXmlRpc {
                        url,
                        username_env,
                        password_env,
                    } => {
                        validate_safe_setup_text("rTorrent URL", url, 2_048)?;
                        match (username_env, password_env) {
                            (None, None) => {}
                            (Some(username), Some(password)) => {
                                validate_environment_name(username)?;
                                validate_environment_name(password)?;
                            }
                            _ => {
                                return Err(StructuralValidationError::new(
                                    "rTorrent HTTP credentials must name both username and password environment variables",
                                ));
                            }
                        }
                    }
                    AutomationRtorrentTransportV1::UnixScgi { socket } => {
                        validate_absolute_constraint_path(socket)?;
                    }
                    AutomationRtorrentTransportV1::LoopbackTcpScgi { port } => {
                        if *port == 0 {
                            return Err(StructuralValidationError::new(
                                "rTorrent SCGI port must be non-zero",
                            ));
                        }
                    }
                    AutomationRtorrentTransportV1::SshLoopbackScgi { ssh_host, port } => {
                        validate_remote_name("rTorrent SSH host", ssh_host)?;
                        if *port == 0 || !ssh_hosts.iter().any(|host| host.name == *ssh_host) {
                            return Err(StructuralValidationError::new(
                                "rTorrent SSH SCGI requires a declared SSH host and non-zero loopback port",
                            ));
                        }
                    }
                }
            }
            let torrent_endpoint_names = names.clone();
            names.clear();
            for location in torrent_locations {
                validate_remote_name("torrent location name", &location.name)?;
                if !names.insert(location.name.as_str()) {
                    return Err(StructuralValidationError::new(
                        "duplicate torrent location name",
                    ));
                }
                if !torrent_endpoint_names.contains(location.endpoint.as_str()) {
                    return Err(StructuralValidationError::new(
                        "torrent location must name a declared torrent endpoint",
                    ));
                }
                validate_absolute_constraint_path(&location.path)?;
            }
            names.clear();
            for sync in syncs {
                validate_remote_name("sync profile name", &sync.name)?;
                if !names.insert(sync.name.as_str()) {
                    return Err(StructuralValidationError::new(
                        "duplicate sync profile name",
                    ));
                }
                if sync.controller != "local"
                    && !ssh_hosts.iter().any(|host| host.name == sync.controller)
                {
                    return Err(StructuralValidationError::new(
                        "sync controller must be local or a declared SSH host",
                    ));
                }
                validate_safe_setup_text("sync source", &sync.source, 1_024)?;
                validate_safe_setup_text("sync destination", &sync.destination, 1_024)?;
                if let Some(endpoint) = &sync.torrent_endpoint {
                    if !torrent_endpoint_names.contains(endpoint.as_str()) {
                        return Err(StructuralValidationError::new(
                            "sync torrent endpoint must name a declared torrent endpoint",
                        ));
                    }
                }
                if sync
                    .timeout_seconds
                    .is_some_and(|value| value == 0 || value > 7_200)
                {
                    return Err(StructuralValidationError::new(
                        "sync timeout must be between 1 and 7200 seconds",
                    ));
                }
                if sync
                    .max_changes
                    .is_some_and(|value| value == 0 || value > 10_000)
                {
                    return Err(StructuralValidationError::new(
                        "sync max_changes must be between 1 and 10000",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_environment_name(value: &str) -> Result<(), StructuralValidationError> {
    if value.is_empty()
        || value.len() > 64
        || !value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_uppercase() || byte == b'_' || (index > 0 && byte.is_ascii_digit())
        })
    {
        return Err(StructuralValidationError::new(
            "credential environment names must use 1-64 uppercase ASCII identifier bytes",
        ));
    }
    Ok(())
}

fn validate_safe_setup_text(
    label: &str,
    value: &str,
    maximum: usize,
) -> Result<(), StructuralValidationError> {
    if value.is_empty()
        || value.len() > maximum
        || value.chars().any(|character| character.is_control())
    {
        return Err(StructuralValidationError::new(format!(
            "{label} must contain 1-{maximum} non-control bytes"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RemoteAutomationRequestV1 {
    pub schema: String,
    pub actions: Vec<RemoteAutomationActionV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteAutomationActionV1 {
    ListRemoteAutomation,
    CompareRemoteFiles {
        /// Exact sync profile name from the private remote manifest.
        profile: String,
    },
    PlanRemoteSync {
        /// Exact sync profile name from the private remote manifest.
        profile: String,
    },
    ApplyRemoteSync {
        /// Exact sync profile name from the private remote manifest.
        profile: String,
        /// Fresh digest returned by plan_remote_sync for this profile.
        accepted_plan_digest: String,
    },
    QueryRemoteRpc {
        /// Exact RPC endpoint name from the private remote manifest.
        endpoint: String,
        operation: RemoteRpcOperationV1,
    },
    CompareRemoteRpc {
        /// Exact RPC endpoint names from the private remote manifest.
        left_endpoint: String,
        right_endpoint: String,
        operation: RemoteRpcOperationV1,
    },
}

impl RemoteAutomationActionV1 {
    pub fn is_mutating(&self) -> bool {
        matches!(self, Self::ApplyRemoteSync { .. })
    }

    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::ListRemoteAutomation => "list_remote_automation",
            Self::CompareRemoteFiles { .. } => "compare_remote_files",
            Self::PlanRemoteSync { .. } => "plan_remote_sync",
            Self::ApplyRemoteSync { .. } => "apply_remote_sync",
            Self::QueryRemoteRpc { .. } => "query_remote_rpc",
            Self::CompareRemoteRpc { .. } => "compare_remote_rpc",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteRpcOperationV1 {
    Session,
    Torrents,
    FreeSpace,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAutomationStatusV1 {
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RemoteAutomationActionReceiptV1 {
    pub action_index: u8,
    pub kind: String,
    pub status: RemoteAutomationStatusV1,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<String>,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torrent_inventory: Option<TorrentInventoryV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torrent_mutation: Option<TorrentMutationReceiptV1>,
    pub duration_millis: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RemoteAutomationReceiptV1 {
    pub schema: String,
    pub request_digest: String,
    pub status: RemoteAutomationStatusV1,
    pub action_results: Vec<RemoteAutomationActionReceiptV1>,
    pub output_bytes: u64,
    pub started_at: String,
    pub finished_at: String,
    pub elapsed_millis: u64,
}

pub fn validate_remote_automation_request(
    request: &RemoteAutomationRequestV1,
) -> Result<(), StructuralValidationError> {
    if request.schema != REMOTE_AUTOMATION_REQUEST_SCHEMA {
        return Err(StructuralValidationError::new(
            "unsupported remote automation request schema",
        ));
    }
    if request.actions.is_empty() || request.actions.len() > MAX_REMOTE_AUTOMATION_ACTIONS {
        return Err(StructuralValidationError::new(format!(
            "remote automation request must contain 1-{MAX_REMOTE_AUTOMATION_ACTIONS} actions"
        )));
    }
    let has_mutation = request
        .actions
        .iter()
        .any(RemoteAutomationActionV1::is_mutating);
    if has_mutation && request.actions.len() != 1 {
        return Err(StructuralValidationError::new(
            "remote sync application must be the only action in a request",
        ));
    }
    for action in &request.actions {
        match action {
            RemoteAutomationActionV1::ListRemoteAutomation => {}
            RemoteAutomationActionV1::CompareRemoteFiles { profile }
            | RemoteAutomationActionV1::PlanRemoteSync { profile } => {
                validate_remote_name("remote profile", profile)?;
            }
            RemoteAutomationActionV1::ApplyRemoteSync {
                profile,
                accepted_plan_digest,
            } => {
                validate_remote_name("remote profile", profile)?;
                validate_lower_sha256("accepted_plan_digest", accepted_plan_digest)?;
            }
            RemoteAutomationActionV1::QueryRemoteRpc { endpoint, .. } => {
                validate_remote_name("RPC endpoint", endpoint)?;
            }
            RemoteAutomationActionV1::CompareRemoteRpc {
                left_endpoint,
                right_endpoint,
                ..
            } => {
                validate_remote_name("left RPC endpoint", left_endpoint)?;
                validate_remote_name("right RPC endpoint", right_endpoint)?;
                if left_endpoint == right_endpoint {
                    return Err(StructuralValidationError::new(
                        "RPC comparison requires two distinct endpoints",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_remote_name(label: &str, value: &str) -> Result<(), StructuralValidationError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(StructuralValidationError::new(format!(
            "{label} must contain 1-64 safe ASCII identifier bytes"
        )));
    }
    Ok(())
}

fn validate_lower_sha256(label: &str, value: &str) -> Result<(), StructuralValidationError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(StructuralValidationError::new(format!(
            "{label} must be one lowercase SHA-256 digest"
        )));
    }
    Ok(())
}

pub fn validate_info_hash(value: &str) -> Result<(), StructuralValidationError> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(StructuralValidationError::new(
            "torrent info hash must be exactly 40 lowercase hexadecimal characters",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DelegationPolicy {
    #[default]
    Legacy,
    Prefer,
    Require,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MechanisticRequestV1 {
    pub schema: String,
    pub expected_revision: String,
    #[serde(default)]
    pub delegation_policy: DelegationPolicy,
    #[serde(default, skip_serializing_if = "is_ordinary_purpose")]
    pub purpose: MechanisticRequestPurposeV1,
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_unisolated_evidence: bool,
    pub actions: Vec<MechanisticActionV1>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_ordinary_purpose(value: &MechanisticRequestPurposeV1) -> bool {
    *value == MechanisticRequestPurposeV1::Ordinary
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MechanisticRequestPurposeV1 {
    #[default]
    Ordinary,
    AutomaticPreflight,
    AutomaticVerification,
    TransparentTool,
    PredictiveWarmup,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SearchMatchModeV1 {
    #[default]
    Literal,
    Regex,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationScopeV1 {
    #[default]
    Focused,
    Full,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MechanisticConfidenceV1 {
    High,
    Medium,
    Low,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MechanisticCacheStatusV1 {
    NotApplicable,
    Miss,
    Hit,
    Ineligible,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MechanisticBrokerDispositionV1 {
    Executed,
    Attached,
    CacheHit,
    Bypassed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MechanisticBrokerReceiptV1 {
    pub disposition: MechanisticBrokerDispositionV1,
    pub work_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_receipt_digest: Option<String>,
    pub consumer_count: u16,
    pub avoided_execution_millis: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rust_api_baseline_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MechanisticRepositoryBindingV1 {
    /// Host-only canonical repository root. CA must never project this field to a model,
    /// session transcript, activity view, or telemetry record.
    pub canonical_root: String,
    pub repository_hash: String,
    pub filesystem_identity: String,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MechanisticEvidenceFragmentV1 {
    pub action_index: u8,
    pub action_kind: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MechanisticEvidenceCapsuleV1 {
    pub revision: String,
    pub action_digest: String,
    pub source_receipt_digest: String,
    pub confidence: MechanisticConfidenceV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_recipes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_impact: Option<MechanisticDependencySelectionV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reason_codes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uncertainties: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fragments: Vec<MechanisticEvidenceFragmentV1>,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broker: Option<MechanisticBrokerReceiptV1>,
    pub digest: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RustApiImpactStatusV1 {
    Unchanged,
    Changed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RustApiImpactV1 {
    pub status: RustApiImpactStatusV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub baseline_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_metadata_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_metadata_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FailureCauseV1 {
    Environmental,
    LikelyPreExisting,
    LikelyChangeCaused,
    Indeterminate,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RecommendedRerunV1 {
    /// Human-readable form of an already-approved typed recipe.
    pub recipe: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Repository-relative working directory under the repository pinned by codex-team.
    pub cwd: Option<String>,
    /// Digest of the typed recipe identity; this is not executable command text.
    pub recipe_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MechanisticFailureDiagnosisV1 {
    pub cause: FailureCauseV1,
    pub confidence: MechanisticConfidenceV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub likely_responsible_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_recipe: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_rerun: Option<RecommendedRerunV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MechanisticDependencySelectionV1 {
    pub scope: VerificationScopeV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ecosystems: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owning_packages: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reverse_dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_targets: Vec<String>,
    pub graph_cache_status: MechanisticCacheStatusV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rust_api_impact: Option<RustApiImpactV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MechanisticDecisionV1 {
    pub confidence: MechanisticConfidenceV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_recipes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reason_codes: Vec<String>,
    pub cache_status: MechanisticCacheStatusV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_receipt_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation_backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_selection: Option<MechanisticDependencySelectionV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_diagnosis: Option<MechanisticFailureDiagnosisV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MechanisticActionV1 {
    ReadTrackedFile {
        /// Repository-relative path under the repository pinned by codex-team.
        path: String,
        #[serde(default = "default_start_line")]
        start_line: u32,
        #[serde(default = "default_max_lines")]
        max_lines: u16,
    },
    SearchText {
        /// Literal text by default, or a Rust regex when match_mode is regex.
        pattern: String,
        #[serde(default)]
        match_mode: SearchMatchModeV1,
        #[serde(default)]
        /// Repository-relative tracked files or directory prefixes. Empty searches all tracked files.
        paths: Vec<String>,
        #[serde(default = "default_max_matches")]
        max_matches: u16,
    },
    ListTrackedFiles {
        #[serde(default)]
        /// Optional repository-relative component prefix under the pinned repository.
        prefix: Option<String>,
        #[serde(default = "default_max_entries")]
        max_entries: u16,
    },
    ListTrackedDirectory {
        #[serde(default)]
        /// Optional repository-relative tracked directory prefix. None is the repository root.
        path: Option<String>,
        #[serde(default = "default_max_entries")]
        max_entries: u16,
    },
    GlobTrackedFiles {
        /// A bounded glob pattern evaluated only against Git's tracked-file list.
        pattern: String,
        #[serde(default)]
        /// Optional repository-relative tracked directory prefix.
        prefix: Option<String>,
        #[serde(default = "default_max_entries")]
        max_entries: u16,
    },
    GitStatusSummary {
        #[serde(default = "default_max_entries")]
        max_entries: u16,
    },
    GitDiffSummary {
        #[serde(default = "default_max_entries")]
        max_entries: u16,
    },
    GitHistory {
        #[serde(default = "default_history_entries")]
        max_entries: u8,
    },
    InspectSymbol {
        /// Exact identifier to locate in tracked UTF-8 source files.
        symbol: String,
        #[serde(default)]
        /// Optional repository-relative tracked files or directory prefixes.
        paths: Vec<String>,
        #[serde(default = "default_max_matches")]
        max_matches: u16,
        #[serde(default = "default_context_lines")]
        context_lines: u8,
    },
    InspectFeature {
        /// Bounded lexical terms used to rank tracked source and test files.
        terms: Vec<String>,
        #[serde(default)]
        /// Optional repository-relative tracked files or directory prefixes.
        paths: Vec<String>,
        #[serde(default = "default_feature_files")]
        max_files: u16,
    },
    SummarizeChangedArea {
        #[serde(default = "default_max_entries")]
        max_entries: u16,
    },
    FindRelevantTests {
        /// Repository-relative changed or seed paths under the pinned repository.
        paths: Vec<String>,
        #[serde(default = "default_max_entries")]
        max_entries: u16,
    },
    DiagnoseFailedRecipe {
        /// Durable job identifier issued by this codex-team server.
        job_id: String,
    },
    VerifyChangedFiles {
        /// Exact repository-relative paths from host-owned mutation receipts.
        paths: Vec<String>,
        #[serde(default)]
        scope: VerificationScopeV1,
        #[serde(default)]
        /// Whether policy permits dependency-graph failure or fan-out to escalate to a full suite.
        allow_full_escalation: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        /// Opaque compiler-metadata baselines issued by the shared broker.
        rust_api_baseline_ids: Vec<String>,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
    Evidence {
        recipe: EvidenceRecipeV1,
        #[serde(default)]
        /// Optional repository-relative working directory under the pinned repository.
        cwd: Option<String>,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
    ObserveProcess {
        label: String,
        pid: u32,
        #[serde(default = "default_observation_interval")]
        interval_seconds: u64,
    },
    ObserveCopy {
        label: String,
        pid: u32,
        #[serde(default)]
        /// Optional repository-relative log path under the pinned repository.
        log_path: Option<String>,
        /// Repository-relative marker path under the pinned repository.
        completion_marker: String,
        #[serde(default = "default_observation_interval")]
        interval_seconds: u64,
    },
}

impl MechanisticActionV1 {
    pub fn is_query(&self) -> bool {
        matches!(
            self,
            Self::ReadTrackedFile { .. }
                | Self::SearchText { .. }
                | Self::ListTrackedFiles { .. }
                | Self::ListTrackedDirectory { .. }
                | Self::GlobTrackedFiles { .. }
                | Self::GitStatusSummary { .. }
                | Self::GitDiffSummary { .. }
                | Self::GitHistory { .. }
                | Self::InspectSymbol { .. }
                | Self::InspectFeature { .. }
                | Self::SummarizeChangedArea { .. }
                | Self::FindRelevantTests { .. }
                | Self::DiagnoseFailedRecipe { .. }
        )
    }

    pub fn is_evidence(&self) -> bool {
        matches!(
            self,
            Self::Evidence { .. } | Self::VerifyChangedFiles { .. }
        )
    }

    pub fn is_observation(&self) -> bool {
        matches!(self, Self::ObserveProcess { .. } | Self::ObserveCopy { .. })
    }

    pub fn category(&self) -> &'static str {
        if self.is_query() {
            "query"
        } else if self.is_evidence() {
            "evidence"
        } else {
            "observation"
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceRecipeV1 {
    CargoCheck,
    CargoBuild,
    CargoTest { filter: Option<String> },
    CargoClippy,
    CargoFmtCheck,
    NpmTest,
    NpmRunTest,
    NpmRunBuild,
    PnpmTest,
    PnpmBuild,
    YarnTest,
    YarnBuild,
    Pytest { args: Vec<String> },
    PythonPytest { args: Vec<String> },
    SwiftBuild { product: Option<String> },
    SwiftReleaseBuild { product: String },
    SwiftTest { filter: Option<String> },
    SwiftVersion,
    LocalSmoke { binary: String, args: Vec<String> },
    Builtin { check: BuiltinRepositoryCheckV1 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinRepositoryCheckV1 {
    SnapshotHygiene,
    ProtectedTrees,
}

impl BuiltinRepositoryCheckV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SnapshotHygiene => "snapshot-hygiene",
            Self::ProtectedTrees => "protected-trees",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DelegationReadinessV1 {
    pub queries_available: bool,
    pub evidence_available: bool,
    pub observation_available: bool,
    pub evidence_backend: Option<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MechanisticReceiptStatusV1 {
    Completed,
    Failed,
    Unavailable,
    Declined,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MechanisticActionResultV1 {
    pub action_index: u8,
    pub category: String,
    pub status: MechanisticReceiptStatusV1,
    pub summary: String,
    #[serde(default)]
    pub entries: Vec<String>,
    pub truncated: bool,
    pub duration_millis: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<MechanisticDecisionV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MechanisticReceiptV1 {
    pub schema: String,
    pub request_digest: String,
    pub repository_revision: String,
    pub status: MechanisticReceiptStatusV1,
    pub readiness: DelegationReadinessV1,
    pub action_results: Vec<MechanisticActionResultV1>,
    pub output_bytes: u64,
    pub started_at: String,
    pub finished_at: String,
    pub elapsed_millis: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broker: Option<MechanisticBrokerReceiptV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capsule: Option<MechanisticEvidenceCapsuleV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DelegationSafetyClassV1 {
    ReadObserveVerifyOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DelegationManifestV1 {
    pub schema: String,
    pub safety_class: DelegationSafetyClassV1,
    pub actions: Vec<MechanisticActionV1>,
    pub applicability: Vec<String>,
    pub maximum_actions: u8,
    pub maximum_output_bytes: u32,
    pub digest: String,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
#[error("{message}")]
pub struct StructuralValidationError {
    message: String,
}

impl StructuralValidationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub fn validate_request_structure(
    request: &MechanisticRequestV1,
) -> Result<(), StructuralValidationError> {
    if request.schema != MECHANISTIC_REQUEST_SCHEMA {
        return Err(StructuralValidationError::new(
            "unsupported mechanistic request schema",
        ));
    }
    if request.expected_revision.len() != 64
        || !request
            .expected_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(StructuralValidationError::new(
            "expected_revision must be one lowercase SHA-256 digest",
        ));
    }
    validate_actions_structure(&request.actions)
}

pub fn validate_actions_structure(
    actions: &[MechanisticActionV1],
) -> Result<(), StructuralValidationError> {
    if actions.is_empty() || actions.len() > MAX_MECHANISTIC_ACTIONS {
        return Err(StructuralValidationError::new(format!(
            "mechanistic request must contain 1-{MAX_MECHANISTIC_ACTIONS} actions"
        )));
    }
    for action in actions {
        validate_action_structure(action)?;
    }
    Ok(())
}

pub fn validate_action_structure(
    action: &MechanisticActionV1,
) -> Result<(), StructuralValidationError> {
    match action {
        MechanisticActionV1::ReadTrackedFile {
            path,
            start_line,
            max_lines,
        } => {
            validate_relative_path(path)?;
            if *start_line == 0 || *max_lines == 0 || *max_lines > 500 {
                return Err(StructuralValidationError::new(
                    "tracked-file read windows must contain 1-500 lines",
                ));
            }
        }
        MechanisticActionV1::SearchText {
            pattern,
            match_mode: _,
            paths,
            max_matches,
        } => {
            if pattern.is_empty() || pattern.len() > 512 || pattern.chars().any(char::is_control) {
                return Err(StructuralValidationError::new(
                    "search patterns must contain 1-512 non-control bytes",
                ));
            }
            if paths.len() > 512 || *max_matches == 0 || *max_matches > 512 {
                return Err(StructuralValidationError::new(
                    "search action exceeds its path or match bound",
                ));
            }
            for path in paths {
                validate_relative_path(path)?;
            }
        }
        MechanisticActionV1::ListTrackedFiles {
            prefix,
            max_entries,
        } => {
            if let Some(prefix) = prefix {
                validate_relative_path(prefix)?;
            }
            validate_entry_limit(*max_entries)?;
        }
        MechanisticActionV1::ListTrackedDirectory { path, max_entries } => {
            if let Some(path) = path {
                validate_relative_path(path)?;
            }
            validate_entry_limit(*max_entries)?;
        }
        MechanisticActionV1::GlobTrackedFiles {
            pattern,
            prefix,
            max_entries,
        } => {
            validate_glob_pattern(pattern)?;
            if let Some(prefix) = prefix {
                validate_relative_path(prefix)?;
            }
            validate_entry_limit(*max_entries)?;
        }
        MechanisticActionV1::GitStatusSummary { max_entries }
        | MechanisticActionV1::GitDiffSummary { max_entries } => {
            validate_entry_limit(*max_entries)?;
        }
        MechanisticActionV1::GitHistory { max_entries } => {
            if *max_entries == 0 || *max_entries > 50 {
                return Err(StructuralValidationError::new(
                    "history entry limit must be between 1 and 50",
                ));
            }
        }
        MechanisticActionV1::InspectSymbol {
            symbol,
            paths,
            max_matches,
            context_lines,
        } => {
            validate_identifier(symbol)?;
            validate_paths(paths)?;
            if *max_matches == 0 || *max_matches > 512 || *context_lines > 12 {
                return Err(StructuralValidationError::new(
                    "symbol inspection exceeds its match or context bound",
                ));
            }
        }
        MechanisticActionV1::InspectFeature {
            terms,
            paths,
            max_files,
        } => {
            if terms.is_empty() || terms.len() > 12 {
                return Err(StructuralValidationError::new(
                    "feature inspection requires 1-12 lexical terms",
                ));
            }
            for term in terms {
                validate_lexical_term(term)?;
            }
            validate_paths(paths)?;
            if *max_files == 0 || *max_files > 64 {
                return Err(StructuralValidationError::new(
                    "feature inspection max_files must be between 1 and 64",
                ));
            }
        }
        MechanisticActionV1::SummarizeChangedArea { max_entries } => {
            validate_entry_limit(*max_entries)?;
        }
        MechanisticActionV1::FindRelevantTests { paths, max_entries } => {
            if paths.is_empty() || paths.len() > 128 {
                return Err(StructuralValidationError::new(
                    "relevant-test discovery requires 1-128 seed paths",
                ));
            }
            validate_paths(paths)?;
            validate_entry_limit(*max_entries)?;
        }
        MechanisticActionV1::DiagnoseFailedRecipe { job_id } => validate_job_id(job_id)?,
        MechanisticActionV1::VerifyChangedFiles {
            paths,
            scope,
            rust_api_baseline_ids,
            timeout_seconds,
            ..
        } => {
            if paths.len() > 128 {
                return Err(StructuralValidationError::new(
                    "changed-file verification accepts at most 128 paths",
                ));
            }
            if paths.is_empty() && *scope != VerificationScopeV1::Full {
                return Err(StructuralValidationError::new(
                    "focused changed-file verification requires at least one path",
                ));
            }
            validate_paths(paths)?;
            if rust_api_baseline_ids.len() > 2 {
                return Err(StructuralValidationError::new(
                    "changed-file verification accepts at most two Rust API baselines",
                ));
            }
            for baseline_id in rust_api_baseline_ids {
                validate_job_id(baseline_id)?;
            }
            if timeout_seconds.is_some_and(|timeout| timeout == 0 || timeout > 7_200) {
                return Err(StructuralValidationError::new(
                    "changed-file verification timeout must be between 1 and 7200 seconds",
                ));
            }
        }
        MechanisticActionV1::Evidence {
            recipe,
            cwd,
            timeout_seconds,
        } => {
            if let Some(cwd) = cwd {
                validate_relative_path(cwd)?;
            }
            if timeout_seconds.is_some_and(|timeout| timeout == 0 || timeout > 3_600) {
                return Err(StructuralValidationError::new(
                    "evidence timeout must be between 1 and 3600 seconds",
                ));
            }
            validate_recipe_structure(recipe)?;
        }
        MechanisticActionV1::ObserveProcess {
            label,
            pid,
            interval_seconds,
        } => validate_observation(label, *pid, *interval_seconds)?,
        MechanisticActionV1::ObserveCopy {
            label,
            pid,
            log_path,
            completion_marker,
            interval_seconds,
        } => {
            validate_observation(label, *pid, *interval_seconds)?;
            if let Some(log_path) = log_path {
                validate_relative_path(log_path)?;
            }
            validate_relative_path(completion_marker)?;
        }
    }
    Ok(())
}

fn validate_recipe_structure(recipe: &EvidenceRecipeV1) -> Result<(), StructuralValidationError> {
    match recipe {
        EvidenceRecipeV1::CargoTest { filter } | EvidenceRecipeV1::SwiftTest { filter } => {
            validate_optional_token(filter.as_deref())
        }
        EvidenceRecipeV1::Pytest { args } | EvidenceRecipeV1::PythonPytest { args } => {
            validate_args(args)
        }
        EvidenceRecipeV1::SwiftBuild { product } => validate_optional_token(product.as_deref()),
        EvidenceRecipeV1::SwiftReleaseBuild { product } => validate_token(product),
        EvidenceRecipeV1::LocalSmoke { binary, args } => {
            validate_token(binary)?;
            validate_args(args)
        }
        _ => Ok(()),
    }
}

fn validate_args(args: &[String]) -> Result<(), StructuralValidationError> {
    if args.len() > 64 {
        return Err(StructuralValidationError::new(
            "typed recipe argument count exceeds 64",
        ));
    }
    for arg in args {
        validate_token(arg)?;
    }
    Ok(())
}

fn validate_paths(paths: &[String]) -> Result<(), StructuralValidationError> {
    if paths.len() > 512 {
        return Err(StructuralValidationError::new(
            "path list exceeds the mechanistic bound",
        ));
    }
    for path in paths {
        validate_relative_path(path)?;
    }
    Ok(())
}

fn validate_identifier(value: &str) -> Result<(), StructuralValidationError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | ':' | '.'))
    {
        return Err(StructuralValidationError::new(
            "symbol must be a bounded identifier",
        ));
    }
    Ok(())
}

fn validate_lexical_term(value: &str) -> Result<(), StructuralValidationError> {
    if value.is_empty()
        || value.len() > 64
        || value.chars().any(char::is_control)
        || !value
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err(StructuralValidationError::new(
            "feature terms must be bounded lexical tokens",
        ));
    }
    Ok(())
}

fn validate_glob_pattern(value: &str) -> Result<(), StructuralValidationError> {
    if value.is_empty()
        || value.len() > 512
        || value.chars().any(char::is_control)
        || value.starts_with('/')
        || value.starts_with('~')
        || value.contains('\\')
        || value.split('/').any(|component| component == "..")
    {
        return Err(StructuralValidationError::new(
            "glob must be a bounded repository-relative pattern without traversal",
        ));
    }
    Ok(())
}

fn validate_job_id(value: &str) -> Result<(), StructuralValidationError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(StructuralValidationError::new(
            "job_id must be a bounded server-issued identifier",
        ));
    }
    Ok(())
}

fn validate_optional_token(value: Option<&str>) -> Result<(), StructuralValidationError> {
    match value {
        Some(value) => validate_token(value),
        None => Ok(()),
    }
}

fn validate_token(value: &str) -> Result<(), StructuralValidationError> {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(StructuralValidationError::new(
            "typed recipe values must contain 1-512 non-control bytes",
        ));
    }
    Ok(())
}

fn validate_observation(
    label: &str,
    pid: u32,
    interval_seconds: u64,
) -> Result<(), StructuralValidationError> {
    if label.is_empty()
        || label.len() > 128
        || label.chars().any(char::is_control)
        || pid == 0
        || !(2..=300).contains(&interval_seconds)
    {
        return Err(StructuralValidationError::new(
            "local observation metadata is outside its bounded contract",
        ));
    }
    Ok(())
}

fn validate_relative_path(value: &str) -> Result<(), StructuralValidationError> {
    if value.is_empty()
        || value.len() > 4_096
        || value.chars().any(char::is_control)
        || Path::new(value).is_absolute()
        || Path::new(value)
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return Err(StructuralValidationError::new(
            "paths must be bounded relative paths without traversal",
        ));
    }
    Ok(())
}

fn validate_entry_limit(limit: u16) -> Result<(), StructuralValidationError> {
    if limit == 0 || limit > 512 {
        return Err(StructuralValidationError::new(
            "query entry limit must be between 1 and 512",
        ));
    }
    Ok(())
}

const fn default_start_line() -> u32 {
    1
}

const fn default_max_lines() -> u16 {
    200
}

const fn default_max_matches() -> u16 {
    200
}

const fn default_max_entries() -> u16 {
    200
}

const fn default_history_entries() -> u8 {
    20
}

const fn default_observation_interval() -> u64 {
    10
}

const fn default_context_lines() -> u8 {
    2
}

const fn default_feature_files() -> u16 {
    12
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::schema_for;

    fn revision() -> String {
        "a".repeat(64)
    }

    #[test]
    fn request_round_trips_and_validates() {
        let request = MechanisticRequestV1 {
            schema: MECHANISTIC_REQUEST_SCHEMA.to_string(),
            purpose: MechanisticRequestPurposeV1::Ordinary,
            expected_revision: revision(),
            delegation_policy: DelegationPolicy::Prefer,
            allow_unisolated_evidence: false,
            actions: vec![MechanisticActionV1::GitHistory { max_entries: 5 }],
        };
        validate_request_structure(&request).expect("valid request");
        let encoded = serde_json::to_value(&request).expect("serialize request");
        let decoded: MechanisticRequestV1 =
            serde_json::from_value(encoded).expect("deserialize request");
        assert_eq!(decoded, request);
    }

    #[test]
    fn automation_setup_is_typed_closed_and_secret_free() {
        let draft = AutomationSetupDraftV1::RemoteAutomation {
            manifest_name: "seedbox".into(),
            ssh_hosts: vec![AutomationSshHostV1 {
                name: "server".into(),
                destination: "ops@example.test".into(),
                port: Some(22),
            }],
            transmission: vec![AutomationTransmissionEndpointV1 {
                name: "seedbox".into(),
                url: "https://seedbox.example/transmission/rpc".into(),
                username_env: Some("SEEDBOX_USER".into()),
                password_env: Some("SEEDBOX_PASSWORD".into()),
                protocol: TransmissionProtocolPreferenceV1::Auto,
                timeout_seconds: None,
            }],
            rtorrent: Vec::new(),
            torrent_locations: Vec::new(),
            syncs: vec![AutomationSyncProfileV1 {
                name: "server_to_seedbox".into(),
                controller: "server".into(),
                source: "/data/completed/".into(),
                destination: "seedbox:/data/incoming/".into(),
                torrent_endpoint: None,
                delete: false,
                timeout_seconds: None,
                max_changes: None,
            }],
        };
        let preview = AutomationSetupRequestV1 {
            schema: AUTOMATION_SETUP_REQUEST_SCHEMA.into(),
            operation: AutomationSetupOperationV1::Preview,
            draft: draft.clone(),
        };
        validate_automation_setup_request(&preview).unwrap();
        let encoded = serde_json::to_value(&preview).unwrap();
        assert_eq!(
            serde_json::from_value::<AutomationSetupRequestV1>(encoded).unwrap(),
            preview
        );
        let secret_shaped = AutomationSetupRequestV1 {
            schema: AUTOMATION_SETUP_REQUEST_SCHEMA.into(),
            operation: AutomationSetupOperationV1::Preview,
            draft: AutomationSetupDraftV1::RemoteAutomation {
                manifest_name: "seedbox".into(),
                ssh_hosts: Vec::new(),
                transmission: vec![AutomationTransmissionEndpointV1 {
                    name: "seedbox".into(),
                    url: "https://seedbox.example/transmission/rpc".into(),
                    username_env: None,
                    password_env: Some("literal-password".into()),
                    protocol: TransmissionProtocolPreferenceV1::Auto,
                    timeout_seconds: None,
                }],
                rtorrent: Vec::new(),
                torrent_locations: Vec::new(),
                syncs: Vec::new(),
            },
        };
        assert!(validate_automation_setup_request(&secret_shaped).is_err());
        let mut invalid_controller = preview;
        let AutomationSetupDraftV1::RemoteAutomation { syncs, .. } = &mut invalid_controller.draft;
        syncs[0].controller = "unknown".into();
        assert!(validate_automation_setup_request(&invalid_controller).is_err());
    }

    #[test]
    fn torrent_contract_is_normalized_closed_and_legacy_compatible() {
        assert!(validate_info_hash(&"a".repeat(40)).is_ok());
        assert!(validate_info_hash(&"A".repeat(40)).is_err());
        assert!(validate_info_hash("short").is_err());

        let setup = AutomationSetupRequestV1 {
            schema: AUTOMATION_SETUP_REQUEST_SCHEMA.into(),
            operation: AutomationSetupOperationV1::Preview,
            draft: AutomationSetupDraftV1::RemoteAutomation {
                manifest_name: "torrent_clients".into(),
                ssh_hosts: Vec::new(),
                transmission: Vec::new(),
                rtorrent: vec![AutomationRtorrentEndpointV1 {
                    name: "rtorrent".into(),
                    transport: AutomationRtorrentTransportV1::LoopbackTcpScgi { port: 5000 },
                    automation_label_slot: Some(RtorrentCustomSlotV1::Custom2),
                    timeout_seconds: Some(30),
                }],
                torrent_locations: vec![AutomationTorrentLocationV1 {
                    name: "downloads".into(),
                    endpoint: "rtorrent".into(),
                    path: "/srv/downloads".into(),
                }],
                syncs: vec![AutomationSyncProfileV1 {
                    name: "downloads_home".into(),
                    controller: "local".into(),
                    source: "/srv/downloads/".into(),
                    destination: "/media/incoming/".into(),
                    torrent_endpoint: Some("rtorrent".into()),
                    delete: false,
                    timeout_seconds: Some(30),
                    max_changes: Some(100),
                }],
            },
        };
        validate_automation_setup_request(&setup).unwrap();
        let encoded = serde_json::to_value(&setup).unwrap();
        assert_eq!(
            serde_json::from_value::<AutomationSetupRequestV1>(encoded).unwrap(),
            setup
        );

        let legacy: WorkflowDraftV1 = serde_json::from_value(serde_json::json!({
            "schema": WORKFLOW_DRAFT_SCHEMA,
            "workflow_id": "legacy",
            "title": "Legacy Transmission query",
            "actions": [{
                "id": "query",
                "action": {
                    "kind": "query_transmission",
                    "endpoint": "seedbox",
                    "operation": "torrents"
                }
            }],
            "limits": {
                "max_runtime_seconds": 30,
                "min_free_space_bytes": 0,
                "deletion_allowed": false,
                "conflict_policy": "block",
                "verification": []
            },
            "rollback": {"kind":"unavailable","limitation":"Read only"}
        }))
        .unwrap();
        assert_eq!(legacy.limits.max_files, None);
        validate_workflow_draft(&legacy).unwrap();

        let receipt = TorrentMutationReceiptV1 {
            endpoint: "rtorrent".into(),
            identity: TorrentClientIdentityV1 {
                kind: TorrentClientKindV1::Rtorrent,
                protocol: TorrentProtocolV1::RtorrentXmlRpc,
                version: Some("0.9.8".into()),
                fingerprint: "b".repeat(64),
            },
            action: TorrentControlActionV1::SetPriority {
                priority: TorrentPriorityV1::High,
            },
            target_hashes: vec!["a".repeat(40)],
            changed_hashes: vec!["a".repeat(40)],
            target_hashes_digest: Some("c".repeat(64)),
            changed_hashes_digest: Some("d".repeat(64)),
            post_state_confirmed: true,
            ambiguous_transport_resolved: false,
            truncated: false,
        };
        assert_eq!(
            serde_json::from_value::<TorrentMutationReceiptV1>(
                serde_json::to_value(&receipt).unwrap()
            )
            .unwrap(),
            receipt
        );
    }

    #[test]
    fn structural_validation_rejects_unsafe_path() {
        let request = MechanisticRequestV1 {
            schema: MECHANISTIC_REQUEST_SCHEMA.to_string(),
            purpose: MechanisticRequestPurposeV1::Ordinary,
            expected_revision: revision(),
            delegation_policy: DelegationPolicy::Prefer,
            allow_unisolated_evidence: false,
            actions: vec![MechanisticActionV1::ReadTrackedFile {
                path: "../secret".to_string(),
                start_line: 1,
                max_lines: 10,
            }],
        };
        assert!(validate_request_structure(&request).is_err());
    }

    #[test]
    fn transparent_query_actions_are_closed_bounded_and_legacy_read_parity_is_500_lines() {
        for action in [
            MechanisticActionV1::ListTrackedDirectory {
                path: Some("src".into()),
                max_entries: 100,
            },
            MechanisticActionV1::GlobTrackedFiles {
                pattern: "**/*.rs".into(),
                prefix: Some("src".into()),
                max_entries: 100,
            },
            MechanisticActionV1::ReadTrackedFile {
                path: "src/main.rs".into(),
                start_line: 1,
                max_lines: 500,
            },
        ] {
            validate_action_structure(&action).expect("transparent action is valid");
            assert!(action.is_query());
            let encoded = serde_json::to_value(&action).unwrap();
            let decoded: MechanisticActionV1 = serde_json::from_value(encoded).unwrap();
            assert_eq!(decoded, action);
        }
        assert!(
            validate_action_structure(&MechanisticActionV1::ReadTrackedFile {
                path: "src/main.rs".into(),
                start_line: 1,
                max_lines: 501,
            })
            .is_err()
        );
        assert!(
            validate_action_structure(&MechanisticActionV1::GlobTrackedFiles {
                pattern: "../**".into(),
                prefix: None,
                max_entries: 10,
            })
            .is_err()
        );
    }

    #[test]
    fn generated_action_schema_and_facade_version_are_stable() {
        let schema = serde_json::to_value(schema_for!(MechanisticActionV1)).expect("schema");
        let encoded = serde_json::to_string(&schema).expect("encode schema");
        assert!(encoded.contains("read_tracked_file"));
        assert!(encoded.contains("observe_copy"));
        assert!(encoded.contains("match_mode"));
        assert!(encoded.contains("inspect_symbol"));
        assert!(encoded.contains("verify_changed_files"));
        assert!(encoded.contains("Repository-relative tracked files or directory prefixes"));
        assert!(encoded.contains("additionalProperties"));
        assert_eq!(
            CA_MECHANISTIC_DELEGATION_SCHEMA,
            "ca.mechanistic_delegation.v1"
        );
    }

    #[test]
    fn checked_in_schemas_match_the_protocol_types() {
        let schema_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ca-automation/schemas");
        let checked_action: serde_json::Value = serde_json::from_slice(
            &std::fs::read(schema_dir.join("mechanistic_action.schema.json")).unwrap(),
        )
        .unwrap();
        let checked_request: serde_json::Value = serde_json::from_slice(
            &std::fs::read(schema_dir.join("mechanistic_request.schema.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            checked_action,
            serde_json::to_value(schema_for!(MechanisticActionV1)).unwrap()
        );
        assert_eq!(
            checked_request,
            serde_json::to_value(schema_for!(MechanisticRequestV1)).unwrap()
        );
    }

    #[test]
    fn legacy_search_defaults_to_literal_and_regex_round_trips() {
        let legacy: MechanisticActionV1 = serde_json::from_value(serde_json::json!({
            "kind": "search_text",
            "pattern": "one|two",
            "paths": ["src"],
            "max_matches": 8
        }))
        .expect("legacy search action");
        assert!(matches!(
            legacy,
            MechanisticActionV1::SearchText {
                match_mode: SearchMatchModeV1::Literal,
                ..
            }
        ));

        let regex = MechanisticActionV1::SearchText {
            pattern: "one|two".into(),
            match_mode: SearchMatchModeV1::Regex,
            paths: vec!["src".into()],
            max_matches: 8,
        };
        let encoded = serde_json::to_value(&regex).expect("regex search serialization");
        assert_eq!(encoded["match_mode"], "regex");
        assert_eq!(
            serde_json::from_value::<MechanisticActionV1>(encoded).expect("regex search decode"),
            regex
        );
    }

    #[test]
    fn composite_actions_round_trip_and_remain_structurally_bounded() {
        let actions = vec![
            MechanisticActionV1::InspectSymbol {
                symbol: "orchestration_available".into(),
                paths: vec!["crates/codex-team".into()],
                max_matches: 20,
                context_lines: 2,
            },
            MechanisticActionV1::InspectFeature {
                terms: vec!["left".into(), "arrow".into()],
                paths: Vec::new(),
                max_files: 12,
            },
            MechanisticActionV1::FindRelevantTests {
                paths: vec!["src/tui/input.rs".into()],
                max_entries: 20,
            },
            MechanisticActionV1::VerifyChangedFiles {
                paths: vec!["src/tui/input.rs".into()],
                scope: VerificationScopeV1::Focused,
                allow_full_escalation: false,
                timeout_seconds: Some(900),
                rust_api_baseline_ids: Vec::new(),
            },
        ];
        validate_actions_structure(&actions).expect("valid composite actions");
        for action in actions {
            let encoded = serde_json::to_value(&action).expect("serialize composite");
            assert_eq!(
                serde_json::from_value::<MechanisticActionV1>(encoded).expect("decode composite"),
                action
            );
        }

        assert!(
            validate_action_structure(&MechanisticActionV1::InspectFeature {
                terms: Vec::new(),
                paths: Vec::new(),
                max_files: 12,
            })
            .is_err()
        );
        assert!(
            validate_action_structure(&MechanisticActionV1::VerifyChangedFiles {
                paths: vec!["../escape".into()],
                scope: VerificationScopeV1::Full,
                allow_full_escalation: false,
                timeout_seconds: None,
                rust_api_baseline_ids: Vec::new(),
            })
            .is_err()
        );
        assert!(
            validate_action_structure(&MechanisticActionV1::VerifyChangedFiles {
                paths: Vec::new(),
                scope: VerificationScopeV1::Full,
                allow_full_escalation: false,
                timeout_seconds: None,
                rust_api_baseline_ids: Vec::new(),
            })
            .is_ok()
        );
        assert!(
            validate_action_structure(&MechanisticActionV1::VerifyChangedFiles {
                paths: Vec::new(),
                scope: VerificationScopeV1::Focused,
                allow_full_escalation: false,
                timeout_seconds: None,
                rust_api_baseline_ids: Vec::new(),
            })
            .is_err()
        );
    }

    #[test]
    fn remote_automation_is_closed_named_and_mutation_is_single_action() {
        let request = RemoteAutomationRequestV1 {
            schema: REMOTE_AUTOMATION_REQUEST_SCHEMA.into(),
            actions: vec![
                RemoteAutomationActionV1::CompareRemoteFiles {
                    profile: "server_to_seedbox".into(),
                },
                RemoteAutomationActionV1::CompareRemoteRpc {
                    left_endpoint: "seedbox_a".into(),
                    right_endpoint: "seedbox_b".into(),
                    operation: RemoteRpcOperationV1::Torrents,
                },
            ],
        };
        validate_remote_automation_request(&request).unwrap();
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(
            serde_json::from_value::<RemoteAutomationRequestV1>(encoded).unwrap(),
            request
        );
        let mixed_apply = RemoteAutomationRequestV1 {
            schema: REMOTE_AUTOMATION_REQUEST_SCHEMA.into(),
            actions: vec![
                RemoteAutomationActionV1::ApplyRemoteSync {
                    profile: "server_to_seedbox".into(),
                    accepted_plan_digest: "a".repeat(64),
                },
                RemoteAutomationActionV1::QueryRemoteRpc {
                    endpoint: "seedbox_a".into(),
                    operation: RemoteRpcOperationV1::Session,
                },
            ],
        };
        assert!(validate_remote_automation_request(&mixed_apply).is_err());
        let shell_shaped = RemoteAutomationRequestV1 {
            schema: REMOTE_AUTOMATION_REQUEST_SCHEMA.into(),
            actions: vec![RemoteAutomationActionV1::PlanRemoteSync {
                profile: "server;rm".into(),
            }],
        };
        assert!(validate_remote_automation_request(&shell_shaped).is_err());
    }

    fn workflow_draft() -> WorkflowDraftV1 {
        WorkflowDraftV1 {
            schema: WORKFLOW_DRAFT_SCHEMA.into(),
            workflow_id: "seedbox_archive".into(),
            title: "Synchronize completed archive torrents".into(),
            actions: vec![
                WorkflowNodeV1 {
                    id: "query".into(),
                    depends_on: Vec::new(),
                    max_retries: 1,
                    action: WorkflowActionV1::QueryTransmission {
                        endpoint: "seedbox".into(),
                        operation: RemoteRpcOperationV1::Torrents,
                    },
                },
                WorkflowNodeV1 {
                    id: "select".into(),
                    depends_on: vec!["query".into()],
                    max_retries: 0,
                    action: WorkflowActionV1::SelectTorrents {
                        source_action: "query".into(),
                        completed: true,
                        label: Some("archive".into()),
                        info_hashes: Vec::new(),
                        states: Vec::new(),
                        maximum_items: 50,
                    },
                },
                WorkflowNodeV1 {
                    id: "plan".into(),
                    depends_on: vec!["select".into()],
                    max_retries: 1,
                    action: WorkflowActionV1::PlanSync {
                        profile: "seedbox_home".into(),
                        selection_action: None,
                    },
                },
                WorkflowNodeV1 {
                    id: "apply".into(),
                    depends_on: vec!["plan".into()],
                    max_retries: 1,
                    action: WorkflowActionV1::ApplySync {
                        profile: "seedbox_home".into(),
                        plan_action: "plan".into(),
                    },
                },
            ],
            limits: WorkflowLimitsV1 {
                max_items: Some(50),
                max_transfer_bytes: Some(100 * 1024 * 1024 * 1024),
                max_files: None,
                max_runtime_seconds: 3_600,
                min_free_space_bytes: 20 * 1024 * 1024 * 1024,
                deletion_allowed: false,
                conflict_policy: WorkflowConflictPolicyV1::KeepBoth,
                verification: vec![WorkflowVerificationV1::Sha256],
            },
            rollback: WorkflowRollbackV1::ResumeOnly {
                limitation: "Transfers may resume, but overwritten remote bytes are not restored"
                    .into(),
            },
        }
    }

    #[test]
    fn workflow_contract_is_closed_bounded_and_round_trips() {
        let draft = workflow_draft();
        validate_workflow_draft(&draft).unwrap();
        let request = WorkflowRequestV1 {
            schema: WORKFLOW_REQUEST_SCHEMA.into(),
            operation: WorkflowOperationV1::Preview {
                draft,
                depth: WorkflowDryRunDepthV1::MutationPreview,
                correction_round: 0,
            },
        };
        validate_workflow_request(&request).unwrap();
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(
            serde_json::from_value::<WorkflowRequestV1>(encoded).unwrap(),
            request
        );
        let mut invalid = workflow_draft();
        invalid.actions[3].depends_on.clear();
        assert!(validate_workflow_draft(&invalid).is_err());
        let mut unsafe_draft = workflow_draft();
        unsafe_draft.actions[2].action = WorkflowActionV1::PlanSync {
            profile: "sync;rm".into(),
            selection_action: None,
        };
        assert!(validate_workflow_draft(&unsafe_draft).is_err());

        let legacy_run: WorkflowRequestV1 = serde_json::from_value(serde_json::json!({
            "schema": WORKFLOW_REQUEST_SCHEMA,
            "operation": {
                "kind": "run",
                "workflow_id": "seedbox_archive"
            }
        }))
        .unwrap();
        validate_workflow_request(&legacy_run).unwrap();
        let WorkflowOperationV1::Run {
            expected_workflow_digest,
            expected_resource_binding_digest,
            ..
        } = legacy_run.operation
        else {
            panic!("expected run operation")
        };
        assert!(expected_workflow_digest.is_none());
        assert!(expected_resource_binding_digest.is_none());

        let legacy_execution: WorkflowExecutionReceiptV1 =
            serde_json::from_value(serde_json::json!({
                "schema": WORKFLOW_EXECUTION_RECEIPT_SCHEMA,
                "workflow_id": "seedbox_archive",
                "workflow_digest": "a".repeat(64),
                "policy_digest": "b".repeat(64),
                "resource_binding_digest": "c".repeat(64),
                "run_plan_digest": "d".repeat(64),
                "status": "completed",
                "action_results": [],
                "deterministic_repairs": 0,
                "verification_passed": true,
                "started_at": "2026-08-27T00:00:00Z",
                "finished_at": "2026-08-27T00:00:01Z",
                "elapsed_millis": 1000
            }))
            .unwrap();
        assert!(legacy_execution.rollback.is_none());
        assert!(legacy_execution.deviations.is_empty());
    }

    #[test]
    fn workflow_corrections_distinguish_tightening_widening_and_conflict() {
        let correction = |changes| WorkflowCorrectionV1 {
            schema: WORKFLOW_CORRECTION_SCHEMA.into(),
            source_plan_digest: "a".repeat(64),
            reason_code: WorkflowCorrectionReasonV1::MissingVerification,
            scope: WorkflowCorrectionScopeV1::Adapter {
                adapter: WorkflowAdapterV1::Rsync,
            },
            changes,
            evidence_refs: vec!["b".repeat(64)],
            correction_round: 1,
        };
        let tightening = correction(vec![WorkflowCorrectionChangeV1::RequireVerification {
            verification: WorkflowVerificationV1::Sha256,
        }]);
        validate_workflow_correction(&tightening).unwrap();
        assert_eq!(
            classify_workflow_correction(&tightening),
            WorkflowCorrectionClassificationV1::Tightening
        );
        assert_eq!(
            classify_workflow_correction(&correction(vec![
                WorkflowCorrectionChangeV1::RaiseMaxItems { maximum: 100 }
            ])),
            WorkflowCorrectionClassificationV1::Widening
        );
        assert_eq!(
            classify_workflow_correction(&correction(vec![
                WorkflowCorrectionChangeV1::DenyDeletion,
                WorkflowCorrectionChangeV1::PermitDeletion,
            ])),
            WorkflowCorrectionClassificationV1::Conflicting
        );
        let target_specific_adapter =
            correction(vec![WorkflowCorrectionChangeV1::NarrowDestinationPrefix {
                prefix: "/media/archive".into(),
            }]);
        assert!(validate_workflow_correction(&target_specific_adapter).is_err());
    }
}
pub mod execution;
