//! Bounded process protocol for the feature-gated RND-097 experiment.
//!
//! Phase-level worker types below describe controller identity and frozen
//! authority.  They are deliberately distinct from the physical-call types:
//! one provider frame authorizes one provider POST and no tool execution, and
//! one tool frame authorizes one exact tool call without carrying provider,
//! configuration, or credential material.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub const WORKER_SPEC_SCHEMA_V1: &str = "ca.reasoning_experiment_worker_spec.v1";
pub const WORKER_RECEIPT_SCHEMA_V1: &str = "ca.reasoning_experiment_worker_receipt.v1";
pub const IPC_FRAME_METADATA_SCHEMA_V1: &str = "ca.reasoning_experiment_ipc_frame_metadata.v1";
pub const RESERVATION_REQUEST_SCHEMA_V1: &str = "ca.reasoning_experiment_reservation_request.v1";
pub const RESERVATION_GRANT_SCHEMA_V1: &str = "ca.reasoning_experiment_reservation_grant.v1";
pub const RESERVATION_SETTLEMENT_SCHEMA_V1: &str =
    "ca.reasoning_experiment_reservation_settlement.v1";
pub const PROVIDER_CALL_SPEC_SCHEMA_V1: &str = "ca.reasoning_experiment_provider_call_spec.v1";
pub const PROVIDER_CALL_RECEIPT_SCHEMA_V1: &str =
    "ca.reasoning_experiment_provider_call_receipt.v1";
pub const TOOL_CALL_SPEC_SCHEMA_V1: &str = "ca.reasoning_experiment_tool_call_spec.v1";
pub const TOOL_CALL_RECEIPT_SCHEMA_V1: &str = "ca.reasoning_experiment_tool_call_receipt.v1";

// Kept as aliases for the phase-level draft while callers move to the three
// distinct durable-ledger schemas above.
pub const CONTROL_REQUEST_SCHEMA_V1: &str = RESERVATION_REQUEST_SCHEMA_V1;
pub const CONTROL_RESPONSE_SCHEMA_V1: &str = RESERVATION_GRANT_SCHEMA_V1;
pub const INTERNAL_BROKER_FLAG: &str = "--internal-reasoning-broker";
pub const INTERNAL_BROKER_PREFLIGHT_FLAG: &str = "--internal-reasoning-broker-preflight";
pub const INTERNAL_TOOL_WORKER_FLAG: &str = "--internal-reasoning-tool-worker";
pub const INTERNAL_TOOL_WORKER_PREFLIGHT_FLAG: &str = "--internal-reasoning-tool-worker-preflight";
pub const STRONG_MODEL: &str = "gpt-5.6-sol";
pub const CHEAP_MODEL: &str = "gpt-5.6-luna";
pub const REASONING_EFFORT: &str = "high";
pub const SERVICE_TIER: &str = "default";
pub const MAX_INPUT_TOKENS_PER_REQUEST: u64 = 64_000;
pub const MAX_OUTPUT_TOKENS_PER_REQUEST: u64 = 8_192;
pub const MAX_PLANNER_SEMANTIC_REQUESTS: u16 = 1;
pub const MAX_EXECUTOR_SEMANTIC_REQUESTS: u16 = 12;
pub const MAX_RECOVERY_SEMANTIC_REQUESTS: u16 = 8;
pub const MAX_EXECUTOR_TOOL_CALLS: u16 = 40;
pub const MAX_RECOVERY_TOOL_CALLS: u16 = 24;
pub const MAX_EXECUTOR_WALL_TIME_SECS: u32 = 10 * 60;
pub const MAX_PHYSICAL_ATTEMPTS_PER_SEMANTIC_REQUEST: u8 = 2;

const MAX_ID_BYTES: usize = 128;
const MAX_PATH_BYTES: usize = 4_096;
const MAX_PROVIDER_RECEIPTS: usize = 24;
const MAX_REASON_BYTES: usize = 1_024;
const MAX_DECLARED_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ALLOWED_PATHS: usize = 256;
const MAX_ALLOWED_TOOLS: usize = 64;
const IPC_PROTOCOL_VERSION_V1: u16 = 1;
const IPC_LENGTH_PREFIX_BYTES: usize = 4;
pub const MAX_PROVIDER_CALL_SPEC_FRAME_BYTES: u32 = 2 * 1024 * 1024;
pub const MAX_PROVIDER_CALL_RECEIPT_FRAME_BYTES: u32 = 4 * 1024 * 1024;
pub const MAX_TOOL_CALL_SPEC_FRAME_BYTES: u32 = 1024 * 1024;
pub const MAX_TOOL_CALL_RECEIPT_FRAME_BYTES: u32 = 2 * 1024 * 1024;
const MAX_PROVIDER_REQUEST_JSON_BYTES: usize = 1024 * 1024;
const MAX_PROVIDER_RESPONSE_JSON_BYTES: usize = 2 * 1024 * 1024;
const MAX_TOOL_ARGUMENT_JSON_BYTES: usize = 256 * 1024;
pub const MAX_TOOL_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_TOOL_NAME_BYTES: usize = 128;
const MAX_TOOL_CALL_ID_BYTES: usize = 256;
const MAX_CALL_ID_BYTES: usize = 128;
const MAX_NONCE_BYTES: usize = 128;
const MAX_CLIENT_REQUEST_ID_BYTES: usize = 512;
const MAX_PROVIDER_LATENCY_MILLIS: u64 = MAX_EXECUTOR_WALL_TIME_SECS as u64 * 1_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentPhase {
    Planner,
    Executor,
    Recovery,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExperimentArm {
    #[serde(rename = "S0")]
    S0,
    #[serde(rename = "C0")]
    C0,
    #[serde(rename = "SS")]
    Ss,
    #[serde(rename = "SC")]
    Sc,
    #[serde(rename = "SC-G")]
    ScG,
    #[serde(rename = "SP")]
    Sp,
}

impl ExperimentArm {
    pub const ALL: [Self; 6] = [Self::S0, Self::C0, Self::Ss, Self::Sc, Self::ScG, Self::Sp];

    pub const fn id(self) -> &'static str {
        match self {
            Self::S0 => "S0",
            Self::C0 => "C0",
            Self::Ss => "SS",
            Self::Sc => "SC",
            Self::ScG => "SC-G",
            Self::Sp => "SP",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentModelRouteV1 {
    pub provider: String,
    pub model: String,
    pub reasoning_effort: String,
    pub service_tier: String,
}

impl ExperimentModelRouteV1 {
    pub fn openai(model: &str) -> Self {
        Self {
            provider: "openai".into(),
            model: model.into(),
            reasoning_effort: REASONING_EFFORT.into(),
            service_tier: SERVICE_TIER.into(),
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolValidationError> {
        validate_id("route.provider", &self.provider)?;
        if self.provider != "openai" {
            return Err(ProtocolValidationError::new(
                "route.provider must be exactly openai",
            ));
        }
        if !matches!(self.model.as_str(), STRONG_MODEL | CHEAP_MODEL) {
            return Err(ProtocolValidationError::new(
                "route.model is not a preregistered RND-097 model",
            ));
        }
        if self.reasoning_effort != REASONING_EFFORT {
            return Err(ProtocolValidationError::new(
                "route.reasoning_effort must be exactly high",
            ));
        }
        if self.service_tier != SERVICE_TIER {
            return Err(ProtocolValidationError::new(
                "route.service_tier must be exactly default",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentRequestLimitsV1 {
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_semantic_requests: u16,
    pub max_physical_posts: u16,
    pub max_tool_calls: u16,
    pub wall_time_secs: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentFileRoot {
    Workspace,
    Controller,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentDeclaredFileV1 {
    pub root: ExperimentFileRoot,
    pub relative_path: PathBuf,
    pub sha256: String,
    pub max_bytes: u64,
}

impl ExperimentDeclaredFileV1 {
    pub fn validate(&self, field: &str) -> Result<(), ProtocolValidationError> {
        validate_relative_path(&format!("{field}.relative_path"), &self.relative_path)?;
        validate_sha256(&format!("{field}.sha256"), &self.sha256)?;
        if self.max_bytes == 0 || self.max_bytes > MAX_DECLARED_FILE_BYTES {
            return Err(ProtocolValidationError::new(format!(
                "{field}.max_bytes is outside the bounded declared-file range"
            )));
        }
        Ok(())
    }

    pub fn resolve<'a>(&self, workspace: &'a Path, controller: &'a Path) -> PathBuf {
        match self.root {
            ExperimentFileRoot::Workspace => workspace.join(&self.relative_path),
            ExperimentFileRoot::Controller => controller.join(&self.relative_path),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExperimentHandoffV1 {
    None,
    StructuredArtifact {
        file: ExperimentDeclaredFileV1,
    },
    SizeMatchedProse {
        file: ExperimentDeclaredFileV1,
        source_artifact_sha256: String,
        source_artifact_bytes: u64,
        prose_bytes: u64,
    },
}

impl ExperimentHandoffV1 {
    fn validate(&self) -> Result<(), ProtocolValidationError> {
        match self {
            Self::None => Ok(()),
            Self::StructuredArtifact { file } => file.validate("handoff.file"),
            Self::SizeMatchedProse {
                file,
                source_artifact_sha256,
                source_artifact_bytes,
                prose_bytes,
            } => {
                file.validate("handoff.file")?;
                validate_sha256("handoff.source_artifact_sha256", source_artifact_sha256)?;
                if *source_artifact_bytes == 0 || *prose_bytes == 0 {
                    return Err(ProtocolValidationError::new(
                        "size-matched handoff byte counts must be nonzero",
                    ));
                }
                let difference = source_artifact_bytes.abs_diff(*prose_bytes);
                if u128::from(difference) * 100 > u128::from(*source_artifact_bytes) * 2 {
                    return Err(ProtocolValidationError::new(
                        "SP prose is not UTF-8 byte-size matched within 2 percent",
                    ));
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentControllerChannelV1 {
    pub channel_id: String,
    pub nonce_sha256: String,
    pub inherited_fd: i32,
}

impl std::fmt::Debug for ExperimentControllerChannelV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExperimentControllerChannelV1")
            .field("channel_id", &self.channel_id)
            .field("nonce_sha256", &"[REDACTED]")
            .field("inherited_fd", &"[PRIVATE]")
            .finish()
    }
}

impl ExperimentControllerChannelV1 {
    fn validate(&self) -> Result<(), ProtocolValidationError> {
        validate_id("controller_channel.channel_id", &self.channel_id)?;
        validate_sha256("controller_channel.nonce_sha256", &self.nonce_sha256)?;
        if !(3..=1_024).contains(&self.inherited_fd) {
            return Err(ProtocolValidationError::new(
                "controller channel descriptor is outside the allowed inherited range",
            ));
        }
        Ok(())
    }
}

impl ExperimentRequestLimitsV1 {
    pub const fn for_phase(phase: ExperimentPhase) -> Self {
        match phase {
            ExperimentPhase::Planner => Self {
                max_input_tokens: MAX_INPUT_TOKENS_PER_REQUEST,
                max_output_tokens: MAX_OUTPUT_TOKENS_PER_REQUEST,
                max_semantic_requests: MAX_PLANNER_SEMANTIC_REQUESTS,
                max_physical_posts: MAX_PLANNER_SEMANTIC_REQUESTS
                    * MAX_PHYSICAL_ATTEMPTS_PER_SEMANTIC_REQUEST as u16,
                max_tool_calls: 0,
                wall_time_secs: MAX_EXECUTOR_WALL_TIME_SECS,
            },
            ExperimentPhase::Executor => Self {
                max_input_tokens: MAX_INPUT_TOKENS_PER_REQUEST,
                max_output_tokens: MAX_OUTPUT_TOKENS_PER_REQUEST,
                max_semantic_requests: MAX_EXECUTOR_SEMANTIC_REQUESTS,
                max_physical_posts: MAX_EXECUTOR_SEMANTIC_REQUESTS
                    * MAX_PHYSICAL_ATTEMPTS_PER_SEMANTIC_REQUEST as u16,
                max_tool_calls: MAX_EXECUTOR_TOOL_CALLS,
                wall_time_secs: MAX_EXECUTOR_WALL_TIME_SECS,
            },
            ExperimentPhase::Recovery => Self {
                max_input_tokens: MAX_INPUT_TOKENS_PER_REQUEST,
                max_output_tokens: MAX_OUTPUT_TOKENS_PER_REQUEST,
                max_semantic_requests: MAX_RECOVERY_SEMANTIC_REQUESTS,
                max_physical_posts: MAX_RECOVERY_SEMANTIC_REQUESTS
                    * MAX_PHYSICAL_ATTEMPTS_PER_SEMANTIC_REQUEST as u16,
                max_tool_calls: MAX_RECOVERY_TOOL_CALLS,
                wall_time_secs: MAX_EXECUTOR_WALL_TIME_SECS,
            },
        }
    }

    pub fn validate_for_phase(
        &self,
        phase: ExperimentPhase,
    ) -> Result<(), ProtocolValidationError> {
        if *self != Self::for_phase(phase) {
            return Err(ProtocolValidationError::new(
                "request limits differ from the frozen RND-097 phase limits",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentWorkerSpecV1 {
    pub schema_version: String,
    pub experiment_id: String,
    pub preregistration_sha256: String,
    pub trial_id: String,
    pub slot_index: u16,
    pub fixture_id: String,
    pub arm: ExperimentArm,
    pub repetition: u8,
    pub phase: ExperimentPhase,
    pub route: ExperimentModelRouteV1,
    pub limits: ExperimentRequestLimitsV1,
    pub workspace_id: String,
    pub workspace_root: PathBuf,
    pub controller_root: PathBuf,
    pub source_manifest_sha256: String,
    pub destination_manifest_sha256: String,
    pub cargo_target_dir: PathBuf,
    pub task_input_sha256: String,
    pub task_input: ExperimentDeclaredFileV1,
    pub handoff: ExperimentHandoffV1,
    pub output_schema: ExperimentDeclaredFileV1,
    pub allowed_tools: Vec<String>,
    pub allowed_read_paths: Vec<PathBuf>,
    pub allowed_write_paths: Vec<PathBuf>,
    pub controller_channel: ExperimentControllerChannelV1,
    pub network_allowed: bool,
}

impl std::fmt::Debug for ExperimentWorkerSpecV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExperimentWorkerSpecV1")
            .field("schema_version", &self.schema_version)
            .field("experiment_id", &self.experiment_id)
            .field("trial_id", &self.trial_id)
            .field("slot_index", &self.slot_index)
            .field("fixture_id", &self.fixture_id)
            .field("arm", &self.arm)
            .field("repetition", &self.repetition)
            .field("phase", &self.phase)
            .field("route", &self.route)
            .field("limits", &self.limits)
            .field("workspace_id", &self.workspace_id)
            .field("workspace_root", &"[PRIVATE]")
            .field("controller_root", &"[PRIVATE]")
            .field("cargo_target_dir", &"[PRIVATE]")
            .field("task_input", &"[DECLARED]")
            .field("handoff", &"[DECLARED]")
            .field("output_schema", &"[DECLARED]")
            .field("allowed_tools", &self.allowed_tools)
            .field("allowed_read_paths", &"[DECLARED]")
            .field("allowed_write_paths", &"[DECLARED]")
            .field("controller_channel", &self.controller_channel)
            .field("network_allowed", &self.network_allowed)
            .finish_non_exhaustive()
    }
}

impl ExperimentWorkerSpecV1 {
    pub fn validate(&self) -> Result<(), ProtocolValidationError> {
        if self.schema_version != WORKER_SPEC_SCHEMA_V1 {
            return Err(ProtocolValidationError::new(
                "worker spec schema_version is unsupported",
            ));
        }
        validate_id("experiment_id", &self.experiment_id)?;
        validate_sha256("preregistration_sha256", &self.preregistration_sha256)?;
        validate_id("trial_id", &self.trial_id)?;
        if self.slot_index >= 72 {
            return Err(ProtocolValidationError::new(
                "slot_index must be in the preregistered 0..72 range",
            ));
        }
        validate_id("fixture_id", &self.fixture_id)?;
        if !matches!(self.repetition, 1 | 2) {
            return Err(ProtocolValidationError::new(
                "repetition must be exactly 1 or 2",
            ));
        }
        self.route.validate()?;
        self.limits.validate_for_phase(self.phase)?;
        validate_id("workspace_id", &self.workspace_id)?;
        validate_path("workspace_root", &self.workspace_root)?;
        validate_path("controller_root", &self.controller_root)?;
        validate_path("cargo_target_dir", &self.cargo_target_dir)?;
        if !self.workspace_root.is_absolute()
            || !self.controller_root.is_absolute()
            || !self.cargo_target_dir.is_absolute()
        {
            return Err(ProtocolValidationError::new(
                "worker workspace, controller, and Cargo target paths must be absolute",
            ));
        }
        if roots_overlap(&self.workspace_root, &self.controller_root)
            || self.cargo_target_dir.starts_with(&self.workspace_root)
            || self.cargo_target_dir.starts_with(&self.controller_root)
        {
            return Err(ProtocolValidationError::new(
                "worker roots must not overlap and CARGO_TARGET_DIR must be outside both roots",
            ));
        }
        validate_sha256("source_manifest_sha256", &self.source_manifest_sha256)?;
        validate_sha256(
            "destination_manifest_sha256",
            &self.destination_manifest_sha256,
        )?;
        validate_sha256("task_input_sha256", &self.task_input_sha256)?;
        self.task_input.validate("task_input")?;
        if self.task_input.root != ExperimentFileRoot::Workspace
            || self.task_input.sha256 != self.task_input_sha256
        {
            return Err(ProtocolValidationError::new(
                "task input must be a digest-bound file under the workspace root",
            ));
        }
        self.handoff.validate()?;
        self.output_schema.validate("output_schema")?;
        if self.output_schema.root != ExperimentFileRoot::Controller {
            return Err(ProtocolValidationError::new(
                "output schema must be a digest-bound controller file",
            ));
        }
        validate_grants(self)?;
        validate_handoff_for_phase(self)?;
        self.controller_channel.validate()?;
        if self.network_allowed {
            return Err(ProtocolValidationError::new(
                "experiment tool workers must have network disabled",
            ));
        }
        Ok(())
    }
}

/// A single digest-bound physical Responses request. This is the only payload
/// the credential-holding broker may accept: it contains no executable tool
/// grant, workspace path, provider configuration, or credential material.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderCallSpecV1 {
    pub schema_version: String,
    pub call_id: String,
    pub channel_id: String,
    /// Ephemeral in-memory binding value. It is never persisted or logged.
    pub nonce: String,
    pub worker_spec_sha256: String,
    pub controller_sha256: String,
    pub worker_sha256: String,
    pub experiment_id: String,
    pub preregistration_sha256: String,
    pub trial_id: String,
    pub slot_index: u16,
    pub phase: ExperimentPhase,
    pub semantic_request_index: u16,
    pub physical_attempt: u8,
    pub route: ExperimentModelRouteV1,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub store: bool,
    /// Bounded request bytes for one in-memory POST; never retained in the
    /// ledger, report, session, or debug output.
    pub request_json: String,
    pub request_sha256: String,
    /// Raw only until HTTP header construction. Receipts retain its digest.
    pub x_client_request_id: String,
}

impl std::fmt::Debug for ProviderCallSpecV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderCallSpecV1")
            .field("schema_version", &self.schema_version)
            .field("call_id", &self.call_id)
            .field("channel_id", &self.channel_id)
            .field("nonce", &"[REDACTED]")
            .field("worker_spec_sha256", &self.worker_spec_sha256)
            .field("controller_sha256", &self.controller_sha256)
            .field("worker_sha256", &self.worker_sha256)
            .field("experiment_id", &self.experiment_id)
            .field("trial_id", &self.trial_id)
            .field("slot_index", &self.slot_index)
            .field("phase", &self.phase)
            .field("semantic_request_index", &self.semantic_request_index)
            .field("physical_attempt", &self.physical_attempt)
            .field("route", &self.route)
            .field("max_input_tokens", &self.max_input_tokens)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("store", &self.store)
            .field("request_json", &"[REDACTED]")
            .field("request_sha256", &self.request_sha256)
            .field("x_client_request_id", &"[REDACTED]")
            .finish()
    }
}

impl ProviderCallSpecV1 {
    pub fn validate(&self) -> Result<(), ProtocolValidationError> {
        if self.schema_version != PROVIDER_CALL_SPEC_SCHEMA_V1 {
            return Err(ProtocolValidationError::new(
                "provider call spec schema_version is unsupported",
            ));
        }
        CallIdentityV1 {
            field: "provider_call",
            call_id: &self.call_id,
            channel_id: &self.channel_id,
            nonce: &self.nonce,
            worker_spec_sha256: &self.worker_spec_sha256,
            experiment_id: &self.experiment_id,
            preregistration_sha256: &self.preregistration_sha256,
            trial_id: &self.trial_id,
            slot_index: self.slot_index,
        }
        .validate()?;
        validate_sha256("controller_sha256", &self.controller_sha256)?;
        validate_sha256("worker_sha256", &self.worker_sha256)?;
        self.route.validate()?;
        validate_phase_request_indices(
            self.phase,
            self.semantic_request_index,
            self.physical_attempt,
        )?;
        if self.max_input_tokens != MAX_INPUT_TOKENS_PER_REQUEST
            || self.max_output_tokens != MAX_OUTPUT_TOKENS_PER_REQUEST
            || self.store
        {
            return Err(ProtocolValidationError::new(
                "provider call options differ from the frozen Responses contract",
            ));
        }
        validate_ascii_token(
            "x_client_request_id",
            &self.x_client_request_id,
            MAX_CLIENT_REQUEST_ID_BYTES,
        )?;
        validate_sha256("request_sha256", &self.request_sha256)?;
        if self.request_json.is_empty()
            || self.request_json.len() > MAX_PROVIDER_REQUEST_JSON_BYTES
            || self.request_sha256 != sha256_hex(self.request_json.as_bytes())
        {
            return Err(ProtocolValidationError::new(
                "provider request is empty, oversized, or has a mismatched digest",
            ));
        }
        validate_frozen_responses_body(&self.request_json, &self.route)
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderCallOutcomeV1 {
    Completed {
        /// Sanitized controller-facing turn data. It is bounded, frame-only,
        /// and excluded from Debug output and all durable receipts.
        controller_response_json: String,
        controller_response_sha256: String,
    },
    VerifiedPreOutputFailure {
        reason: String,
    },
    AmbiguousDelivery {
        reason: String,
    },
}

impl std::fmt::Debug for ProviderCallOutcomeV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Completed { .. } => formatter
                .debug_struct("ProviderCallOutcomeV1::Completed")
                .field("controller_response", &"[REDACTED]")
                .finish(),
            Self::VerifiedPreOutputFailure { reason } => formatter
                .debug_struct("ProviderCallOutcomeV1::VerifiedPreOutputFailure")
                .field("reason", reason)
                .finish(),
            Self::AmbiguousDelivery { reason } => formatter
                .debug_struct("ProviderCallOutcomeV1::AmbiguousDelivery")
                .field("reason", reason)
                .finish(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderCallReceiptV1 {
    pub schema_version: String,
    pub call_id: String,
    pub channel_id: String,
    pub nonce: String,
    pub worker_spec_sha256: String,
    pub request_sha256: String,
    pub reservation_id: String,
    pub x_client_request_id_sha256: String,
    pub x_request_id_sha256: Option<String>,
    pub returned_model: Option<String>,
    pub returned_service_tier: Option<String>,
    pub usage: Option<ExperimentUsageV1>,
    pub elapsed_millis: u64,
    pub outcome: ProviderCallOutcomeV1,
}

impl std::fmt::Debug for ProviderCallReceiptV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let outcome = match &self.outcome {
            ProviderCallOutcomeV1::Completed { .. } => "completed",
            ProviderCallOutcomeV1::VerifiedPreOutputFailure { .. } => "verified_pre_output_failure",
            ProviderCallOutcomeV1::AmbiguousDelivery { .. } => "ambiguous_delivery",
        };
        formatter
            .debug_struct("ProviderCallReceiptV1")
            .field("schema_version", &self.schema_version)
            .field("call_id", &self.call_id)
            .field("channel_id", &self.channel_id)
            .field("nonce", &"[REDACTED]")
            .field("worker_spec_sha256", &self.worker_spec_sha256)
            .field("request_sha256", &self.request_sha256)
            .field("reservation_id", &self.reservation_id)
            .field(
                "x_client_request_id_sha256",
                &self.x_client_request_id_sha256,
            )
            .field("x_request_id_sha256", &self.x_request_id_sha256)
            .field("returned_model", &self.returned_model)
            .field("returned_service_tier", &self.returned_service_tier)
            .field("usage", &self.usage)
            .field("elapsed_millis", &self.elapsed_millis)
            .field("outcome", &outcome)
            .finish()
    }
}

impl ProviderCallReceiptV1 {
    pub fn validate_against(
        &self,
        spec: &ProviderCallSpecV1,
    ) -> Result<(), ProtocolValidationError> {
        spec.validate()?;
        if self.schema_version != PROVIDER_CALL_RECEIPT_SCHEMA_V1
            || self.call_id != spec.call_id
            || self.channel_id != spec.channel_id
            || self.nonce != spec.nonce
            || self.worker_spec_sha256 != spec.worker_spec_sha256
            || self.request_sha256 != spec.request_sha256
        {
            return Err(ProtocolValidationError::new(
                "provider receipt is not bound to its physical call spec",
            ));
        }
        validate_id("reservation_id", &self.reservation_id)?;
        validate_sha256(
            "x_client_request_id_sha256",
            &self.x_client_request_id_sha256,
        )?;
        if self.x_client_request_id_sha256 != sha256_hex(spec.x_client_request_id.as_bytes()) {
            return Err(ProtocolValidationError::new(
                "provider receipt client request ID digest is mismatched",
            ));
        }
        if self.elapsed_millis > MAX_PROVIDER_LATENCY_MILLIS {
            return Err(ProtocolValidationError::new(
                "provider receipt exceeds the bounded phase duration",
            ));
        }
        match &self.outcome {
            ProviderCallOutcomeV1::Completed {
                controller_response_json,
                controller_response_sha256,
            } => {
                let (request_id, model, tier, usage) = (
                    self.x_request_id_sha256.as_deref(),
                    self.returned_model.as_deref(),
                    self.returned_service_tier.as_deref(),
                    self.usage,
                );
                validate_sha256(
                    "x_request_id_sha256",
                    request_id.ok_or_else(|| {
                        ProtocolValidationError::new(
                            "completed provider receipt lacks request metadata",
                        )
                    })?,
                )?;
                if model != Some(spec.route.model.as_str()) || tier != Some(SERVICE_TIER) {
                    return Err(ProtocolValidationError::new(
                        "completed provider receipt model or service tier drifted",
                    ));
                }
                usage
                    .ok_or_else(|| {
                        ProtocolValidationError::new("completed provider receipt lacks usage")
                    })?
                    .validate()?;
                validate_frame_json(
                    "controller_response_json",
                    controller_response_json,
                    MAX_PROVIDER_RESPONSE_JSON_BYTES,
                )?;
                validate_sha256("controller_response_sha256", controller_response_sha256)?;
                if controller_response_sha256 != &sha256_hex(controller_response_json.as_bytes()) {
                    return Err(ProtocolValidationError::new(
                        "controller response payload has a mismatched digest",
                    ));
                }
            }
            ProviderCallOutcomeV1::VerifiedPreOutputFailure { reason }
            | ProviderCallOutcomeV1::AmbiguousDelivery { reason } => {
                validate_reason(reason)?;
                if self.x_request_id_sha256.is_some()
                    || self.returned_model.is_some()
                    || self.returned_service_tier.is_some()
                    || self.usage.is_some()
                {
                    return Err(ProtocolValidationError::new(
                        "noncompleted provider receipt may not retain response metadata or usage",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// One exact local tool call. It carries no provider route, credential,
/// request body, or configurable network grant.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolCallSpecV1 {
    pub schema_version: String,
    pub call_id: String,
    pub channel_id: String,
    pub nonce: String,
    pub worker_spec_sha256: String,
    pub workspace_id: String,
    pub workspace_root: PathBuf,
    pub source_manifest_sha256: String,
    pub cargo_target_dir: PathBuf,
    pub tool_name: String,
    pub tool_call_id: String,
    pub arguments_json: String,
    pub arguments_sha256: String,
    pub allowed_read_paths: Vec<PathBuf>,
    pub allowed_write_paths: Vec<PathBuf>,
    pub timeout_millis: u64,
}

impl std::fmt::Debug for ToolCallSpecV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ToolCallSpecV1")
            .field("schema_version", &self.schema_version)
            .field("call_id", &self.call_id)
            .field("channel_id", &self.channel_id)
            .field("nonce", &"[REDACTED]")
            .field("worker_spec_sha256", &self.worker_spec_sha256)
            .field("workspace_id", &self.workspace_id)
            .field("workspace_root", &"[PRIVATE]")
            .field("source_manifest_sha256", &self.source_manifest_sha256)
            .field("cargo_target_dir", &"[PRIVATE]")
            .field("tool_name", &self.tool_name)
            .field("tool_call_id", &self.tool_call_id)
            .field("arguments_json", &"[REDACTED]")
            .field("arguments_sha256", &self.arguments_sha256)
            .field("allowed_read_paths", &"[DECLARED]")
            .field("allowed_write_paths", &"[DECLARED]")
            .field("timeout_millis", &self.timeout_millis)
            .finish()
    }
}

impl ToolCallSpecV1 {
    pub fn validate(&self) -> Result<(), ProtocolValidationError> {
        if self.schema_version != TOOL_CALL_SPEC_SCHEMA_V1 {
            return Err(ProtocolValidationError::new(
                "tool call spec schema_version is unsupported",
            ));
        }
        validate_id("tool_call.call_id", &self.call_id)?;
        validate_id("tool_call.channel_id", &self.channel_id)?;
        validate_nonce(&self.nonce)?;
        validate_sha256("tool_call.worker_spec_sha256", &self.worker_spec_sha256)?;
        validate_id("workspace_id", &self.workspace_id)?;
        validate_path("workspace_root", &self.workspace_root)?;
        validate_path("cargo_target_dir", &self.cargo_target_dir)?;
        if !self.workspace_root.is_absolute()
            || !self.cargo_target_dir.is_absolute()
            || self.cargo_target_dir.starts_with(&self.workspace_root)
        {
            return Err(ProtocolValidationError::new(
                "tool workspace and CARGO_TARGET_DIR roots are invalid or overlap",
            ));
        }
        validate_sha256("source_manifest_sha256", &self.source_manifest_sha256)?;
        validate_ascii_token("tool_name", &self.tool_name, MAX_TOOL_NAME_BYTES)?;
        validate_ascii_token("tool_call_id", &self.tool_call_id, MAX_TOOL_CALL_ID_BYTES)?;
        validate_frame_json(
            "arguments_json",
            &self.arguments_json,
            MAX_TOOL_ARGUMENT_JSON_BYTES,
        )?;
        validate_sha256("arguments_sha256", &self.arguments_sha256)?;
        if self.arguments_sha256 != sha256_hex(self.arguments_json.as_bytes()) {
            return Err(ProtocolValidationError::new(
                "tool arguments have a mismatched digest",
            ));
        }
        validate_relative_grants(&self.allowed_read_paths, &self.allowed_write_paths)?;
        if self.timeout_millis == 0 || self.timeout_millis > MAX_PROVIDER_LATENCY_MILLIS {
            return Err(ProtocolValidationError::new(
                "tool timeout is outside the bounded phase duration",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallOutcomeV1 {
    Completed,
    TimedOut,
    Failed { reason: String },
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolCallReceiptV1 {
    pub schema_version: String,
    pub call_id: String,
    pub channel_id: String,
    pub nonce: String,
    pub worker_spec_sha256: String,
    pub arguments_sha256: String,
    pub elapsed_millis: u64,
    pub outcome: ToolCallOutcomeV1,
    pub result_json: Option<String>,
    pub result_sha256: Option<String>,
}

impl std::fmt::Debug for ToolCallReceiptV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ToolCallReceiptV1")
            .field("schema_version", &self.schema_version)
            .field("call_id", &self.call_id)
            .field("channel_id", &self.channel_id)
            .field("nonce", &"[REDACTED]")
            .field("worker_spec_sha256", &self.worker_spec_sha256)
            .field("arguments_sha256", &self.arguments_sha256)
            .field("elapsed_millis", &self.elapsed_millis)
            .field("outcome", &self.outcome)
            .field("result_json", &"[REDACTED]")
            .field("result_sha256", &self.result_sha256)
            .finish()
    }
}

impl ToolCallReceiptV1 {
    pub fn validate_against(&self, spec: &ToolCallSpecV1) -> Result<(), ProtocolValidationError> {
        spec.validate()?;
        if self.schema_version != TOOL_CALL_RECEIPT_SCHEMA_V1
            || self.call_id != spec.call_id
            || self.channel_id != spec.channel_id
            || self.nonce != spec.nonce
            || self.worker_spec_sha256 != spec.worker_spec_sha256
            || self.arguments_sha256 != spec.arguments_sha256
        {
            return Err(ProtocolValidationError::new(
                "tool receipt is not bound to its exact tool call spec",
            ));
        }
        if self.elapsed_millis > spec.timeout_millis {
            return Err(ProtocolValidationError::new(
                "tool receipt exceeded its exact call timeout",
            ));
        }
        match &self.outcome {
            ToolCallOutcomeV1::Completed => {
                let result = self.result_json.as_deref().ok_or_else(|| {
                    ProtocolValidationError::new("completed tool receipt lacks result data")
                })?;
                let digest = self.result_sha256.as_deref().ok_or_else(|| {
                    ProtocolValidationError::new("completed tool receipt lacks result digest")
                })?;
                validate_frame_json("result_json", result, MAX_TOOL_OUTPUT_BYTES)?;
                validate_sha256("result_sha256", digest)?;
                if digest != sha256_hex(result.as_bytes()) {
                    return Err(ProtocolValidationError::new(
                        "tool result has a mismatched digest",
                    ));
                }
            }
            ToolCallOutcomeV1::TimedOut => {
                if self.result_json.is_some() || self.result_sha256.is_some() {
                    return Err(ProtocolValidationError::new(
                        "timed-out tool receipt may not retain partial result data",
                    ));
                }
            }
            ToolCallOutcomeV1::Failed { reason } => {
                validate_reason(reason)?;
                if self.result_json.is_some() || self.result_sha256.is_some() {
                    return Err(ProtocolValidationError::new(
                        "failed tool receipt may not retain result data",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IpcFrameKindV1 {
    ProviderCallSpec,
    ProviderCallReceipt,
    ToolCallSpec,
    ToolCallReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IpcFrameMetadataV1 {
    pub schema_version: String,
    pub protocol_version: u16,
    pub kind: IpcFrameKindV1,
    pub channel_id: String,
    pub nonce_sha256: String,
    pub payload_sha256: String,
    pub payload_bytes: u32,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct IpcFrameV1<T> {
    metadata: IpcFrameMetadataV1,
    payload: T,
}

/// Physical IPC payloads are bound to an ephemeral channel nonce. The raw
/// nonce appears only in the bounded inherited pipe frame and is never emitted
/// by Debug or persisted in an experiment receipt.
pub trait IpcPayloadV1: Serialize + DeserializeOwned + Sized {
    const FRAME_KIND: IpcFrameKindV1;

    fn channel_id(&self) -> &str;
    fn nonce(&self) -> &str;
    fn validate_ipc_payload(&self) -> Result<(), ProtocolValidationError>;
}

impl IpcPayloadV1 for ProviderCallSpecV1 {
    const FRAME_KIND: IpcFrameKindV1 = IpcFrameKindV1::ProviderCallSpec;

    fn channel_id(&self) -> &str {
        &self.channel_id
    }

    fn nonce(&self) -> &str {
        &self.nonce
    }

    fn validate_ipc_payload(&self) -> Result<(), ProtocolValidationError> {
        self.validate()
    }
}

impl IpcPayloadV1 for ProviderCallReceiptV1 {
    const FRAME_KIND: IpcFrameKindV1 = IpcFrameKindV1::ProviderCallReceipt;

    fn channel_id(&self) -> &str {
        &self.channel_id
    }

    fn nonce(&self) -> &str {
        &self.nonce
    }

    fn validate_ipc_payload(&self) -> Result<(), ProtocolValidationError> {
        validate_id("provider_receipt.channel_id", &self.channel_id)?;
        validate_nonce(&self.nonce)?;
        validate_id("provider_receipt.call_id", &self.call_id)?;
        validate_sha256(
            "provider_receipt.worker_spec_sha256",
            &self.worker_spec_sha256,
        )?;
        validate_sha256("provider_receipt.request_sha256", &self.request_sha256)?;
        validate_id("provider_receipt.reservation_id", &self.reservation_id)?;
        validate_sha256(
            "provider_receipt.x_client_request_id_sha256",
            &self.x_client_request_id_sha256,
        )
    }
}

impl IpcPayloadV1 for ToolCallSpecV1 {
    const FRAME_KIND: IpcFrameKindV1 = IpcFrameKindV1::ToolCallSpec;

    fn channel_id(&self) -> &str {
        &self.channel_id
    }

    fn nonce(&self) -> &str {
        &self.nonce
    }

    fn validate_ipc_payload(&self) -> Result<(), ProtocolValidationError> {
        self.validate()
    }
}

impl IpcPayloadV1 for ToolCallReceiptV1 {
    const FRAME_KIND: IpcFrameKindV1 = IpcFrameKindV1::ToolCallReceipt;

    fn channel_id(&self) -> &str {
        &self.channel_id
    }

    fn nonce(&self) -> &str {
        &self.nonce
    }

    fn validate_ipc_payload(&self) -> Result<(), ProtocolValidationError> {
        validate_id("tool_receipt.channel_id", &self.channel_id)?;
        validate_nonce(&self.nonce)?;
        validate_id("tool_receipt.call_id", &self.call_id)?;
        validate_sha256("tool_receipt.worker_spec_sha256", &self.worker_spec_sha256)?;
        validate_sha256("tool_receipt.arguments_sha256", &self.arguments_sha256)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IpcFrameError {
    #[error("IPC frame validation failed: {0}")]
    Validation(#[from] ProtocolValidationError),
    #[error("IPC frame I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("IPC frame JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IPC frame length is invalid")]
    InvalidLength,
}

/// Serialize one canonical, length-prefixed, nonce-bound frame. `max_frame`
/// includes its four-byte big-endian length prefix.
pub fn encode_ipc_frame<T: IpcPayloadV1>(
    payload: &T,
    max_frame: u32,
) -> Result<Vec<u8>, IpcFrameError> {
    payload.validate_ipc_payload()?;
    let payload_bytes = serde_json::to_vec(payload)?;
    let frame_bytes =
        u32::try_from(payload_bytes.len()).map_err(|_| IpcFrameError::InvalidLength)?;
    if frame_bytes > max_frame.saturating_sub(IPC_LENGTH_PREFIX_BYTES as u32) {
        return Err(IpcFrameError::InvalidLength);
    }
    let metadata = IpcFrameMetadataV1 {
        schema_version: IPC_FRAME_METADATA_SCHEMA_V1.into(),
        protocol_version: IPC_PROTOCOL_VERSION_V1,
        kind: T::FRAME_KIND,
        channel_id: payload.channel_id().into(),
        nonce_sha256: sha256_hex(payload.nonce().as_bytes()),
        payload_sha256: sha256_hex(&payload_bytes),
        payload_bytes: frame_bytes,
    };
    let encoded = serde_json::to_vec(&IpcFrameV1 { metadata, payload })?;
    let encoded_len = u32::try_from(encoded.len()).map_err(|_| IpcFrameError::InvalidLength)?;
    if encoded_len > max_frame.saturating_sub(IPC_LENGTH_PREFIX_BYTES as u32) {
        return Err(IpcFrameError::InvalidLength);
    }
    let mut frame = Vec::with_capacity(IPC_LENGTH_PREFIX_BYTES + encoded.len());
    frame.extend_from_slice(&encoded_len.to_be_bytes());
    frame.extend_from_slice(&encoded);
    Ok(frame)
}

pub fn decode_ipc_frame<T: IpcPayloadV1>(frame: &[u8], max_frame: u32) -> Result<T, IpcFrameError> {
    if frame.len() < IPC_LENGTH_PREFIX_BYTES
        || frame.len() > usize::try_from(max_frame).unwrap_or(usize::MAX)
    {
        return Err(IpcFrameError::InvalidLength);
    }
    let declared = u32::from_be_bytes(
        frame[..IPC_LENGTH_PREFIX_BYTES]
            .try_into()
            .map_err(|_| IpcFrameError::InvalidLength)?,
    );
    if declared > max_frame.saturating_sub(IPC_LENGTH_PREFIX_BYTES as u32)
        || usize::try_from(declared).ok() != Some(frame.len() - IPC_LENGTH_PREFIX_BYTES)
    {
        return Err(IpcFrameError::InvalidLength);
    }
    let parsed: IpcFrameV1<T> = serde_json::from_slice(&frame[IPC_LENGTH_PREFIX_BYTES..])?;
    let payload_bytes = serde_json::to_vec(&parsed.payload)?;
    validate_ipc_metadata::<T>(&parsed.metadata, &parsed.payload, &payload_bytes)?;
    Ok(parsed.payload)
}

pub fn write_ipc_frame<W: Write, T: IpcPayloadV1>(
    writer: &mut W,
    payload: &T,
    max_frame: u32,
) -> Result<(), IpcFrameError> {
    let frame = encode_ipc_frame(payload, max_frame)?;
    writer.write_all(&frame)?;
    writer.flush()?;
    Ok(())
}

pub fn read_ipc_frame<R: Read, T: IpcPayloadV1>(
    reader: &mut R,
    max_frame: u32,
) -> Result<T, IpcFrameError> {
    let mut prefix = [0_u8; IPC_LENGTH_PREFIX_BYTES];
    reader.read_exact(&mut prefix)?;
    let length = u32::from_be_bytes(prefix);
    if length > max_frame.saturating_sub(IPC_LENGTH_PREFIX_BYTES as u32) {
        return Err(IpcFrameError::InvalidLength);
    }
    let mut frame = Vec::with_capacity(IPC_LENGTH_PREFIX_BYTES + length as usize);
    frame.extend_from_slice(&prefix);
    frame.resize(IPC_LENGTH_PREFIX_BYTES + length as usize, 0);
    reader.read_exact(&mut frame[IPC_LENGTH_PREFIX_BYTES..])?;
    decode_ipc_frame(&frame, max_frame)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentReservationRequestV1 {
    pub schema_version: String,
    pub channel_id: String,
    pub experiment_id: String,
    pub preregistration_sha256: String,
    pub trial_id: String,
    pub slot_index: u16,
    pub phase: ExperimentPhase,
    pub semantic_request_index: u16,
    pub physical_attempt: u8,
    pub route: ExperimentModelRouteV1,
    pub request_sha256: String,
    pub x_client_request_id: String,
}

impl ExperimentReservationRequestV1 {
    pub fn validate(&self, spec: &ExperimentWorkerSpecV1) -> Result<(), ProtocolValidationError> {
        spec.validate()?;
        if self.schema_version != RESERVATION_REQUEST_SCHEMA_V1
            || self.channel_id != spec.controller_channel.channel_id
            || self.experiment_id != spec.experiment_id
            || self.preregistration_sha256 != spec.preregistration_sha256
            || self.trial_id != spec.trial_id
            || self.slot_index != spec.slot_index
            || self.phase != spec.phase
            || self.route != spec.route
        {
            return Err(ProtocolValidationError::new(
                "reservation request is not bound to its worker spec",
            ));
        }
        if self.semantic_request_index >= spec.limits.max_semantic_requests
            || !(1..=MAX_PHYSICAL_ATTEMPTS_PER_SEMANTIC_REQUEST).contains(&self.physical_attempt)
        {
            return Err(ProtocolValidationError::new(
                "reservation request exceeds semantic or physical attempt limits",
            ));
        }
        validate_sha256("request_sha256", &self.request_sha256)?;
        validate_id("x_client_request_id", &self.x_client_request_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentReservationGrantV1 {
    pub schema_version: String,
    pub channel_id: String,
    pub reservation_id: String,
    pub request_sha256: String,
    pub reserved_atto_usd: String,
    pub ledger_event_sha256: String,
}

impl ExperimentReservationGrantV1 {
    pub fn validate_against(
        &self,
        request: &ExperimentReservationRequestV1,
    ) -> Result<(), ProtocolValidationError> {
        if self.schema_version != RESERVATION_GRANT_SCHEMA_V1
            || self.channel_id != request.channel_id
            || self.request_sha256 != request.request_sha256
        {
            return Err(ProtocolValidationError::new(
                "reservation grant is not bound to its request",
            ));
        }
        validate_id("reservation_id", &self.reservation_id)?;
        validate_atto_usd("reserved_atto_usd", &self.reserved_atto_usd)?;
        validate_sha256("ledger_event_sha256", &self.ledger_event_sha256)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExperimentReservationSettlementV1 {
    Settled {
        schema_version: String,
        channel_id: String,
        reservation_id: String,
        request_sha256: String,
        provider_receipt: ExperimentProviderReceiptV1,
    },
    Unknown {
        schema_version: String,
        channel_id: String,
        reservation_id: String,
        request_sha256: String,
        reason: String,
    },
    ReleasedPreSend {
        schema_version: String,
        channel_id: String,
        reservation_id: String,
        request_sha256: String,
        reason: String,
    },
}

impl ExperimentReservationSettlementV1 {
    pub fn validate_against(
        &self,
        grant: &ExperimentReservationGrantV1,
        route: &ExperimentModelRouteV1,
        limits: &ExperimentRequestLimitsV1,
    ) -> Result<(), ProtocolValidationError> {
        let (schema, channel, reservation, request_sha) = match self {
            Self::Settled {
                schema_version,
                channel_id,
                reservation_id,
                request_sha256,
                provider_receipt,
            } => {
                provider_receipt.validate(route, limits)?;
                if provider_receipt.reservation_id != *reservation_id {
                    return Err(ProtocolValidationError::new(
                        "provider receipt reservation does not match settlement",
                    ));
                }
                (schema_version, channel_id, reservation_id, request_sha256)
            }
            Self::Unknown {
                schema_version,
                channel_id,
                reservation_id,
                request_sha256,
                reason,
            }
            | Self::ReleasedPreSend {
                schema_version,
                channel_id,
                reservation_id,
                request_sha256,
                reason,
            } => {
                validate_reason(reason)?;
                (schema_version, channel_id, reservation_id, request_sha256)
            }
        };
        if schema != RESERVATION_SETTLEMENT_SCHEMA_V1
            || channel != &grant.channel_id
            || reservation != &grant.reservation_id
            || request_sha != &grant.request_sha256
        {
            return Err(ProtocolValidationError::new(
                "reservation settlement is not bound to its durable grant",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentUsageV1 {
    pub regular_input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_tokens: u64,
    pub regular_output_tokens: u64,
    pub reasoning_output_tokens: u64,
}

impl ExperimentUsageV1 {
    pub fn checked_total_input(self) -> Option<u64> {
        self.regular_input_tokens
            .checked_add(self.cached_input_tokens)?
            .checked_add(self.cache_write_tokens)
    }

    pub fn checked_total_output(self) -> Option<u64> {
        self.regular_output_tokens
            .checked_add(self.reasoning_output_tokens)
    }

    pub fn validate(&self) -> Result<(), ProtocolValidationError> {
        let input = self.checked_total_input().ok_or_else(|| {
            ProtocolValidationError::new("provider input-token categories overflow")
        })?;
        let output = self.checked_total_output().ok_or_else(|| {
            ProtocolValidationError::new("provider output-token categories overflow")
        })?;
        if input > MAX_INPUT_TOKENS_PER_REQUEST || output > MAX_OUTPUT_TOKENS_PER_REQUEST {
            return Err(ProtocolValidationError::new(
                "provider usage exceeds a preregistered physical-request bound",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentProviderReceiptV1 {
    pub reservation_id: String,
    pub semantic_request_index: u16,
    pub physical_attempt: u8,
    pub x_client_request_id: String,
    pub x_request_id: String,
    pub requested_model: String,
    pub returned_model: String,
    pub returned_service_tier: String,
    pub usage: ExperimentUsageV1,
}

impl ExperimentProviderReceiptV1 {
    pub fn validate(
        &self,
        route: &ExperimentModelRouteV1,
        limits: &ExperimentRequestLimitsV1,
    ) -> Result<(), ProtocolValidationError> {
        validate_id("reservation_id", &self.reservation_id)?;
        if self.semantic_request_index >= limits.max_semantic_requests {
            return Err(ProtocolValidationError::new(
                "semantic request index exceeds the phase bound",
            ));
        }
        if !(1..=MAX_PHYSICAL_ATTEMPTS_PER_SEMANTIC_REQUEST).contains(&self.physical_attempt) {
            return Err(ProtocolValidationError::new(
                "physical_attempt must be one or two",
            ));
        }
        validate_id("x_client_request_id", &self.x_client_request_id)?;
        validate_id("x_request_id", &self.x_request_id)?;
        if self.requested_model != route.model || self.returned_model != route.model {
            return Err(ProtocolValidationError::new(
                "provider returned-model identity drifted from the preregistration",
            ));
        }
        if self.returned_service_tier != SERVICE_TIER {
            return Err(ProtocolValidationError::new(
                "provider returned a non-default service tier",
            ));
        }
        self.usage.validate()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentWorkerOutcomeV1 {
    Completed,
    DeterministicVerifierFailed,
    VerifiedPreOutputTransportFailure { reason: String },
    AmbiguousProviderDelivery { reason: String },
    BudgetStopped,
    WorkerFailed { reason: String },
}

impl ExperimentWorkerOutcomeV1 {
    fn validate(&self) -> Result<(), ProtocolValidationError> {
        match self {
            Self::VerifiedPreOutputTransportFailure { reason }
            | Self::AmbiguousProviderDelivery { reason }
            | Self::WorkerFailed { reason } => validate_reason(reason),
            Self::Completed | Self::DeterministicVerifierFailed | Self::BudgetStopped => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentWorkerReceiptV1 {
    pub schema_version: String,
    pub experiment_id: String,
    pub preregistration_sha256: String,
    pub trial_id: String,
    pub slot_index: u16,
    pub fixture_id: String,
    pub arm: ExperimentArm,
    pub repetition: u8,
    pub phase: ExperimentPhase,
    pub route: ExperimentModelRouteV1,
    pub semantic_requests: u16,
    pub tool_calls: u16,
    pub elapsed_millis: u64,
    pub provider_receipts: Vec<ExperimentProviderReceiptV1>,
    pub final_manifest_sha256: String,
    pub recovery_artifact_sha256: String,
    pub recovery_artifact_size: u64,
    pub outcome: ExperimentWorkerOutcomeV1,
}

impl ExperimentWorkerReceiptV1 {
    pub fn validate_against(
        &self,
        spec: &ExperimentWorkerSpecV1,
    ) -> Result<(), ProtocolValidationError> {
        spec.validate()?;
        if self.schema_version != WORKER_RECEIPT_SCHEMA_V1 {
            return Err(ProtocolValidationError::new(
                "worker receipt schema_version is unsupported",
            ));
        }
        if self.experiment_id != spec.experiment_id
            || self.preregistration_sha256 != spec.preregistration_sha256
            || self.trial_id != spec.trial_id
            || self.slot_index != spec.slot_index
            || self.fixture_id != spec.fixture_id
            || self.arm != spec.arm
            || self.repetition != spec.repetition
            || self.phase != spec.phase
            || self.route != spec.route
        {
            return Err(ProtocolValidationError::new(
                "worker receipt identity differs from its immutable spec",
            ));
        }
        if self.semantic_requests > spec.limits.max_semantic_requests
            || self.tool_calls > spec.limits.max_tool_calls
            || self.provider_receipts.len() > MAX_PROVIDER_RECEIPTS
            || self.provider_receipts.len() > usize::from(spec.limits.max_physical_posts)
            || self.elapsed_millis > u64::from(spec.limits.wall_time_secs) * 1_000
        {
            return Err(ProtocolValidationError::new(
                "worker receipt exceeds a preregistered phase bound",
            ));
        }
        for receipt in &self.provider_receipts {
            receipt.validate(&spec.route, &spec.limits)?;
        }
        validate_sha256("final_manifest_sha256", &self.final_manifest_sha256)?;
        validate_sha256("recovery_artifact_sha256", &self.recovery_artifact_sha256)?;
        if self.recovery_artifact_size == 0 {
            return Err(ProtocolValidationError::new(
                "recovery artifact must contain at least one byte",
            ));
        }
        self.outcome.validate()
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[error("{message}")]
pub struct ProtocolValidationError {
    message: String,
}

impl ProtocolValidationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

fn validate_ipc_metadata<T: IpcPayloadV1>(
    metadata: &IpcFrameMetadataV1,
    payload: &T,
    payload_bytes: &[u8],
) -> Result<(), ProtocolValidationError> {
    if metadata.schema_version != IPC_FRAME_METADATA_SCHEMA_V1
        || metadata.protocol_version != IPC_PROTOCOL_VERSION_V1
        || metadata.kind != T::FRAME_KIND
        || metadata.channel_id != payload.channel_id()
        || metadata.nonce_sha256 != sha256_hex(payload.nonce().as_bytes())
        || metadata.payload_sha256 != sha256_hex(payload_bytes)
        || usize::try_from(metadata.payload_bytes).ok() != Some(payload_bytes.len())
    {
        return Err(ProtocolValidationError::new(
            "IPC frame metadata is not bound to its payload",
        ));
    }
    validate_id("frame.channel_id", &metadata.channel_id)?;
    validate_sha256("frame.nonce_sha256", &metadata.nonce_sha256)?;
    validate_sha256("frame.payload_sha256", &metadata.payload_sha256)?;
    payload.validate_ipc_payload()
}

struct CallIdentityV1<'a> {
    field: &'a str,
    call_id: &'a str,
    channel_id: &'a str,
    nonce: &'a str,
    worker_spec_sha256: &'a str,
    experiment_id: &'a str,
    preregistration_sha256: &'a str,
    trial_id: &'a str,
    slot_index: u16,
}

impl CallIdentityV1<'_> {
    fn validate(&self) -> Result<(), ProtocolValidationError> {
        validate_ascii_token(
            &format!("{}.call_id", self.field),
            self.call_id,
            MAX_CALL_ID_BYTES,
        )?;
        validate_id(&format!("{}.channel_id", self.field), self.channel_id)?;
        validate_nonce(self.nonce)?;
        validate_sha256(
            &format!("{}.worker_spec_sha256", self.field),
            self.worker_spec_sha256,
        )?;
        validate_id(&format!("{}.experiment_id", self.field), self.experiment_id)?;
        validate_sha256(
            &format!("{}.preregistration_sha256", self.field),
            self.preregistration_sha256,
        )?;
        validate_id(&format!("{}.trial_id", self.field), self.trial_id)?;
        if self.slot_index >= 72 {
            return Err(ProtocolValidationError::new(
                "physical call slot is outside the preregistered range",
            ));
        }
        Ok(())
    }
}

fn validate_phase_request_indices(
    phase: ExperimentPhase,
    semantic_request_index: u16,
    physical_attempt: u8,
) -> Result<(), ProtocolValidationError> {
    let limits = ExperimentRequestLimitsV1::for_phase(phase);
    if semantic_request_index >= limits.max_semantic_requests
        || !(1..=MAX_PHYSICAL_ATTEMPTS_PER_SEMANTIC_REQUEST).contains(&physical_attempt)
    {
        return Err(ProtocolValidationError::new(
            "physical call exceeds its frozen semantic or retry limits",
        ));
    }
    Ok(())
}

fn validate_nonce(value: &str) -> Result<(), ProtocolValidationError> {
    validate_ascii_token("nonce", value, MAX_NONCE_BYTES)
}

fn validate_ascii_token(
    field: &str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), ProtocolValidationError> {
    if value.is_empty()
        || value.len() > maximum_bytes
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'\'' | b'\"' | b'\\'))
    {
        return Err(ProtocolValidationError::new(format!(
            "{field} must be bounded printable ASCII without quoting characters"
        )));
    }
    Ok(())
}

fn validate_frame_json(
    field: &str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), ProtocolValidationError> {
    if value.is_empty() || value.len() > maximum_bytes {
        return Err(ProtocolValidationError::new(format!(
            "{field} is empty or exceeds its frame bound"
        )));
    }
    serde_json::from_str::<serde_json::Value>(value)
        .map_err(|_| ProtocolValidationError::new(format!("{field} must contain valid JSON")))?;
    Ok(())
}

fn validate_frozen_responses_body(
    value: &str,
    route: &ExperimentModelRouteV1,
) -> Result<(), ProtocolValidationError> {
    validate_frame_json("request_json", value, MAX_PROVIDER_REQUEST_JSON_BYTES)?;
    let object = serde_json::from_str::<serde_json::Value>(value)
        .map_err(|_| ProtocolValidationError::new("request_json must contain valid JSON"))?;
    let object = object
        .as_object()
        .ok_or_else(|| ProtocolValidationError::new("request_json must be an object"))?;
    const ALLOWED_FIELDS: [&str; 10] = [
        "model",
        "input",
        "tools",
        "text",
        "stream",
        "reasoning",
        "include",
        "max_output_tokens",
        "store",
        "service_tier",
    ];
    if object
        .keys()
        .any(|field| !ALLOWED_FIELDS.contains(&field.as_str()))
        || object
            .get("input")
            .and_then(serde_json::Value::as_array)
            .is_none_or(Vec::is_empty)
    {
        return Err(ProtocolValidationError::new(
            "provider request body contains an unfrozen field or invalid input",
        ));
    }
    let reasoning = object
        .get("reasoning")
        .and_then(serde_json::Value::as_object);
    if object.get("model").and_then(serde_json::Value::as_str) != Some(route.model.as_str())
        || object
            .get("max_output_tokens")
            .and_then(serde_json::Value::as_u64)
            != Some(MAX_OUTPUT_TOKENS_PER_REQUEST)
        || object.get("store").and_then(serde_json::Value::as_bool) != Some(false)
        || object
            .get("service_tier")
            .and_then(serde_json::Value::as_str)
            != Some(SERVICE_TIER)
        || object.get("stream").and_then(serde_json::Value::as_bool) != Some(true)
        || reasoning
            .and_then(|reasoning| reasoning.get("effort"))
            .and_then(serde_json::Value::as_str)
            != Some(REASONING_EFFORT)
        || reasoning.is_none_or(|reasoning| reasoning.len() != 1)
        || object.get("include") != Some(&serde_json::json!(["reasoning.encrypted_content"]))
        || object.contains_key("previous_response_id")
        || object.contains_key("prompt_cache_key")
    {
        return Err(ProtocolValidationError::new(
            "provider request body differs from the frozen Responses options",
        ));
    }
    if let Some(tools) = object.get("tools") {
        let tools = tools.as_array().ok_or_else(|| {
            ProtocolValidationError::new("provider request tools must be a bounded array")
        })?;
        let mut names = std::collections::BTreeSet::new();
        if tools.is_empty()
            || tools.len() > 3
            || tools.iter().any(|tool| {
                let Some(tool) = tool.as_object() else {
                    return true;
                };
                let name = tool.get("name").and_then(serde_json::Value::as_str);
                tool.get("type").and_then(serde_json::Value::as_str) != Some("function")
                    || tool.get("strict").and_then(serde_json::Value::as_bool) != Some(true)
                    || tool
                        .get("parameters")
                        .and_then(serde_json::Value::as_object)
                        .is_none()
                    || !matches!(name, Some("read_file" | "write_file" | "run_command"))
                    || !names.insert(name.unwrap_or_default())
            })
        {
            return Err(ProtocolValidationError::new(
                "provider request tools differ from the frozen experiment surface",
            ));
        }
    }
    if let Some(text) = object.get("text") {
        let format = text
            .as_object()
            .filter(|text| text.len() == 1)
            .and_then(|text| text.get("format"))
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| {
                ProtocolValidationError::new(
                    "provider response format differs from the frozen structured-output shape",
                )
            })?;
        let name = format.get("name").and_then(serde_json::Value::as_str);
        if format.len() != 4
            || format.get("type").and_then(serde_json::Value::as_str) != Some("json_schema")
            || format.get("strict").and_then(serde_json::Value::as_bool) != Some(true)
            || format
                .get("schema")
                .and_then(serde_json::Value::as_object)
                .is_none()
            || name.is_none_or(|name| {
                name.is_empty()
                    || name.len() > 64
                    || !name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            })
        {
            return Err(ProtocolValidationError::new(
                "provider response format differs from the frozen structured-output shape",
            ));
        }
    }
    Ok(())
}

fn validate_relative_grants(
    read_paths: &[PathBuf],
    write_paths: &[PathBuf],
) -> Result<(), ProtocolValidationError> {
    if read_paths.len() > MAX_ALLOWED_PATHS || write_paths.len() > MAX_ALLOWED_PATHS {
        return Err(ProtocolValidationError::new(
            "tool path grants exceed the protocol bounds",
        ));
    }
    let mut reads = std::collections::BTreeSet::new();
    for path in read_paths {
        validate_relative_path("tool.allowed_read_paths", path)?;
        if !reads.insert(path) {
            return Err(ProtocolValidationError::new(
                "tool allowed_read_paths contains a duplicate",
            ));
        }
    }
    let mut writes = std::collections::BTreeSet::new();
    for path in write_paths {
        validate_relative_path("tool.allowed_write_paths", path)?;
        if !writes.insert(path) || !reads.contains(path) {
            return Err(ProtocolValidationError::new(
                "tool allowed_write_paths must be unique read grants",
            ));
        }
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

fn validate_id(field: &str, value: &str) -> Result<(), ProtocolValidationError> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(ProtocolValidationError::new(format!(
            "{field} must be a bounded printable identifier"
        )));
    }
    Ok(())
}

fn validate_sha256(field: &str, value: &str) -> Result<(), ProtocolValidationError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ProtocolValidationError::new(format!(
            "{field} must be 64 lowercase hexadecimal characters"
        )));
    }
    Ok(())
}

fn validate_path(field: &str, value: &std::path::Path) -> Result<(), ProtocolValidationError> {
    let rendered = value.to_string_lossy();
    if rendered.is_empty()
        || rendered.len() > MAX_PATH_BYTES
        || rendered.chars().any(char::is_control)
    {
        return Err(ProtocolValidationError::new(format!(
            "{field} must be a bounded path without control characters"
        )));
    }
    Ok(())
}

fn validate_relative_path(field: &str, value: &Path) -> Result<(), ProtocolValidationError> {
    validate_path(field, value)?;
    if value.is_absolute()
        || value
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ProtocolValidationError::new(format!(
            "{field} must be a normalized relative path"
        )));
    }
    Ok(())
}

fn roots_overlap(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

fn validate_grants(spec: &ExperimentWorkerSpecV1) -> Result<(), ProtocolValidationError> {
    if spec.allowed_tools.len() > MAX_ALLOWED_TOOLS
        || spec.allowed_read_paths.len() > MAX_ALLOWED_PATHS
        || spec.allowed_write_paths.len() > MAX_ALLOWED_PATHS
    {
        return Err(ProtocolValidationError::new(
            "worker tool or path grants exceed the protocol bounds",
        ));
    }
    let mut tools = std::collections::BTreeSet::new();
    for tool in &spec.allowed_tools {
        validate_id("allowed_tools", tool)?;
        if !tools.insert(tool) {
            return Err(ProtocolValidationError::new(
                "allowed_tools contains a duplicate",
            ));
        }
    }
    if spec.phase == ExperimentPhase::Planner && !spec.allowed_tools.is_empty() {
        return Err(ProtocolValidationError::new(
            "read-only planner may not receive executable tools",
        ));
    }
    if spec.phase != ExperimentPhase::Planner && spec.allowed_tools.is_empty() {
        return Err(ProtocolValidationError::new(
            "executor and recovery phases require an exact nonempty tool grant",
        ));
    }
    let mut reads = std::collections::BTreeSet::new();
    for path in &spec.allowed_read_paths {
        validate_relative_path("allowed_read_paths", path)?;
        if !reads.insert(path) {
            return Err(ProtocolValidationError::new(
                "allowed_read_paths contains a duplicate",
            ));
        }
    }
    let mut writes = std::collections::BTreeSet::new();
    for path in &spec.allowed_write_paths {
        validate_relative_path("allowed_write_paths", path)?;
        if !writes.insert(path) {
            return Err(ProtocolValidationError::new(
                "allowed_write_paths contains a duplicate",
            ));
        }
        if !reads.contains(path) {
            return Err(ProtocolValidationError::new(
                "every allowed write path must also be an allowed read path",
            ));
        }
    }
    if spec.phase == ExperimentPhase::Planner && !writes.is_empty() {
        return Err(ProtocolValidationError::new(
            "planner may not receive write paths",
        ));
    }
    Ok(())
}

fn validate_handoff_for_phase(
    spec: &ExperimentWorkerSpecV1,
) -> Result<(), ProtocolValidationError> {
    let expected_model = match spec.phase {
        ExperimentPhase::Planner | ExperimentPhase::Recovery => STRONG_MODEL,
        ExperimentPhase::Executor => match spec.arm {
            ExperimentArm::S0 | ExperimentArm::Ss => STRONG_MODEL,
            ExperimentArm::C0 | ExperimentArm::Sc | ExperimentArm::Sp => CHEAP_MODEL,
            // The frozen deterministic SC-G predicate may pre-route either way.
            ExperimentArm::ScG => spec.route.model.as_str(),
        },
    };
    if spec.route.model != expected_model {
        return Err(ProtocolValidationError::new(
            "phase route model does not match the frozen arm contract",
        ));
    }
    let valid_handoff = match spec.phase {
        ExperimentPhase::Planner => matches!(spec.handoff, ExperimentHandoffV1::None),
        ExperimentPhase::Executor => match spec.arm {
            ExperimentArm::S0 | ExperimentArm::C0 => {
                matches!(spec.handoff, ExperimentHandoffV1::None)
            }
            ExperimentArm::Ss | ExperimentArm::Sc | ExperimentArm::ScG => {
                matches!(spec.handoff, ExperimentHandoffV1::StructuredArtifact { .. })
            }
            ExperimentArm::Sp => {
                matches!(spec.handoff, ExperimentHandoffV1::SizeMatchedProse { .. })
            }
        },
        ExperimentPhase::Recovery => {
            spec.arm == ExperimentArm::ScG
                && matches!(spec.handoff, ExperimentHandoffV1::StructuredArtifact { .. })
        }
    };
    if !valid_handoff {
        return Err(ProtocolValidationError::new(
            "handoff kind does not match the frozen arm and phase contract",
        ));
    }
    Ok(())
}

fn validate_atto_usd(field: &str, value: &str) -> Result<(), ProtocolValidationError> {
    if value.is_empty()
        || value.len() > 32
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || value.parse::<u128>().is_err()
    {
        return Err(ProtocolValidationError::new(format!(
            "{field} must be a bounded unsigned atto-USD integer"
        )));
    }
    Ok(())
}

fn validate_reason(value: &str) -> Result<(), ProtocolValidationError> {
    if value.is_empty() || value.len() > MAX_REASON_BYTES || value.chars().any(char::is_control) {
        return Err(ProtocolValidationError::new(
            "worker failure reason must be bounded printable text",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest() -> String {
        "a".repeat(64)
    }

    fn spec() -> ExperimentWorkerSpecV1 {
        ExperimentWorkerSpecV1 {
            schema_version: WORKER_SPEC_SCHEMA_V1.into(),
            experiment_id: "experiment-1".into(),
            preregistration_sha256: digest(),
            trial_id: "trial-1".into(),
            slot_index: 0,
            fixture_id: "unicode_parser_boundary".into(),
            arm: ExperimentArm::Sc,
            repetition: 1,
            phase: ExperimentPhase::Executor,
            route: ExperimentModelRouteV1::openai(CHEAP_MODEL),
            limits: ExperimentRequestLimitsV1::for_phase(ExperimentPhase::Executor),
            workspace_id: "workspace-1".into(),
            workspace_root: PathBuf::from("/private/tmp/workspace"),
            controller_root: PathBuf::from("/private/tmp/controller"),
            source_manifest_sha256: digest(),
            destination_manifest_sha256: digest(),
            cargo_target_dir: PathBuf::from("/private/tmp/cargo-target"),
            task_input_sha256: digest(),
            task_input: ExperimentDeclaredFileV1 {
                root: ExperimentFileRoot::Workspace,
                relative_path: PathBuf::from("task.md"),
                sha256: digest(),
                max_bytes: 64 * 1024,
            },
            handoff: ExperimentHandoffV1::StructuredArtifact {
                file: ExperimentDeclaredFileV1 {
                    root: ExperimentFileRoot::Controller,
                    relative_path: PathBuf::from("handoffs/trial-1.json"),
                    sha256: digest(),
                    max_bytes: 1024 * 1024,
                },
            },
            output_schema: ExperimentDeclaredFileV1 {
                root: ExperimentFileRoot::Controller,
                relative_path: PathBuf::from("schemas/executor.json"),
                sha256: digest(),
                max_bytes: 1024 * 1024,
            },
            allowed_tools: vec!["read_file".into(), "apply_patch".into()],
            allowed_read_paths: vec![PathBuf::from("src/lib.rs")],
            allowed_write_paths: vec![PathBuf::from("src/lib.rs")],
            controller_channel: ExperimentControllerChannelV1 {
                channel_id: "channel-1".into(),
                nonce_sha256: digest(),
                inherited_fd: 9,
            },
            network_allowed: false,
        }
    }

    #[test]
    fn worker_spec_is_exact_bounded_and_redacts_paths() {
        let spec = spec();
        spec.validate().unwrap();
        let debug = format!("{spec:?}");
        assert!(!debug.contains("/private/tmp/workspace"));
        assert!(!debug.contains("/private/tmp/cargo-target"));
        assert!(!debug.contains("/private/tmp/controller"));

        let mut invalid = spec.clone();
        invalid.route.reasoning_effort = "medium".into();
        assert!(invalid.validate().is_err());
        let mut invalid = spec;
        invalid.cargo_target_dir = invalid.workspace_root.join("target");
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn phase_limits_are_the_frozen_paid_contract() {
        let planner = ExperimentRequestLimitsV1::for_phase(ExperimentPhase::Planner);
        assert_eq!(planner.max_semantic_requests, 1);
        assert_eq!(planner.max_physical_posts, 2);
        let executor = ExperimentRequestLimitsV1::for_phase(ExperimentPhase::Executor);
        assert_eq!(executor.max_semantic_requests, 12);
        assert_eq!(executor.max_physical_posts, 24);
        assert_eq!(executor.max_tool_calls, 40);
        let recovery = ExperimentRequestLimitsV1::for_phase(ExperimentPhase::Recovery);
        assert_eq!(recovery.max_semantic_requests, 8);
        assert_eq!(recovery.max_physical_posts, 16);
        assert_eq!(recovery.max_tool_calls, 24);
        assert_eq!(executor.max_input_tokens, 64_000);
        assert_eq!(executor.max_output_tokens, 8_192);
    }

    #[test]
    fn receipt_refuses_returned_model_or_service_tier_drift() {
        let spec = spec();
        let mut provider = ExperimentProviderReceiptV1 {
            reservation_id: "reservation-1".into(),
            semantic_request_index: 0,
            physical_attempt: 1,
            x_client_request_id: "client-1".into(),
            x_request_id: "request-1".into(),
            requested_model: CHEAP_MODEL.into(),
            returned_model: CHEAP_MODEL.into(),
            returned_service_tier: SERVICE_TIER.into(),
            usage: ExperimentUsageV1::default(),
        };
        provider.validate(&spec.route, &spec.limits).unwrap();
        provider.returned_model = STRONG_MODEL.into();
        assert!(provider.validate(&spec.route, &spec.limits).is_err());
        provider.returned_model = CHEAP_MODEL.into();
        provider.returned_service_tier = "priority".into();
        assert!(provider.validate(&spec.route, &spec.limits).is_err());
    }

    #[test]
    fn usage_categories_are_disjoint_and_bounded() {
        let valid = ExperimentUsageV1 {
            regular_input_tokens: 30_000,
            cached_input_tokens: 20_000,
            cache_write_tokens: 14_000,
            regular_output_tokens: 4_096,
            reasoning_output_tokens: 4_096,
        };
        valid.validate().unwrap();
        let mut invalid = valid;
        invalid.cache_write_tokens += 1;
        assert!(invalid.validate().is_err());
    }

    fn provider_call() -> ProviderCallSpecV1 {
        let request_json = format!(
            r#"{{"model":"{CHEAP_MODEL}","input":[{{"role":"user","content":"fixture"}}],"stream":true,"reasoning":{{"effort":"high"}},"include":["reasoning.encrypted_content"],"max_output_tokens":8192,"store":false,"service_tier":"default"}}"#
        );
        ProviderCallSpecV1 {
            schema_version: PROVIDER_CALL_SPEC_SCHEMA_V1.into(),
            call_id: "provider-call-1".into(),
            channel_id: "channel-1".into(),
            nonce: "nonce-alpha".into(),
            worker_spec_sha256: digest(),
            controller_sha256: digest(),
            worker_sha256: digest(),
            experiment_id: "experiment-1".into(),
            preregistration_sha256: digest(),
            trial_id: "trial-1".into(),
            slot_index: 0,
            phase: ExperimentPhase::Executor,
            semantic_request_index: 0,
            physical_attempt: 1,
            route: ExperimentModelRouteV1::openai(CHEAP_MODEL),
            max_input_tokens: MAX_INPUT_TOKENS_PER_REQUEST,
            max_output_tokens: MAX_OUTPUT_TOKENS_PER_REQUEST,
            store: false,
            request_sha256: sha256_hex(request_json.as_bytes()),
            request_json,
            x_client_request_id: "rnd-097-client-1".into(),
        }
    }

    fn provider_receipt(spec: &ProviderCallSpecV1) -> ProviderCallReceiptV1 {
        let response = r#"{"assistant_text":"fixture response"}"#.to_string();
        ProviderCallReceiptV1 {
            schema_version: PROVIDER_CALL_RECEIPT_SCHEMA_V1.into(),
            call_id: spec.call_id.clone(),
            channel_id: spec.channel_id.clone(),
            nonce: spec.nonce.clone(),
            worker_spec_sha256: spec.worker_spec_sha256.clone(),
            request_sha256: spec.request_sha256.clone(),
            reservation_id: "reservation-1".into(),
            x_client_request_id_sha256: sha256_hex(spec.x_client_request_id.as_bytes()),
            x_request_id_sha256: Some(digest()),
            returned_model: Some(spec.route.model.clone()),
            returned_service_tier: Some(SERVICE_TIER.into()),
            usage: Some(ExperimentUsageV1::default()),
            elapsed_millis: 5,
            outcome: ProviderCallOutcomeV1::Completed {
                controller_response_sha256: sha256_hex(response.as_bytes()),
                controller_response_json: response,
            },
        }
    }

    fn tool_call() -> ToolCallSpecV1 {
        let arguments_json = r#"{"path":"src/lib.rs"}"#.to_string();
        ToolCallSpecV1 {
            schema_version: TOOL_CALL_SPEC_SCHEMA_V1.into(),
            call_id: "tool-call-1".into(),
            channel_id: "channel-1".into(),
            nonce: "nonce-alpha".into(),
            worker_spec_sha256: digest(),
            workspace_id: "workspace-1".into(),
            workspace_root: PathBuf::from("/private/tmp/workspace"),
            source_manifest_sha256: digest(),
            cargo_target_dir: PathBuf::from("/private/tmp/cargo-target"),
            tool_name: "read_file".into(),
            tool_call_id: "model-tool-1".into(),
            arguments_sha256: sha256_hex(arguments_json.as_bytes()),
            arguments_json,
            allowed_read_paths: vec![PathBuf::from("src/lib.rs")],
            allowed_write_paths: Vec::new(),
            timeout_millis: 5_000,
        }
    }

    #[test]
    fn provider_call_frames_are_bounded_nonce_bound_and_redacted() {
        let spec = provider_call();
        spec.validate().unwrap();
        let debug = format!("{spec:?}");
        assert!(!debug.contains("fixture"));
        assert!(!debug.contains("nonce-alpha"));
        assert!(!debug.contains("rnd-097-client-1"));

        let frame = encode_ipc_frame(&spec, MAX_PROVIDER_CALL_SPEC_FRAME_BYTES).unwrap();
        let decoded: ProviderCallSpecV1 =
            decode_ipc_frame(&frame, MAX_PROVIDER_CALL_SPEC_FRAME_BYTES).unwrap();
        assert_eq!(decoded, spec);

        let mut nonce_tampered = frame;
        let at = nonce_tampered
            .windows(b"nonce-alpha".len())
            .position(|window| window == b"nonce-alpha")
            .unwrap();
        nonce_tampered[at..at + b"nonce-alpha".len()].copy_from_slice(b"nonce-omega");
        assert!(decode_ipc_frame::<ProviderCallSpecV1>(
            &nonce_tampered,
            MAX_PROVIDER_CALL_SPEC_FRAME_BYTES
        )
        .is_err());
    }

    #[test]
    fn provider_call_refuses_drift_and_receipt_must_match_exact_call() {
        let spec = provider_call();
        let mut invalid = spec.clone();
        invalid.request_json = invalid
            .request_json
            .replace("\"store\":false", "\"store\":true");
        invalid.request_sha256 = sha256_hex(invalid.request_json.as_bytes());
        assert!(invalid.validate().is_err());

        let mut missing_replay = spec.clone();
        missing_replay.request_json = missing_replay
            .request_json
            .replace(r#","include":["reasoning.encrypted_content"]"#, "");
        missing_replay.request_sha256 = sha256_hex(missing_replay.request_json.as_bytes());
        assert!(missing_replay.validate().is_err());

        let mut cache_warming = spec.clone();
        cache_warming.request_json = cache_warming.request_json.replace(
            r#""service_tier":"default""#,
            r#""service_tier":"default","prompt_cache_key":"not-allowed""#,
        );
        cache_warming.request_sha256 = sha256_hex(cache_warming.request_json.as_bytes());
        assert!(cache_warming.validate().is_err());

        let mut extra_field = spec.clone();
        extra_field.request_json = extra_field.request_json.replace(
            r#""service_tier":"default""#,
            r#""service_tier":"default","metadata":{"leak":"not-allowed"}"#,
        );
        extra_field.request_sha256 = sha256_hex(extra_field.request_json.as_bytes());
        assert!(extra_field.validate().is_err());

        let mut structured = spec.clone();
        structured.request_json = structured.request_json.replace(
            r#""service_tier":"default""#,
            r#""service_tier":"default","text":{"format":{"type":"json_schema","name":"planner_proposal","strict":true,"schema":{"type":"object","properties":{"objective":{"type":"string"}},"required":["objective"],"additionalProperties":false}}}"#,
        );
        structured.request_sha256 = sha256_hex(structured.request_json.as_bytes());
        structured.validate().unwrap();

        let receipt = provider_receipt(&spec);
        receipt.validate_against(&spec).unwrap();
        let debug = format!("{receipt:?}");
        assert!(!debug.contains("fixture response"));
        assert!(!debug.contains("nonce-alpha"));
        let mut mismatched = receipt;
        mismatched.returned_model = Some(STRONG_MODEL.into());
        assert!(mismatched.validate_against(&spec).is_err());
    }

    #[test]
    fn tool_frames_have_no_provider_authority_and_reject_tampering() {
        let spec = tool_call();
        spec.validate().unwrap();
        let serialized = serde_json::to_value(&spec).unwrap();
        let object = serialized.as_object().unwrap();
        assert!(!object.contains_key("route"));
        assert!(!object.contains_key("provider"));
        assert!(!object.contains_key("request_json"));
        assert!(!object.contains_key("network_allowed"));

        let frame = encode_ipc_frame(&spec, MAX_TOOL_CALL_SPEC_FRAME_BYTES).unwrap();
        let decoded: ToolCallSpecV1 =
            decode_ipc_frame(&frame, MAX_TOOL_CALL_SPEC_FRAME_BYTES).unwrap();
        assert_eq!(decoded, spec);

        let mut value = serde_json::to_value(spec).unwrap();
        value.as_object_mut().unwrap().insert(
            "provider".into(),
            serde_json::Value::String("openai".into()),
        );
        assert!(serde_json::from_value::<ToolCallSpecV1>(value).is_err());
    }

    #[test]
    fn tool_receipts_and_frame_lengths_fail_closed() {
        let spec = tool_call();
        let result = r#"{"ok":true}"#.to_string();
        let receipt = ToolCallReceiptV1 {
            schema_version: TOOL_CALL_RECEIPT_SCHEMA_V1.into(),
            call_id: spec.call_id.clone(),
            channel_id: spec.channel_id.clone(),
            nonce: spec.nonce.clone(),
            worker_spec_sha256: spec.worker_spec_sha256.clone(),
            arguments_sha256: spec.arguments_sha256.clone(),
            elapsed_millis: 10,
            outcome: ToolCallOutcomeV1::Completed,
            result_sha256: Some(sha256_hex(result.as_bytes())),
            result_json: Some(result),
        };
        receipt.validate_against(&spec).unwrap();
        assert!(!format!("{receipt:?}").contains("\"ok\":true"));

        let mut frame = encode_ipc_frame(&spec, MAX_TOOL_CALL_SPEC_FRAME_BYTES).unwrap();
        frame[..IPC_LENGTH_PREFIX_BYTES].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(
            decode_ipc_frame::<ToolCallSpecV1>(&frame, MAX_TOOL_CALL_SPEC_FRAME_BYTES).is_err()
        );
    }
}
