//! Controlled learning compiler for evidence-only Rust recipes.
//!
//! Observation is not authority.  Successful typed command chains become
//! repository-local proposals; an exact human approval publishes an immutable
//! machine-local revision.  Matching is structural, execution is isolated,
//! and failures quarantine before any model fallback can occur.

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::domain::policy::AutomationLevel;
use crate::domain::task::TaskSpec;
use crate::paths::Paths;
use crate::revision::repository_revision;
use crate::verification::isolation::{
    read_contained_single_link_file, read_contained_single_link_file_with_mode,
    require_isolation_backend, IsolationReadiness, IsolationSession,
};
use crate::verification::recipe::VerificationCommand;
use crate::verification::runner::{
    run_isolated_deterministic_command_in_session, DeterministicResultCategory,
};

pub(crate) const PROPOSAL_SCHEMA: &str = "codex-team.recipe_proposal.v1";
pub(crate) const APPROVED_SCHEMA: &str = "codex-team.approved_recipe.v1";
pub(crate) const ACTIVATION_SCHEMA: &str = "codex-team.recipe_activation.v1";
pub(crate) const EXECUTION_SCHEMA: &str = "codex-team.recipe_execution.v1";
const DECISION_SCHEMA: &str = "codex-team.recipe_decision.v1";
const FAILURE_SCHEMA: &str = "codex-team.recipe_failure.v1";
const FAILURE_TRANSACTION_SCHEMA: &str = "codex-team.recipe_failure_transaction.v1";
const MAX_COMMANDS: usize = 8;
const MAX_COMMAND_TIMEOUT_SECONDS: u64 = 15 * 60;
const MAX_TOTAL_TIMEOUT_SECONDS: u64 = 20 * 60;
const MAX_RECORD_BYTES: u64 = 1024 * 1024;
static RECORD_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecipeLimits {
    pub(crate) max_commands: usize,
    pub(crate) max_command_seconds: u64,
    pub(crate) max_total_seconds: u64,
}

impl Default for RecipeLimits {
    fn default() -> Self {
        Self {
            max_commands: MAX_COMMANDS,
            max_command_seconds: MAX_COMMAND_TIMEOUT_SECONDS,
            max_total_seconds: MAX_TOTAL_TIMEOUT_SECONDS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecipeMatchContract {
    pub(crate) ecosystem: String,
    pub(crate) required_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) lockfile_format: Option<String>,
    pub(crate) toolchain_family: Vec<String>,
    #[serde(default)]
    pub(crate) manifest_capabilities: Vec<String>,
    #[serde(default)]
    pub(crate) task_kinds: Vec<String>,
    pub(crate) evidence_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecipeDefinition {
    pub(crate) commands: Vec<VerificationCommand>,
    pub(crate) applicability: RecipeMatchContract,
    pub(crate) limits: RecipeLimits,
    pub(crate) safety_classification: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecipeProposal {
    pub(crate) schema: String,
    pub(crate) recipe_id: String,
    pub(crate) digest: String,
    pub(crate) observed_at: String,
    pub(crate) observation_count: u64,
    pub(crate) definition: RecipeDefinition,
    pub(crate) source_evidence_ids: Vec<String>,
    pub(crate) observed_outcomes: Vec<RecipeObservedOutcome>,
    pub(crate) summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecipeObservedOutcome {
    pub(crate) observed_at: String,
    pub(crate) disposition: String,
    pub(crate) evidence_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApprovedRecipe {
    pub(crate) schema: String,
    pub(crate) recipe_id: String,
    pub(crate) revision_digest: String,
    pub(crate) approved_at: String,
    pub(crate) approval_receipt_id: String,
    pub(crate) origin_repository_fingerprint: String,
    pub(crate) validation_evidence_hashes: Vec<String>,
    pub(crate) definition: RecipeDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActivationStatus {
    Active,
    Drifted,
    Quarantined,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecipeActivation {
    pub(crate) schema: String,
    pub(crate) recipe_id: String,
    pub(crate) revision_digest: String,
    pub(crate) repository_fingerprint: String,
    pub(crate) compatibility_fingerprint: String,
    pub(crate) status: ActivationStatus,
    pub(crate) updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecipeCommandExecution {
    pub(crate) command_fingerprint: String,
    pub(crate) category: String,
    pub(crate) exit_code: i32,
    pub(crate) duration_ms: u128,
    pub(crate) evidence_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionDisposition {
    Passed,
    Quarantined,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecipeExecutionReceipt {
    pub(crate) schema: String,
    pub(crate) receipt_id: String,
    pub(crate) recipe_id: String,
    pub(crate) revision_digest: String,
    pub(crate) task_id: String,
    pub(crate) repository_fingerprint: String,
    pub(crate) compatibility_fingerprint: String,
    pub(crate) started_at: String,
    pub(crate) finished_at: String,
    pub(crate) disposition: ExecutionDisposition,
    pub(crate) commands: Vec<RecipeCommandExecution>,
    pub(crate) ai_calls_avoided: u32,
    pub(crate) estimated_tokens_avoided: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecipeDecisionReceipt {
    pub(crate) schema: String,
    pub(crate) receipt_id: String,
    pub(crate) recipe_id: String,
    pub(crate) digest: String,
    pub(crate) decision: String,
    pub(crate) decided_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RecipeFailureReceipt {
    schema: String,
    recipe_id: String,
    revision_digest: String,
    repository_fingerprint: String,
    failed_at: String,
    command_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RecipeFailureTransaction {
    schema: String,
    activation: RecipeActivation,
    failure: RecipeFailureReceipt,
}

#[derive(Debug, Deserialize)]
struct LegacyRecipeObservation {
    recipe_id: String,
    #[serde(default)]
    commands: Vec<VerificationCommand>,
    #[serde(default)]
    source_logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RecipeListReceipt {
    pub(crate) schema: &'static str,
    pub(crate) repository_revision: String,
    pub(crate) proposals: Vec<RecipeProposal>,
    pub(crate) approved: Vec<ApprovedRecipe>,
    pub(crate) activations: Vec<RecipeActivation>,
    pub(crate) isolation: IsolationReadiness,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RecipePreviewReceipt {
    pub(crate) schema: &'static str,
    pub(crate) repository_revision: String,
    pub(crate) proposal: RecipeProposal,
    pub(crate) isolation: IsolationReadiness,
    pub(crate) global_scope_warning: String,
    pub(crate) known_risks: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ApplicableRecipe {
    pub(crate) approved: ApprovedRecipe,
    pub(crate) activation: RecipeActivation,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LearningCatalogSummary {
    pub(crate) healthy: bool,
    pub(crate) proposal_count: usize,
    pub(crate) approved_matching_count: usize,
    pub(crate) quarantine_count: usize,
    pub(crate) execution_count: usize,
    pub(crate) ai_calls_avoided: u64,
    pub(crate) estimated_tokens_avoided: u64,
    pub(crate) applicable_recipe_id: Option<String>,
    pub(crate) activation_state: Option<String>,
}

pub(crate) fn global_catalog_root() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is unavailable for learning catalog")?;
    let home = PathBuf::from(home);
    let metadata = fs::symlink_metadata(&home).context("inspect HOME for learning catalog")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("HOME must be a real directory for the learning catalog");
    }
    Ok(fs::canonicalize(home)?.join(".codex-team").join("learning"))
}

pub(crate) fn propose_observed_commands(
    paths: &Paths,
    commands: Vec<VerificationCommand>,
    source_evidence_ids: Vec<String>,
) -> Result<RecipeProposal> {
    validate_definition_commands(&commands)?;
    if source_evidence_ids.is_empty() {
        bail!("learning proposal requires at least one bounded source evidence identifier");
    }
    for evidence_id in &source_evidence_ids {
        validate_bounded_identifier("source evidence identifier", evidence_id, 192)?;
    }
    let applicability = infer_match_contract(paths, &commands)?
        .context("recipe proposal requires an explicit ecosystem manifest")?;
    let definition = RecipeDefinition {
        commands,
        applicability,
        limits: RecipeLimits::default(),
        safety_classification: "evidence_only_no_product_writes_no_network".to_string(),
    };
    let identity = canonical_definition_digest(&definition)?;
    let recipe_id = format!("learned-{}", &identity[..20]);
    let path = proposal_path(paths, &recipe_id);
    if let Some(mut existing) = read_optional_json::<RecipeProposal>(&path)? {
        validate_proposal(&existing)?;
        if existing.digest != identity || existing.definition != definition {
            bail!("existing recipe proposal identity does not match observed definition");
        }
        let mut evidence = existing
            .source_evidence_ids
            .iter()
            .cloned()
            .chain(source_evidence_ids)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        evidence.truncate(64);
        let observed_at = Utc::now().to_rfc3339();
        let mut outcome_hashes = existing
            .observed_outcomes
            .iter()
            .map(|outcome| outcome.evidence_hash.clone())
            .collect::<BTreeSet<_>>();
        for evidence_id in &evidence {
            let evidence_hash = sha256_hex(evidence_id.as_bytes());
            if outcome_hashes.insert(evidence_hash.clone()) && existing.observed_outcomes.len() < 64
            {
                existing.observed_outcomes.push(RecipeObservedOutcome {
                    observed_at: observed_at.clone(),
                    disposition: "passed".to_string(),
                    evidence_hash,
                });
            }
        }
        existing.source_evidence_ids = evidence;
        existing.observation_count = existing.observation_count.saturating_add(1);
        persist_local_record(&path, &existing, false)?;
        return Ok(existing);
    }
    let observed_at = Utc::now().to_rfc3339();
    let source_evidence_ids = source_evidence_ids
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(64)
        .collect::<Vec<_>>();
    let observed_outcomes = source_evidence_ids
        .iter()
        .map(|evidence_id| RecipeObservedOutcome {
            observed_at: observed_at.clone(),
            disposition: "passed".to_string(),
            evidence_hash: sha256_hex(evidence_id.as_bytes()),
        })
        .collect();
    let proposal = RecipeProposal {
        schema: PROPOSAL_SCHEMA.to_string(),
        recipe_id,
        digest: identity,
        observed_at,
        observation_count: 1,
        definition,
        source_evidence_ids,
        observed_outcomes,
        summary: "Successful typed command chain proposed for evidence-only isolated Rust execution. Observation is not approval.".to_string(),
    };
    persist_local_record(&path, &proposal, true)?;
    Ok(proposal)
}

pub(crate) fn observe_successful_commands(
    paths: &Paths,
    commands: Vec<VerificationCommand>,
    source_evidence_ids: Vec<String>,
) -> Result<Option<RecipeProposal>> {
    if infer_match_contract(paths, &commands)?.is_none() {
        return Ok(None);
    }
    propose_observed_commands(paths, commands, source_evidence_ids).map(Some)
}

pub(crate) fn autonomously_approve_observed_recipe_if_enrolled(
    paths: &Paths,
    task: &TaskSpec,
    proposal: &RecipeProposal,
) -> Result<Option<ApprovedRecipe>> {
    #[cfg(test)]
    {
        // Unit tests must never inspect or mutate the real machine learning vault.
        let _ = (paths, task, proposal);
        Ok(None)
    }
    #[cfg(not(test))]
    {
        if !task_is_evidence_only(task) {
            return Ok(None);
        }
        let Some(vault) = crate::self_improvement::LearningVault::existing_machine()? else {
            return Ok(None);
        };
        let status = crate::self_improvement::status(&vault)?;
        if !status.enrolled || !status.running_allowed {
            return Ok(None);
        }
        let revision = repository_revision(paths)?;
        approve_recipe(paths, &proposal.recipe_id, &proposal.digest, &revision).map(Some)
    }
}

pub(crate) fn migrate_legacy_observations(paths: &Paths) -> Result<usize> {
    let metadata = match fs::symlink_metadata(&paths.recipes) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("legacy recipe directory must be a real directory");
    }
    let mut candidates = fs::read_dir(&paths.recipes)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    candidates.sort();
    if candidates.len() > 10_000 {
        bail!("legacy recipe directory exceeds entry limit");
    }
    let mut imported = 0usize;
    for path in candidates {
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Some(observation) = read_optional_json::<LegacyRecipeObservation>(&path)? else {
            continue;
        };
        if observation.commands.is_empty() {
            continue;
        }
        let mut evidence = vec![format!(
            "legacy:{}",
            sha256_hex(observation.recipe_id.as_bytes())
        )];
        evidence.extend(
            observation
                .source_logs
                .iter()
                .take(63)
                .map(|source| format!("legacy-log:{}", sha256_hex(source.as_bytes()))),
        );
        if observe_successful_commands(paths, observation.commands, evidence)?.is_some() {
            imported += 1;
        }
    }
    Ok(imported)
}

pub(crate) fn list_recipes(paths: &Paths, include_global: bool) -> Result<RecipeListReceipt> {
    let proposals = read_json_directory::<RecipeProposal>(&paths.recipes.join("proposals"))?;
    for proposal in &proposals {
        validate_proposal(proposal)?;
    }
    let approved = if include_global {
        read_approved_recipes(&global_catalog_root()?)?
    } else {
        read_approved_recipes(&global_catalog_root()?)?
            .into_iter()
            .filter(|recipe| recipe_matches_repository(paths, &recipe.definition).unwrap_or(false))
            .collect()
    };
    let activations = read_json_directory::<RecipeActivation>(&activation_directory(paths))?;
    for activation in &activations {
        validate_activation(activation)?;
    }
    let isolation = isolation_readiness();
    Ok(RecipeListReceipt {
        schema: "codex-team.recipe_list.v1",
        repository_revision: repository_revision(paths)?,
        proposals,
        approved,
        activations,
        isolation,
    })
}

pub(crate) fn preview_recipe(paths: &Paths, recipe_id: &str) -> Result<RecipePreviewReceipt> {
    validate_id(recipe_id)?;
    let proposal = read_required_json::<RecipeProposal>(&proposal_path(paths, recipe_id))?;
    validate_proposal(&proposal)?;
    Ok(RecipePreviewReceipt {
        schema: "codex-team.recipe_preview.v1",
        repository_revision: repository_revision(paths)?,
        proposal,
        isolation: isolation_readiness(),
        global_scope_warning: "Approval applies to this machine and automatically activates this exact revision in matching repositories for admitted evidence-only tasks.".to_string(),
        known_risks: vec![
            "Verification executes project build/test code only inside required isolation.".to_string(),
            "A real command failure quarantines the recipe in that repository.".to_string(),
            "Failures in two distinct repositories retire this revision globally.".to_string(),
        ],
    })
}

pub(crate) fn catalog_summary(
    paths: &Paths,
    task: Option<&TaskSpec>,
) -> Result<LearningCatalogSummary> {
    let proposals = read_json_directory::<RecipeProposal>(&paths.recipes.join("proposals"))?;
    for proposal in &proposals {
        validate_proposal(proposal)?;
    }
    let root = global_catalog_root()?;
    let approved = read_approved_recipes(&root)?;
    let activations = read_json_directory::<RecipeActivation>(&activation_directory(paths))?;
    let pending_failures = read_json_directory::<RecipeFailureTransaction>(
        &paths.recipes.join("failure-transactions"),
    )?;
    let executions =
        read_json_directory::<RecipeExecutionReceipt>(&paths.recipes.join("executions"))?;
    for activation in &activations {
        validate_activation(activation)?;
    }
    for transaction in &pending_failures {
        let approved = read_required_global_json::<ApprovedRecipe>(
            &root,
            &approved_recipe_path(
                &root,
                &transaction.failure.recipe_id,
                &transaction.failure.revision_digest,
            ),
        )?;
        validate_approved(&approved)?;
        validate_failure_transaction(transaction, &approved)?;
    }
    for execution in &executions {
        validate_execution(execution)?;
    }
    let mut matching = Vec::new();
    for recipe in &approved {
        if is_retired(&root, &recipe.recipe_id, &recipe.revision_digest)?
            || !recipe_matches_repository(paths, &recipe.definition)?
            || task
                .map(|task| !recipe_matches_task(task, &recipe.definition))
                .unwrap_or(false)
        {
            continue;
        }
        matching.push(recipe);
    }
    let applicable_recipe_id = (matching.len() == 1).then(|| matching[0].recipe_id.clone());
    let activation_state = applicable_recipe_id.as_ref().and_then(|id| {
        activations
            .iter()
            .find(|activation| &activation.recipe_id == id)
            .map(|activation| {
                match activation.status {
                    ActivationStatus::Active => "active",
                    ActivationStatus::Drifted => "drifted",
                    ActivationStatus::Quarantined => "quarantined",
                    ActivationStatus::Unsupported => "unsupported",
                }
                .to_string()
            })
    });
    Ok(LearningCatalogSummary {
        healthy: pending_failures.is_empty(),
        proposal_count: proposals.len(),
        approved_matching_count: matching.len(),
        quarantine_count: activations
            .iter()
            .filter(|activation| activation.status == ActivationStatus::Quarantined)
            .count()
            .saturating_add(pending_failures.len()),
        execution_count: executions.len(),
        ai_calls_avoided: executions
            .iter()
            .map(|receipt| u64::from(receipt.ai_calls_avoided))
            .sum(),
        estimated_tokens_avoided: executions
            .iter()
            .map(|receipt| receipt.estimated_tokens_avoided)
            .sum(),
        applicable_recipe_id,
        activation_state,
    })
}

pub(crate) fn approve_recipe(
    paths: &Paths,
    recipe_id: &str,
    expected_digest: &str,
    expected_revision: &str,
) -> Result<ApprovedRecipe> {
    approve_recipe_at_root(
        paths,
        recipe_id,
        expected_digest,
        expected_revision,
        &global_catalog_root()?,
    )
}

fn approve_recipe_at_root(
    paths: &Paths,
    recipe_id: &str,
    expected_digest: &str,
    expected_revision: &str,
    root: &Path,
) -> Result<ApprovedRecipe> {
    require_exact_revision(paths, expected_revision)?;
    let proposal = read_required_json::<RecipeProposal>(&proposal_path(paths, recipe_id))?;
    validate_proposal(&proposal)?;
    if proposal.digest != expected_digest {
        bail!("recipe proposal digest is stale");
    }
    if let Some(rejection) = read_optional_json::<RecipeDecisionReceipt>(&rejection_path(
        paths,
        recipe_id,
        expected_digest,
    ))? {
        validate_decision(&rejection, recipe_id, expected_digest, "rejected")?;
        bail!("rejected recipe revisions cannot be approved; observe a changed definition");
    }
    let path = approved_recipe_path(root, recipe_id, expected_digest);
    {
        let _catalog_lock = CatalogLock::acquire(root)?;
        if let Some(existing) = read_optional_global_json::<ApprovedRecipe>(root, &path)? {
            validate_approved(&existing)?;
            return Ok(existing);
        }
    }
    require_isolation_backend()?;
    let isolation = IsolationSession::new(paths)?;
    let mut validation_hashes = Vec::new();
    let started = std::time::Instant::now();
    for command in &proposal.definition.commands {
        let remaining = proposal
            .definition
            .limits
            .max_total_seconds
            .saturating_sub(started.elapsed().as_secs());
        if remaining == 0 {
            bail!("recipe approval validation exceeded aggregate time limit");
        }
        let mut command = command.clone();
        command.timeout_seconds = Some(
            command
                .timeout_seconds
                .unwrap_or(MAX_COMMAND_TIMEOUT_SECONDS)
                .min(proposal.definition.limits.max_command_seconds)
                .min(remaining),
        );
        let result = run_isolated_deterministic_command_in_session(paths, &command, &isolation)?;
        if !result.category.is_success_like() {
            bail!("recipe approval validation did not pass");
        }
        validation_hashes.push(sha256_hex(result.evidence.as_bytes()));
    }
    require_exact_revision(paths, expected_revision)?;
    let repository_fingerprint = repository_fingerprint(paths)?;
    let approval_receipt_id = format!(
        "approve-{}-{}",
        recipe_id,
        &expected_digest[..expected_digest.len().min(16)]
    );
    let approved = ApprovedRecipe {
        schema: APPROVED_SCHEMA.to_string(),
        recipe_id: recipe_id.to_string(),
        revision_digest: expected_digest.to_string(),
        approved_at: Utc::now().to_rfc3339(),
        approval_receipt_id,
        origin_repository_fingerprint: repository_fingerprint,
        validation_evidence_hashes: validation_hashes,
        definition: proposal.definition,
    };
    let _catalog_lock = CatalogLock::acquire(root)?;
    if let Some(existing) = read_optional_global_json::<ApprovedRecipe>(root, &path)? {
        validate_approved(&existing)?;
        return Ok(existing);
    }
    if let Some(rejection) = read_optional_json::<RecipeDecisionReceipt>(&rejection_path(
        paths,
        recipe_id,
        expected_digest,
    ))? {
        validate_decision(&rejection, recipe_id, expected_digest, "rejected")?;
        bail!("recipe revision was rejected during approval validation");
    }
    persist_global_record(root, &path, &approved, true)?;
    Ok(approved)
}

pub(crate) fn reject_recipe(
    paths: &Paths,
    recipe_id: &str,
    expected_digest: &str,
) -> Result<RecipeDecisionReceipt> {
    let proposal = read_required_json::<RecipeProposal>(&proposal_path(paths, recipe_id))?;
    validate_proposal(&proposal)?;
    if proposal.digest != expected_digest {
        bail!("recipe proposal digest is stale");
    }
    let path = rejection_path(paths, recipe_id, expected_digest);
    if let Some(existing) = read_optional_json::<RecipeDecisionReceipt>(&path)? {
        validate_decision(&existing, recipe_id, expected_digest, "rejected")?;
        return Ok(existing);
    }
    let receipt = decision_receipt(recipe_id, expected_digest, "rejected");
    persist_local_record(&path, &receipt, true)?;
    Ok(receipt)
}

pub(crate) fn retire_recipe(
    recipe_id: &str,
    expected_digest: &str,
) -> Result<RecipeDecisionReceipt> {
    validate_id(recipe_id)?;
    validate_sha256(expected_digest)?;
    let root = global_catalog_root()?;
    let _catalog_lock = CatalogLock::acquire(&root)?;
    let approved = read_required_global_json::<ApprovedRecipe>(
        &root,
        &approved_recipe_path(&root, recipe_id, expected_digest),
    )?;
    validate_approved(&approved)?;
    let path = retired_recipe_path(&root, recipe_id, expected_digest);
    if let Some(existing) = read_optional_global_json::<RecipeDecisionReceipt>(&root, &path)? {
        validate_decision(&existing, recipe_id, expected_digest, "retired")?;
        return Ok(existing);
    }
    let receipt = decision_receipt(recipe_id, expected_digest, "retired");
    persist_global_record(&root, &path, &receipt, true)?;
    Ok(receipt)
}

pub(crate) fn reactivate_recipe(
    paths: &Paths,
    recipe_id: &str,
    expected_digest: &str,
    expected_revision: &str,
) -> Result<RecipeActivation> {
    require_exact_revision(paths, expected_revision)?;
    let root = global_catalog_root()?;
    let approved = read_required_global_json::<ApprovedRecipe>(
        &root,
        &approved_recipe_path(&root, recipe_id, expected_digest),
    )?;
    if is_retired(&root, recipe_id, expected_digest)? {
        bail!("retired recipe revisions cannot be reactivated; approve a new revision");
    }
    if read_optional_json::<RecipeActivation>(&activation_path(paths, recipe_id))?
        .is_some_and(|activation| activation.status == ActivationStatus::Quarantined)
    {
        bail!("quarantined recipe revisions cannot be reactivated; approve a new revision");
    }
    if require_isolation_backend().is_err() {
        let unsupported = RecipeActivation {
            status: ActivationStatus::Unsupported,
            reason: Some("required verification isolation is unavailable".to_string()),
            updated_at: Utc::now().to_rfc3339(),
            ..active_activation(paths, &approved)?
        };
        persist_local_record(&activation_path(paths, recipe_id), &unsupported, false)?;
        bail!("required verification isolation is unavailable");
    }
    if !recipe_matches_repository(paths, &approved.definition)? {
        bail!("recipe does not match this repository");
    }
    let activation = active_activation(paths, &approved)?;
    persist_local_record(&activation_path(paths, recipe_id), &activation, false)?;
    Ok(activation)
}

pub(crate) fn select_applicable_recipe(
    paths: &Paths,
    task: &TaskSpec,
) -> Result<Option<ApplicableRecipe>> {
    select_applicable_recipe_from_root(paths, task, &global_catalog_root()?)
}

fn select_applicable_recipe_from_root(
    paths: &Paths,
    task: &TaskSpec,
    root: &Path,
) -> Result<Option<ApplicableRecipe>> {
    recover_pending_failure_transactions(paths, root)?;
    if !task_is_evidence_only(task) {
        return Ok(None);
    }
    let mut matches = Vec::new();
    for approved in read_approved_recipes(root)? {
        if is_retired(root, &approved.recipe_id, &approved.revision_digest)?
            || !recipe_matches_task(task, &approved.definition)
            || !recipe_matches_repository(paths, &approved.definition)?
        {
            continue;
        }
        let expected = active_activation(paths, &approved)?;
        if let Some(current) =
            read_optional_json::<RecipeActivation>(&activation_path(paths, &approved.recipe_id))?
        {
            if current.revision_digest == approved.revision_digest
                && current.status == ActivationStatus::Quarantined
            {
                continue;
            }
            if current.compatibility_fingerprint != expected.compatibility_fingerprint {
                let drifted = RecipeActivation {
                    status: ActivationStatus::Drifted,
                    reason: Some("repository compatibility inputs changed".to_string()),
                    updated_at: Utc::now().to_rfc3339(),
                    ..current
                };
                persist_local_record(
                    &activation_path(paths, &approved.recipe_id),
                    &drifted,
                    false,
                )?;
                continue;
            }
        }
        matches.push(ApplicableRecipe {
            approved,
            activation: expected,
        });
    }
    if matches.len() > 1 {
        bail!("multiple approved learning recipes match this task; human review is required");
    }
    let Some(selected) = matches.pop() else {
        return Ok(None);
    };
    persist_local_record(
        &activation_path(paths, &selected.approved.recipe_id),
        &selected.activation,
        false,
    )?;
    Ok(Some(selected))
}

pub(crate) fn execute_applicable_recipe(
    paths: &Paths,
    task: &TaskSpec,
    selected: &ApplicableRecipe,
) -> Result<RecipeExecutionReceipt> {
    if require_isolation_backend().is_err() {
        let unsupported = RecipeActivation {
            status: ActivationStatus::Unsupported,
            reason: Some("required verification isolation is unavailable".to_string()),
            updated_at: Utc::now().to_rfc3339(),
            ..selected.activation.clone()
        };
        persist_local_record(
            &activation_path(paths, &selected.approved.recipe_id),
            &unsupported,
            false,
        )?;
        bail!("required verification isolation is unavailable");
    }
    let current = active_activation(paths, &selected.approved)?;
    if current.compatibility_fingerprint != selected.activation.compatibility_fingerprint {
        bail!("recipe compatibility changed before execution");
    }
    let started_at = Utc::now().to_rfc3339();
    let started = std::time::Instant::now();
    let isolation = IsolationSession::new(paths)?;
    let mut commands = Vec::new();
    let mut disposition = ExecutionDisposition::Passed;
    for command in &selected.approved.definition.commands {
        let remaining = selected
            .approved
            .definition
            .limits
            .max_total_seconds
            .saturating_sub(started.elapsed().as_secs());
        if remaining == 0 {
            disposition = ExecutionDisposition::Stopped;
            break;
        }
        let mut command = command.clone();
        command.timeout_seconds = Some(
            command
                .timeout_seconds
                .unwrap_or(MAX_COMMAND_TIMEOUT_SECONDS)
                .min(selected.approved.definition.limits.max_command_seconds)
                .min(remaining),
        );
        let result = run_isolated_deterministic_command_in_session(paths, &command, &isolation)?;
        commands.push(RecipeCommandExecution {
            command_fingerprint: sha256_hex(command.fingerprint().as_bytes()),
            category: result.category.as_str().to_string(),
            exit_code: result.exit_code,
            duration_ms: result.duration_ms,
            evidence_hash: sha256_hex(result.evidence.as_bytes()),
        });
        if result.category == DeterministicResultCategory::CommandFailed {
            disposition = ExecutionDisposition::Quarantined;
            quarantine_after_failure(paths, &selected.approved, &command)?;
            break;
        }
        if !result.category.is_success_like() {
            disposition = ExecutionDisposition::Stopped;
            break;
        }
    }
    let receipt_id = format!(
        "exec-{}-{}-{}",
        selected.approved.recipe_id,
        Utc::now().timestamp_millis().unsigned_abs(),
        RECORD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let passed = disposition == ExecutionDisposition::Passed;
    let receipt = RecipeExecutionReceipt {
        schema: EXECUTION_SCHEMA.to_string(),
        receipt_id: receipt_id.clone(),
        recipe_id: selected.approved.recipe_id.clone(),
        revision_digest: selected.approved.revision_digest.clone(),
        task_id: task.task_id.clone(),
        repository_fingerprint: repository_fingerprint(paths)?,
        compatibility_fingerprint: current.compatibility_fingerprint,
        started_at,
        finished_at: Utc::now().to_rfc3339(),
        disposition,
        commands,
        ai_calls_avoided: u32::from(passed) * 2,
        estimated_tokens_avoided: if passed { 37_000 } else { 0 },
    };
    persist_local_record(
        &paths
            .recipes
            .join("executions")
            .join(format!("{receipt_id}.json")),
        &receipt,
        true,
    )?;
    #[cfg(not(test))]
    if passed {
        crate::self_improvement::record_recipe_avoidance_if_enrolled(
            u64::from(receipt.ai_calls_avoided),
            receipt.estimated_tokens_avoided,
        )?;
    }
    Ok(receipt)
}

fn quarantine_after_failure(
    paths: &Paths,
    approved: &ApprovedRecipe,
    command: &VerificationCommand,
) -> Result<()> {
    quarantine_after_failure_at_root(paths, approved, command, &global_catalog_root()?)
}

fn quarantine_after_failure_at_root(
    paths: &Paths,
    approved: &ApprovedRecipe,
    command: &VerificationCommand,
    root: &Path,
) -> Result<()> {
    let repository_fingerprint = repository_fingerprint(paths)?;
    let activation = RecipeActivation {
        status: ActivationStatus::Quarantined,
        reason: Some("isolated recipe command failed".to_string()),
        updated_at: Utc::now().to_rfc3339(),
        ..active_activation(paths, approved)?
    };
    let failure = RecipeFailureReceipt {
        schema: FAILURE_SCHEMA.to_string(),
        recipe_id: approved.recipe_id.clone(),
        revision_digest: approved.revision_digest.clone(),
        repository_fingerprint: repository_fingerprint.clone(),
        failed_at: Utc::now().to_rfc3339(),
        command_fingerprint: sha256_hex(command.fingerprint().as_bytes()),
    };
    let transaction = RecipeFailureTransaction {
        schema: FAILURE_TRANSACTION_SCHEMA.to_string(),
        activation,
        failure,
    };
    let transaction_path =
        failure_transaction_path(paths, &approved.recipe_id, &approved.revision_digest);
    persist_local_record(&transaction_path, &transaction, false)?;
    finish_failure_transaction(paths, approved, root, &transaction_path, &transaction)
}

fn finish_failure_transaction(
    paths: &Paths,
    approved: &ApprovedRecipe,
    root: &Path,
    transaction_path: &Path,
    transaction: &RecipeFailureTransaction,
) -> Result<()> {
    validate_failure_transaction(transaction, approved)?;
    let failure = &transaction.failure;
    let activation = &transaction.activation;
    let _catalog_lock = CatalogLock::acquire(root)?;
    let failure_dir = root
        .join("failures")
        .join(&approved.recipe_id)
        .join(&approved.revision_digest);
    let path = failure_dir.join(format!("{}.json", failure.repository_fingerprint));
    if let Some(existing) = read_optional_global_json::<RecipeFailureReceipt>(root, &path)? {
        validate_failure(&existing, approved)?;
        if existing.repository_fingerprint != failure.repository_fingerprint
            || existing.command_fingerprint != failure.command_fingerprint
        {
            bail!("existing recipe failure evidence conflicts with recovery transaction");
        }
    } else {
        persist_global_record(root, &path, failure, true)?;
    }
    let failures = read_json_directory_private::<RecipeFailureReceipt>(root, &failure_dir)?;
    for failure in &failures {
        validate_failure(failure, approved)?;
    }
    let distinct = failures
        .iter()
        .map(|failure| failure.repository_fingerprint.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    if distinct >= 2 && !is_retired(root, &approved.recipe_id, &approved.revision_digest)? {
        let receipt = decision_receipt(
            &approved.recipe_id,
            &approved.revision_digest,
            "retired_after_distinct_repository_failures",
        );
        persist_global_record(
            root,
            &retired_recipe_path(root, &approved.recipe_id, &approved.revision_digest),
            &receipt,
            true,
        )?;
    }
    drop(_catalog_lock);
    persist_local_record(
        &activation_path(paths, &approved.recipe_id),
        activation,
        false,
    )?;
    remove_local_transaction(transaction_path)?;
    Ok(())
}

fn recover_pending_failure_transactions(paths: &Paths, root: &Path) -> Result<()> {
    let directory = paths.recipes.join("failure-transactions");
    let transactions = read_json_directory::<RecipeFailureTransaction>(&directory)?;
    for transaction in transactions {
        let approved = read_required_global_json::<ApprovedRecipe>(
            root,
            &approved_recipe_path(
                root,
                &transaction.failure.recipe_id,
                &transaction.failure.revision_digest,
            ),
        )?;
        validate_approved(&approved)?;
        let path = failure_transaction_path(
            paths,
            &transaction.failure.recipe_id,
            &transaction.failure.revision_digest,
        );
        finish_failure_transaction(paths, &approved, root, &path, &transaction)?;
    }
    Ok(())
}

fn validate_failure_transaction(
    transaction: &RecipeFailureTransaction,
    approved: &ApprovedRecipe,
) -> Result<()> {
    if transaction.schema != FAILURE_TRANSACTION_SCHEMA {
        bail!("unsupported recipe failure transaction schema");
    }
    validate_failure(&transaction.failure, approved)?;
    validate_activation(&transaction.activation)?;
    if transaction.activation.status != ActivationStatus::Quarantined
        || transaction.activation.recipe_id != transaction.failure.recipe_id
        || transaction.activation.revision_digest != transaction.failure.revision_digest
        || transaction.activation.repository_fingerprint
            != transaction.failure.repository_fingerprint
    {
        bail!("recipe failure transaction records do not agree");
    }
    Ok(())
}

fn infer_match_contract(
    paths: &Paths,
    commands: &[VerificationCommand],
) -> Result<Option<RecipeMatchContract>> {
    let programs = commands
        .iter()
        .map(|command| command.program.as_str())
        .collect::<BTreeSet<_>>();
    let (ecosystem, candidates) = if programs.contains("cargo") {
        ("cargo", vec!["Cargo.toml", "Cargo.lock"])
    } else if programs.contains("swift") {
        ("swift", vec!["Package.swift", "Package.resolved"])
    } else if programs
        .iter()
        .any(|program| matches!(*program, "npm" | "pnpm" | "yarn"))
    {
        (
            "node",
            vec![
                "package.json",
                "package-lock.json",
                "pnpm-lock.yaml",
                "yarn.lock",
            ],
        )
    } else if programs
        .iter()
        .any(|program| matches!(*program, "pytest" | "python"))
    {
        (
            "python",
            vec!["pyproject.toml", "pytest.ini", "requirements.txt"],
        )
    } else {
        ("generic", Vec::new())
    };
    let mut required_files = Vec::new();
    for candidate in candidates {
        match fs::symlink_metadata(paths.root.join(candidate)) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    bail!("recipe applicability input must be a regular non-symlink file");
                }
                read_contained_single_link_file(&paths.root, candidate, 4 * 1024 * 1024)?;
                required_files.push(candidate.to_string());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    if ecosystem != "generic" && required_files.is_empty() {
        return Ok(None);
    }
    let lockfile_format = required_files
        .iter()
        .find(|path| path.contains("lock") || path.ends_with("resolved"))
        .map(|path| detect_lockfile_format(paths, path))
        .transpose()?
        .flatten();
    let manifest_capabilities = commands
        .iter()
        .filter_map(|command| {
            if matches!(command.program.as_str(), "npm" | "pnpm" | "yarn") {
                let script = match command.args.as_slice() {
                    [value, ..] if value == "test" => Some("test"),
                    [run, value, ..] if run == "run" => Some(value.as_str()),
                    _ => None,
                }?;
                Some(format!("script:{script}"))
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(Some(RecipeMatchContract {
        ecosystem: ecosystem.to_string(),
        required_files,
        lockfile_format,
        toolchain_family: programs.into_iter().map(str::to_string).collect(),
        manifest_capabilities,
        task_kinds: Vec::new(),
        evidence_only: true,
    }))
}

fn validate_definition_commands(commands: &[VerificationCommand]) -> Result<()> {
    if commands.is_empty() || commands.len() > MAX_COMMANDS {
        bail!("learning recipe must contain 1-{MAX_COMMANDS} commands");
    }
    for command in commands {
        if !crate::app::is_deterministic_command_structured_allowed(command) {
            bail!("learning recipe contains a command outside the typed deterministic allowlist");
        }
        let timeout = command
            .timeout_seconds
            .unwrap_or(MAX_COMMAND_TIMEOUT_SECONDS);
        if timeout == 0 || timeout > MAX_COMMAND_TIMEOUT_SECONDS {
            bail!("learning recipe command timeout exceeds the v1 limit");
        }
    }
    Ok(())
}

fn validate_proposal(proposal: &RecipeProposal) -> Result<()> {
    if proposal.schema != PROPOSAL_SCHEMA {
        bail!("unsupported recipe proposal schema");
    }
    validate_id(&proposal.recipe_id)?;
    validate_sha256(&proposal.digest)?;
    validate_definition(&proposal.definition)?;
    if proposal.observation_count == 0
        || proposal.source_evidence_ids.is_empty()
        || proposal.source_evidence_ids.len() > 64
        || proposal.observed_outcomes.is_empty()
        || proposal.observed_outcomes.len() > 64
        || proposal.summary.is_empty()
        || proposal.summary.len() > 512
        || chrono::DateTime::parse_from_rfc3339(&proposal.observed_at).is_err()
    {
        bail!("recipe proposal metadata is invalid or oversized");
    }
    for evidence_id in &proposal.source_evidence_ids {
        validate_bounded_identifier("source evidence identifier", evidence_id, 192)?;
    }
    for outcome in &proposal.observed_outcomes {
        if outcome.disposition != "passed"
            || chrono::DateTime::parse_from_rfc3339(&outcome.observed_at).is_err()
        {
            bail!("recipe observed outcome is invalid");
        }
        validate_sha256(&outcome.evidence_hash)?;
    }
    if canonical_definition_digest(&proposal.definition)? != proposal.digest {
        bail!("recipe proposal digest does not match its definition");
    }
    Ok(())
}

fn validate_approved(approved: &ApprovedRecipe) -> Result<()> {
    if approved.schema != APPROVED_SCHEMA {
        bail!("unsupported approved recipe schema");
    }
    validate_id(&approved.recipe_id)?;
    validate_sha256(&approved.revision_digest)?;
    validate_definition(&approved.definition)?;
    validate_bounded_identifier(
        "approval receipt identifier",
        &approved.approval_receipt_id,
        256,
    )?;
    validate_sha256(&approved.origin_repository_fingerprint)?;
    if approved.validation_evidence_hashes.is_empty()
        || approved.validation_evidence_hashes.len() > MAX_COMMANDS
        || chrono::DateTime::parse_from_rfc3339(&approved.approved_at).is_err()
    {
        bail!("approved recipe metadata is invalid or oversized");
    }
    for hash in &approved.validation_evidence_hashes {
        validate_sha256(hash)?;
    }
    if canonical_definition_digest(&approved.definition)? != approved.revision_digest {
        bail!("approved recipe digest does not match its definition");
    }
    Ok(())
}

fn validate_definition(definition: &RecipeDefinition) -> Result<()> {
    validate_definition_commands(&definition.commands)?;
    if definition.limits.max_commands == 0
        || definition.limits.max_commands > MAX_COMMANDS
        || definition.commands.len() > definition.limits.max_commands
        || definition.limits.max_command_seconds == 0
        || definition.limits.max_command_seconds > MAX_COMMAND_TIMEOUT_SECONDS
        || definition.limits.max_total_seconds == 0
        || definition.limits.max_total_seconds > MAX_TOTAL_TIMEOUT_SECONDS
        || definition.safety_classification != "evidence_only_no_product_writes_no_network"
        || !definition.applicability.evidence_only
    {
        bail!("recipe limits or safety classification exceed the v1 authority ceiling");
    }
    if !matches!(
        definition.applicability.ecosystem.as_str(),
        "cargo" | "node" | "python" | "swift" | "generic"
    ) || definition.applicability.required_files.len() > 32
        || definition.applicability.toolchain_family.is_empty()
        || definition.applicability.toolchain_family.len() > 16
        || definition.applicability.manifest_capabilities.len() > 32
        || definition.applicability.task_kinds.len() > 32
    {
        bail!("recipe applicability contract is invalid or oversized");
    }
    if definition.applicability.ecosystem != "generic"
        && definition.applicability.required_files.is_empty()
    {
        bail!("non-generic recipes require a structural manifest predicate");
    }
    let command_families = definition
        .commands
        .iter()
        .map(|command| command.program.clone())
        .collect::<BTreeSet<_>>();
    let declared_families = definition
        .applicability
        .toolchain_family
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if command_families != declared_families {
        bail!("recipe toolchain-family contract does not match its typed commands");
    }
    for path in &definition.applicability.required_files {
        validate_safe_relative_path(path)?;
    }
    for family in &definition.applicability.toolchain_family {
        validate_bounded_identifier("toolchain family", family, 64)?;
    }
    for capability in &definition.applicability.manifest_capabilities {
        validate_bounded_identifier("manifest capability", capability, 128)?;
    }
    for kind in &definition.applicability.task_kinds {
        validate_bounded_identifier("task kind", kind, 64)?;
    }
    Ok(())
}

fn validate_activation(activation: &RecipeActivation) -> Result<()> {
    if activation.schema != ACTIVATION_SCHEMA {
        bail!("unsupported recipe activation schema");
    }
    validate_id(&activation.recipe_id)?;
    validate_sha256(&activation.revision_digest)?;
    validate_sha256(&activation.repository_fingerprint)?;
    validate_sha256(&activation.compatibility_fingerprint)?;
    if chrono::DateTime::parse_from_rfc3339(&activation.updated_at).is_err()
        || activation
            .reason
            .as_ref()
            .is_some_and(|reason| reason.len() > 1024)
    {
        bail!("recipe activation metadata is invalid or oversized");
    }
    Ok(())
}

fn validate_execution(execution: &RecipeExecutionReceipt) -> Result<()> {
    if execution.schema != EXECUTION_SCHEMA || execution.commands.len() > MAX_COMMANDS {
        bail!("unsupported or oversized recipe execution receipt");
    }
    validate_id(&execution.recipe_id)?;
    if execution.task_id.is_empty()
        || execution.task_id.len() > 128
        || !execution
            .task_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        bail!("recipe execution task identifier is invalid");
    }
    validate_sha256(&execution.revision_digest)?;
    validate_sha256(&execution.repository_fingerprint)?;
    validate_sha256(&execution.compatibility_fingerprint)?;
    validate_bounded_identifier(
        "recipe execution receipt identifier",
        &execution.receipt_id,
        256,
    )?;
    if chrono::DateTime::parse_from_rfc3339(&execution.started_at).is_err()
        || chrono::DateTime::parse_from_rfc3339(&execution.finished_at).is_err()
        || (execution.disposition != ExecutionDisposition::Passed
            && (execution.ai_calls_avoided != 0 || execution.estimated_tokens_avoided != 0))
    {
        bail!("recipe execution metadata is invalid");
    }
    for command in &execution.commands {
        validate_sha256(&command.command_fingerprint)?;
        validate_sha256(&command.evidence_hash)?;
        if !matches!(
            command.category.as_str(),
            "passed" | "noMatches" | "knownEnvironmentBlocker" | "commandFailed" | "timedOut"
        ) {
            bail!("recipe command execution category is invalid");
        }
    }
    Ok(())
}

fn validate_failure(failure: &RecipeFailureReceipt, approved: &ApprovedRecipe) -> Result<()> {
    if failure.schema != FAILURE_SCHEMA
        || failure.recipe_id != approved.recipe_id
        || failure.revision_digest != approved.revision_digest
    {
        bail!("recipe failure receipt does not match its approved revision");
    }
    validate_sha256(&failure.repository_fingerprint)?;
    validate_sha256(&failure.command_fingerprint)
}

fn validate_decision(
    receipt: &RecipeDecisionReceipt,
    recipe_id: &str,
    digest: &str,
    expected_prefix: &str,
) -> Result<()> {
    if receipt.schema != DECISION_SCHEMA
        || receipt.recipe_id != recipe_id
        || receipt.digest != digest
        || !receipt.decision.starts_with(expected_prefix)
        || chrono::DateTime::parse_from_rfc3339(&receipt.decided_at).is_err()
    {
        bail!("recipe decision receipt does not match its bound revision");
    }
    validate_bounded_identifier(
        "recipe decision receipt identifier",
        &receipt.receipt_id,
        384,
    )
}

fn canonical_definition_digest(definition: &RecipeDefinition) -> Result<String> {
    Ok(sha256_hex(&serde_json::to_vec(definition)?))
}

fn recipe_matches_task(task: &TaskSpec, definition: &RecipeDefinition) -> bool {
    definition.applicability.evidence_only
        && task_is_evidence_only(task)
        && (definition.applicability.task_kinds.is_empty()
            || definition
                .applicability
                .task_kinds
                .iter()
                .any(|kind| kind.eq_ignore_ascii_case(&task.kind)))
}

pub(crate) fn task_is_evidence_only(task: &TaskSpec) -> bool {
    task.automation_level == AutomationLevel::RustOnly
        && !task.requires_network
        && !task.allowed_files.is_empty()
        && task.allowed_files.iter().all(|path| {
            matches!(
                path.trim().to_ascii_lowercase().as_str(),
                "tracking docs" | "next_steps.md" | "next_steps_non-ai.md"
            )
        })
}

fn recipe_matches_repository(paths: &Paths, definition: &RecipeDefinition) -> Result<bool> {
    let ecosystem_manifest = match definition.applicability.ecosystem.as_str() {
        "cargo" => Some("Cargo.toml"),
        "node" => Some("package.json"),
        "swift" => Some("Package.swift"),
        "python" => None,
        "generic" => None,
        _ => return Ok(false),
    };
    if let Some(manifest) = ecosystem_manifest {
        let metadata = match fs::symlink_metadata(paths.root.join(manifest)) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Ok(false);
        }
    }
    for relative in &definition.applicability.required_files {
        validate_safe_relative_path(relative)?;
        let path = paths.root.join(relative);
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Ok(false);
        }
        read_contained_single_link_file(&paths.root, relative, 4 * 1024 * 1024)?;
    }
    if let Some(expected) = definition.applicability.lockfile_format.as_deref() {
        let Some(lockfile) = definition
            .applicability
            .required_files
            .iter()
            .find(|path| path.contains("lock") || path.ends_with("resolved"))
        else {
            return Ok(false);
        };
        if detect_lockfile_format(paths, lockfile)?.as_deref() != Some(expected) {
            return Ok(false);
        }
    }
    if !definition.applicability.manifest_capabilities.is_empty() {
        let package = paths.root.join("package.json");
        let metadata = match fs::symlink_metadata(&package) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 1024 * 1024
        {
            return Ok(false);
        }
        let manifest: serde_json::Value = serde_json::from_slice(
            &read_contained_single_link_file(&paths.root, "package.json", 1024 * 1024)?,
        )?;
        for capability in &definition.applicability.manifest_capabilities {
            let Some(script) = capability.strip_prefix("script:") else {
                return Ok(false);
            };
            if manifest
                .get("scripts")
                .and_then(|scripts| scripts.get(script))
                .and_then(serde_json::Value::as_str)
                .is_none()
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn detect_lockfile_format(paths: &Paths, relative: &str) -> Result<Option<String>> {
    validate_safe_relative_path(relative)?;
    let path = paths.root.join(relative);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 4 * 1024 * 1024
    {
        bail!("recipe lockfile must be a bounded regular non-symlink file");
    }
    let bytes = read_contained_single_link_file(&paths.root, relative, 4 * 1024 * 1024)?;
    let text = std::str::from_utf8(&bytes).context("recipe lockfile is not UTF-8")?;
    let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
    let format = match name {
        "Cargo.lock" => text
            .lines()
            .find_map(|line| line.trim().strip_prefix("version = "))
            .map(|version| format!("cargo-lock-v{}", version.trim_matches('"'))),
        "package-lock.json" => serde_json::from_str::<serde_json::Value>(text)?
            .get("lockfileVersion")
            .and_then(serde_json::Value::as_u64)
            .map(|version| format!("npm-lock-v{version}")),
        "pnpm-lock.yaml" => text
            .lines()
            .find_map(|line| line.trim().strip_prefix("lockfileVersion:"))
            .map(|version| format!("pnpm-lock-v{}", version.trim().trim_matches('\''))),
        "yarn.lock" => Some(if text.starts_with("# yarn lockfile v1") {
            "yarn-lock-v1".to_string()
        } else {
            "yarn-lock-unknown".to_string()
        }),
        "Package.resolved" => serde_json::from_str::<serde_json::Value>(text)?
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .map(|version| format!("swift-resolved-v{version}")),
        _ => None,
    };
    Ok(format)
}

fn active_activation(paths: &Paths, approved: &ApprovedRecipe) -> Result<RecipeActivation> {
    Ok(RecipeActivation {
        schema: ACTIVATION_SCHEMA.to_string(),
        recipe_id: approved.recipe_id.clone(),
        revision_digest: approved.revision_digest.clone(),
        repository_fingerprint: repository_fingerprint(paths)?,
        compatibility_fingerprint: compatibility_fingerprint(paths, &approved.definition)?,
        status: ActivationStatus::Active,
        updated_at: Utc::now().to_rfc3339(),
        reason: None,
    })
}

fn compatibility_fingerprint(paths: &Paths, definition: &RecipeDefinition) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(&definition.applicability)?);
    for relative in &definition.applicability.required_files {
        validate_safe_relative_path(relative)?;
        let bytes = read_contained_single_link_file(&paths.root, relative, 4 * 1024 * 1024)?;
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(Sha256::digest(&bytes));
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn repository_fingerprint(paths: &Paths) -> Result<String> {
    let canonical = fs::canonicalize(&paths.root)?;
    Ok(sha256_hex(canonical.as_os_str().as_encoded_bytes()))
}

fn require_exact_revision(paths: &Paths, expected: &str) -> Result<()> {
    validate_sha256(expected)?;
    if repository_revision(paths)? != expected {
        bail!("repository revision is stale");
    }
    Ok(())
}

fn isolation_readiness() -> IsolationReadiness {
    require_isolation_backend().unwrap_or_else(|_| IsolationReadiness {
        ready: false,
        backend: "unavailable".to_string(),
        summary:
            "required verification isolation is unavailable; approval and execution are disabled"
                .to_string(),
    })
}

fn read_approved_recipes(root: &Path) -> Result<Vec<ApprovedRecipe>> {
    validate_global_catalog_root_read_only(root)?;
    let mut approved =
        read_json_tree_one_level_private::<ApprovedRecipe>(root, &root.join("approved"))?;
    for record in &approved {
        validate_approved(record)?;
    }
    approved.sort_by(|left, right| left.recipe_id.cmp(&right.recipe_id));
    Ok(approved)
}

pub(crate) fn global_approved_recipe_revisions() -> Result<Vec<String>> {
    #[cfg(test)]
    {
        // Unit tests never inspect the operator's real machine catalog.
        Ok(Vec::new())
    }
    #[cfg(not(test))]
    {
        Ok(read_approved_recipes(&global_catalog_root()?)?
            .into_iter()
            .map(|recipe| format!("{}@{}", recipe.recipe_id, recipe.revision_digest))
            .collect())
    }
}

fn validate_global_catalog_root_read_only(root: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = root.parent() {
                if let Ok(parent_metadata) = fs::symlink_metadata(parent) {
                    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
                        bail!("global learning catalog parent is not a real directory");
                    }
                }
            }
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!("global learning catalog root must be a private real directory");
    }
    let parent = root
        .parent()
        .context("global learning catalog has no parent")?;
    let parent_metadata = fs::symlink_metadata(parent)?;
    if parent_metadata.file_type().is_symlink()
        || !parent_metadata.is_dir()
        || parent_metadata.permissions().mode() & 0o077 != 0
    {
        bail!("global learning catalog parent must be a private real directory");
    }
    Ok(())
}

fn proposal_path(paths: &Paths, recipe_id: &str) -> PathBuf {
    paths
        .recipes
        .join("proposals")
        .join(format!("{recipe_id}.json"))
}

fn activation_directory(paths: &Paths) -> PathBuf {
    paths.recipes.join("activations")
}

fn activation_path(paths: &Paths, recipe_id: &str) -> PathBuf {
    activation_directory(paths).join(format!("{recipe_id}.json"))
}

fn rejection_path(paths: &Paths, recipe_id: &str, digest: &str) -> PathBuf {
    paths
        .recipes
        .join("decisions")
        .join(format!("{}-{}-rejected.json", recipe_id, &digest[..16]))
}

fn failure_transaction_path(paths: &Paths, recipe_id: &str, digest: &str) -> PathBuf {
    paths
        .recipes
        .join("failure-transactions")
        .join(format!("{recipe_id}-{digest}.json"))
}

fn remove_local_transaction(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("learning recovery transaction is not a regular file");
    }
    fs::remove_file(path)?;
    File::open(
        path.parent()
            .context("learning recovery transaction has no parent")?,
    )?
    .sync_all()?;
    Ok(())
}

fn approved_recipe_path(root: &Path, recipe_id: &str, digest: &str) -> PathBuf {
    root.join("approved")
        .join(recipe_id)
        .join(format!("{digest}.json"))
}

fn retired_recipe_path(root: &Path, recipe_id: &str, digest: &str) -> PathBuf {
    root.join("retired")
        .join(recipe_id)
        .join(format!("{digest}.json"))
}

fn is_retired(root: &Path, recipe_id: &str, digest: &str) -> Result<bool> {
    let path = retired_recipe_path(root, recipe_id, digest);
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            let receipt = read_required_global_json::<RecipeDecisionReceipt>(
                root,
                &retired_recipe_path(root, recipe_id, digest),
            )?;
            validate_decision(&receipt, recipe_id, digest, "retired")?;
            Ok(true)
        }
        Ok(_) => bail!("retirement record must be a regular non-symlink file"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn decision_receipt(recipe_id: &str, digest: &str, decision: &str) -> RecipeDecisionReceipt {
    RecipeDecisionReceipt {
        schema: DECISION_SCHEMA.to_string(),
        receipt_id: format!(
            "decision-{}-{}-{}",
            recipe_id,
            &digest[..digest.len().min(16)],
            decision
        ),
        recipe_id: recipe_id.to_string(),
        digest: digest.to_string(),
        decision: decision.to_string(),
        decided_at: Utc::now().to_rfc3339(),
    }
}

fn persist_local_record<T: Serialize>(path: &Path, value: &T, create_new: bool) -> Result<()> {
    let root = path
        .ancestors()
        .find(|ancestor| {
            ancestor.file_name().and_then(|name| name.to_str()) == Some(".orchestrator")
        })
        .context("local learning record is outside .orchestrator")?;
    persist_record(root, path, value, create_new, false)
}

fn persist_global_record<T: Serialize>(
    root: &Path,
    path: &Path,
    value: &T,
    create_new: bool,
) -> Result<()> {
    ensure_global_catalog_root(root)?;
    persist_record(root, path, value, create_new, true)
}

fn persist_record<T: Serialize>(
    root: &Path,
    path: &Path,
    value: &T,
    create_new: bool,
    private: bool,
) -> Result<()> {
    if !path.starts_with(root) {
        bail!("learning record path escapes its root");
    }
    ensure_directory_chain(
        root,
        path.parent().context("learning record has no parent")?,
        private,
    )?;
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        bail!("learning record exceeds size limit");
    }
    if create_new {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("learning record target is a symlink")
            }
            Ok(metadata) if metadata.is_file() => {
                let existing = fs::read(path)?;
                if existing == bytes {
                    return Ok(());
                }
                bail!("immutable learning record already exists with different content");
            }
            Ok(_) => bail!("learning record target is not a regular file"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let temp = path.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        RECORD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mode = if private { 0o600 } else { 0o644 };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&temp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    if create_new {
        fs::hard_link(&temp, path)
            .with_context(|| format!("publish immutable learning record {}", path.display()))?;
        fs::remove_file(&temp)?;
    } else {
        fs::rename(&temp, path)?;
    }
    File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}

fn ensure_directory_chain(root: &Path, target: &Path, private: bool) -> Result<()> {
    if !target.starts_with(root) {
        bail!("learning directory escapes its root");
    }
    let mut current = root.to_path_buf();
    if !current.exists() {
        bail!("learning record root does not exist");
    }
    require_real_directory(&current)?;
    for component in target.strip_prefix(root)?.components() {
        let Component::Normal(name) = component else {
            bail!("learning directory path is invalid");
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    bail!("learning directory chain contains a non-directory or symlink");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)?;
                if private {
                    fs::set_permissions(&current, fs::Permissions::from_mode(0o700))?;
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn ensure_global_catalog_root(root: &Path) -> Result<()> {
    let home = root
        .parent()
        .and_then(Path::parent)
        .context("learning catalog root has no HOME anchor")?;
    require_real_directory(home)?;
    let expected = home.join(".codex-team").join("learning");
    if root != expected {
        bail!("learning catalog root is not the expected private machine catalog");
    }
    let mut current = home.to_path_buf();
    for name in [".codex-team", "learning"] {
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    bail!("learning catalog contains a non-directory or symlink");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)?;
            }
            Err(error) => return Err(error.into()),
        }
        fs::set_permissions(&current, fs::Permissions::from_mode(0o700))?;
    }
    File::open(&current)?.sync_all()?;
    Ok(())
}

struct CatalogLock {
    file: File,
}

impl CatalogLock {
    fn acquire(root: &Path) -> Result<Self> {
        ensure_global_catalog_root(root)?;
        let path = root.join("catalog.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.permissions().mode() & 0o077 != 0
        {
            bail!("learning catalog lock must be a private single-link regular file");
        }
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            bail!("learning catalog is busy");
        }
        Ok(Self { file })
    }
}

impl Drop for CatalogLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn require_real_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("learning root must be a real directory");
    }
    Ok(())
}

fn read_optional_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_RECORD_BYTES
    {
        bail!("learning record must be a bounded regular non-symlink file");
    }
    Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
}

fn read_required_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    read_optional_json(path)?.ok_or_else(|| anyhow!("learning record does not exist"))
}

fn read_optional_global_json<T: for<'de> Deserialize<'de>>(
    root: &Path,
    path: &Path,
) -> Result<Option<T>> {
    if !path.starts_with(root) {
        bail!("global learning record escapes the catalog root");
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_RECORD_BYTES
    {
        bail!("global learning record must be a private bounded single-link regular file");
    }
    let relative = path
        .strip_prefix(root)?
        .to_str()
        .context("global learning record path is not UTF-8")?;
    let (bytes, mode) =
        read_contained_single_link_file_with_mode(root, relative, MAX_RECORD_BYTES)?;
    if mode & 0o077 != 0 {
        bail!("global learning record permissions are not private");
    }
    Ok(Some(serde_json::from_slice(&bytes)?))
}

fn read_required_global_json<T: for<'de> Deserialize<'de>>(root: &Path, path: &Path) -> Result<T> {
    read_optional_global_json(root, path)?
        .ok_or_else(|| anyhow!("global learning record does not exist"))
}

fn read_json_directory<T: for<'de> Deserialize<'de>>(directory: &Path) -> Result<Vec<T>> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("learning record directory must be a real directory");
    }
    let mut paths = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    if paths.len() > 10_000 {
        bail!("learning record directory exceeds entry limit");
    }
    let mut values = Vec::new();
    for path in paths {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("learning record directory contains an invalid entry");
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            bail!("learning record directory contains an unexpected file");
        }
        values.push(read_required_json(&path)?);
    }
    Ok(values)
}

fn read_json_directory_private<T: for<'de> Deserialize<'de>>(
    catalog_root: &Path,
    directory: &Path,
) -> Result<Vec<T>> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!("global learning directory must be private, real, and non-symlink");
    }
    let mut paths = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    if paths.len() > 10_000 {
        bail!("global learning record directory exceeds entry limit");
    }
    let mut values = Vec::new();
    for path in paths {
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            bail!("global learning record directory contains an unexpected entry");
        }
        values.push(read_required_global_json(catalog_root, &path)?);
    }
    Ok(values)
}

fn read_json_tree_one_level_private<T: for<'de> Deserialize<'de>>(
    catalog_root: &Path,
    directory: &Path,
) -> Result<Vec<T>> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!("global learning catalog directory must be private and non-symlink");
    }
    let mut values = Vec::new();
    let mut entries = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort();
    if entries.len() > 10_000 {
        bail!("global learning catalog exceeds entry limit");
    }
    for path in entries {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.permissions().mode() & 0o077 != 0
        {
            bail!("global learning catalog contains an invalid directory entry");
        }
        values.extend(read_json_directory_private::<T>(catalog_root, &path)?);
    }
    Ok(values)
}

fn validate_safe_relative_path(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > 4096 || Path::new(path).is_absolute() {
        bail!("recipe applicability path is invalid");
    }
    for component in Path::new(path).components() {
        if !matches!(component, Component::Normal(_)) {
            bail!("recipe applicability path escapes the repository");
        }
    }
    let first = path
        .split('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        first.as_str(),
        ".git" | ".orchestrator" | "oracles" | "reference"
    ) {
        bail!("recipe applicability path targets a private or protected root");
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!("recipe identifier is invalid");
    }
    Ok(())
}

fn validate_bounded_identifier(label: &str, value: &str, max: usize) -> Result<()> {
    if value.is_empty()
        || value.len() > max
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
    {
        bail!("{label} is invalid");
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("expected SHA-256 value is invalid");
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policy::{AutomationLevel, ReviewTier};
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-team-learning-{name}-{nonce}"))
    }

    fn test_paths(root: &Path) -> Result<Paths> {
        fs::create_dir_all(root.join(".orchestrator/recipes"))?;
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        )?;
        Paths::new(root)
    }

    fn test_command() -> VerificationCommand {
        VerificationCommand::new(
            "git".to_string(),
            vec!["diff".to_string(), "--check".to_string()],
            "learning.test",
        )
    }

    fn test_definition() -> RecipeDefinition {
        RecipeDefinition {
            commands: vec![test_command()],
            applicability: RecipeMatchContract {
                ecosystem: "cargo".to_string(),
                required_files: vec!["Cargo.toml".to_string()],
                lockfile_format: None,
                toolchain_family: vec!["git".to_string()],
                manifest_capabilities: Vec::new(),
                task_kinds: Vec::new(),
                evidence_only: true,
            },
            limits: RecipeLimits::default(),
            safety_classification: "evidence_only_no_product_writes_no_network".to_string(),
        }
    }

    fn approved_recipe(recipe_id: &str) -> Result<ApprovedRecipe> {
        let definition = test_definition();
        Ok(ApprovedRecipe {
            schema: APPROVED_SCHEMA.to_string(),
            recipe_id: recipe_id.to_string(),
            revision_digest: canonical_definition_digest(&definition)?,
            approved_at: Utc::now().to_rfc3339(),
            approval_receipt_id: format!("approve-{recipe_id}"),
            origin_repository_fingerprint: "a".repeat(64),
            validation_evidence_hashes: vec!["b".repeat(64)],
            definition,
        })
    }

    fn persist_test_approved(root: &Path, approved: &ApprovedRecipe) -> Result<()> {
        let path = approved_recipe_path(root, &approved.recipe_id, &approved.revision_digest);
        persist_global_record(root, &path, approved, true)
    }

    fn evidence_task() -> TaskSpec {
        TaskSpec {
            task_id: "LEARN-1".to_string(),
            parent_task: None,
            status: "approved".to_string(),
            priority: "normal".to_string(),
            review_tier: ReviewTier::RustOnly,
            automation_level: AutomationLevel::RustOnly,
            dependencies: Vec::new(),
            title: "Verify".to_string(),
            kind: "verification".to_string(),
            objective: "Collect evidence".to_string(),
            why_unblocks_progress: "Evidence".to_string(),
            allowed_files: vec!["NEXT_STEPS.md".to_string()],
            read_only_files: vec!["src/**".to_string()],
            forbidden_actions: vec!["No product edits".to_string()],
            exact_steps: vec!["Verify".to_string()],
            success_criteria: vec!["Pass".to_string()],
            tests_to_run: vec!["git diff --check".to_string()],
            stop_conditions: vec!["Stop on failure".to_string()],
            discovery_budget: "Named files".to_string(),
            estimated_minutes: 5,
            requires_network: false,
            prefer_worktree: None,
        }
    }

    #[test]
    fn evidence_only_contract_is_strict() {
        let mut task = evidence_task();
        assert!(task_is_evidence_only(&task));
        task.allowed_files = vec!["src/lib.rs".to_string()];
        assert!(!task_is_evidence_only(&task));
        task.allowed_files = vec!["NEXT_STEPS.md".to_string()];
        task.automation_level = AutomationLevel::AiWorkerRustReview;
        assert!(!task_is_evidence_only(&task));
    }

    #[test]
    fn definition_digest_binds_commands_and_match_contract() -> Result<()> {
        let command = VerificationCommand::new(
            "git".to_string(),
            vec!["diff".to_string(), "--check".to_string()],
            "test",
        );
        let definition = RecipeDefinition {
            commands: vec![command],
            applicability: RecipeMatchContract {
                ecosystem: "generic".to_string(),
                required_files: Vec::new(),
                lockfile_format: None,
                toolchain_family: vec!["git".to_string()],
                manifest_capabilities: Vec::new(),
                task_kinds: Vec::new(),
                evidence_only: true,
            },
            limits: RecipeLimits::default(),
            safety_classification: "evidence_only_no_product_writes_no_network".to_string(),
        };
        let first = canonical_definition_digest(&definition)?;
        let mut changed = definition.clone();
        changed.applicability.ecosystem = "cargo".to_string();
        assert_ne!(first, canonical_definition_digest(&changed)?);
        Ok(())
    }

    #[test]
    fn safe_relative_paths_reject_private_and_escaping_roots() {
        assert!(validate_safe_relative_path("Cargo.toml").is_ok());
        assert!(validate_safe_relative_path("../Cargo.toml").is_err());
        assert!(validate_safe_relative_path(".Orchestrator/state.json").is_err());
        assert!(validate_safe_relative_path("ORACLES/evidence").is_err());
    }

    #[test]
    fn first_success_proposes_and_repeat_observation_only_updates_evidence() -> Result<()> {
        let root = temp_root("proposal");
        fs::create_dir_all(&root)?;
        let paths = test_paths(&root)?;
        let first = propose_observed_commands(
            &paths,
            vec![test_command()],
            vec!["evidence-one".to_string()],
        )?;
        let second = propose_observed_commands(
            &paths,
            vec![test_command()],
            vec!["evidence-two".to_string()],
        )?;
        assert_eq!(first.digest, second.digest);
        assert_eq!(second.observation_count, 2);
        assert_eq!(second.source_evidence_ids.len(), 2);
        assert_eq!(second.observed_outcomes.len(), 2);
        assert!(second
            .observed_outcomes
            .iter()
            .all(|outcome| outcome.disposition == "passed"));
        assert!(!paths.recipes.join("approved").exists());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn legacy_labels_migrate_only_to_unapproved_proposals() -> Result<()> {
        let root = temp_root("legacy");
        fs::create_dir_all(&root)?;
        let paths = test_paths(&root)?;
        fs::write(
            paths.recipes.join("legacy.json"),
            serde_json::to_vec(&json!({
                "recipe_id": "legacy-trusted",
                "status": "auto-runnable",
                "commands": [test_command()],
                "source_logs": ["/private/source/run.jsonl"]
            }))?,
        )?;
        assert_eq!(migrate_legacy_observations(&paths)?, 1);
        let proposals = read_json_directory::<RecipeProposal>(&paths.recipes.join("proposals"))?;
        assert_eq!(proposals.len(), 1);
        assert!(proposals[0]
            .source_evidence_ids
            .iter()
            .all(|source| !source.contains("/private/source")));
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn exact_approval_revalidates_once_and_publishes_private_path_free_revision() -> Result<()> {
        crate::verification::isolation::enable_test_isolation_backend();
        let home = temp_root("approval-home");
        let repo = home.join("repo");
        fs::create_dir_all(&repo)?;
        let status = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&repo)
            .status()?;
        assert!(status.success());
        let paths = test_paths(&repo)?;
        let proposal = propose_observed_commands(
            &paths,
            vec![test_command()],
            vec!["evidence-approval".to_string()],
        )?;
        let revision = repository_revision(&paths)?;
        let catalog = home.join(".codex-team").join("learning");
        assert!(approve_recipe_at_root(
            &paths,
            &proposal.recipe_id,
            &"0".repeat(64),
            &revision,
            &catalog,
        )
        .is_err());
        let approved = approve_recipe_at_root(
            &paths,
            &proposal.recipe_id,
            &proposal.digest,
            &revision,
            &catalog,
        )?;
        assert_eq!(approved.revision_digest, proposal.digest);
        assert_eq!(approved.validation_evidence_hashes.len(), 1);
        let path = approved_recipe_path(&catalog, &approved.recipe_id, &approved.revision_digest);
        let metadata = fs::symlink_metadata(&path)?;
        assert_eq!(metadata.permissions().mode() & 0o077, 0);
        let stored = fs::read_to_string(path)?;
        assert!(!stored.contains(repo.to_string_lossy().as_ref()));
        let replay = approve_recipe_at_root(
            &paths,
            &proposal.recipe_id,
            &proposal.digest,
            &revision,
            &catalog,
        )?;
        assert_eq!(approved, replay);
        fs::remove_dir_all(home)?;
        Ok(())
    }

    #[test]
    fn exact_match_activates_across_repositories_then_drift_deactivates() -> Result<()> {
        let root = temp_root("activation");
        let catalog = root.join("home/.codex-team/learning");
        let repo_one = root.join("one");
        let repo_two = root.join("two");
        fs::create_dir_all(root.join("home"))?;
        fs::create_dir_all(&repo_one)?;
        fs::create_dir_all(&repo_two)?;
        let paths_one = test_paths(&repo_one)?;
        let paths_two = test_paths(&repo_two)?;
        let approved = approved_recipe("cross-repo")?;
        persist_test_approved(&catalog, &approved)?;

        let task = evidence_task();
        assert!(select_applicable_recipe_from_root(&paths_one, &task, &catalog)?.is_some());
        assert!(select_applicable_recipe_from_root(&paths_two, &task, &catalog)?.is_some());
        fs::write(
            repo_two.join("Cargo.toml"),
            "[package]\nname='drifted'\nversion='0.2.0'\n",
        )?;
        assert!(select_applicable_recipe_from_root(&paths_two, &task, &catalog)?.is_none());
        let activation = read_required_json::<RecipeActivation>(&activation_path(
            &paths_two,
            &approved.recipe_id,
        ))?;
        assert_eq!(activation.status, ActivationStatus::Drifted);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn one_failure_quarantines_locally_and_two_distinct_repositories_retire() -> Result<()> {
        let root = temp_root("retirement");
        let catalog = root.join("home/.codex-team/learning");
        let repo_one = root.join("one");
        let repo_two = root.join("two");
        fs::create_dir_all(root.join("home"))?;
        fs::create_dir_all(&repo_one)?;
        fs::create_dir_all(&repo_two)?;
        let paths_one = test_paths(&repo_one)?;
        let paths_two = test_paths(&repo_two)?;
        let approved = approved_recipe("failure-policy")?;
        persist_test_approved(&catalog, &approved)?;
        let command = test_command();

        quarantine_after_failure_at_root(&paths_one, &approved, &command, &catalog)?;
        assert!(!is_retired(
            &catalog,
            &approved.recipe_id,
            &approved.revision_digest
        )?);
        let first = read_required_json::<RecipeActivation>(&activation_path(
            &paths_one,
            &approved.recipe_id,
        ))?;
        assert_eq!(first.status, ActivationStatus::Quarantined);

        quarantine_after_failure_at_root(&paths_two, &approved, &command, &catalog)?;
        assert!(is_retired(
            &catalog,
            &approved.recipe_id,
            &approved.revision_digest
        )?);
        assert!(
            select_applicable_recipe_from_root(&paths_one, &evidence_task(), &catalog)?.is_none()
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn pending_failure_transaction_recovers_global_evidence_and_local_quarantine() -> Result<()> {
        let root = temp_root("failure-recovery");
        let catalog = root.join("home/.codex-team/learning");
        let repo = root.join("repo");
        fs::create_dir_all(root.join("home"))?;
        fs::create_dir_all(&repo)?;
        let paths = test_paths(&repo)?;
        let approved = approved_recipe("recovery")?;
        persist_test_approved(&catalog, &approved)?;
        let activation = RecipeActivation {
            status: ActivationStatus::Quarantined,
            reason: Some("isolated recipe command failed".to_string()),
            updated_at: Utc::now().to_rfc3339(),
            ..active_activation(&paths, &approved)?
        };
        let failure = RecipeFailureReceipt {
            schema: FAILURE_SCHEMA.to_string(),
            recipe_id: approved.recipe_id.clone(),
            revision_digest: approved.revision_digest.clone(),
            repository_fingerprint: activation.repository_fingerprint.clone(),
            failed_at: Utc::now().to_rfc3339(),
            command_fingerprint: sha256_hex(test_command().fingerprint().as_bytes()),
        };
        let transaction = RecipeFailureTransaction {
            schema: FAILURE_TRANSACTION_SCHEMA.to_string(),
            activation,
            failure,
        };
        let transaction_path =
            failure_transaction_path(&paths, &approved.recipe_id, &approved.revision_digest);
        persist_local_record(&transaction_path, &transaction, false)?;

        recover_pending_failure_transactions(&paths, &catalog)?;

        assert!(!transaction_path.exists());
        let recovered =
            read_required_json::<RecipeActivation>(&activation_path(&paths, &approved.recipe_id))?;
        assert_eq!(recovered.status, ActivationStatus::Quarantined);
        let global_failure = catalog
            .join("failures")
            .join(&approved.recipe_id)
            .join(&approved.revision_digest)
            .join(format!("{}.json", recovered.repository_fingerprint));
        assert!(
            read_required_global_json::<RecipeFailureReceipt>(&catalog, &global_failure).is_ok()
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn ambiguous_exact_matches_pause_instead_of_selecting() -> Result<()> {
        let root = temp_root("ambiguity");
        let catalog = root.join("home/.codex-team/learning");
        let repo = root.join("repo");
        fs::create_dir_all(root.join("home"))?;
        fs::create_dir_all(&repo)?;
        let paths = test_paths(&repo)?;
        persist_test_approved(&catalog, &approved_recipe("first")?)?;
        persist_test_approved(&catalog, &approved_recipe("second")?)?;
        let error = select_applicable_recipe_from_root(&paths, &evidence_task(), &catalog)
            .expect_err("two exact matches must require human review");
        assert!(error.to_string().contains("multiple approved"));
        assert!(!activation_directory(&paths).exists());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn global_catalog_rejects_symlinked_private_root() -> Result<()> {
        let root = temp_root("catalog-symlink");
        let home = root.join("home");
        let outside = root.join("outside");
        fs::create_dir_all(&home)?;
        fs::create_dir_all(&outside)?;
        std::os::unix::fs::symlink(&outside, home.join(".codex-team"))?;
        let catalog = home.join(".codex-team/learning");
        let approved = approved_recipe("symlink")?;
        assert!(persist_test_approved(&catalog, &approved).is_err());
        assert!(fs::read_dir(&outside)?.next().is_none());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn tampered_limits_and_failed_execution_metrics_are_rejected() -> Result<()> {
        let definition = test_definition();
        let mut proposal = RecipeProposal {
            schema: PROPOSAL_SCHEMA.to_string(),
            recipe_id: "tamper".to_string(),
            digest: canonical_definition_digest(&definition)?,
            observed_at: Utc::now().to_rfc3339(),
            observation_count: 1,
            definition,
            source_evidence_ids: vec!["evidence-one".to_string()],
            observed_outcomes: vec![RecipeObservedOutcome {
                observed_at: Utc::now().to_rfc3339(),
                disposition: "passed".to_string(),
                evidence_hash: sha256_hex(b"evidence-one"),
            }],
            summary: "Bounded proposal".to_string(),
        };
        proposal.definition.limits.max_total_seconds = MAX_TOTAL_TIMEOUT_SECONDS + 1;
        proposal.digest = canonical_definition_digest(&proposal.definition)?;
        assert!(validate_proposal(&proposal).is_err());

        let execution = RecipeExecutionReceipt {
            schema: EXECUTION_SCHEMA.to_string(),
            receipt_id: "exec-tamper".to_string(),
            recipe_id: "tamper".to_string(),
            revision_digest: "a".repeat(64),
            task_id: "TASK-1".to_string(),
            repository_fingerprint: "b".repeat(64),
            compatibility_fingerprint: "c".repeat(64),
            started_at: Utc::now().to_rfc3339(),
            finished_at: Utc::now().to_rfc3339(),
            disposition: ExecutionDisposition::Stopped,
            commands: Vec::new(),
            ai_calls_avoided: 2,
            estimated_tokens_avoided: 37_000,
        };
        assert!(validate_execution(&execution).is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn global_catalog_records_are_private_and_permission_tamper_fails_closed() -> Result<()> {
        let root = temp_root("catalog-permissions");
        let catalog = root.join("home/.codex-team/learning");
        fs::create_dir_all(root.join("home"))?;
        let approved = approved_recipe("private")?;
        persist_test_approved(&catalog, &approved)?;
        let path = approved_recipe_path(&catalog, &approved.recipe_id, &approved.revision_digest);
        assert_eq!(
            fs::symlink_metadata(&catalog)?.permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::symlink_metadata(&path)?.permissions().mode() & 0o777,
            0o600
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
        assert!(read_approved_recipes(&catalog).is_err());
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
