//! Durable, body-free receipts for one RND-097 provider call.
//!
//! The broker returns a bounded controller response over its private IPC
//! channel, but that response may never enter a durable trial record.  This
//! module is the controller's persistence boundary: it validates the full
//! receipt while it is in memory and atomically retains only the metadata that
//! is needed for audit, cost reporting, and crash classification.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::reasoning_experiment_protocol::{
    ExperimentUsageV1, ProviderCallOutcomeV1, ProviderCallReceiptV1, ProviderCallSpecV1,
    ToolCallOutcomeV1, ToolCallReceiptV1, ToolCallSpecV1,
};

const RECEIPT_STORE_SCHEMA_V1: &str = "ca.rnd097.provider_receipt.v1";
const RECEIPT_DIRECTORY: &str = "provider-receipts-v1";
const MAX_RECEIPT_BYTES: usize = 32 * 1024;
const RECEIPT_HASH_DOMAIN: &[u8] = b"ca-rnd097-provider-receipt-v1\0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StoredProviderOutcomeV1 {
    Completed { controller_response_sha256: String },
    VerifiedPreOutputFailure { reason: String },
    AmbiguousDelivery { reason: String },
}

/// The sole durable form of a broker result.  It deliberately excludes the
/// raw client request ID, request bytes, assistant text/tool payload, response
/// body, endpoint, credentials, and opaque reasoning state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StoredProviderReceiptV1 {
    pub schema_version: String,
    pub preregistration_sha256: String,
    pub trial_id: String,
    pub slot_index: u16,
    pub phase: String,
    pub semantic_request_index: u16,
    pub physical_attempt: u8,
    pub call_id: String,
    pub request_sha256: String,
    pub reservation_id: String,
    pub x_client_request_id_sha256: String,
    pub x_request_id_sha256: Option<String>,
    pub returned_model: Option<String>,
    pub returned_service_tier: Option<String>,
    pub usage: Option<ExperimentUsageV1>,
    pub elapsed_millis: u64,
    pub outcome: StoredProviderOutcomeV1,
    pub recorded_at: String,
}

/// Body-free projection of one tool receipt.  Tool arguments and stdout/stderr
/// can contain private repository content, so only their controller-validated
/// digests are durable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StoredToolOutcomeV1 {
    Completed { result_sha256: String },
    TimedOut,
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StoredToolReceiptV1 {
    pub schema_version: String,
    pub preregistration_sha256: String,
    pub trial_id: String,
    pub slot_index: u16,
    pub call_id: String,
    pub tool_name: String,
    pub tool_call_id_sha256: String,
    pub worker_spec_sha256: String,
    pub arguments_sha256: String,
    pub elapsed_millis: u64,
    pub outcome: StoredToolOutcomeV1,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredReceiptHandleV1 {
    pub sha256: String,
    pub path: PathBuf,
}

pub struct ProviderReceiptStore {
    root: PathBuf,
    preregistration_sha256: String,
}

impl std::fmt::Debug for ProviderReceiptStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderReceiptStore")
            .field("root", &"[PRIVATE]")
            .field("preregistration_sha256", &self.preregistration_sha256)
            .finish()
    }
}

impl ProviderReceiptStore {
    /// Opens or creates the owner-only receipt directory under a validated
    /// private experiment directory.  Reporting never calls this constructor;
    /// it is for the future controller only after it has durably started a
    /// trial.
    pub fn create(experiment_dir: &Path, preregistration_sha256: &str) -> Result<Self> {
        require_unix()?;
        validate_sha256(preregistration_sha256)?;
        let experiment_dir = experiment_dir
            .canonicalize()
            .context("cannot canonicalize experiment directory")?;
        require_owner_directory(&experiment_dir)?;
        let root = experiment_dir.join(RECEIPT_DIRECTORY);
        match fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                bail!("provider receipt directory is invalid")
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&root)?;
                #[cfg(unix)]
                fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
            }
            Err(error) => return Err(error.into()),
        }
        let root = root.canonicalize()?;
        require_owner_directory(&root)?;
        Ok(Self {
            root,
            preregistration_sha256: preregistration_sha256.into(),
        })
    }

    /// Validates the full one-call broker receipt, then atomically records its
    /// sanitized projection.  A duplicate call ID is always refused rather
    /// than being interpreted as permission to retry or overwrite evidence.
    pub fn record_provider_receipt(
        &self,
        spec: &ProviderCallSpecV1,
        receipt: &ProviderCallReceiptV1,
    ) -> Result<StoredReceiptHandleV1> {
        spec.validate()
            .map_err(|error| anyhow!("provider call spec is invalid: {error}"))?;
        receipt
            .validate_against(spec)
            .map_err(|error| anyhow!("provider receipt is invalid: {error}"))?;
        if spec.preregistration_sha256 != self.preregistration_sha256 {
            bail!("provider call is not bound to this preregistration");
        }
        let stored = StoredProviderReceiptV1::from_broker(spec, receipt)?;
        let bytes = serde_json::to_vec(&stored)?;
        if bytes.len() > MAX_RECEIPT_BYTES {
            bail!("sanitized provider receipt exceeds the bound");
        }
        let path = self
            .root
            .join(format!("{:03}-{}.json", spec.slot_index, spec.call_id));
        write_immutable_file(&self.root, &path, &bytes)?;
        Ok(StoredReceiptHandleV1 {
            sha256: receipt_sha256(&bytes),
            path,
        })
    }

    pub fn load(&self, slot_index: u16, call_id: &str) -> Result<StoredProviderReceiptV1> {
        if slot_index >= 72 || !valid_id(call_id) {
            bail!("provider receipt locator is invalid");
        }
        let path = self.root.join(format!("{slot_index:03}-{call_id}.json"));
        let mut file = open_owner_regular_file(&path)?;
        let metadata = file.metadata()?;
        if metadata.len() > MAX_RECEIPT_BYTES as u64 {
            bail!("provider receipt exceeds the bound");
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.read_to_end(&mut bytes)?;
        let receipt: StoredProviderReceiptV1 =
            serde_json::from_slice(&bytes).context("provider receipt is not strict JSON")?;
        receipt.validate(&self.preregistration_sha256)?;
        Ok(receipt)
    }

    /// Records a tool result using the same immutable, body-free policy as a
    /// broker receipt.  The controller supplies the trial identity because an
    /// exact tool payload intentionally carries no provider/pilot authority.
    pub fn record_tool_receipt(
        &self,
        trial_id: &str,
        slot_index: u16,
        spec: &ToolCallSpecV1,
        receipt: &ToolCallReceiptV1,
    ) -> Result<StoredReceiptHandleV1> {
        if !valid_id(trial_id) || slot_index >= 72 {
            bail!("tool receipt trial binding is invalid");
        }
        spec.validate()
            .map_err(|error| anyhow!("tool call spec is invalid: {error}"))?;
        receipt
            .validate_against(spec)
            .map_err(|error| anyhow!("tool receipt is invalid: {error}"))?;
        let stored = StoredToolReceiptV1::from_worker(
            &self.preregistration_sha256,
            trial_id,
            slot_index,
            spec,
            receipt,
        )?;
        let bytes = serde_json::to_vec(&stored)?;
        if bytes.len() > MAX_RECEIPT_BYTES {
            bail!("sanitized tool receipt exceeds the bound");
        }
        let path = self
            .root
            .join(format!("{slot_index:03}-tool-{}.json", spec.call_id));
        write_immutable_file(&self.root, &path, &bytes)?;
        Ok(StoredReceiptHandleV1 {
            sha256: receipt_sha256(&bytes),
            path,
        })
    }

    pub fn load_tool(&self, slot_index: u16, call_id: &str) -> Result<StoredToolReceiptV1> {
        if slot_index >= 72 || !valid_id(call_id) {
            bail!("tool receipt locator is invalid");
        }
        let path = self
            .root
            .join(format!("{slot_index:03}-tool-{call_id}.json"));
        let mut file = open_owner_regular_file(&path)?;
        let metadata = file.metadata()?;
        if metadata.len() > MAX_RECEIPT_BYTES as u64 {
            bail!("tool receipt exceeds the bound");
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.read_to_end(&mut bytes)?;
        let receipt: StoredToolReceiptV1 =
            serde_json::from_slice(&bytes).context("tool receipt is not strict JSON")?;
        receipt.validate(&self.preregistration_sha256)?;
        Ok(receipt)
    }
}

impl StoredProviderReceiptV1 {
    fn from_broker(spec: &ProviderCallSpecV1, receipt: &ProviderCallReceiptV1) -> Result<Self> {
        let outcome = match &receipt.outcome {
            ProviderCallOutcomeV1::Completed {
                controller_response_sha256,
                ..
            } => StoredProviderOutcomeV1::Completed {
                controller_response_sha256: controller_response_sha256.clone(),
            },
            ProviderCallOutcomeV1::VerifiedPreOutputFailure { reason } => {
                StoredProviderOutcomeV1::VerifiedPreOutputFailure {
                    reason: reason.clone(),
                }
            }
            ProviderCallOutcomeV1::AmbiguousDelivery { reason } => {
                StoredProviderOutcomeV1::AmbiguousDelivery {
                    reason: reason.clone(),
                }
            }
        };
        let stored = Self {
            schema_version: RECEIPT_STORE_SCHEMA_V1.into(),
            preregistration_sha256: spec.preregistration_sha256.clone(),
            trial_id: spec.trial_id.clone(),
            slot_index: spec.slot_index,
            phase: serde_json::to_string(&spec.phase)?.trim_matches('"').into(),
            semantic_request_index: spec.semantic_request_index,
            physical_attempt: spec.physical_attempt,
            call_id: spec.call_id.clone(),
            request_sha256: receipt.request_sha256.clone(),
            reservation_id: receipt.reservation_id.clone(),
            x_client_request_id_sha256: receipt.x_client_request_id_sha256.clone(),
            x_request_id_sha256: receipt.x_request_id_sha256.clone(),
            returned_model: receipt.returned_model.clone(),
            returned_service_tier: receipt.returned_service_tier.clone(),
            usage: receipt.usage,
            elapsed_millis: receipt.elapsed_millis,
            outcome,
            recorded_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        };
        stored.validate(&spec.preregistration_sha256)?;
        Ok(stored)
    }

    fn validate(&self, expected_preregistration_sha256: &str) -> Result<()> {
        if self.schema_version != RECEIPT_STORE_SCHEMA_V1
            || self.preregistration_sha256 != expected_preregistration_sha256
            || self.slot_index >= 72
            || !valid_id(&self.trial_id)
            || !valid_id(&self.call_id)
            || !valid_id(&self.reservation_id)
            || !matches!(self.phase.as_str(), "planner" | "executor" | "recovery")
            || chrono::DateTime::parse_from_rfc3339(&self.recorded_at).is_err()
        {
            bail!("stored provider receipt is invalid");
        }
        for value in [
            &self.preregistration_sha256,
            &self.request_sha256,
            &self.x_client_request_id_sha256,
        ] {
            validate_sha256(value)?;
        }
        if let Some(value) = &self.x_request_id_sha256 {
            validate_sha256(value)?;
        }
        if let Some(usage) = self.usage {
            usage
                .validate()
                .map_err(|error| anyhow!("stored usage is invalid: {error}"))?;
        }
        match &self.outcome {
            StoredProviderOutcomeV1::Completed {
                controller_response_sha256,
            } => {
                validate_sha256(controller_response_sha256)?;
                if self.x_request_id_sha256.is_none()
                    || self.returned_model.is_none()
                    || self.returned_service_tier.is_none()
                    || self.usage.is_none()
                {
                    bail!("completed provider receipt lacks required metadata");
                }
            }
            StoredProviderOutcomeV1::VerifiedPreOutputFailure { reason }
            | StoredProviderOutcomeV1::AmbiguousDelivery { reason } => {
                if !valid_reason(reason)
                    || self.x_request_id_sha256.is_some()
                    || self.returned_model.is_some()
                    || self.returned_service_tier.is_some()
                    || self.usage.is_some()
                {
                    bail!("noncompleted provider receipt has invalid metadata");
                }
            }
        }
        Ok(())
    }
}

impl StoredToolReceiptV1 {
    fn from_worker(
        preregistration_sha256: &str,
        trial_id: &str,
        slot_index: u16,
        spec: &ToolCallSpecV1,
        receipt: &ToolCallReceiptV1,
    ) -> Result<Self> {
        let outcome = match &receipt.outcome {
            ToolCallOutcomeV1::Completed => StoredToolOutcomeV1::Completed {
                result_sha256: receipt
                    .result_sha256
                    .clone()
                    .ok_or_else(|| anyhow!("completed tool receipt lacks result digest"))?,
            },
            ToolCallOutcomeV1::TimedOut => StoredToolOutcomeV1::TimedOut,
            ToolCallOutcomeV1::Failed { reason } => StoredToolOutcomeV1::Failed {
                reason: reason.clone(),
            },
        };
        let stored = Self {
            schema_version: RECEIPT_STORE_SCHEMA_V1.into(),
            preregistration_sha256: preregistration_sha256.into(),
            trial_id: trial_id.into(),
            slot_index,
            call_id: spec.call_id.clone(),
            tool_name: spec.tool_name.clone(),
            tool_call_id_sha256: raw_sha256(spec.tool_call_id.as_bytes()),
            worker_spec_sha256: spec.worker_spec_sha256.clone(),
            arguments_sha256: spec.arguments_sha256.clone(),
            elapsed_millis: receipt.elapsed_millis,
            outcome,
            recorded_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        };
        stored.validate(preregistration_sha256)?;
        Ok(stored)
    }

    fn validate(&self, expected_preregistration_sha256: &str) -> Result<()> {
        if self.schema_version != RECEIPT_STORE_SCHEMA_V1
            || self.preregistration_sha256 != expected_preregistration_sha256
            || self.slot_index >= 72
            || !valid_id(&self.trial_id)
            || !valid_id(&self.call_id)
            || !matches!(
                self.tool_name.as_str(),
                "read_file" | "write_file" | "run_command"
            )
            || chrono::DateTime::parse_from_rfc3339(&self.recorded_at).is_err()
        {
            bail!("stored tool receipt is invalid");
        }
        for value in [
            &self.preregistration_sha256,
            &self.tool_call_id_sha256,
            &self.worker_spec_sha256,
            &self.arguments_sha256,
        ] {
            validate_sha256(value)?;
        }
        match &self.outcome {
            StoredToolOutcomeV1::Completed { result_sha256 } => validate_sha256(result_sha256)?,
            StoredToolOutcomeV1::TimedOut => {}
            StoredToolOutcomeV1::Failed { reason } if valid_reason(reason) => {}
            StoredToolOutcomeV1::Failed { .. } => bail!("stored tool failure reason is invalid"),
        }
        Ok(())
    }
}

fn write_immutable_file(root: &Path, path: &Path, bytes: &[u8]) -> Result<()> {
    if path.parent() != Some(root) || path.exists() {
        bail!("provider receipt already exists or has an invalid parent");
    }
    let temporary = root.join(format!(".receipt-{}.tmp", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options.open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = fs::hard_link(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(anyhow!("cannot publish provider receipt: {error}"));
    }
    fs::remove_file(temporary)?;
    File::open(root)?.sync_all()?;
    Ok(())
}

fn open_owner_regular_file(path: &Path) -> Result<File> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("provider receipt file is invalid");
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        bail!("provider receipt is not owner-only");
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    Ok(options.open(path)?)
}

fn require_owner_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("provider receipt directory is invalid");
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        bail!("provider receipt directory is not owner-only");
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn valid_reason(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("SHA-256 value is invalid");
    }
    Ok(())
}

fn receipt_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(RECEIPT_HASH_DOMAIN);
    hasher.update(bytes);
    encode_hex(&hasher.finalize())
}

fn raw_sha256(bytes: &[u8]) -> String {
    encode_hex(&Sha256::digest(bytes))
}

fn encode_hex(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(TABLE[(byte >> 4) as usize] as char);
        output.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    output
}

fn require_unix() -> Result<()> {
    #[cfg(not(unix))]
    bail!("RND-097 provider receipts require Unix owner-only files");
    #[cfg(unix)]
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning_experiment_protocol::{
        ExperimentModelRouteV1, ExperimentPhase, ProviderCallOutcomeV1, CHEAP_MODEL,
        MAX_INPUT_TOKENS_PER_REQUEST, MAX_OUTPUT_TOKENS_PER_REQUEST,
        PROVIDER_CALL_RECEIPT_SCHEMA_V1, PROVIDER_CALL_SPEC_SCHEMA_V1, TOOL_CALL_RECEIPT_SCHEMA_V1,
        TOOL_CALL_SPEC_SCHEMA_V1,
    };

    fn digest(byte: char) -> String {
        byte.to_string().repeat(64)
    }

    fn call() -> ProviderCallSpecV1 {
        let request_json = format!(
            r#"{{"model":"{CHEAP_MODEL}","input":[{{"role":"user","content":"private request body"}}],"stream":true,"reasoning":{{"effort":"high"}},"include":["reasoning.encrypted_content"],"max_output_tokens":8192,"store":false,"service_tier":"default"}}"#
        );
        ProviderCallSpecV1 {
            schema_version: PROVIDER_CALL_SPEC_SCHEMA_V1.into(),
            call_id: "provider-call-1".into(),
            channel_id: "channel-1".into(),
            nonce: "nonce-1".into(),
            worker_spec_sha256: digest('a'),
            controller_sha256: digest('b'),
            worker_sha256: digest('c'),
            experiment_id: "pilot-1".into(),
            preregistration_sha256: digest('d'),
            trial_id: "trial-1".into(),
            slot_index: 0,
            phase: ExperimentPhase::Executor,
            semantic_request_index: 0,
            physical_attempt: 1,
            route: ExperimentModelRouteV1::openai(CHEAP_MODEL),
            max_input_tokens: MAX_INPUT_TOKENS_PER_REQUEST,
            max_output_tokens: MAX_OUTPUT_TOKENS_PER_REQUEST,
            store: false,
            request_sha256: raw_sha256(&request_json),
            request_json,
            x_client_request_id: "private-client-request-id".into(),
        }
    }

    fn receipt(call: &ProviderCallSpecV1) -> ProviderCallReceiptV1 {
        let response = r#"{"text":"private assistant response"}"#;
        ProviderCallReceiptV1 {
            schema_version: PROVIDER_CALL_RECEIPT_SCHEMA_V1.into(),
            call_id: call.call_id.clone(),
            channel_id: call.channel_id.clone(),
            nonce: call.nonce.clone(),
            worker_spec_sha256: call.worker_spec_sha256.clone(),
            request_sha256: call.request_sha256.clone(),
            reservation_id: "reservation-1".into(),
            x_client_request_id_sha256: raw_sha256(&call.x_client_request_id),
            x_request_id_sha256: Some(digest('e')),
            returned_model: Some(CHEAP_MODEL.into()),
            returned_service_tier: Some("default".into()),
            usage: Some(ExperimentUsageV1 {
                regular_input_tokens: 10,
                cached_input_tokens: 0,
                cache_write_tokens: 0,
                regular_output_tokens: 2,
                reasoning_output_tokens: 0,
            }),
            elapsed_millis: 1,
            outcome: ProviderCallOutcomeV1::Completed {
                controller_response_sha256: raw_sha256(response),
                controller_response_json: response.into(),
            },
        }
    }

    fn raw_sha256(value: &str) -> String {
        encode_hex(&Sha256::digest(value.as_bytes()))
    }

    fn private_experiment() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn tool_call(workspace: &Path, target: &Path) -> ToolCallSpecV1 {
        let arguments_json = r#"{"executable":"/usr/bin/env","arguments":["private argument"]}"#;
        ToolCallSpecV1 {
            schema_version: TOOL_CALL_SPEC_SCHEMA_V1.into(),
            call_id: "tool-call-1".into(),
            channel_id: "tool-channel-1".into(),
            nonce: "tool-nonce-1".into(),
            worker_spec_sha256: digest('a'),
            workspace_id: "workspace-1".into(),
            workspace_root: workspace.to_path_buf(),
            source_manifest_sha256: digest('b'),
            cargo_target_dir: target.to_path_buf(),
            tool_name: "run_command".into(),
            tool_call_id: "private-tool-call-id".into(),
            arguments_json: arguments_json.into(),
            arguments_sha256: raw_sha256(arguments_json),
            allowed_read_paths: vec![PathBuf::from("src")],
            allowed_write_paths: vec![],
            timeout_millis: 5_000,
        }
    }

    fn tool_receipt(call: &ToolCallSpecV1) -> ToolCallReceiptV1 {
        let output = r#"{"stdout":"private tool output"}"#;
        ToolCallReceiptV1 {
            schema_version: TOOL_CALL_RECEIPT_SCHEMA_V1.into(),
            call_id: call.call_id.clone(),
            channel_id: call.channel_id.clone(),
            nonce: call.nonce.clone(),
            worker_spec_sha256: call.worker_spec_sha256.clone(),
            arguments_sha256: call.arguments_sha256.clone(),
            elapsed_millis: 1,
            outcome: ToolCallOutcomeV1::Completed,
            result_sha256: Some(raw_sha256(output)),
            result_json: Some(output.into()),
        }
    }

    #[test]
    fn record_is_body_free_immutable_and_bound_to_one_call() {
        let outer = private_experiment();
        let root = outer.path().canonicalize().unwrap().join("experiment");
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let call = call();
        let store = ProviderReceiptStore::create(&root, &call.preregistration_sha256).unwrap();
        let handle = store
            .record_provider_receipt(&call, &receipt(&call))
            .unwrap();
        assert_eq!(handle.sha256.len(), 64);
        let raw = fs::read_to_string(&handle.path).unwrap();
        assert!(!raw.contains("private request body"));
        assert!(!raw.contains("private assistant response"));
        assert!(!raw.contains("private-client-request-id"));
        let stored = store.load(0, &call.call_id).unwrap();
        assert_eq!(stored.call_id, call.call_id);
        assert!(matches!(
            stored.outcome,
            StoredProviderOutcomeV1::Completed { .. }
        ));
        assert!(store
            .record_provider_receipt(&call, &receipt(&call))
            .is_err());
    }

    #[test]
    fn invalid_broker_binding_is_never_persisted() {
        let outer = private_experiment();
        let root = outer.path().canonicalize().unwrap().join("experiment");
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let call = call();
        let store = ProviderReceiptStore::create(&root, &call.preregistration_sha256).unwrap();
        let mut receipt = receipt(&call);
        receipt.returned_model = Some("unexpected".into());
        assert!(store.record_provider_receipt(&call, &receipt).is_err());
        assert!(fs::read_dir(root.join(RECEIPT_DIRECTORY))
            .unwrap()
            .next()
            .is_none());
    }

    #[test]
    fn tool_projection_never_retains_arguments_or_output() {
        let outer = private_experiment();
        let root = outer.path().canonicalize().unwrap().join("experiment");
        let workspace = outer.path().join("workspace");
        let target = outer.path().join("target");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&target).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let target = target.canonicalize().unwrap();
        let provider_call = call();
        let tool_call = tool_call(&workspace, &target);
        let store =
            ProviderReceiptStore::create(&root, &provider_call.preregistration_sha256).unwrap();
        let handle = store
            .record_tool_receipt("trial-1", 0, &tool_call, &tool_receipt(&tool_call))
            .unwrap();
        let raw = fs::read_to_string(&handle.path).unwrap();
        assert!(!raw.contains("private argument"));
        assert!(!raw.contains("private tool output"));
        assert!(!raw.contains("private-tool-call-id"));
        assert!(matches!(
            store.load_tool(0, &tool_call.call_id).unwrap().outcome,
            StoredToolOutcomeV1::Completed { .. }
        ));
    }
}
