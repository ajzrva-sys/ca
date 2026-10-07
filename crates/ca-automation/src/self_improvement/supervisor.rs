use super::{
    canonical_digest, ComponentV1, DeploymentReceiptV1, ImprovementObjectiveV1, LearningTraceV1,
    LearningVault, VaultHealthV1,
};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const STATE_SCHEMA: &str = "codex-team.self_improvement_state.v1";
pub const ENROLLMENT_SCHEMA: &str = "codex-team.self_improvement_enrollment.v1";
pub const SAFETY_KERNEL_SCHEMA: &str = "codex-team.safety_kernel_manifest.v1";

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero_usize(value: &usize) -> bool {
    *value == 0
}

fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DelegationModeV1 {
    Same,
    Fixed,
    Learned,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RepositoryEnrollmentV1 {
    pub name: String,
    /// Deprecated enrollment-v1 field. New records never persist canonical paths;
    /// a supervisor tick removes this field from migrated state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub path_fingerprint: String,
    pub branch: String,
    pub head: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CaEvidenceProfileEnrollmentV1 {
    pub name: String,
    pub profile_fingerprint: String,
    pub state_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CaExecutionRouteV1 {
    pub provider: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CaExecutionProfileEnrollmentV1 {
    pub name: String,
    pub sandbox_required: bool,
    pub routes: Vec<CaExecutionRouteV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRootEnrollmentV1 {
    pub kind: String,
    pub path_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentContractV1 {
    pub schema: String,
    pub mode: String,
    pub continuous: bool,
    pub scope: String,
    pub providers: Vec<String>,
    pub trace_sharing: String,
    pub trace_retention: String,
    pub promotion: String,
    pub source_changes: String,
    pub source_branch: String,
    pub deployment: String,
    pub budget: String,
    pub install_login_service: bool,
    pub repositories: Vec<RepositoryEnrollmentV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_roots: Vec<EvidenceRootEnrollmentV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ca_evidence_profiles: Vec<CaEvidenceProfileEnrollmentV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_execution_profile: Option<CaExecutionProfileEnrollmentV1>,
    pub supervisor_sha256: String,
    pub ca_binary_sha256: String,
    pub codex_team_binary_sha256: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SafetyKernelManifestV1 {
    pub schema: String,
    pub supervisor_sha256: String,
    pub protected_source_paths: Vec<String>,
    pub protected_trees: Vec<String>,
    pub forbidden_source_transitions: Vec<String>,
    pub dependency_changes_allowed: bool,
    pub created_at: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PauseStateV1 {
    pub reason: String,
    pub source: String,
    pub paused_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BinarySlotV1 {
    pub slot: String,
    pub binary_sha256: String,
    pub source_commit: String,
    pub verified: bool,
    pub activated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ActivePointerV1 {
    pub schema: String,
    pub component: String,
    pub slot: String,
    pub binary_sha256: String,
    pub state_sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ActivationTransactionV1 {
    schema: String,
    component: String,
    candidate_id: String,
    from_slot: String,
    to_slot: String,
    started_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct BridgeEnrollmentTransactionV1 {
    schema: String,
    enrollment_digest: String,
    profiles: Vec<CaEvidenceProfileEnrollmentV1>,
    completed_profiles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ComponentSlotsV1 {
    pub active: String,
    pub previous: Option<String>,
    pub slots: BTreeMap<String, Option<BinarySlotV1>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KnownGoodPairV1 {
    pub ca_slot: String,
    pub ca_sha256: String,
    pub codex_team_slot: String,
    pub codex_team_sha256: String,
    pub protocol_matrix_digest: String,
    pub verified_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UsageTotalsV1 {
    pub provider_calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub estimated_microusd: u64,
    pub experiments: u64,
    pub promotions: u64,
    pub rollbacks: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub ai_calls_avoided: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub estimated_tokens_avoided: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SelfImprovementStateV1 {
    pub schema: String,
    pub enrolled: bool,
    pub enrollment_digest: String,
    pub continuous: bool,
    pub pause: Option<PauseStateV1>,
    pub repositories: Vec<RepositoryEnrollmentV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_roots: Vec<EvidenceRootEnrollmentV1>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ca_evidence_profiles: Vec<CaEvidenceProfileEnrollmentV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_execution_profile: Option<CaExecutionProfileEnrollmentV1>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ca_bridge_cursors: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ca_bridge_healthy: bool,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub ca_bridge_queued_intents: usize,
    pub active_objective_id: Option<String>,
    pub shared_core_revision: Option<String>,
    pub provider_overlay_revisions: BTreeMap<String, String>,
    pub components: BTreeMap<String, ComponentSlotsV1>,
    pub known_good_pair: Option<KnownGoodPairV1>,
    pub usage: UsageTotalsV1,
    pub last_health_at: Option<String>,
    pub state_sequence: u64,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SelfImprovementStatusV1 {
    pub schema: String,
    pub enrolled: bool,
    pub running_allowed: bool,
    pub paused: bool,
    pub pause: Option<PauseStateV1>,
    pub revision: String,
    pub vault: VaultHealthV1,
    pub active_objective_id: Option<String>,
    pub ca_evidence_profiles: Vec<CaEvidenceProfileEnrollmentV1>,
    pub ca_execution_profile: Option<CaExecutionProfileEnrollmentV1>,
    pub bridge_healthy: bool,
    pub queued_learning_intents: usize,
    pub workflow_stage: Option<String>,
    pub active_candidate_id: Option<String>,
    pub active_candidate_kind: Option<super::ArtifactKindV1>,
    pub active_candidate_author: Option<String>,
    pub active_judges: Vec<String>,
    pub current_decision: Option<String>,
    pub replay_completed: usize,
    pub replay_total: usize,
    pub cooldown_until: Option<String>,
    pub failure_fingerprint_count: usize,
    pub workflow_attempt: u32,
    pub shared_core_revision: Option<String>,
    pub active_skill_revisions: Vec<String>,
    pub active_recipe_revisions: Vec<String>,
    pub provider_overlay_revisions: BTreeMap<String, String>,
    pub components: BTreeMap<String, ComponentSlotsV1>,
    pub known_good_pair: Option<KnownGoodPairV1>,
    pub trace_count: usize,
    pub objective_count: usize,
    pub deployment_count: usize,
    pub usage: UsageTotalsV1,
    pub privacy_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SelfImprovementDoctorV1 {
    pub schema: String,
    pub healthy: bool,
    pub enrolled: bool,
    pub state_revision: String,
    pub issues: Vec<String>,
    pub vault: VaultHealthV1,
    pub supervisor_binary_matches_manifest: bool,
    pub known_good_pair_available: bool,
    pub bridge_healthy: bool,
    pub queued_learning_intents: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MutationReceiptV1 {
    pub schema: String,
    pub action: String,
    pub changed: bool,
    pub revision: String,
    pub detail: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LearnedPromptContextV1 {
    pub prompt_fragment: String,
    pub core_revision: Option<String>,
    pub overlay_revision: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EnrollmentInputs {
    pub ca_repo: PathBuf,
    pub orchestrator_repo: PathBuf,
    pub install_login_service: bool,
    pub supervisor_binary: PathBuf,
    pub ca_binary: PathBuf,
    pub codex_team_binary: PathBuf,
    pub ca_evidence_profiles: Vec<String>,
    pub ca_execution_profile: String,
}

#[derive(Debug, Clone)]
pub struct KnowledgeEnrollmentInputs {
    pub install_login_service: bool,
    pub supervisor_binary: PathBuf,
    pub ca_binary: PathBuf,
    pub codex_team_binary: PathBuf,
    pub ca_evidence_profiles: Vec<String>,
    pub ca_execution_profile: String,
    pub evidence_roots: Vec<(String, PathBuf)>,
}

impl SelfImprovementStateV1 {
    fn blank() -> Self {
        Self {
            schema: STATE_SCHEMA.to_string(),
            enrolled: false,
            enrollment_digest: String::new(),
            continuous: true,
            pause: None,
            repositories: Vec::new(),
            evidence_roots: Vec::new(),
            ca_evidence_profiles: Vec::new(),
            ca_execution_profile: None,
            ca_bridge_cursors: BTreeMap::new(),
            ca_bridge_healthy: false,
            ca_bridge_queued_intents: 0,
            active_objective_id: None,
            shared_core_revision: None,
            provider_overlay_revisions: BTreeMap::new(),
            components: BTreeMap::new(),
            known_good_pair: None,
            usage: UsageTotalsV1 {
                provider_calls: 0,
                input_tokens: 0,
                output_tokens: 0,
                estimated_microusd: 0,
                experiments: 0,
                promotions: 0,
                rollbacks: 0,
                ai_calls_avoided: 0,
                estimated_tokens_avoided: 0,
            },
            last_health_at: None,
            state_sequence: 0,
            revision: String::new(),
        }
    }

    pub(super) fn refresh_revision(&mut self) -> Result<()> {
        self.revision.clear();
        self.revision = canonical_digest(self)?;
        Ok(())
    }

    fn verify_revision(&self) -> Result<()> {
        let expected = self.revision.clone();
        let mut unsigned = self.clone();
        unsigned.revision.clear();
        let actual = canonical_digest(&unsigned)?;
        if actual != expected {
            bail!("self-improvement state revision mismatch");
        }
        Ok(())
    }
}

pub fn default_enrollment_inputs(
    ca_repo: PathBuf,
    orchestrator_repo: PathBuf,
    install_login_service: bool,
    ca_evidence_profiles: Vec<String>,
    ca_execution_profile: String,
) -> Result<EnrollmentInputs> {
    let codex_team_binary = fs::canonicalize(std::env::current_exe()?)?;
    let binary_dir = codex_team_binary
        .parent()
        .context("codex-team binary has no parent")?;
    let supervisor_binary = binary_dir.join("codex-team-supervisor");
    let ca_binary = ca_repo.join("target/release/ca");
    Ok(EnrollmentInputs {
        ca_repo,
        orchestrator_repo,
        install_login_service,
        supervisor_binary,
        ca_binary,
        codex_team_binary,
        ca_evidence_profiles,
        ca_execution_profile,
    })
}

pub fn enrollment_contract(inputs: &EnrollmentInputs) -> Result<EnrollmentContractV1> {
    let ca = inspect_repository("ca", &inputs.ca_repo)?;
    let orchestrator = inspect_repository("codex-team", &inputs.orchestrator_repo)?;
    inspect_executable(&inputs.supervisor_binary)?;
    inspect_executable(&inputs.ca_binary)?;
    inspect_executable(&inputs.codex_team_binary)?;
    let mut evidence_names = inputs.ca_evidence_profiles.clone();
    evidence_names.sort();
    evidence_names.dedup();
    if evidence_names.len() != inputs.ca_evidence_profiles.len() {
        bail!("ca evidence profiles must be unique");
    }
    let ca_evidence_profiles = evidence_names
        .iter()
        .map(|profile| inspect_ca_evidence_profile(&inputs.ca_binary, profile))
        .collect::<Result<Vec<_>>>()?;
    let ca_execution_profile =
        inspect_ca_execution_profile(&inputs.ca_binary, &inputs.ca_execution_profile)?;
    Ok(EnrollmentContractV1 {
        schema: ENROLLMENT_SCHEMA.to_string(),
        mode: "autonomous".to_string(),
        continuous: true,
        scope: "machine".to_string(),
        providers: vec![
            "codex".to_string(),
            "deepseek".to_string(),
            "mimo".to_string(),
        ],
        trace_sharing: "full".to_string(),
        trace_retention: "indefinite".to_string(),
        promotion: "replay_only".to_string(),
        source_changes: "inactive_artifact_learning_only".to_string(),
        source_branch: "current".to_string(),
        deployment: "rollback_only_no_candidate_activation".to_string(),
        budget: "unbounded".to_string(),
        install_login_service: inputs.install_login_service,
        repositories: vec![ca, orchestrator],
        evidence_roots: Vec::new(),
        ca_evidence_profiles,
        ca_execution_profile: Some(ca_execution_profile),
        supervisor_sha256: file_sha256(&inputs.supervisor_binary)?,
        ca_binary_sha256: file_sha256(&inputs.ca_binary)?,
        codex_team_binary_sha256: file_sha256(&inputs.codex_team_binary)?,
        created_at: "digest-bound-at-enrollment".to_string(),
    })
}

pub fn knowledge_enrollment_contract(
    inputs: &KnowledgeEnrollmentInputs,
) -> Result<EnrollmentContractV1> {
    inspect_executable(&inputs.supervisor_binary)?;
    inspect_executable(&inputs.ca_binary)?;
    inspect_executable(&inputs.codex_team_binary)?;
    let mut evidence_names = inputs.ca_evidence_profiles.clone();
    evidence_names.sort();
    evidence_names.dedup();
    if evidence_names.len() != inputs.ca_evidence_profiles.len() || evidence_names.is_empty() {
        bail!("knowledge-only enrollment requires unique ca evidence profiles");
    }
    let ca_evidence_profiles = evidence_names
        .iter()
        .map(|profile| inspect_ca_evidence_profile(&inputs.ca_binary, profile))
        .collect::<Result<Vec<_>>>()?;
    let ca_execution_profile =
        inspect_ca_execution_profile(&inputs.ca_binary, &inputs.ca_execution_profile)?;
    let mut evidence_roots = Vec::new();
    let mut root_kinds = BTreeMap::new();
    for (kind, root) in &inputs.evidence_roots {
        if kind.is_empty() || kind.len() > 80 || root_kinds.insert(kind, ()).is_some() {
            bail!("knowledge-only evidence root kinds must be unique and bounded");
        }
        let requested =
            fs::symlink_metadata(root).with_context(|| format!("inspect evidence root {kind}"))?;
        if requested.file_type().is_symlink() {
            bail!("knowledge-only evidence root cannot be symbolic");
        }
        let canonical =
            fs::canonicalize(root).with_context(|| format!("resolve evidence root {kind}"))?;
        let metadata = fs::symlink_metadata(&canonical)?;
        let valid_mimocode_database = kind == "mimocode"
            && metadata.is_file()
            && canonical.file_name().and_then(|name| name.to_str()) == Some("mimocode.db");
        if metadata.file_type().is_symlink() || (!metadata.is_dir() && !valid_mimocode_database) {
            bail!("knowledge-only evidence root must be a real directory or MiMoCode database");
        }
        evidence_roots.push(EvidenceRootEnrollmentV1 {
            kind: kind.clone(),
            path_fingerprint: format!(
                "{:x}",
                Sha256::digest(canonical.as_os_str().as_encoded_bytes())
            ),
        });
    }
    evidence_roots.sort_by(|left, right| left.kind.cmp(&right.kind));
    let mut providers = ca_execution_profile
        .routes
        .iter()
        .map(|route| route.provider.clone())
        .collect::<Vec<_>>();
    providers.sort();
    providers.dedup();
    Ok(EnrollmentContractV1 {
        schema: ENROLLMENT_SCHEMA.to_string(),
        mode: "autonomous_knowledge_only".to_string(),
        continuous: true,
        scope: "machine".to_string(),
        providers,
        trace_sharing: "sanitized".to_string(),
        trace_retention: "indefinite".to_string(),
        promotion: "replay_approved_memory_and_inert_skills_only".to_string(),
        source_changes: "forbidden".to_string(),
        source_branch: "none".to_string(),
        deployment: "disabled".to_string(),
        budget: "unbounded_metered".to_string(),
        install_login_service: inputs.install_login_service,
        repositories: Vec::new(),
        evidence_roots,
        ca_evidence_profiles,
        ca_execution_profile: Some(ca_execution_profile),
        supervisor_sha256: file_sha256(&inputs.supervisor_binary)?,
        ca_binary_sha256: file_sha256(&inputs.ca_binary)?,
        codex_team_binary_sha256: file_sha256(&inputs.codex_team_binary)?,
        created_at: "digest-bound-at-enrollment".to_string(),
    })
}

pub fn enroll(
    vault: &LearningVault,
    inputs: &EnrollmentInputs,
    confirmation: &str,
) -> Result<MutationReceiptV1> {
    let _lock = vault.acquire_lock()?;
    let contract = enrollment_contract(inputs)?;
    let expected = canonical_digest(&contract)?;
    if confirmation != expected {
        bail!(
            "enrollment confirmation mismatch; no state changed. Review and rerun with --confirm {expected}"
        );
    }
    if let Some(existing) = load_state(vault)? {
        if existing.enrolled && existing.enrollment_digest == expected {
            return Ok(MutationReceiptV1 {
                schema: "codex-team.self_improvement_mutation.v1".to_string(),
                action: "enroll".to_string(),
                changed: false,
                revision: existing.revision,
                detail: "already enrolled with the exact contract".to_string(),
            });
        }
        if existing.enrolled {
            bail!("self-improvement is already enrolled with a different immutable contract");
        }
    }

    enroll_ca_evidence_profiles(vault, &inputs.ca_binary, &contract, &expected)?;

    let installed_supervisor = vault.resolve_relative("supervisor/bin/codex-team-supervisor")?;
    copy_private_executable(&inputs.supervisor_binary, &installed_supervisor)?;
    let mut manifest = SafetyKernelManifestV1 {
        schema: SAFETY_KERNEL_SCHEMA.to_string(),
        supervisor_sha256: contract.supervisor_sha256.clone(),
        protected_source_paths: vec![
            "src/self_improvement".to_string(),
            "src/bin/codex-team-supervisor.rs".to_string(),
            "src/guard.rs".to_string(),
            "src/verification".to_string(),
            "src/cancellation.rs".to_string(),
        ],
        protected_trees: vec!["oracles".to_string(), "reference".to_string()],
        forbidden_source_transitions: vec![
            "dependency_addition".to_string(),
            "lockfile_dependency_change".to_string(),
            "stash".to_string(),
            "reset".to_string(),
            "clean".to_string(),
            "force_checkout".to_string(),
            "history_rewrite".to_string(),
        ],
        dependency_changes_allowed: false,
        created_at: Utc::now().to_rfc3339(),
        digest: String::new(),
    };
    manifest.digest = manifest_digest(&manifest)?;
    let manifest_path = vault.resolve_relative("supervisor/safety-kernel.json")?;
    write_immutable_private_json(&manifest_path, &manifest)?;

    let ca_slot = seed_slot(
        vault,
        "ca",
        "a",
        &inputs.ca_binary,
        &contract.repositories[0].head,
    )?;
    let orchestrator_slot = seed_slot(
        vault,
        "codex-team",
        "a",
        &inputs.codex_team_binary,
        &contract.repositories[1].head,
    )?;
    let mut components = BTreeMap::new();
    components.insert("ca".to_string(), initial_component(ca_slot.clone()));
    components.insert(
        "codex-team".to_string(),
        initial_component(orchestrator_slot.clone()),
    );
    let protocol_matrix_digest = canonical_digest(&serde_json::json!({
        "ca": ca_slot.binary_sha256,
        "codex_team": orchestrator_slot.binary_sha256,
        "checks": ["version", "help", "doctor", "role_run_preflight", "mcp_discovery"]
    }))?;
    let mut state = SelfImprovementStateV1::blank();
    state.enrolled = true;
    state.enrollment_digest = expected;
    state.repositories = contract.repositories;
    state.evidence_roots = contract.evidence_roots;
    state.ca_evidence_profiles = contract.ca_evidence_profiles;
    state.ca_execution_profile = contract.ca_execution_profile;
    state.ca_bridge_healthy = true;
    state.components = components;
    state.known_good_pair = Some(KnownGoodPairV1 {
        ca_slot: "a".to_string(),
        ca_sha256: ca_slot.binary_sha256,
        codex_team_slot: "a".to_string(),
        codex_team_sha256: orchestrator_slot.binary_sha256,
        protocol_matrix_digest,
        verified_at: Utc::now().to_rfc3339(),
    });
    state.state_sequence = 1;
    state.refresh_revision()?;
    save_state(vault, &state)?;
    write_active_pointer(vault, "ca", &state)?;
    write_active_pointer(vault, "codex-team", &state)?;

    if inputs.install_login_service {
        install_login_service(vault, &installed_supervisor)?;
    }
    vault.remove_mutable("supervisor/bridge-enrollment-transaction.json")?;
    Ok(MutationReceiptV1 {
        schema: "codex-team.self_improvement_mutation.v1".to_string(),
        action: "enroll".to_string(),
        changed: true,
        revision: state.revision,
        detail: "enrolled; autonomous work remains bounded by the immutable safety kernel"
            .to_string(),
    })
}

pub fn enroll_knowledge(
    vault: &LearningVault,
    inputs: &KnowledgeEnrollmentInputs,
    confirmation: &str,
) -> Result<MutationReceiptV1> {
    let _lock = vault.acquire_lock()?;
    let contract = knowledge_enrollment_contract(inputs)?;
    let expected = canonical_digest(&contract)?;
    if confirmation != expected {
        bail!(
            "knowledge-only enrollment confirmation mismatch; no state changed. Review and rerun with --confirm {expected}"
        );
    }
    if let Some(existing) = load_state(vault)? {
        if existing.enrolled && existing.enrollment_digest == expected {
            if inputs.install_login_service {
                let installed_supervisor =
                    vault.resolve_relative("supervisor/bin/codex-team-supervisor")?;
                install_login_service(vault, &installed_supervisor)?;
            }
            return Ok(MutationReceiptV1 {
                schema: "codex-team.self_improvement_mutation.v1".to_string(),
                action: "enroll_knowledge".to_string(),
                changed: false,
                revision: existing.revision,
                detail: "already enrolled with the exact knowledge-only contract".to_string(),
            });
        }
        if existing.enrolled {
            bail!("learning is already enrolled with a different immutable contract");
        }
    }

    enroll_ca_evidence_profiles(vault, &inputs.ca_binary, &contract, &expected)?;
    let installed_supervisor = vault.resolve_relative("supervisor/bin/codex-team-supervisor")?;
    copy_private_executable(&inputs.supervisor_binary, &installed_supervisor)?;
    let mut manifest = SafetyKernelManifestV1 {
        schema: SAFETY_KERNEL_SCHEMA.to_string(),
        supervisor_sha256: contract.supervisor_sha256.clone(),
        protected_source_paths: vec![
            "src/self_improvement".to_string(),
            "src/bin/codex-team-supervisor.rs".to_string(),
            "src/guard.rs".to_string(),
            "src/verification".to_string(),
            "src/cancellation.rs".to_string(),
        ],
        protected_trees: vec!["oracles".to_string(), "reference".to_string()],
        forbidden_source_transitions: vec![
            "all_source_changes".to_string(),
            "binary_candidates".to_string(),
            "permission_changes".to_string(),
            "provider_setting_changes".to_string(),
            "routing_changes".to_string(),
            "role_prompt_changes".to_string(),
            "executable_artifacts".to_string(),
            "plugin_changes".to_string(),
            "dependency_changes".to_string(),
        ],
        dependency_changes_allowed: false,
        created_at: Utc::now().to_rfc3339(),
        digest: String::new(),
    };
    manifest.digest = manifest_digest(&manifest)?;
    write_immutable_private_json(
        &vault.resolve_relative("supervisor/safety-kernel.json")?,
        &manifest,
    )?;

    let source_binding = format!("knowledge-only:{}", &expected[..16]);
    let ca_slot = seed_slot(vault, "ca", "a", &inputs.ca_binary, &source_binding)?;
    let orchestrator_slot = seed_slot(
        vault,
        "codex-team",
        "a",
        &inputs.codex_team_binary,
        &source_binding,
    )?;
    let mut components = BTreeMap::new();
    components.insert("ca".to_string(), initial_component(ca_slot.clone()));
    components.insert(
        "codex-team".to_string(),
        initial_component(orchestrator_slot.clone()),
    );
    let protocol_matrix_digest = canonical_digest(&serde_json::json!({
        "ca": ca_slot.binary_sha256,
        "codex_team": orchestrator_slot.binary_sha256,
        "checks": ["version", "help", "doctor", "role_run_preflight", "learning_bridge"],
        "authority": "knowledge_only"
    }))?;
    let mut state = SelfImprovementStateV1::blank();
    state.enrolled = true;
    state.enrollment_digest = expected;
    state.repositories = Vec::new();
    state.evidence_roots = contract.evidence_roots;
    state.ca_evidence_profiles = contract.ca_evidence_profiles;
    state.ca_execution_profile = contract.ca_execution_profile;
    state.ca_bridge_healthy = true;
    state.components = components;
    state.known_good_pair = Some(KnownGoodPairV1 {
        ca_slot: "a".to_string(),
        ca_sha256: ca_slot.binary_sha256,
        codex_team_slot: "a".to_string(),
        codex_team_sha256: orchestrator_slot.binary_sha256,
        protocol_matrix_digest,
        verified_at: Utc::now().to_rfc3339(),
    });
    state.state_sequence = 1;
    state.refresh_revision()?;
    save_state(vault, &state)?;
    write_active_pointer(vault, "ca", &state)?;
    write_active_pointer(vault, "codex-team", &state)?;
    if inputs.install_login_service {
        install_login_service(vault, &installed_supervisor)?;
    }
    vault.remove_mutable("supervisor/bridge-enrollment-transaction.json")?;
    Ok(MutationReceiptV1 {
        schema: "codex-team.self_improvement_mutation.v1".to_string(),
        action: "enroll_knowledge".to_string(),
        changed: true,
        revision: state.revision,
        detail: "enrolled one continuous knowledge-only service; only replay-approved memory and inert skills may publish"
            .to_string(),
    })
}

pub fn status(vault: &LearningVault) -> Result<SelfImprovementStatusV1> {
    let health = vault.health()?;
    let state = load_state(vault)?.unwrap_or_else(SelfImprovementStateV1::blank);
    let trace_count = vault.list_immutable::<LearningTraceV1>("traces")?.len();
    let objective_count = vault
        .list_immutable::<ImprovementObjectiveV1>("objectives")?
        .len();
    let deployment_count = vault
        .list_immutable::<DeploymentReceiptV1>("deployments/receipts")?
        .len();
    let bridge_healthy = state.ca_bridge_healthy
        && vault
            .read_mutable::<BridgeEnrollmentTransactionV1>(
                "supervisor/bridge-enrollment-transaction.json",
            )?
            .is_none();
    let workflow = super::workflow_summary(vault, state.active_objective_id.as_deref())?;
    let candidate = workflow
        .as_ref()
        .and_then(|item| item.candidate_id.as_deref())
        .map(|id| vault.read_immutable::<super::ArtifactCandidateV1>("candidates", id))
        .transpose()?;
    let current_decision = candidate
        .as_ref()
        .and_then(|candidate| candidate.candidate_id.strip_prefix("candidate-"))
        .map(|suffix| format!("evaluation-{suffix}"))
        .map(|id| {
            vault
                .read_optional_immutable::<super::EvaluationReceiptV1>("evaluations/receipts", &id)
                .map(|receipt| {
                    receipt.map(|receipt| {
                        if receipt.promoted {
                            if workflow
                                .as_ref()
                                .is_some_and(|workflow| workflow.stage == "promoted")
                            {
                                "promoted"
                            } else {
                                "needs_review"
                            }
                            .to_string()
                        } else {
                            "rejected".to_string()
                        }
                    })
                })
        })
        .transpose()?
        .flatten();
    let artifacts = vault.list_immutable::<super::ArtifactRevisionV1>("artifacts")?;
    let active_skill_revisions = artifacts
        .iter()
        .filter(|artifact| !artifact.retired && artifact.kind == super::ArtifactKindV1::Skill)
        .rev()
        .take(16)
        .map(|artifact| artifact.revision.clone())
        .collect();
    let mut active_recipe_revisions = artifacts
        .iter()
        .filter(|artifact| !artifact.retired && artifact.kind == super::ArtifactKindV1::Recipe)
        .rev()
        .take(16)
        .map(|artifact| artifact.revision.clone())
        .collect::<Vec<_>>();
    active_recipe_revisions.extend(crate::learning::global_approved_recipe_revisions()?);
    active_recipe_revisions.sort();
    active_recipe_revisions.dedup();
    Ok(SelfImprovementStatusV1 {
        schema: "codex-team.self_improvement_status.v1".to_string(),
        enrolled: state.enrolled,
        running_allowed: state.enrolled && state.pause.is_none() && health.healthy,
        paused: state.pause.is_some() || !health.healthy,
        pause: state.pause,
        revision: state.revision,
        vault: health,
        active_objective_id: state.active_objective_id,
        ca_evidence_profiles: state.ca_evidence_profiles,
        ca_execution_profile: state.ca_execution_profile,
        bridge_healthy,
        queued_learning_intents: state.ca_bridge_queued_intents,
        workflow_stage: workflow.as_ref().map(|item| item.stage.clone()),
        active_candidate_id: workflow.as_ref().and_then(|item| item.candidate_id.clone()),
        active_candidate_kind: candidate.as_ref().map(|item| item.kind.clone()),
        active_candidate_author: candidate
            .as_ref()
            .map(|item| format!("{}/{}", item.author_provider, item.author_model)),
        active_judges: workflow
            .as_ref()
            .map_or_else(Vec::new, |item| item.judge_routes.clone()),
        current_decision,
        replay_completed: workflow.as_ref().map_or(0, |item| item.replay_completed),
        replay_total: workflow.as_ref().map_or(0, |item| item.replay_total),
        cooldown_until: workflow.as_ref().and_then(|item| item.cooldown_until.clone()),
        failure_fingerprint_count: workflow
            .as_ref()
            .map_or(0, |item| item.failure_fingerprint_count),
        workflow_attempt: workflow.as_ref().map_or(0, |item| item.attempt),
        shared_core_revision: state.shared_core_revision,
        active_skill_revisions,
        active_recipe_revisions,
        provider_overlay_revisions: state.provider_overlay_revisions,
        components: state.components,
        known_good_pair: state.known_good_pair,
        trace_count,
        objective_count,
        deployment_count,
        usage: state.usage,
        privacy_warnings: vec![
            "Only explicitly enrolled ca profiles contribute sanitized session evidence."
                .to_string(),
            "Credentials, environment values, canonical workspace paths, and unknown tool arguments are excluded."
                .to_string(),
        ],
    })
}

pub fn unenrolled_status() -> Result<SelfImprovementStatusV1> {
    let mut state = SelfImprovementStateV1::blank();
    state.refresh_revision()?;
    Ok(SelfImprovementStatusV1 {
        schema: "codex-team.self_improvement_status.v1".to_string(),
        enrolled: false,
        running_allowed: false,
        paused: false,
        pause: None,
        revision: state.revision,
        vault: VaultHealthV1 {
            schema: "codex-team.self_improvement_vault_health.v1".to_string(),
            healthy: false,
            root: String::new(),
            free_bytes: 0,
            total_bytes: 0,
            minimum_free_bytes: 0,
            issues: vec!["self-improvement is not enrolled".to_string()],
        },
        active_objective_id: None,
        ca_evidence_profiles: Vec::new(),
        ca_execution_profile: None,
        bridge_healthy: false,
        queued_learning_intents: 0,
        workflow_stage: None,
        active_candidate_id: None,
        active_candidate_kind: None,
        active_candidate_author: None,
        active_judges: Vec::new(),
        current_decision: None,
        replay_completed: 0,
        replay_total: 0,
        cooldown_until: None,
        failure_fingerprint_count: 0,
        workflow_attempt: 0,
        shared_core_revision: None,
        active_skill_revisions: Vec::new(),
        active_recipe_revisions: Vec::new(),
        provider_overlay_revisions: BTreeMap::new(),
        components: BTreeMap::new(),
        known_good_pair: None,
        trace_count: 0,
        objective_count: 0,
        deployment_count: 0,
        usage: state.usage,
        privacy_warnings: Vec::new(),
    })
}

pub fn doctor(vault: &LearningVault) -> Result<SelfImprovementDoctorV1> {
    let health = vault.health()?;
    let state = load_state(vault)?.unwrap_or_else(SelfImprovementStateV1::blank);
    let mut issues = health.issues.clone();
    if state.enrolled {
        if let Err(error) = crate::verification::isolation::require_isolation_backend() {
            issues.push(format!(
                "required verification isolation is unavailable: {error:#}"
            ));
        }
    }
    if vault
        .read_mutable::<ActivationTransactionV1>("supervisor/activation-transaction.json")?
        .is_some()
    {
        issues.push("an interrupted component activation requires operator review".to_string());
    }
    let transaction_clear = vault
        .read_mutable::<BridgeEnrollmentTransactionV1>(
            "supervisor/bridge-enrollment-transaction.json",
        )?
        .is_none();
    if !transaction_clear {
        issues.push("an interrupted ca learning-profile enrollment requires recovery".to_string());
    }
    let (contract_healthy, queued_learning_intents) = if state.enrolled && transaction_clear {
        match inspect_active_ca_learning_contract(vault, &state) {
            Ok(queued) => (true, queued),
            Err(error) => {
                issues.push(format!(
                    "ca learning bridge or execution route drifted: {error:#}"
                ));
                (false, 0)
            }
        }
    } else {
        (false, 0)
    };
    let bridge_healthy = transaction_clear && contract_healthy;
    let manifest = read_safety_manifest(vault).ok();
    let installed_supervisor = vault.resolve_relative("supervisor/bin/codex-team-supervisor")?;
    let supervisor_binary_matches_manifest = manifest
        .as_ref()
        .and_then(|manifest| {
            file_sha256(&installed_supervisor)
                .ok()
                .map(|hash| hash == manifest.supervisor_sha256)
        })
        .unwrap_or(false);
    if state.enrolled && !supervisor_binary_matches_manifest {
        issues.push("stable supervisor does not match the immutable safety manifest".to_string());
    }
    let known_good_pair_available = state
        .known_good_pair
        .as_ref()
        .map(|pair| pair_files_available(vault, pair))
        .transpose()?
        .unwrap_or(false);
    if state.enrolled && !known_good_pair_available {
        issues.push("a complete compatible known-good binary pair is unavailable".to_string());
    }
    if state.enrolled {
        for component in ["ca", "codex-team"] {
            match vault
                .read_mutable::<ActivePointerV1>(&format!("supervisor/pointers/{component}.json"))?
            {
                Some(pointer) if active_pointer_matches(component, &pointer, &state) => {}
                _ => issues.push(format!(
                    "{component} launcher pointer does not match supervisor state"
                )),
            }
        }
    }
    Ok(SelfImprovementDoctorV1 {
        schema: "codex-team.self_improvement_doctor.v1".to_string(),
        healthy: state.enrolled && issues.is_empty(),
        enrolled: state.enrolled,
        state_revision: state.revision,
        issues,
        vault: health,
        supervisor_binary_matches_manifest,
        known_good_pair_available,
        bridge_healthy,
        queued_learning_intents,
    })
}

pub fn unenrolled_doctor() -> Result<SelfImprovementDoctorV1> {
    let status = unenrolled_status()?;
    Ok(SelfImprovementDoctorV1 {
        schema: "codex-team.self_improvement_doctor.v1".to_string(),
        healthy: false,
        enrolled: false,
        state_revision: status.revision,
        issues: vec!["self-improvement is not enrolled".to_string()],
        vault: status.vault,
        supervisor_binary_matches_manifest: false,
        known_good_pair_available: false,
        bridge_healthy: false,
        queued_learning_intents: 0,
    })
}

pub fn pause(vault: &LearningVault, reason: &str) -> Result<MutationReceiptV1> {
    if reason.trim().is_empty() || reason.chars().count() > 512 {
        bail!("pause reason must contain 1 to 512 characters");
    }
    mutate_state(vault, "pause", |state| {
        state.pause = Some(PauseStateV1 {
            reason: reason.trim().to_string(),
            source: "human".to_string(),
            paused_at: Utc::now().to_rfc3339(),
        });
        Ok("operator pause recorded; no new objective may start".to_string())
    })
}

pub fn resume(vault: &LearningVault, expected_revision: &str) -> Result<MutationReceiptV1> {
    mutate_state(vault, "resume", |state| {
        if state.revision != expected_revision {
            bail!("stale self-improvement revision; no state changed");
        }
        state.pause = None;
        Ok("operator pause cleared; health gates still apply".to_string())
    })
}

pub fn rollback(vault: &LearningVault, component: ComponentV1) -> Result<MutationReceiptV1> {
    rollback_with_revision(vault, component, None)
}

pub fn record_recipe_avoidance_if_enrolled(
    ai_calls_avoided: u64,
    estimated_tokens_avoided: u64,
) -> Result<()> {
    let Some(vault) = LearningVault::existing_machine()? else {
        return Ok(());
    };
    let _lock = vault.acquire_lock()?;
    let Some(mut state) = load_state(&vault)? else {
        return Ok(());
    };
    if !state.enrolled || state.pause.is_some() {
        return Ok(());
    }
    state.usage.ai_calls_avoided = state
        .usage
        .ai_calls_avoided
        .saturating_add(ai_calls_avoided);
    state.usage.estimated_tokens_avoided = state
        .usage
        .estimated_tokens_avoided
        .saturating_add(estimated_tokens_avoided);
    state.state_sequence = state.state_sequence.saturating_add(1);
    state.refresh_revision()?;
    save_state(&vault, &state)
}

pub fn rollback_with_revision(
    vault: &LearningVault,
    component: ComponentV1,
    expected_revision: Option<&str>,
) -> Result<MutationReceiptV1> {
    let key = component_key(&component);
    let receipt = mutate_state(vault, "rollback", |state| {
        if let Some(expected_revision) = expected_revision {
            if state.revision != expected_revision {
                bail!("stale self-improvement revision; no state changed");
            }
        }
        let (restored_slot, restored_hash) = {
            let slots = state
                .components
                .get_mut(key)
                .context("component has no installed slots")?;
            let previous = slots
                .previous
                .clone()
                .context("component has no previous verified slot")?;
            let record = slots
                .slots
                .get(&previous)
                .and_then(Option::as_ref)
                .context("previous component slot is empty")?;
            if !record.verified {
                bail!("previous component slot is not verified");
            }
            let restored_hash = record.binary_sha256.clone();
            let current = std::mem::replace(&mut slots.active, previous.clone());
            slots.previous = Some(current);
            (previous, restored_hash)
        };
        let known_good = state
            .known_good_pair
            .as_mut()
            .context("known-good pair is unavailable")?;
        match &component {
            ComponentV1::Ca => {
                known_good.ca_slot = restored_slot.clone();
                known_good.ca_sha256 = restored_hash;
            }
            ComponentV1::CodexTeam => {
                known_good.codex_team_slot = restored_slot.clone();
                known_good.codex_team_sha256 = restored_hash;
            }
        }
        known_good.protocol_matrix_digest = canonical_digest(&serde_json::json!({
            "ca": known_good.ca_sha256,
            "codex_team": known_good.codex_team_sha256,
            "disposition": "operator_rollback"
        }))?;
        known_good.verified_at = Utc::now().to_rfc3339();
        state.usage.rollbacks = state.usage.rollbacks.saturating_add(1);
        state.pause = Some(PauseStateV1 {
            reason: format!("operator rolled back {key}; review is required before resuming"),
            source: "human_rollback".to_string(),
            paused_at: Utc::now().to_rfc3339(),
        });
        Ok(format!("restored {key} to slot {restored_slot}"))
    })?;
    let state = load_state(vault)?.context("self-improvement state disappeared after rollback")?;
    write_active_pointer(vault, key, &state)?;
    Ok(receipt)
}

/// Health-check and activate a content-addressed component in the inactive A/B
/// slot. The supplied health function owns the complete fake/real protocol
/// matrix and returns its immutable evidence digest. A failed health function
/// leaves the active slot and state unchanged.
pub fn activate_candidate_with_health<F>(
    vault: &LearningVault,
    candidate: &mut super::DeploymentCandidateV1,
    binary: &Path,
    health: F,
) -> Result<DeploymentReceiptV1>
where
    F: FnOnce(&Path, &Path) -> Result<String>,
{
    let _lock = vault.acquire_lock()?;
    let mut state = load_state(vault)?.context("self-improvement is not enrolled")?;
    if state.pause.is_some() || !state.enrolled {
        bail!("component activation is unavailable while self-improvement is paused or disabled");
    }
    if !vault.health()?.healthy {
        bail!("component activation requires a healthy learning vault");
    }
    inspect_executable(binary)?;
    let binary_sha256 = file_sha256(binary)?;
    if candidate.binary_sha256 != binary_sha256 {
        bail!("deployment candidate binary digest does not match the executable");
    }
    let component = component_key(&candidate.component);
    let slots = state
        .components
        .get(component)
        .context("component A/B slots are missing")?;
    let previous_slot = slots.active.clone();
    let active_slot = if previous_slot == "a" { "b" } else { "a" }.to_string();
    if candidate.inactive_slot != active_slot {
        bail!("deployment candidate is stale for the current inactive slot");
    }
    let peer = if component == "ca" {
        "codex-team"
    } else {
        "ca"
    };
    let peer_slots = state
        .components
        .get(peer)
        .context("known-good peer slots are missing")?;
    let peer_binary = vault.resolve_relative(&format!(
        "supervisor/slots/{peer}/{}/binary",
        peer_slots.active
    ))?;
    inspect_executable(&peer_binary)?;
    let transaction = ActivationTransactionV1 {
        schema: "codex-team.activation_transaction.v1".to_string(),
        component: component.to_string(),
        candidate_id: candidate.candidate_id.clone(),
        from_slot: previous_slot.clone(),
        to_slot: active_slot.clone(),
        started_at: Utc::now().to_rfc3339(),
    };
    vault.write_mutable("supervisor/activation-transaction.json", &transaction)?;
    let matrix_digest = match health(binary, &peer_binary) {
        Ok(digest) if digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()) => {
            digest.to_ascii_lowercase()
        }
        Ok(_) => {
            vault.remove_mutable("supervisor/activation-transaction.json")?;
            bail!("component health matrix returned an invalid evidence digest");
        }
        Err(error) => {
            vault.remove_mutable("supervisor/activation-transaction.json")?;
            return Err(error).context("component candidate failed the compatibility matrix");
        }
    };
    if candidate.protocol_matrix_digest != matrix_digest {
        vault.remove_mutable("supervisor/activation-transaction.json")?;
        bail!("component candidate is not bound to the observed protocol matrix");
    }
    let destination = vault.resolve_relative(&format!(
        "supervisor/slots/{component}/{active_slot}/binary"
    ))?;
    copy_replace_private_executable(binary, &destination)?;
    candidate.schema = super::DEPLOYMENT_CANDIDATE_SCHEMA.to_string();
    vault.write_immutable(candidate)?;
    let slot = BinarySlotV1 {
        slot: active_slot.clone(),
        binary_sha256: binary_sha256.clone(),
        source_commit: candidate.source_commit.clone(),
        verified: true,
        activated_at: Utc::now().to_rfc3339(),
    };
    let component_slots = state
        .components
        .get_mut(component)
        .expect("component slots checked above");
    component_slots.previous = Some(previous_slot.clone());
    component_slots.active = active_slot.clone();
    component_slots
        .slots
        .insert(active_slot.clone(), Some(slot));
    let known_good = state
        .known_good_pair
        .as_mut()
        .context("known-good pair is unavailable")?;
    match &candidate.component {
        ComponentV1::Ca => {
            known_good.ca_slot = active_slot.clone();
            known_good.ca_sha256 = binary_sha256.clone();
        }
        ComponentV1::CodexTeam => {
            known_good.codex_team_slot = active_slot.clone();
            known_good.codex_team_sha256 = binary_sha256.clone();
        }
    }
    known_good.protocol_matrix_digest = matrix_digest.clone();
    known_good.verified_at = Utc::now().to_rfc3339();
    state.usage.promotions = state.usage.promotions.saturating_add(1);
    state.state_sequence = state.state_sequence.saturating_add(1);
    state.refresh_revision()?;
    save_state(vault, &state)?;
    write_active_pointer(vault, component, &state)?;
    let pair_digest = canonical_digest(
        state
            .known_good_pair
            .as_ref()
            .expect("known-good pair retained"),
    )?;
    let created_at = Utc::now().to_rfc3339();
    let deployment_id = format!(
        "deployment-{}",
        &format!(
            "{:x}",
            Sha256::digest(
                format!("{}\0{component}\0{created_at}", candidate.candidate_id).as_bytes()
            )
        )[..32]
    );
    let mut receipt = DeploymentReceiptV1 {
        schema: super::DEPLOYMENT_RECEIPT_SCHEMA.to_string(),
        deployment_id,
        candidate_id: candidate.candidate_id.clone(),
        component: candidate.component.clone(),
        previous_slot,
        active_slot,
        binary_sha256,
        source_commit: candidate.source_commit.clone(),
        health_evidence_ids: vec![matrix_digest],
        known_good_pair_digest: pair_digest,
        disposition: "activated".to_string(),
        created_at,
        digest: String::new(),
    };
    vault.write_immutable(&mut receipt)?;
    vault.remove_mutable("supervisor/activation-transaction.json")?;
    Ok(receipt)
}

pub fn supervisor_tick(vault: &LearningVault) -> Result<SelfImprovementDoctorV1> {
    let _lock = vault.acquire_lock()?;
    let mut state = load_state(vault)?.context("self-improvement is not enrolled")?;
    for repository in &mut state.repositories {
        repository.path = None;
    }
    let doctor = doctor(vault)?;
    state.ca_bridge_healthy = doctor.bridge_healthy;
    state.ca_bridge_queued_intents = doctor.queued_learning_intents;
    state.last_health_at = Some(Utc::now().to_rfc3339());
    if !doctor.healthy && state.pause.is_none() {
        state.pause = Some(PauseStateV1 {
            reason: doctor.issues.join("; "),
            source: "safety_kernel".to_string(),
            paused_at: Utc::now().to_rfc3339(),
        });
    }
    state.state_sequence = state.state_sequence.saturating_add(1);
    state.refresh_revision()?;
    save_state(vault, &state)?;
    Ok(doctor)
}

pub fn run_supervisor(vault: LearningVault, once: bool) -> Result<()> {
    loop {
        // ca owns evidence harvesting, occurrence admission and worker launch.
        // The stable service retains health/watchdog/rollback responsibility.
        // A competing authorized transaction may temporarily own the vault.
        // Keep the stable watcher alive and try health again on the next tick.
        let _ = supervisor_tick(&vault);
        if once {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(30));
    }
}

pub fn list_objectives(
    vault: &LearningVault,
    id: Option<&str>,
) -> Result<Vec<ImprovementObjectiveV1>> {
    if let Some(id) = id {
        return Ok(vec![vault.read_immutable("objectives", id)?]);
    }
    vault.list_immutable("objectives")
}

pub fn list_traces(vault: &LearningVault, id: Option<&str>) -> Result<Vec<LearningTraceV1>> {
    if let Some(id) = id {
        return Ok(vec![vault.read_immutable("traces", id)?]);
    }
    vault.list_immutable("traces")
}

pub fn list_deployments(vault: &LearningVault) -> Result<Vec<DeploymentReceiptV1>> {
    vault.list_immutable("deployments/receipts")
}

pub fn promote_shared_artifact(
    vault: &LearningVault,
    content: &mut super::ArtifactContentV1,
    artifact: &mut super::ArtifactRevisionV1,
) -> Result<MutationReceiptV1> {
    let _lock = vault.acquire_lock()?;
    let mut state = load_state(vault)?.context("self-improvement is not enrolled")?;
    ensure_promotion_available(vault, &state)?;
    validate_promoted_artifact(vault, content, artifact)?;
    if matches!(artifact.kind, super::ArtifactKindV1::Source) {
        bail!("source revisions must pass the independent binary deployment path");
    }
    if state.shared_core_revision.as_deref() == Some(artifact.revision.as_str()) {
        let existing =
            vault.read_immutable::<super::ArtifactRevisionV1>("artifacts", &artifact.revision)?;
        if existing.content_tree_sha256 != artifact.content_tree_sha256
            || existing.evaluation_receipt_id != artifact.evaluation_receipt_id
        {
            bail!("active shared artifact does not match the recovered promotion");
        }
        return Ok(MutationReceiptV1 {
            schema: "codex-team.self_improvement_mutation.v1".to_string(),
            action: "promote_shared_artifact".to_string(),
            changed: false,
            revision: state.revision,
            detail: format!("artifact {} was already active", artifact.revision),
        });
    }
    vault.write_immutable(content)?;
    vault.write_immutable(artifact)?;
    state.shared_core_revision = Some(artifact.revision.clone());
    state.usage.promotions = state.usage.promotions.saturating_add(1);
    state.state_sequence = state.state_sequence.saturating_add(1);
    state.refresh_revision()?;
    save_state(vault, &state)?;
    Ok(MutationReceiptV1 {
        schema: "codex-team.self_improvement_mutation.v1".to_string(),
        action: "promote_shared_artifact".to_string(),
        changed: true,
        revision: state.revision,
        detail: format!(
            "activated immutable shared-core artifact {}",
            artifact.revision
        ),
    })
}

pub fn promote_provider_overlay(
    vault: &LearningVault,
    overlay: &mut super::ProviderOverlayV1,
) -> Result<MutationReceiptV1> {
    let _lock = vault.acquire_lock()?;
    let mut state = load_state(vault)?.context("self-improvement is not enrolled")?;
    ensure_promotion_available(vault, &state)?;
    let receipt = vault.read_immutable::<super::EvaluationReceiptV1>(
        "evaluations/receipts",
        &overlay.evaluation_receipt_id,
    )?;
    if !receipt.promoted {
        bail!("provider overlay evaluation was not promoted");
    }
    let artifact = vault.read_immutable::<super::ArtifactRevisionV1>(
        "artifacts",
        &overlay.prompt_framing_revision,
    )?;
    if artifact.evaluation_receipt_id != overlay.evaluation_receipt_id || artifact.retired {
        bail!("provider overlay prompt framing is not the promoted active revision");
    }
    super::validate_common_identifier(&overlay.overlay_id, "overlay id")?;
    if overlay.provider.trim().is_empty() || overlay.model.trim().is_empty() {
        bail!("provider overlay route must be nonempty");
    }
    let route_key = format!("{}/{}", overlay.provider, overlay.model);
    if state.provider_overlay_revisions.get(&route_key) == Some(&overlay.overlay_id) {
        let existing =
            vault.read_immutable::<super::ProviderOverlayV1>("overlays", &overlay.overlay_id)?;
        if existing.prompt_framing_revision != overlay.prompt_framing_revision
            || existing.evaluation_receipt_id != overlay.evaluation_receipt_id
        {
            bail!("active provider overlay does not match the recovered promotion");
        }
        return Ok(MutationReceiptV1 {
            schema: "codex-team.self_improvement_mutation.v1".to_string(),
            action: "promote_provider_overlay".to_string(),
            changed: false,
            revision: state.revision,
            detail: format!("overlay {} was already active", overlay.overlay_id),
        });
    }
    vault.write_immutable(overlay)?;
    state
        .provider_overlay_revisions
        .insert(route_key, overlay.overlay_id.clone());
    state.usage.promotions = state.usage.promotions.saturating_add(1);
    state.state_sequence = state.state_sequence.saturating_add(1);
    state.refresh_revision()?;
    save_state(vault, &state)?;
    Ok(MutationReceiptV1 {
        schema: "codex-team.self_improvement_mutation.v1".to_string(),
        action: "promote_provider_overlay".to_string(),
        changed: true,
        revision: state.revision,
        detail: format!(
            "activated immutable overlay {} for {}/{}",
            overlay.overlay_id, overlay.provider, overlay.model
        ),
    })
}

pub fn learned_prompt_context_if_enrolled(
    provider: &str,
    model: &str,
    role: &str,
) -> Result<Option<LearnedPromptContextV1>> {
    let Some(vault) = LearningVault::existing_machine()? else {
        return Ok(None);
    };
    learned_prompt_context(&vault, provider, model, role)
}

pub fn learned_prompt_context(
    vault: &LearningVault,
    provider: &str,
    model: &str,
    role: &str,
) -> Result<Option<LearnedPromptContextV1>> {
    let Some(state) = load_state(vault)? else {
        return Ok(None);
    };
    if !state.enrolled || state.pause.is_some() || !vault.health()?.healthy {
        return Ok(None);
    }
    let mut fragments = Vec::new();
    let mut core_revision = None;
    if let Some(revision) = state.shared_core_revision.as_deref() {
        if let Some(fragment) = render_artifact_for_role(vault, revision, role)? {
            fragments.push(format!(
                "<learned-shared-core revision=\"{revision}\">\n{fragment}\n</learned-shared-core>"
            ));
            core_revision = Some(revision.to_string());
        }
    }
    let route = format!("{provider}/{model}");
    let mut overlay_revision = None;
    if let Some(overlay_id) = state.provider_overlay_revisions.get(&route) {
        let overlay = vault.read_immutable::<super::ProviderOverlayV1>("overlays", overlay_id)?;
        if overlay.provider != provider || overlay.model != model {
            bail!("active provider overlay route does not match its state key");
        }
        if overlay.suitable_roles.is_empty()
            || overlay.suitable_roles.iter().any(|item| item == role)
        {
            if let Some(fragment) =
                render_artifact_for_role(vault, &overlay.prompt_framing_revision, role)?
            {
                fragments.push(format!(
                    "<learned-provider-overlay revision=\"{}\">\n{}\n</learned-provider-overlay>",
                    overlay.overlay_id, fragment
                ));
                overlay_revision = Some(overlay.overlay_id);
            }
        }
    }
    if fragments.is_empty() {
        return Ok(None);
    }
    Ok(Some(LearnedPromptContextV1 {
        prompt_fragment: fragments.join("\n\n"),
        core_revision,
        overlay_revision,
    }))
}

fn ensure_promotion_available(vault: &LearningVault, state: &SelfImprovementStateV1) -> Result<()> {
    if !state.enrolled || state.pause.is_some() {
        bail!("artifact promotion is unavailable while self-improvement is disabled or paused");
    }
    if !doctor(vault)?.healthy {
        bail!("artifact promotion requires a healthy stable supervisor");
    }
    Ok(())
}

fn validate_promoted_artifact(
    vault: &LearningVault,
    content: &super::ArtifactContentV1,
    artifact: &super::ArtifactRevisionV1,
) -> Result<()> {
    let tree_digest = super::validate_artifact_content(content)?;
    if artifact.schema != super::ARTIFACT_SCHEMA
        || artifact.content_tree_sha256 != tree_digest
        || artifact.retired
    {
        bail!("artifact revision is not bound to the supplied active content tree");
    }
    let receipt = vault.read_immutable::<super::EvaluationReceiptV1>(
        "evaluations/receipts",
        &artifact.evaluation_receipt_id,
    )?;
    let plan =
        vault.read_immutable::<super::EvaluationPlanV1>("evaluations/plans", &receipt.plan_id)?;
    if !receipt.promoted
        || receipt.plan_digest != plan.digest
        || artifact.objective_id != plan.objective_id
    {
        bail!("artifact is not bound to a promoted frozen evaluation");
    }
    Ok(())
}

fn render_artifact_for_role(
    vault: &LearningVault,
    revision: &str,
    role: &str,
) -> Result<Option<String>> {
    let artifact = vault.read_immutable::<super::ArtifactRevisionV1>("artifacts", revision)?;
    if artifact.retired
        || (!artifact.applicability.is_empty()
            && !artifact
                .applicability
                .iter()
                .any(|item| item == "*" || item == role || item == &format!("role:{role}")))
    {
        return Ok(None);
    }
    let content = vault.read_immutable::<super::ArtifactContentV1>(
        "artifacts/content",
        &artifact.content_tree_sha256,
    )?;
    let rendered = super::render_injected_material(&content)?;
    Ok((!rendered.is_empty()).then_some(rendered))
}

#[allow(clippy::too_many_arguments)]
pub fn record_role_trace_if_enrolled(
    provider: &str,
    model: &str,
    role: &str,
    prompt: &str,
    response: &str,
    task_id: Option<&str>,
    repository_identity: &str,
    started_at: &str,
    finished_at: &str,
    elapsed_millis: u64,
    input_tokens: u64,
    output_tokens: u64,
    outcome: &str,
) -> Result<Option<PathBuf>> {
    let Some(vault) = LearningVault::existing_machine()? else {
        return Ok(None);
    };
    let Some(state) = load_state(&vault)? else {
        return Ok(None);
    };
    if !state.enrolled || state.pause.is_some() || !vault.health()?.healthy {
        return Ok(None);
    }
    let created_at = Utc::now().to_rfc3339();
    let identity_material = format!(
        "{provider}\0{model}\0{role}\0{}\0{repository_identity}\0{started_at}\0{finished_at}",
        task_id.unwrap_or("none")
    );
    let trace_id = format!(
        "trace-{}",
        &format!("{:x}", Sha256::digest(identity_material.as_bytes()))[..32]
    );
    let overlay_key = format!("{provider}/{model}");
    let mut trace = LearningTraceV1 {
        schema: super::TRACE_SCHEMA.to_string(),
        trace_id,
        provider: provider.chars().take(80).collect(),
        model: model.chars().take(160).collect(),
        role: role.chars().take(80).collect(),
        prompt: super::sanitize_trace_text(prompt),
        response: super::sanitize_trace_text(response),
        tool_events: Vec::new(),
        task_identity: format!("{:x}", Sha256::digest(task_id.unwrap_or("none").as_bytes())),
        repository_fingerprint: format!("{:x}", Sha256::digest(repository_identity.as_bytes())),
        acceptance_evidence_ids: Vec::new(),
        timing: super::TimingV1 {
            started_at: started_at.to_string(),
            finished_at: finished_at.to_string(),
            elapsed_millis,
        },
        usage: super::UsageV1 {
            input_tokens,
            output_tokens,
            estimated_microusd: 0,
        },
        outcome: outcome.to_string(),
        core_revision: state
            .shared_core_revision
            .unwrap_or_else(|| format!("builtin-core-{}", env!("CARGO_PKG_VERSION"))),
        overlay_revision: state
            .provider_overlay_revisions
            .get(&overlay_key)
            .cloned()
            .unwrap_or_else(|| "none".to_string()),
        created_at,
        digest: String::new(),
    };
    let path = vault.resolve_relative(&format!("traces/{}.json", trace.trace_id))?;
    let already_recorded = path.exists();
    let path = vault.write_immutable(&mut trace)?;
    if !already_recorded {
        let _lock = vault.acquire_lock()?;
        if let Some(mut current) = load_state(&vault)? {
            current.usage.provider_calls = current.usage.provider_calls.saturating_add(1);
            current.usage.input_tokens = current.usage.input_tokens.saturating_add(input_tokens);
            current.usage.output_tokens = current.usage.output_tokens.saturating_add(output_tokens);
            current.state_sequence = current.state_sequence.saturating_add(1);
            current.refresh_revision()?;
            save_state(&vault, &current)?;
        }
    }
    Ok(Some(path))
}

pub(super) fn discover_one_trace_objective(vault: &LearningVault) -> Result<bool> {
    let _lock = vault.acquire_lock()?;
    let mut state = load_state(vault)?.context("self-improvement is not enrolled")?;
    if state.pause.is_some() || state.active_objective_id.is_some() {
        return Ok(false);
    }
    let before = state.active_objective_id.clone();
    discover_repeated_failure_objective(vault, &mut state)?;
    let changed = state.active_objective_id != before;
    if changed {
        state.state_sequence = state.state_sequence.saturating_add(1);
        state.refresh_revision()?;
        save_state(vault, &state)?;
    }
    Ok(changed)
}

fn discover_repeated_failure_objective(
    vault: &LearningVault,
    state: &mut SelfImprovementStateV1,
) -> Result<()> {
    let traces = vault.list_immutable::<LearningTraceV1>("traces")?;
    let existing = vault.list_immutable::<ImprovementObjectiveV1>("objectives")?;
    let existing_evidence = existing
        .iter()
        .flat_map(|objective| objective.evidence_ids.iter())
        .collect::<std::collections::BTreeSet<_>>();
    let mut role_failures: BTreeMap<String, Vec<&LearningTraceV1>> = BTreeMap::new();
    for trace in traces.iter().filter(|trace| trace.outcome == "failed") {
        let key = format!("{}\0{}", trace.role, trace.repository_fingerprint);
        role_failures.entry(key).or_default().push(trace);
    }
    if let Some((key, evidence)) = role_failures.into_iter().find(|(_, evidence)| {
        evidence.len() >= 2
            && evidence
                .iter()
                .map(|trace| trace.provider.as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                >= 2
            && evidence
                .iter()
                .any(|trace| !existing_evidence.contains(&trace.trace_id))
    }) {
        let digest = format!("{:x}", Sha256::digest(key.as_bytes()));
        let objective_id = format!("objective-role-{}", &digest[..24]);
        let first = evidence[0];
        let mut objective = ImprovementObjectiveV1 {
            schema: super::OBJECTIVE_SCHEMA.to_string(),
            objective_id: objective_id.clone(),
            source: super::ObjectiveSourceV1::RepeatedFailure,
            summary: format!(
                "Improve repeated {} role failures across configured providers without weakening deterministic acceptance",
                first.role
            ),
            evidence_ids: evidence
                .iter()
                .take(32)
                .map(|trace| trace.trace_id.clone())
                .collect(),
            improvement_class: "role_prompt".to_string(),
            frozen_evaluation_plan_id: String::new(),
            repository_fingerprint: Some(first.repository_fingerprint.clone()),
            state: "awaiting_frozen_evaluation".to_string(),
            cooldown_until: None,
            created_at: Utc::now().to_rfc3339(),
            digest: String::new(),
        };
        vault.write_immutable(&mut objective)?;
        if state.active_objective_id.is_none() {
            state.active_objective_id = Some(objective_id);
        }
        return Ok(());
    }
    let mut failures: BTreeMap<String, Vec<&LearningTraceV1>> = BTreeMap::new();
    for trace in traces.iter().filter(|trace| trace.outcome == "failed") {
        let key = format!(
            "{}\0{}\0{}\0{}",
            trace.provider, trace.model, trace.role, trace.repository_fingerprint
        );
        failures.entry(key).or_default().push(trace);
    }
    let Some((key, evidence)) = failures.into_iter().find(|(_, evidence)| {
        evidence.len() >= 2
            && evidence
                .iter()
                .any(|trace| !existing_evidence.contains(&trace.trace_id))
    }) else {
        return Ok(());
    };
    let digest = format!("{:x}", Sha256::digest(key.as_bytes()));
    let objective_id = format!("objective-failure-{}", &digest[..24]);
    let first = evidence[0];
    let mut objective = ImprovementObjectiveV1 {
        schema: super::OBJECTIVE_SCHEMA.to_string(),
        objective_id: objective_id.clone(),
        source: super::ObjectiveSourceV1::RepeatedFailure,
        summary: format!(
            "Improve repeated {} failures for {}/{} without weakening deterministic acceptance",
            first.role, first.provider, first.model
        ),
        evidence_ids: evidence
            .iter()
            .take(32)
            .map(|trace| trace.trace_id.clone())
            .collect(),
        improvement_class: "provider_overlay".to_string(),
        // The evaluator freezes cases immediately before candidate generation;
        // objective discovery itself does not authorize a candidate.
        frozen_evaluation_plan_id: String::new(),
        repository_fingerprint: Some(first.repository_fingerprint.clone()),
        state: "awaiting_frozen_evaluation".to_string(),
        cooldown_until: None,
        created_at: Utc::now().to_rfc3339(),
        digest: String::new(),
    };
    vault.write_immutable(&mut objective)?;
    if state.active_objective_id.is_none() {
        state.active_objective_id = Some(objective_id);
    }
    Ok(())
}

fn mutate_state<F>(vault: &LearningVault, action: &str, mutation: F) -> Result<MutationReceiptV1>
where
    F: FnOnce(&mut SelfImprovementStateV1) -> Result<String>,
{
    let _lock = vault.acquire_lock()?;
    let mut state = load_state(vault)?.context("self-improvement is not enrolled")?;
    if !state.enrolled {
        bail!("self-improvement is not enrolled");
    }
    let detail = mutation(&mut state)?;
    state.state_sequence = state.state_sequence.saturating_add(1);
    state.refresh_revision()?;
    save_state(vault, &state)?;
    Ok(MutationReceiptV1 {
        schema: "codex-team.self_improvement_mutation.v1".to_string(),
        action: action.to_string(),
        changed: true,
        revision: state.revision,
        detail,
    })
}

pub(super) fn load_state(vault: &LearningVault) -> Result<Option<SelfImprovementStateV1>> {
    let state = vault.read_mutable::<SelfImprovementStateV1>("supervisor/state.json")?;
    if let Some(state) = &state {
        if state.schema != STATE_SCHEMA {
            bail!("unsupported self-improvement state schema");
        }
        state.verify_revision()?;
    }
    Ok(state)
}

pub(super) fn save_state(vault: &LearningVault, state: &SelfImprovementStateV1) -> Result<()> {
    state.verify_revision()?;
    vault.write_mutable("supervisor/state.json", state)?;
    Ok(())
}

fn inspect_repository(name: &str, path: &Path) -> Result<RepositoryEnrollmentV1> {
    let path = fs::canonicalize(path).with_context(|| format!("canonicalize {name} repository"))?;
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("{name} repository must be a real directory");
    }
    let head = git_output(&path, &["rev-parse", "HEAD"])?;
    let branch = git_output(&path, &["branch", "--show-current"])?;
    if branch.is_empty() {
        bail!("{name} repository must be on a named branch");
    }
    if !git_output(
        &path,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?
    .is_empty()
    {
        bail!("{name} repository must have a clean worktree and index");
    }
    Ok(RepositoryEnrollmentV1 {
        name: name.to_string(),
        path: None,
        path_fingerprint: format!("{:x}", Sha256::digest(path.as_os_str().as_encoded_bytes())),
        branch,
        head,
    })
}

fn git_output(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn inspect_executable(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect required executable {}", path.display()))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.permissions().mode() & 0o111 == 0
    {
        bail!(
            "required executable must be a regular non-symlink executable: {}",
            path.display()
        );
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaBridgePreflightWireV1 {
    schema: String,
    ready: bool,
    profile: String,
    state_digest: String,
    enrolled: bool,
    enrollment_digest: Option<String>,
    queued_intents: usize,
    issues: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaRoleRunPreflightWireV1 {
    schema: String,
    ready: bool,
    profile: String,
    sandbox_required: bool,
    routes: Vec<CaExecutionRouteV1>,
    issues: Vec<String>,
}

fn inspect_ca_evidence_profile(
    ca_binary: &Path,
    profile: &str,
) -> Result<CaEvidenceProfileEnrollmentV1> {
    validate_ca_evidence_profile_name(profile)?;
    let receipt: CaBridgePreflightWireV1 = run_bounded_json_command(
        ca_binary,
        &["learning-bridge", "preflight", "--profile", profile],
        "ca learning-bridge preflight",
    )?;
    if receipt.schema != "ca.learning_bridge_preflight.v1"
        || !receipt.ready
        || receipt.profile != profile
        || !receipt.issues.is_empty()
    {
        bail!("ca evidence profile {profile:?} is not ready for supervised learning");
    }
    if receipt.enrolled != receipt.enrollment_digest.is_some() {
        bail!("ca evidence profile {profile:?} reported inconsistent enrollment state");
    }
    let _queued_intents = receipt.queued_intents;
    validate_sha256(&receipt.state_digest, "ca learning state digest")?;
    Ok(CaEvidenceProfileEnrollmentV1 {
        name: profile.to_string(),
        profile_fingerprint: format!("{:x}", Sha256::digest(profile.as_bytes())),
        state_digest: receipt.state_digest,
    })
}

fn inspect_ca_execution_profile(
    ca_binary: &Path,
    profile: &str,
) -> Result<CaExecutionProfileEnrollmentV1> {
    validate_ca_profile_name(profile)?;
    let receipt: CaRoleRunPreflightWireV1 = run_bounded_json_command(
        ca_binary,
        &["role-run", "--profile", profile, "--preflight"],
        "ca role-run preflight",
    )?;
    if receipt.schema != "ca.role_run_preflight.v1"
        || !receipt.ready
        || receipt.profile != profile
        || !receipt.sandbox_required
        || !receipt.issues.is_empty()
        || receipt.routes.is_empty()
    {
        bail!("ca execution profile {profile:?} is not ready for isolated role execution");
    }
    for route in &receipt.routes {
        if route.provider.is_empty()
            || route.models.is_empty()
            || route.models.iter().any(String::is_empty)
            || !valid_provider_id(&route.provider)
        {
            bail!("ca execution profile reported an invalid provider/model route");
        }
    }
    Ok(CaExecutionProfileEnrollmentV1 {
        name: profile.to_string(),
        sandbox_required: true,
        routes: receipt.routes,
    })
}

fn inspect_active_ca_learning_contract(
    vault: &LearningVault,
    state: &SelfImprovementStateV1,
) -> Result<usize> {
    let ca_binary = active_component_binary(vault, "ca")?;
    let mut queued = 0_usize;
    for expected in &state.ca_evidence_profiles {
        let receipt: CaBridgePreflightWireV1 = run_bounded_json_command(
            &ca_binary,
            &["learning-bridge", "preflight", "--profile", &expected.name],
            "active ca learning-bridge preflight",
        )?;
        if receipt.schema != "ca.learning_bridge_preflight.v1"
            || !receipt.ready
            || receipt.profile != expected.name
            || !receipt.enrolled
            || receipt.enrollment_digest.as_deref() != Some(state.enrollment_digest.as_str())
            || !receipt.issues.is_empty()
            || expected.profile_fingerprint
                != format!("{:x}", Sha256::digest(expected.name.as_bytes()))
        {
            bail!(
                "evidence profile {:?} no longer matches enrollment",
                expected.name
            );
        }
        validate_sha256(&receipt.state_digest, "ca learning state digest")?;
        queued = queued.saturating_add(receipt.queued_intents);
    }
    let expected = state
        .ca_execution_profile
        .as_ref()
        .context("enrollment-v1 has no execution profile; exact re-enrollment is required")?;
    let mut current = inspect_ca_execution_profile(&ca_binary, &expected.name)?;
    let mut expected = expected.clone();
    normalize_execution_routes(&mut current);
    normalize_execution_routes(&mut expected);
    if current != expected {
        bail!("execution profile routes changed after exact enrollment");
    }
    Ok(queued)
}

fn normalize_execution_routes(profile: &mut CaExecutionProfileEnrollmentV1) {
    for route in &mut profile.routes {
        route.models.sort();
        route.models.dedup();
    }
    profile
        .routes
        .sort_by(|left, right| left.provider.cmp(&right.provider));
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaBridgeEnrollmentWireV1 {
    schema: String,
    profile: String,
    enrollment_digest: String,
    enrolled_at: String,
    digest: String,
}

fn enroll_ca_evidence_profiles(
    vault: &LearningVault,
    ca_binary: &Path,
    contract: &EnrollmentContractV1,
    enrollment_digest: &str,
) -> Result<()> {
    if contract.ca_evidence_profiles.is_empty() {
        return Ok(());
    }
    let transaction_path = "supervisor/bridge-enrollment-transaction.json";
    let mut transaction =
        match vault.read_mutable::<BridgeEnrollmentTransactionV1>(transaction_path)? {
            Some(existing) => {
                if existing.schema != "codex-team.bridge_enrollment_transaction.v1"
                    || existing.enrollment_digest != enrollment_digest
                    || existing.profiles != contract.ca_evidence_profiles
                {
                    bail!("a different ca profile enrollment transaction requires recovery");
                }
                existing
            }
            None => BridgeEnrollmentTransactionV1 {
                schema: "codex-team.bridge_enrollment_transaction.v1".to_string(),
                enrollment_digest: enrollment_digest.to_string(),
                profiles: contract.ca_evidence_profiles.clone(),
                completed_profiles: Vec::new(),
            },
        };
    vault.write_mutable(transaction_path, &transaction)?;
    for profile in &contract.ca_evidence_profiles {
        if transaction.completed_profiles.contains(&profile.name) {
            continue;
        }
        let receipt: CaBridgeEnrollmentWireV1 = run_bounded_json_command(
            ca_binary,
            &[
                "learning-bridge",
                "enroll",
                "--profile",
                &profile.name,
                "--enrollment-digest",
                enrollment_digest,
                "--expected-state-digest",
                &profile.state_digest,
            ],
            "ca learning-bridge enroll",
        )?;
        if receipt.schema != "ca.learning_bridge_enrollment.v1"
            || receipt.profile != profile.name
            || receipt.enrollment_digest != enrollment_digest
            || receipt.enrolled_at.is_empty()
        {
            bail!("ca learning-bridge returned an invalid enrollment receipt");
        }
        validate_sha256(&receipt.digest, "ca bridge enrollment receipt digest")?;
        transaction.completed_profiles.push(profile.name.clone());
        vault.write_mutable(transaction_path, &transaction)?;
    }
    Ok(())
}

fn validate_ca_profile_name(profile: &str) -> Result<()> {
    let valid = !matches!(profile, "default" | "bare")
        && !profile.is_empty()
        && profile.len() <= 64
        && profile.as_bytes()[0].is_ascii_lowercase()
        && profile.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        });
    if !valid {
        bail!("ca learning profiles must be nonreserved safe named profiles");
    }
    Ok(())
}

fn validate_ca_evidence_profile_name(profile: &str) -> Result<()> {
    if profile == "default" {
        return Ok(());
    }
    validate_ca_profile_name(profile)
}

fn valid_provider_id(provider: &str) -> bool {
    !provider.is_empty()
        && provider.len() <= 80
        && provider
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn validate_sha256(value: &str, label: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("{label} is not a lowercase SHA-256 digest");
    }
    Ok(())
}

fn run_bounded_json_command<T: for<'de> Deserialize<'de>>(
    binary: &Path,
    args: &[&str],
    label: &str,
) -> Result<T> {
    let output = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("launch {label}"))?;
    if !output.status.success() {
        bail!("{label} failed without changing enrollment state");
    }
    if output.stdout.len() > 2 * 1024 * 1024 || output.stderr.len() > 1024 * 1024 {
        bail!("{label} exceeded its output bound");
    }
    serde_json::from_slice(&output.stdout).with_context(|| format!("parse {label} receipt"))
}

fn file_sha256(path: &Path) -> Result<String> {
    inspect_executable(path)?;
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn copy_private_executable(source: &Path, destination: &Path) -> Result<()> {
    inspect_executable(source)?;
    if destination.exists() {
        if file_sha256(destination)? == file_sha256(source)? {
            return Ok(());
        }
        bail!("stable supervisor target already exists with different content");
    }
    let parent = destination
        .parent()
        .context("executable destination has no parent")?;
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temp = parent.join(format!(".binary.tmp-{}-{nonce}", std::process::id()));
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temp)?;
    std::io::copy(&mut input, &mut output)?;
    output.sync_all()?;
    drop(output);
    fs::rename(&temp, destination)?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o700))?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn copy_replace_private_executable(source: &Path, destination: &Path) -> Result<()> {
    inspect_executable(source)?;
    let parent = destination
        .parent()
        .context("slot executable destination has no parent")?;
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    if let Ok(metadata) = fs::symlink_metadata(destination) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("slot executable target must be a regular non-symlink file");
        }
    }
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temporary = parent.join(format!(".candidate.tmp-{}-{nonce}", std::process::id()));
    let result = (|| -> Result<()> {
        let mut input = File::open(source)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&temporary)?;
        std::io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        drop(output);
        fs::rename(&temporary, destination)?;
        fs::set_permissions(destination, fs::Permissions::from_mode(0o700))?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn seed_slot(
    vault: &LearningVault,
    component: &str,
    slot: &str,
    binary: &Path,
    source_commit: &str,
) -> Result<BinarySlotV1> {
    let destination =
        vault.resolve_relative(&format!("supervisor/slots/{component}/{slot}/binary"))?;
    copy_private_executable(binary, &destination)?;
    Ok(BinarySlotV1 {
        slot: slot.to_string(),
        binary_sha256: file_sha256(&destination)?,
        source_commit: source_commit.to_string(),
        verified: true,
        activated_at: Utc::now().to_rfc3339(),
    })
}

fn initial_component(slot: BinarySlotV1) -> ComponentSlotsV1 {
    ComponentSlotsV1 {
        active: "a".to_string(),
        previous: None,
        slots: [("a".to_string(), Some(slot)), ("b".to_string(), None)]
            .into_iter()
            .collect(),
    }
}

fn write_active_pointer(
    vault: &LearningVault,
    component: &str,
    state: &SelfImprovementStateV1,
) -> Result<()> {
    let slots = state
        .components
        .get(component)
        .context("component slots are missing")?;
    let slot = slots
        .slots
        .get(&slots.active)
        .and_then(Option::as_ref)
        .context("active component slot is empty")?;
    vault.write_mutable(
        &format!("supervisor/pointers/{component}.json"),
        &ActivePointerV1 {
            schema: "codex-team.active_pointer.v1".to_string(),
            component: component.to_string(),
            slot: slots.active.clone(),
            binary_sha256: slot.binary_sha256.clone(),
            state_sequence: state.state_sequence,
        },
    )?;
    Ok(())
}

fn active_pointer_matches(
    component: &str,
    pointer: &ActivePointerV1,
    state: &SelfImprovementStateV1,
) -> bool {
    let Some(slots) = state.components.get(component) else {
        return false;
    };
    let Some(slot) = slots.slots.get(&slots.active).and_then(Option::as_ref) else {
        return false;
    };
    pointer.schema == "codex-team.active_pointer.v1"
        && pointer.component == component
        && pointer.slot == slots.active
        && pointer.binary_sha256 == slot.binary_sha256
        && pointer.state_sequence <= state.state_sequence
}

pub fn active_component_binary(vault: &LearningVault, component: &str) -> Result<PathBuf> {
    if !matches!(component, "ca" | "codex-team") {
        bail!("unknown self-improvement component");
    }
    let state = load_state(vault)?.context("self-improvement is not enrolled")?;
    let pointer = vault
        .read_mutable::<ActivePointerV1>(&format!("supervisor/pointers/{component}.json"))?
        .context("component active pointer is missing")?;
    if !active_pointer_matches(component, &pointer, &state) {
        bail!("component active pointer does not match supervisor state");
    }
    let binary = vault.resolve_relative(&format!(
        "supervisor/slots/{component}/{}/binary",
        pointer.slot
    ))?;
    inspect_executable(&binary)?;
    if file_sha256(&binary)? != pointer.binary_sha256 {
        bail!("component active binary hash does not match its pointer");
    }
    Ok(binary)
}

fn manifest_digest(manifest: &SafetyKernelManifestV1) -> Result<String> {
    let mut unsigned = manifest.clone();
    unsigned.digest.clear();
    canonical_digest(&unsigned)
}

fn read_safety_manifest(vault: &LearningVault) -> Result<SafetyKernelManifestV1> {
    let manifest = vault
        .read_mutable::<SafetyKernelManifestV1>("supervisor/safety-kernel.json")?
        .context("safety kernel manifest is missing")?;
    if manifest.schema != SAFETY_KERNEL_SCHEMA || manifest_digest(&manifest)? != manifest.digest {
        bail!("safety kernel manifest is corrupt or tampered");
    }
    Ok(manifest)
}

fn write_immutable_private_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if path.exists() {
        let existing = fs::read(path)?;
        if existing == serde_json::to_vec_pretty(value)? {
            return Ok(());
        }
        bail!("immutable safety-kernel record already exists with different content");
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o400))?;
    File::open(path.parent().context("manifest path has no parent")?)?.sync_all()?;
    Ok(())
}

fn pair_files_available(vault: &LearningVault, pair: &KnownGoodPairV1) -> Result<bool> {
    for (component, slot, expected) in [
        ("ca", pair.ca_slot.as_str(), pair.ca_sha256.as_str()),
        (
            "codex-team",
            pair.codex_team_slot.as_str(),
            pair.codex_team_sha256.as_str(),
        ),
    ] {
        let path =
            vault.resolve_relative(&format!("supervisor/slots/{component}/{slot}/binary"))?;
        if file_sha256(&path).ok().as_deref() != Some(expected) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn component_key(component: &ComponentV1) -> &'static str {
    match component {
        ComponentV1::Ca => "ca",
        ComponentV1::CodexTeam => "codex-team",
    }
}

fn install_login_service(vault: &LearningVault, supervisor_binary: &Path) -> Result<()> {
    let home = std::env::var_os("HOME").context("HOME is required to install login service")?;
    let launch_agents = PathBuf::from(home).join("Library/LaunchAgents");
    fs::create_dir_all(&launch_agents)?;
    let plist_path = launch_agents.join("com.codex-team.self-improvement.plist");
    let plist = render_login_service_plist(supervisor_binary, vault.root());
    if plist_path.exists() {
        let existing = fs::read_to_string(&plist_path)?;
        if existing != plist {
            bail!("login service already exists with different content");
        }
    } else {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&plist_path)?;
        file.write_all(plist.as_bytes())?;
        file.sync_all()?;
    }
    #[cfg(target_os = "macos")]
    {
        let domain = format!("gui/{}", unsafe { libc::geteuid() });
        let loaded = Command::new("/bin/launchctl")
            .args([
                "print",
                &format!("{domain}/com.codex-team.self-improvement"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !loaded {
            let status = Command::new("/bin/launchctl")
                .args(["bootstrap", &domain, plist_path.to_string_lossy().as_ref()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .status()?;
            if !status.success() {
                bail!("launchctl rejected the autonomous learning service");
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        bail!("login service installation is supported only on macOS")
    }
}

fn render_login_service_plist(supervisor_binary: &Path, vault_root: &Path) -> String {
    let binary = xml_escape(&supervisor_binary.display().to_string());
    let root = xml_escape(&vault_root.display().to_string());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>Label</key><string>com.codex-team.self-improvement</string>\n<key>ProgramArguments</key><array><string>{binary}</string><string>run</string><string>--vault</string><string>{root}</string></array>\n<key>RunAtLoad</key><true/><key>KeepAlive</key><true/><key>ProcessType</key><string>Background</string>\n<key>LowPriorityIO</key><true/><key>ThrottleInterval</key><integer>30</integer>\n</dict></plist>\n"
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-team-supervisor-{name}-{nonce}"))
    }

    fn fake_executable(path: &Path, version: &str) -> Result<()> {
        fs::create_dir_all(path.parent().unwrap())?;
        let body = if matches!(version, "ca" | "candidate-ca") {
            "#!/bin/sh\nif [ \"$1\" = \"role-run\" ]; then\n  printf '%s\\n' '{\"schema\":\"ca.role_run_preflight.v1\",\"ready\":true,\"profile\":\"codex-team\",\"sandbox_required\":true,\"routes\":[{\"provider\":\"codex\",\"models\":[\"model\"]},{\"provider\":\"deepseek\",\"models\":[\"model\"]},{\"provider\":\"mimo\",\"models\":[\"model\"]}],\"issues\":[]}'\nelse\n  printf '%s\\n' 'ca'\nfi\n".to_string()
        } else {
            format!("#!/bin/sh\nprintf '%s\\n' '{version}'\n")
        };
        fs::write(path, body)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        Ok(())
    }

    #[test]
    fn knowledge_only_contract_binds_binaries_roots_routes_and_default_profile_without_repositories(
    ) -> Result<()> {
        let root = temp_root("knowledge-contract");
        fs::create_dir_all(&root)?;
        let bin = root.join("bin");
        let ca = bin.join("ca");
        let codex_team = bin.join("codex-team");
        let supervisor = bin.join("codex-team-supervisor");
        fake_executable(&codex_team, "codex-team")?;
        fake_executable(&supervisor, "supervisor")?;
        fs::create_dir_all(&bin)?;
        fs::write(
            &ca,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"learning-bridge\" ] && [ \"$2\" = \"preflight\" ]; then printf '%s\\n' '{{\"schema\":\"ca.learning_bridge_preflight.v1\",\"ready\":true,\"profile\":\"default\",\"state_digest\":\"{}\",\"enrolled\":false,\"enrollment_digest\":null,\"queued_intents\":0,\"issues\":[]}}'; exit 0; fi\nif [ \"$1\" = \"role-run\" ] && [ \"$4\" = \"--preflight\" ]; then printf '%s\\n' '{{\"schema\":\"ca.role_run_preflight.v1\",\"ready\":true,\"profile\":\"ca-learning\",\"sandbox_required\":true,\"routes\":[{{\"provider\":\"openai-codex\",\"models\":[\"sol\"]}},{{\"provider\":\"mimo\",\"models\":[\"v2\"]}}],\"issues\":[]}}'; exit 0; fi\nexit 44\n",
                "a".repeat(64)
            ),
        )?;
        fs::set_permissions(&ca, fs::Permissions::from_mode(0o700))?;
        let roots = [
            "ca-active",
            "ca-archive",
            "codex-active",
            "codex-archive",
            "mimocode",
        ]
        .into_iter()
        .map(|kind| {
            let path = if kind == "mimocode" {
                let path = root.join("mimocode.db");
                fs::write(&path, []).unwrap();
                path
            } else {
                let path = root.join(kind);
                fs::create_dir_all(&path).unwrap();
                path
            };
            (kind.to_string(), path)
        })
        .collect::<Vec<_>>();
        let inputs = KnowledgeEnrollmentInputs {
            install_login_service: false,
            supervisor_binary: supervisor,
            ca_binary: ca,
            codex_team_binary: codex_team,
            ca_evidence_profiles: vec!["default".into()],
            ca_execution_profile: "ca-learning".into(),
            evidence_roots: roots,
        };
        let contract = knowledge_enrollment_contract(&inputs)?;
        assert!(contract.repositories.is_empty());
        assert_eq!(contract.evidence_roots.len(), 5);
        assert_eq!(contract.source_changes, "forbidden");
        assert_eq!(contract.budget, "unbounded_metered");
        assert_eq!(contract.ca_evidence_profiles[0].name, "default");
        assert_eq!(
            contract.providers,
            vec!["mimo".to_string(), "openai-codex".to_string()]
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn autonomous_launchagent_is_one_run_at_load_keepalive_supervisor() {
        let plist = render_login_service_plist(
            Path::new("/private/supervisor"),
            Path::new("/private/vault"),
        );
        assert!(plist.contains("com.codex-team.self-improvement"));
        assert!(plist.contains("<key>RunAtLoad</key><true/>"));
        assert!(plist.contains("<key>KeepAlive</key><true/>"));
        assert!(plist.contains("<string>/private/supervisor</string>"));
        assert!(plist.contains("<string>run</string>"));
        assert!(!plist.contains("com.ca.learn"));
        assert!(!plist.contains("/bin/sh"));
    }

    fn git_repo(path: &Path) -> Result<()> {
        fs::create_dir_all(path)?;
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(path)
            .status()?;
        Command::new("git")
            .args(["config", "user.email", "tests@example.invalid"])
            .current_dir(path)
            .status()?;
        Command::new("git")
            .args(["config", "user.name", "Tests"])
            .current_dir(path)
            .status()?;
        fs::write(path.join("README.md"), "test\n")?;
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(path)
            .status()?;
        Command::new("git")
            .args(["commit", "-q", "-m", "initial"])
            .current_dir(path)
            .status()?;
        Ok(())
    }

    fn enrollment_fixture(name: &str) -> Result<(PathBuf, LearningVault, EnrollmentInputs)> {
        crate::verification::isolation::enable_test_isolation_backend();
        let root = temp_root(name);
        fs::create_dir_all(&root)?;
        let vault = LearningVault::open_at(root.join("vault"))?;
        let ca_repo = root.join("ca");
        let orch_repo = root.join("orch");
        git_repo(&ca_repo)?;
        git_repo(&orch_repo)?;
        let bin = root.join("bin");
        let supervisor = bin.join("codex-team-supervisor");
        let ca = bin.join("ca");
        let codex_team = bin.join("codex-team");
        fake_executable(&supervisor, "supervisor")?;
        fake_executable(&ca, "ca")?;
        fake_executable(&codex_team, "codex-team")?;
        Ok((
            root,
            vault,
            EnrollmentInputs {
                ca_repo,
                orchestrator_repo: orch_repo,
                install_login_service: false,
                supervisor_binary: supervisor,
                ca_binary: ca,
                codex_team_binary: codex_team,
                ca_evidence_profiles: Vec::new(),
                ca_execution_profile: "codex-team".to_string(),
            },
        ))
    }

    #[test]
    fn exact_enrollment_is_digest_bound_and_idempotent() -> Result<()> {
        let (root, vault, inputs) = enrollment_fixture("enroll")?;
        assert!(enroll(&vault, &inputs, "wrong").is_err());
        assert!(!vault.resolve_relative("supervisor/state.json")?.exists());
        let digest = canonical_digest(&enrollment_contract(&inputs)?)?;
        let first = enroll(&vault, &inputs, &digest)?;
        assert!(first.changed);
        let second = enroll(&vault, &inputs, &digest)?;
        assert!(!second.changed);
        assert!(doctor(&vault)?.healthy);
        let state = load_state(&vault)?.unwrap();
        assert!(state
            .repositories
            .iter()
            .all(|repository| repository.path.is_none()));
        let encoded = serde_json::to_string(&state)?;
        assert!(!encoded.contains(&inputs.ca_repo.display().to_string()));
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn enrollment_v1_shape_defaults_to_no_evidence_profiles() -> Result<()> {
        let (root, vault, inputs) = enrollment_fixture("v1-migration")?;
        let digest = canonical_digest(&enrollment_contract(&inputs)?)?;
        enroll(&vault, &inputs, &digest)?;
        let mut state = load_state(&vault)?.unwrap();
        state.ca_evidence_profiles.clear();
        state.ca_execution_profile = None;
        state.ca_bridge_cursors.clear();
        state.ca_bridge_healthy = false;
        state.ca_bridge_queued_intents = 0;
        state.refresh_revision()?;
        let mut value = serde_json::to_value(&state)?;
        let object = value.as_object_mut().unwrap();
        assert!(object.remove("ca_evidence_profiles").is_none());
        assert!(object.remove("ca_execution_profile").is_none());
        assert!(object.remove("ca_bridge_cursors").is_none());
        assert!(object.remove("ca_bridge_healthy").is_none());
        assert!(object.remove("ca_bridge_queued_intents").is_none());
        let migrated: SelfImprovementStateV1 = serde_json::from_value(value)?;
        migrated.verify_revision()?;
        assert!(migrated.ca_evidence_profiles.is_empty());
        assert!(migrated.ca_execution_profile.is_none());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn dirty_repository_blocks_enrollment_without_state() -> Result<()> {
        let (root, vault, inputs) = enrollment_fixture("dirty")?;
        fs::write(inputs.ca_repo.join("dirty.txt"), "dirty")?;
        assert!(enrollment_contract(&inputs).is_err());
        assert!(!vault.resolve_relative("supervisor/state.json")?.exists());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn resume_requires_exact_revision() -> Result<()> {
        let (root, vault, inputs) = enrollment_fixture("resume")?;
        let digest = canonical_digest(&enrollment_contract(&inputs)?)?;
        enroll(&vault, &inputs, &digest)?;
        let paused = pause(&vault, "review")?;
        assert!(resume(&vault, "stale").is_err());
        assert!(status(&vault)?.paused);
        resume(&vault, &paused.revision)?;
        assert!(!status(&vault)?.paused);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn corrupt_state_fails_closed() -> Result<()> {
        let (root, vault, inputs) = enrollment_fixture("corrupt")?;
        let digest = canonical_digest(&enrollment_contract(&inputs)?)?;
        enroll(&vault, &inputs, &digest)?;
        let state_path = vault.resolve_relative("supervisor/state.json")?;
        let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&state_path)?)?;
        value["continuous"] = serde_json::json!(false);
        vault.write_mutable("supervisor/state.json", &value)?;
        assert!(status(&vault).is_err());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn candidate_activation_switches_only_after_health_and_rollback_restores_pair() -> Result<()> {
        let (root, vault, inputs) = enrollment_fixture("activate")?;
        let digest = canonical_digest(&enrollment_contract(&inputs)?)?;
        enroll(&vault, &inputs, &digest)?;
        let candidate_binary = root.join("candidate-ca");
        fake_executable(&candidate_binary, "candidate-ca")?;
        let matrix_digest = "d".repeat(64);
        let mut candidate = super::super::DeploymentCandidateV1 {
            schema: super::super::DEPLOYMENT_CANDIDATE_SCHEMA.to_string(),
            candidate_id: "candidate-ca-001".to_string(),
            component: ComponentV1::Ca,
            source_commit: "0123456789abcdef".to_string(),
            binary_sha256: file_sha256(&candidate_binary)?,
            evaluation_receipt_id: "evaluation-001".to_string(),
            protocol_matrix_digest: matrix_digest.clone(),
            inactive_slot: "b".to_string(),
            created_at: Utc::now().to_rfc3339(),
            digest: String::new(),
        };

        let receipt = activate_candidate_with_health(
            &vault,
            &mut candidate,
            &candidate_binary,
            |candidate, peer| {
                inspect_executable(candidate)?;
                inspect_executable(peer)?;
                Ok(matrix_digest.clone())
            },
        )?;
        assert_eq!(receipt.active_slot, "b");
        let activated = status(&vault)?;
        assert_eq!(activated.components["ca"].active, "b");
        assert_eq!(activated.components["ca"].previous.as_deref(), Some("a"));
        assert_eq!(activated.deployment_count, 1);
        assert!(doctor(&vault)?.healthy);

        rollback_with_revision(&vault, ComponentV1::Ca, Some(&activated.revision))?;
        let rolled_back = status(&vault)?;
        assert_eq!(rolled_back.components["ca"].active, "a");
        assert_eq!(rolled_back.components["ca"].previous.as_deref(), Some("b"));
        assert!(rolled_back.paused);
        assert!(doctor(&vault)?.healthy);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn failed_health_never_switches_active_slot() -> Result<()> {
        let (root, vault, inputs) = enrollment_fixture("activate-failure")?;
        let digest = canonical_digest(&enrollment_contract(&inputs)?)?;
        enroll(&vault, &inputs, &digest)?;
        let candidate_binary = root.join("candidate-ca");
        fake_executable(&candidate_binary, "candidate-ca")?;
        let mut candidate = super::super::DeploymentCandidateV1 {
            schema: super::super::DEPLOYMENT_CANDIDATE_SCHEMA.to_string(),
            candidate_id: "candidate-ca-failed".to_string(),
            component: ComponentV1::Ca,
            source_commit: "fedcba9876543210".to_string(),
            binary_sha256: file_sha256(&candidate_binary)?,
            evaluation_receipt_id: "evaluation-failed".to_string(),
            protocol_matrix_digest: "e".repeat(64),
            inactive_slot: "b".to_string(),
            created_at: Utc::now().to_rfc3339(),
            digest: String::new(),
        };
        assert!(activate_candidate_with_health(
            &vault,
            &mut candidate,
            &candidate_binary,
            |_candidate, _peer| bail!("health failed")
        )
        .is_err());
        let unchanged = status(&vault)?;
        assert_eq!(unchanged.components["ca"].active, "a");
        assert_eq!(unchanged.deployment_count, 0);
        assert!(doctor(&vault)?.healthy);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn promoted_shared_core_and_route_overlay_are_injected_only_for_matching_roles() -> Result<()> {
        let (root, vault, inputs) = enrollment_fixture("learned-context")?;
        let digest = canonical_digest(&enrollment_contract(&inputs)?)?;
        enroll(&vault, &inputs, &digest)?;

        let mut plan = super::super::EvaluationPlanV1 {
            schema: super::super::EVALUATION_PLAN_SCHEMA.to_string(),
            plan_id: "plan-core-001".to_string(),
            objective_id: "objective-core-001".to_string(),
            baseline_revisions: vec!["builtin".to_string()],
            cases: (0..5)
                .map(|index| super::super::EvaluationCaseV1 {
                    case_id: format!("case-{index}"),
                    input_evidence_ids: vec![format!("trace-{index}")],
                    repository_fingerprint: "repository-fingerprint".to_string(),
                    route: "codex/model".to_string(),
                    grants_digest: "grants".to_string(),
                    acceptance_contract_digest: "acceptance".to_string(),
                    adversarial: false,
                })
                .collect(),
            deterministic_acceptance: vec!["accepted".to_string()],
            available_non_author_judges: vec![],
            frozen_at: Utc::now().to_rfc3339(),
            digest: String::new(),
        };
        vault.write_immutable(&mut plan)?;
        let candidate_results = (0..5)
            .map(|index| super::super::EvaluationCaseResultV1 {
                case_id: format!("case-{index}"),
                deterministic_pass: true,
                acceptance_properties: ["safe".to_string()].into_iter().collect(),
                output_equivalence_digest: format!("output-{index}"),
                elapsed_millis: 90,
                estimated_microusd: 90,
            })
            .collect::<Vec<_>>();
        let baseline_results = candidate_results
            .iter()
            .cloned()
            .map(|mut result| {
                result.elapsed_millis = 100;
                result.estimated_microusd = 100;
                result
            })
            .collect::<Vec<_>>();
        let decision = super::super::decide_promotion(
            &plan,
            &baseline_results,
            &candidate_results,
            &[],
            None,
        )?;
        assert!(decision.promoted);
        let mut receipt = super::super::EvaluationReceiptV1 {
            schema: super::super::EVALUATION_RECEIPT_SCHEMA.to_string(),
            receipt_id: "evaluation-core-001".to_string(),
            plan_id: plan.plan_id.clone(),
            plan_digest: plan.digest.clone(),
            baseline_results,
            candidate_results,
            judge_votes: vec![],
            human_disposition: None,
            promoted: decision.promoted,
            reasons: decision.reasons,
            created_at: Utc::now().to_rfc3339(),
            digest: String::new(),
        };
        vault.write_immutable(&mut receipt)?;
        let files = vec![super::super::ArtifactFileV1 {
            path: "SKILL.md".to_string(),
            encoding: super::super::ArtifactEncodingV1::Utf8,
            content: "Check deterministic evidence before judgment.".to_string(),
            inject_into_prompt: true,
            executable: false,
        }];
        let content_id = super::super::artifact_tree_digest(&files)?;
        let mut content = super::super::ArtifactContentV1 {
            schema: super::super::ARTIFACT_CONTENT_SCHEMA.to_string(),
            content_id: content_id.clone(),
            files,
            digest: String::new(),
        };
        let mut artifact = super::super::ArtifactRevisionV1 {
            schema: super::super::ARTIFACT_SCHEMA.to_string(),
            artifact_id: "shared-evidence-review".to_string(),
            revision: "shared-evidence-review-v1".to_string(),
            kind: super::super::ArtifactKindV1::Skill,
            content_tree_sha256: content_id,
            parent_revision: None,
            objective_id: plan.objective_id.clone(),
            evaluation_receipt_id: receipt.receipt_id.clone(),
            applicability: vec!["role:worker".to_string()],
            retired: false,
            created_at: Utc::now().to_rfc3339(),
            digest: String::new(),
        };
        promote_shared_artifact(&vault, &mut content, &mut artifact)?;
        let worker = learned_prompt_context(&vault, "codex", "model", "worker")?
            .context("worker should receive active shared context")?;
        assert!(worker.prompt_fragment.contains("deterministic evidence"));
        assert_eq!(
            worker.core_revision.as_deref(),
            Some("shared-evidence-review-v1")
        );
        assert!(learned_prompt_context(&vault, "codex", "model", "ceo")?.is_none());

        let mut overlay = super::super::ProviderOverlayV1 {
            schema: super::super::OVERLAY_SCHEMA.to_string(),
            overlay_id: "codex-model-overlay-v1".to_string(),
            provider: "codex".to_string(),
            model: "model".to_string(),
            triggering: vec!["evidence-only".to_string()],
            prompt_framing_revision: artifact.revision.clone(),
            reasoning_effort: "high".to_string(),
            context_packaging_revision: artifact.revision.clone(),
            tool_order: vec!["verify".to_string()],
            suitable_roles: vec!["worker".to_string()],
            evaluation_receipt_id: receipt.receipt_id,
            created_at: Utc::now().to_rfc3339(),
            digest: String::new(),
        };
        promote_provider_overlay(&vault, &mut overlay)?;
        let matching = learned_prompt_context(&vault, "codex", "model", "worker")?
            .context("matching overlay should be active")?;
        assert_eq!(
            matching.overlay_revision.as_deref(),
            Some("codex-model-overlay-v1")
        );
        let other_route = learned_prompt_context(&vault, "mimo", "model", "worker")?
            .context("shared core should remain provider neutral")?;
        assert!(other_route.overlay_revision.is_none());
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
