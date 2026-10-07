use super::{
    canonical_digest, ArtifactContentV1, ArtifactEncodingV1, ArtifactRevisionV1, LearningVault,
};
use crate::domain::TaskSpec;
use crate::mechanistic::{
    validate_action_contract, DelegationManifestV1, MechanisticReceiptStatusV1,
    MechanisticReceiptV1, MechanisticRequestV1, DELEGATION_MANIFEST_SCHEMA,
    MAX_MECHANISTIC_ACTIONS, MAX_MECHANISTIC_OUTPUT_BYTES,
};
use crate::paths::Paths;
use crate::revision::repository_revision;
use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

const AUTHORITY_SCHEMA: &str = "codex-team.delegation_authority.v1";
const MAX_SHADOW_RECEIPTS: usize = 64;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DelegationAuthorityStatusV1 {
    Shadow,
    Proposed,
    Approved,
    HumanRejected,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DelegationShadowEvidenceV1 {
    pub receipt_digest: String,
    pub request_digest: String,
    pub repository_revision: String,
    pub category: String,
    pub elapsed_millis: u64,
    pub avoided_model_calls: u32,
    pub matched: bool,
    pub succeeded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DelegationAuthorityStateV1 {
    pub schema: String,
    pub manifest_digest: String,
    pub artifact_revision: String,
    pub status: DelegationAuthorityStatusV1,
    pub approval_revision: Option<String>,
    pub shadow_evidence: Vec<DelegationShadowEvidenceV1>,
    pub quarantine_fingerprints: Vec<String>,
    pub safety_or_correctness_failures: u32,
    pub human_negative_evidence: bool,
    pub updated_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ActiveDelegationManifestV1 {
    pub schema: String,
    pub artifact_revision: String,
    pub manifest: DelegationManifestV1,
    pub authority: DelegationAuthorityStateV1,
}

impl DelegationAuthorityStateV1 {
    fn new(artifact_revision: &str, manifest_digest: &str) -> Result<Self> {
        validate_sha256(manifest_digest, "manifest digest")?;
        let mut state = Self {
            schema: AUTHORITY_SCHEMA.to_string(),
            manifest_digest: manifest_digest.to_string(),
            artifact_revision: artifact_revision.to_string(),
            status: DelegationAuthorityStatusV1::Shadow,
            approval_revision: None,
            shadow_evidence: Vec::new(),
            quarantine_fingerprints: Vec::new(),
            safety_or_correctness_failures: 0,
            human_negative_evidence: false,
            updated_at: Utc::now().to_rfc3339(),
            digest: String::new(),
        };
        refresh_state_digest(&mut state)?;
        Ok(state)
    }

    fn validate(&self) -> Result<()> {
        if self.schema != AUTHORITY_SCHEMA
            || self.shadow_evidence.len() > MAX_SHADOW_RECEIPTS
            || self.quarantine_fingerprints.len() > MAX_SHADOW_RECEIPTS
            || state_digest(self)? != self.digest
        {
            bail!("delegation authority state is corrupt or outside its bounds");
        }
        validate_sha256(&self.manifest_digest, "manifest digest")?;
        if let Some(revision) = &self.approval_revision {
            validate_sha256(revision, "approval revision")?;
        }
        for item in &self.shadow_evidence {
            validate_sha256(&item.receipt_digest, "receipt digest")?;
            validate_sha256(&item.request_digest, "request digest")?;
            validate_sha256(&item.repository_revision, "repository revision")?;
            validate_category(&item.category)?;
        }
        for fingerprint in &self.quarantine_fingerprints {
            validate_sha256(fingerprint, "repository fingerprint")?;
        }
        Ok(())
    }

    pub fn may_activate(
        &self,
        current_revision: &str,
        repository_fingerprint: &str,
        structural_matches: usize,
    ) -> bool {
        self.validate().is_ok()
            && self.status == DelegationAuthorityStatusV1::Approved
            && !self.human_negative_evidence
            && self.approval_revision.as_deref() == Some(current_revision)
            && structural_matches == 1
            && !self
                .quarantine_fingerprints
                .iter()
                .any(|fingerprint| fingerprint == repository_fingerprint)
    }
}

pub fn delegation_manifest_from_content(
    content: &ArtifactContentV1,
) -> Result<Option<DelegationManifestV1>> {
    let manifests = content
        .files
        .iter()
        .filter(|file| package_filename(&file.path) == Some("DELEGATION.json"))
        .collect::<Vec<_>>();
    if manifests.len() > 1 {
        bail!("learned artifact contains ambiguous delegation manifests");
    }
    let Some(file) = manifests.first().copied() else {
        return Ok(None);
    };
    if file.encoding != ArtifactEncodingV1::Utf8
        || file.inject_into_prompt
        || file.executable
        || content.files.iter().any(|member| member.executable)
    {
        bail!("delegation manifests and their package members must remain inert");
    }
    let manifest: DelegationManifestV1 =
        serde_json::from_str(&file.content).context("parse strict DELEGATION.json")?;
    validate_delegation_manifest(&manifest)?;
    Ok(Some(manifest))
}

pub fn canonicalize_delegation_manifest_file(files: &mut [super::ArtifactFileV1]) -> Result<bool> {
    let positions = files
        .iter()
        .enumerate()
        .filter(|(_, file)| package_filename(&file.path) == Some("DELEGATION.json"))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if positions.is_empty() {
        return Ok(false);
    }
    if positions.len() != 1 || files.iter().any(|file| file.executable) {
        bail!("delegation proposal package is ambiguous or executable");
    }
    let file = &mut files[positions[0]];
    if file.encoding != ArtifactEncodingV1::Utf8 || file.inject_into_prompt {
        bail!("DELEGATION.json must be inert UTF-8 and excluded from prompts");
    }
    let mut manifest: DelegationManifestV1 =
        serde_json::from_str(&file.content).context("parse strict DELEGATION.json")?;
    manifest.digest.clear();
    manifest.digest = delegation_manifest_digest(&manifest)?;
    validate_delegation_manifest(&manifest)?;
    file.content = serde_json::to_string_pretty(&manifest)?;
    Ok(true)
}

pub fn validate_delegation_manifest(manifest: &DelegationManifestV1) -> Result<()> {
    if manifest.schema != DELEGATION_MANIFEST_SCHEMA {
        bail!("unsupported delegation manifest schema");
    }
    if manifest.actions.is_empty()
        || manifest.actions.len() > MAX_MECHANISTIC_ACTIONS
        || usize::from(manifest.maximum_actions) < manifest.actions.len()
        || usize::from(manifest.maximum_actions) > MAX_MECHANISTIC_ACTIONS
    {
        bail!("delegation manifest action bounds are invalid");
    }
    if manifest.maximum_output_bytes == 0
        || manifest.maximum_output_bytes as usize > MAX_MECHANISTIC_OUTPUT_BYTES
    {
        bail!("delegation manifest output bound exceeds 64 KiB");
    }
    if manifest.applicability.is_empty() || manifest.applicability.len() > 32 {
        bail!("delegation manifest needs 1-32 structural applicability predicates");
    }
    for predicate in &manifest.applicability {
        validate_applicability_predicate(predicate)?;
    }
    for action in &manifest.actions {
        validate_action_contract(action)?;
    }
    if delegation_manifest_digest(manifest)? != manifest.digest {
        bail!("delegation manifest digest does not match its exact typed content");
    }
    Ok(())
}

pub fn delegation_manifest_digest(manifest: &DelegationManifestV1) -> Result<String> {
    let mut unsigned = manifest.clone();
    unsigned.digest.clear();
    canonical_digest(&unsigned)
}

pub fn register_delegation_manifest(
    vault: &LearningVault,
    artifact_revision: &str,
    manifest: &DelegationManifestV1,
) -> Result<DelegationAuthorityStateV1> {
    validate_delegation_manifest(manifest)?;
    let relative = authority_path(&manifest.digest)?;
    if let Some(state) = vault.read_mutable::<DelegationAuthorityStateV1>(&relative)? {
        state.validate()?;
        if state.artifact_revision != artifact_revision {
            bail!("delegation manifest digest is already bound to another artifact revision");
        }
        return Ok(state);
    }
    let state = DelegationAuthorityStateV1::new(artifact_revision, &manifest.digest)?;
    vault.write_mutable(&relative, &state)?;
    Ok(state)
}

pub fn record_delegation_shadow(
    vault: &LearningVault,
    artifact_revision: &str,
    manifest: &DelegationManifestV1,
    request: &MechanisticRequestV1,
    receipt: &MechanisticReceiptV1,
    category: &str,
    avoided_model_calls: u32,
) -> Result<DelegationAuthorityStateV1> {
    validate_category(category)?;
    let mut state = register_delegation_manifest(vault, artifact_revision, manifest)?;
    if matches!(
        state.status,
        DelegationAuthorityStatusV1::HumanRejected | DelegationAuthorityStatusV1::Retired
    ) {
        return Ok(state);
    }
    let matched = request.actions == manifest.actions;
    let succeeded = receipt.status == MechanisticReceiptStatusV1::Completed;
    let receipt_digest = canonical_digest(receipt)?;
    if !state
        .shadow_evidence
        .iter()
        .any(|item| item.receipt_digest == receipt_digest)
    {
        state.shadow_evidence.push(DelegationShadowEvidenceV1 {
            receipt_digest,
            request_digest: receipt.request_digest.clone(),
            repository_revision: receipt.repository_revision.clone(),
            category: category.to_string(),
            elapsed_millis: receipt.elapsed_millis,
            avoided_model_calls,
            matched,
            succeeded,
        });
        if state.shadow_evidence.len() > MAX_SHADOW_RECEIPTS {
            state.shadow_evidence.remove(0);
        }
    }
    if matched && !succeeded {
        state.safety_or_correctness_failures =
            state.safety_or_correctness_failures.saturating_add(1);
    }
    let successful_matches = state
        .shadow_evidence
        .iter()
        .filter(|item| item.matched && item.succeeded)
        .count();
    let revisions = state
        .shadow_evidence
        .iter()
        .filter(|item| item.matched && item.succeeded)
        .map(|item| item.repository_revision.as_str())
        .collect::<BTreeSet<_>>();
    if state.status == DelegationAuthorityStatusV1::Shadow
        && successful_matches >= 3
        && revisions.len() >= 2
        && state.safety_or_correctness_failures == 0
        && !state.human_negative_evidence
    {
        state.status = DelegationAuthorityStatusV1::Proposed;
    }
    save_authority(vault, &mut state)?;
    Ok(state)
}

pub fn approve_delegation_manifest(
    vault: &LearningVault,
    manifest: &DelegationManifestV1,
    expected_manifest_digest: &str,
    current_repository_revision: &str,
    expected_repository_revision: &str,
    human_confirmed: bool,
) -> Result<DelegationAuthorityStateV1> {
    if !human_confirmed {
        bail!("delegation manifest promotion requires exact human confirmation");
    }
    validate_delegation_manifest(manifest)?;
    if manifest.digest != expected_manifest_digest {
        bail!("delegation manifest digest changed before approval");
    }
    validate_sha256(expected_repository_revision, "expected repository revision")?;
    if current_repository_revision != expected_repository_revision {
        bail!("repository revision drifted before delegation manifest approval");
    }
    let relative = authority_path(expected_manifest_digest)?;
    let mut state = vault
        .read_mutable::<DelegationAuthorityStateV1>(&relative)?
        .context("delegation manifest has not reached a shadow proposal")?;
    state.validate()?;
    if state.status != DelegationAuthorityStatusV1::Proposed
        || state.human_negative_evidence
        || state.safety_or_correctness_failures != 0
    {
        bail!("delegation manifest is not an unfailed human-approvable proposal");
    }
    state.status = DelegationAuthorityStatusV1::Approved;
    state.approval_revision = Some(expected_repository_revision.to_string());
    save_authority(vault, &mut state)?;
    Ok(state)
}

pub fn record_delegation_failure(
    vault: &LearningVault,
    manifest_digest: &str,
    repository_fingerprint: &str,
) -> Result<DelegationAuthorityStateV1> {
    validate_sha256(repository_fingerprint, "repository fingerprint")?;
    let relative = authority_path(manifest_digest)?;
    let mut state = vault
        .read_mutable::<DelegationAuthorityStateV1>(&relative)?
        .context("delegation manifest authority state is missing")?;
    state.validate()?;
    if !state
        .quarantine_fingerprints
        .iter()
        .any(|fingerprint| fingerprint == repository_fingerprint)
    {
        state
            .quarantine_fingerprints
            .push(repository_fingerprint.to_string());
    }
    if state.quarantine_fingerprints.len() >= 2 {
        state.status = DelegationAuthorityStatusV1::Retired;
    }
    save_authority(vault, &mut state)?;
    Ok(state)
}

pub fn record_delegation_human_correction(
    vault: &LearningVault,
    manifest_digest: &str,
) -> Result<DelegationAuthorityStateV1> {
    let relative = authority_path(manifest_digest)?;
    let mut state = vault
        .read_mutable::<DelegationAuthorityStateV1>(&relative)?
        .context("delegation manifest authority state is missing")?;
    state.validate()?;
    state.human_negative_evidence = true;
    state.status = DelegationAuthorityStatusV1::HumanRejected;
    state.approval_revision = None;
    save_authority(vault, &mut state)?;
    Ok(state)
}

pub fn record_active_manifest_receipt(
    request: &MechanisticRequestV1,
    receipt: &MechanisticReceiptV1,
    category: &str,
    avoided_model_calls: u32,
) -> Result<Option<DelegationAuthorityStateV1>> {
    let Some(vault) = LearningVault::existing_machine()? else {
        return Ok(None);
    };
    let status = super::status(&vault)?;
    let Some(revision) = status.shared_core_revision else {
        return Ok(None);
    };
    let artifact = vault.read_immutable::<ArtifactRevisionV1>("artifacts", &revision)?;
    let content = vault
        .read_immutable::<ArtifactContentV1>("artifacts/content", &artifact.content_tree_sha256)?;
    let Some(manifest) = delegation_manifest_from_content(&content).ok().flatten() else {
        return Ok(None);
    };
    let mut state = record_delegation_shadow(
        &vault,
        &revision,
        &manifest,
        request,
        receipt,
        category,
        avoided_model_calls,
    )?;
    if state.status == DelegationAuthorityStatusV1::Approved
        && request.actions == manifest.actions
        && receipt.status != MechanisticReceiptStatusV1::Completed
    {
        state = record_delegation_failure(&vault, &manifest.digest, &receipt.repository_revision)?;
    }
    Ok(Some(state))
}

// Retained for the legacy role hierarchy; native automation binds authority
// through its execution context rather than an implicit pre-worker selection.
#[allow(dead_code)]
pub(crate) fn select_approved_manifest_for_task(
    task: &TaskSpec,
    current_revision: &str,
) -> Result<Option<DelegationManifestV1>> {
    let Some(vault) = LearningVault::existing_machine()? else {
        return Ok(None);
    };
    let status = super::status(&vault)?;
    let Some(revision) = status.shared_core_revision else {
        return Ok(None);
    };
    let artifact = vault.read_immutable::<ArtifactRevisionV1>("artifacts", &revision)?;
    let content = vault
        .read_immutable::<ArtifactContentV1>("artifacts/content", &artifact.content_tree_sha256)?;
    let Some(manifest) = delegation_manifest_from_content(&content).ok().flatten() else {
        return Ok(None);
    };
    if !manifest_matches_task(&manifest, task)? {
        return Ok(None);
    }
    let Some(state) = load_authority(&vault, &manifest.digest)? else {
        return Ok(None);
    };
    if state.artifact_revision != revision {
        return Ok(None);
    }
    Ok(state
        .may_activate(current_revision, current_revision, 1)
        .then_some(manifest))
}

pub(crate) fn active_manifest_digest_for_actions(
    actions: &[crate::mechanistic::MechanisticActionV1],
) -> Result<Option<String>> {
    let Some(vault) = LearningVault::existing_machine()? else {
        return Ok(None);
    };
    let Some(active) = active_delegation_manifest_status(&vault)? else {
        return Ok(None);
    };
    Ok((active.manifest.actions == actions).then_some(active.manifest.digest))
}

pub fn approve_active_delegation_manifest(
    repo: &Path,
    expected_manifest_digest: &str,
    expected_repository_revision: &str,
    human_confirmed: bool,
) -> Result<DelegationAuthorityStateV1> {
    let vault = LearningVault::existing_machine()?.context("self-improvement is not enrolled")?;
    let status = super::status(&vault)?;
    let revision = status
        .shared_core_revision
        .context("no active learned artifact contains a delegation proposal")?;
    let artifact = vault.read_immutable::<ArtifactRevisionV1>("artifacts", &revision)?;
    let content = vault
        .read_immutable::<ArtifactContentV1>("artifacts/content", &artifact.content_tree_sha256)?;
    let manifest = delegation_manifest_from_content(&content)?
        .context("active learned artifact has no valid DELEGATION.json")?;
    let current = repository_revision(&Paths::new(repo)?)?;
    approve_delegation_manifest(
        &vault,
        &manifest,
        expected_manifest_digest,
        &current,
        expected_repository_revision,
        human_confirmed,
    )
}

pub fn active_delegation_manifest_status(
    vault: &LearningVault,
) -> Result<Option<ActiveDelegationManifestV1>> {
    let status = super::status(vault)?;
    let Some(revision) = status.shared_core_revision else {
        return Ok(None);
    };
    let artifact = vault.read_immutable::<ArtifactRevisionV1>("artifacts", &revision)?;
    let content = vault
        .read_immutable::<ArtifactContentV1>("artifacts/content", &artifact.content_tree_sha256)?;
    let Some(manifest) = delegation_manifest_from_content(&content).ok().flatten() else {
        return Ok(None);
    };
    let Some(authority) = load_authority(vault, &manifest.digest)? else {
        return Ok(None);
    };
    if authority.artifact_revision != revision {
        return Ok(None);
    }
    Ok(Some(ActiveDelegationManifestV1 {
        schema: "codex-team.active_delegation_manifest.v1".to_string(),
        artifact_revision: revision,
        manifest,
        authority,
    }))
}

fn manifest_matches_task(manifest: &DelegationManifestV1, task: &TaskSpec) -> Result<bool> {
    let review_tier = serde_json::to_string(&task.review_tier)?;
    let review_tier = review_tier.trim_matches('"');
    let automation_level = serde_json::to_string(&task.automation_level)?;
    let automation_level = automation_level.trim_matches('"');
    Ok(manifest.applicability.iter().all(|predicate| {
        predicate == "*"
            || predicate
                .strip_prefix("task_kind:")
                .is_some_and(|value| value == task.kind)
            || predicate
                .strip_prefix("review_tier:")
                .is_some_and(|value| value == review_tier)
            || predicate
                .strip_prefix("automation_level:")
                .is_some_and(|value| value == automation_level)
            || predicate
                .strip_prefix("allowed_path:")
                .is_some_and(|value| task.allowed_files.iter().any(|path| path == value))
            || predicate
                .strip_prefix("read_only_path:")
                .is_some_and(|value| task.read_only_files.iter().any(|path| path == value))
    }))
}

fn validate_applicability_predicate(value: &str) -> Result<()> {
    let valid_prefix = value == "*"
        || [
            "task_kind:",
            "review_tier:",
            "automation_level:",
            "allowed_path:",
            "read_only_path:",
        ]
        .iter()
        .any(|prefix| value.starts_with(prefix) && value.len() > prefix.len());
    if !valid_prefix
        || value.len() > 512
        || value.chars().any(char::is_control)
        || value.contains("..")
        || value.contains('\\')
    {
        bail!("delegation applicability predicate is not a bounded structural match");
    }
    Ok(())
}

fn validate_category(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        bail!("delegation telemetry category is invalid");
    }
    Ok(())
}

fn authority_path(manifest_digest: &str) -> Result<String> {
    validate_sha256(manifest_digest, "manifest digest")?;
    Ok(format!("delegation-manifest-{manifest_digest}.json"))
}

fn load_authority(
    vault: &LearningVault,
    manifest_digest: &str,
) -> Result<Option<DelegationAuthorityStateV1>> {
    let Some(state) =
        vault.read_mutable::<DelegationAuthorityStateV1>(&authority_path(manifest_digest)?)?
    else {
        return Ok(None);
    };
    state.validate()?;
    Ok(Some(state))
}

fn save_authority(vault: &LearningVault, state: &mut DelegationAuthorityStateV1) -> Result<()> {
    state.updated_at = Utc::now().to_rfc3339();
    refresh_state_digest(state)?;
    vault.write_mutable(&authority_path(&state.manifest_digest)?, state)?;
    Ok(())
}

fn refresh_state_digest(state: &mut DelegationAuthorityStateV1) -> Result<()> {
    state.digest.clear();
    state.digest = state_digest(state)?;
    Ok(())
}

fn state_digest(state: &DelegationAuthorityStateV1) -> Result<String> {
    let mut unsigned = state.clone();
    unsigned.digest.clear();
    canonical_digest(&unsigned)
}

fn validate_sha256(value: &str, label: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("{label} must be one lowercase SHA-256 digest");
    }
    Ok(())
}

fn package_filename(path: &str) -> Option<&str> {
    path.rsplit('/').next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mechanistic::{
        DelegationReadinessV1, DelegationSafetyClassV1, MechanisticActionResultV1,
        MechanisticActionV1, MechanisticReceiptStatusV1, MECHANISTIC_RECEIPT_SCHEMA,
        MECHANISTIC_REQUEST_SCHEMA,
    };
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn manifest() -> DelegationManifestV1 {
        let mut manifest = DelegationManifestV1 {
            schema: DELEGATION_MANIFEST_SCHEMA.to_string(),
            safety_class: DelegationSafetyClassV1::ReadObserveVerifyOnly,
            actions: vec![MechanisticActionV1::GitStatusSummary { max_entries: 20 }],
            applicability: vec!["task_kind:verification".to_string()],
            maximum_actions: 1,
            maximum_output_bytes: 4096,
            digest: String::new(),
        };
        manifest.digest = delegation_manifest_digest(&manifest).unwrap();
        manifest
    }

    fn receipt(
        request: &MechanisticRequestV1,
        revision: &str,
        suffix: &str,
    ) -> MechanisticReceiptV1 {
        MechanisticReceiptV1 {
            schema: MECHANISTIC_RECEIPT_SCHEMA.to_string(),
            request_digest: format!("{}{}", &request.expected_revision[..62], suffix),
            repository_revision: revision.to_string(),
            status: MechanisticReceiptStatusV1::Completed,
            readiness: DelegationReadinessV1 {
                queries_available: true,
                evidence_available: false,
                observation_available: true,
                evidence_backend: None,
                reasons: Vec::new(),
            },
            action_results: vec![MechanisticActionResultV1 {
                action_index: 0,
                category: "query".to_string(),
                status: MechanisticReceiptStatusV1::Completed,
                summary: "bounded query completed RAW-RECEIPT-CANARY".to_string(),
                entries: Vec::new(),
                truncated: false,
                duration_millis: 1,
                decision: None,
            }],
            output_bytes: 0,
            started_at: "2026-01-01T00:00:00Z".to_string(),
            finished_at: "2026-01-01T00:00:01Z".to_string(),
            elapsed_millis: 1,
            fallback_reason: None,
            broker: None,
            capsule: None,
        }
    }

    #[test]
    fn inert_manifest_is_strict_digest_bound_and_executable_members_disable_it() -> Result<()> {
        let manifest = manifest();
        let mut content = ArtifactContentV1 {
            schema: super::super::ARTIFACT_CONTENT_SCHEMA.to_string(),
            content_id: "unused".to_string(),
            files: vec![super::super::ArtifactFileV1 {
                path: "DELEGATION.json".to_string(),
                encoding: ArtifactEncodingV1::Utf8,
                content: serde_json::to_string(&manifest)?,
                inject_into_prompt: false,
                executable: false,
            }],
            digest: String::new(),
        };
        assert_eq!(delegation_manifest_from_content(&content)?, Some(manifest));
        content.files.push(super::super::ArtifactFileV1 {
            path: "scripts/check.sh".to_string(),
            encoding: ArtifactEncodingV1::Utf8,
            content: "exit 0".to_string(),
            inject_into_prompt: false,
            executable: true,
        });
        assert!(delegation_manifest_from_content(&content).is_err());
        Ok(())
    }

    #[test]
    fn manifest_is_canonicalized_before_namespacing_and_remains_discoverable() -> Result<()> {
        let mut unsigned = manifest();
        unsigned.digest.clear();
        let mut files = vec![super::super::ArtifactFileV1 {
            path: "DELEGATION.json".to_string(),
            encoding: ArtifactEncodingV1::Utf8,
            content: serde_json::to_string(&unsigned)?,
            inject_into_prompt: false,
            executable: false,
        }];
        assert!(canonicalize_delegation_manifest_file(&mut files)?);
        files[0].path = "skills/0123456789abcdef/DELEGATION.json".to_string();
        let content = ArtifactContentV1 {
            schema: super::super::ARTIFACT_CONTENT_SCHEMA.to_string(),
            content_id: super::super::artifact_tree_digest(&files)?,
            files,
            digest: String::new(),
        };
        assert_eq!(
            delegation_manifest_from_content(&content)?,
            Some(manifest())
        );
        Ok(())
    }

    #[test]
    fn three_matches_on_two_revisions_propose_then_exact_human_approval_activates() -> Result<()> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!("codex-team-delegation-shadow-{nonce}"));
        let vault = LearningVault::open_at(root.clone())?;
        let manifest = manifest();
        let revision_a = "a".repeat(64);
        let revision_b = "b".repeat(64);
        for (revision, suffix) in [
            (&revision_a, "01"),
            (&revision_a, "02"),
            (&revision_b, "03"),
        ] {
            let request = MechanisticRequestV1 {
                schema: MECHANISTIC_REQUEST_SCHEMA.to_string(),
                purpose: Default::default(),
                expected_revision: revision.clone(),
                delegation_policy: crate::mechanistic::DelegationPolicy::Prefer,
                allow_unisolated_evidence: false,
                actions: manifest.actions.clone(),
            };
            let state = record_delegation_shadow(
                &vault,
                "artifact-test",
                &manifest,
                &request,
                &receipt(&request, revision, suffix),
                "shadow_match",
                1,
            )?;
            if suffix == "03" {
                assert_eq!(state.status, DelegationAuthorityStatusV1::Proposed);
                assert!(!serde_json::to_string(&state)?.contains("RAW-RECEIPT-CANARY"));
            }
        }
        let approved = approve_delegation_manifest(
            &vault,
            &manifest,
            &manifest.digest,
            &revision_b,
            &revision_b,
            true,
        )?;
        assert!(approved.may_activate(&revision_b, &revision_b, 1));
        assert!(!approved.may_activate(&revision_a, &revision_a, 1));
        assert!(!approved.may_activate(&revision_b, &revision_b, 2));
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn failure_quarantines_one_fingerprint_two_retire_and_human_correction_wins() -> Result<()> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!("codex-team-delegation-failure-{nonce}"));
        let vault = LearningVault::open_at(root.clone())?;
        let manifest = manifest();
        register_delegation_manifest(&vault, "artifact-test", &manifest)?;
        let one = record_delegation_failure(&vault, &manifest.digest, &"c".repeat(64))?;
        assert_eq!(one.status, DelegationAuthorityStatusV1::Shadow);
        let two = record_delegation_failure(&vault, &manifest.digest, &"d".repeat(64))?;
        assert_eq!(two.status, DelegationAuthorityStatusV1::Retired);
        let corrected = record_delegation_human_correction(&vault, &manifest.digest)?;
        assert_eq!(corrected.status, DelegationAuthorityStatusV1::HumanRejected);
        assert!(corrected.human_negative_evidence);
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
