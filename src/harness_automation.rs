//! ca-owned admission and exact review for frozen harness experiments.
//! Legacy plans remain readable; their automatic_promotion flag is not authority.
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{
    canonical_digest, evaluate_harness, load_harness_cycle, HarnessStage, HarnessVerdict,
    LearningVault, SourceRepositorySnapshotV1, VaultRecord,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::path::{Path, PathBuf};

#[path = "harness_automation_budget.rs"]
mod run_budget;
#[path = "harness_vault_migration.rs"]
mod vault_migration;
pub(crate) use run_budget::{
    current_usage, finish as finish_request, reserve as reserve_request, with_budget,
};
pub(crate) use vault_migration::{
    apply_vault_migration, migration_source, preview_vault_migration,
};

pub(crate) fn worker_run_id() -> Result<uuid::Uuid> {
    match ca_automation::runtime::current_run_store() {
        Some(run) => uuid::Uuid::parse_str(&run.status()?.run_id)
            .context("ca harness occurrence must have a durable UUID identity"),
        None => Ok(uuid::Uuid::new_v4()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionReview {
    pub schema: String,
    pub cycle_id: String,
    pub plan_digest: String,
    pub candidate_digest: String,
    pub evaluation_digest: String,
    pub baseline: SourceRepositorySnapshotV1,
    pub baseline_binary_digest: String,
    pub repository: PathBuf,
    pub baseline_binary: PathBuf,
    pub runtime_guidance_digest: String,
    pub digest: String,
}

impl VaultRecord for PromotionReview {
    fn record_directory(&self) -> &'static str {
        "harness/reviews"
    }
    fn record_id(&self) -> &str {
        &self.cycle_id
    }
    fn digest(&self) -> &str {
        &self.digest
    }
    fn set_digest(&mut self, digest: String) {
        self.digest = digest;
    }
}

/// A preview does not create journals or approval records. The current baseline
/// must still be the one evaluated, including the active runtime paragraph.
pub async fn preview_review(
    vault: &LearningVault,
    id: &str,
    repository: &Path,
    binary: &Path,
) -> Result<PromotionReview> {
    let (plan, cycle) = load_harness_cycle(vault, id)?;
    if cycle.stage != HarnessStage::ReadyToPromote
        || !cycle.reservations.is_empty()
        || !cycle.active_reservations.is_empty()
    {
        bail!("candidate is not settled and ready for promotion review");
    }
    let candidate_digest = cycle
        .candidate_digest
        .clone()
        .context("missing evaluated candidate")?;
    let decision = evaluate_harness(
        &plan,
        &candidate_digest,
        &cycle.trials,
        cycle.engineering_checks_passed,
    )?;
    if decision.verdict != HarnessVerdict::Promote
        || cycle.decision.as_ref().map(canonical_digest).transpose()?
            != Some(canonical_digest(&decision)?)
    {
        bail!("promotion evaluation changed or did not pass");
    }
    let repository = repository.canonicalize()?;
    let inspected_repository = crate::harness_snapshot::resolve(vault, &plan, &repository)?;
    let baseline = crate::harness_engineering::source_snapshot(&inspected_repository).await?;
    if baseline != plan.baseline {
        bail!("current repository baseline differs from the evaluated baseline");
    }
    let binary = binary.canonicalize()?;
    let inspect = binary.clone();
    let baseline_binary_digest =
        tokio::task::spawn_blocking(move || crate::harness_engineering::artifact_digest(&inspect))
            .await??;
    if baseline_binary_digest != plan.baseline_binary_digest {
        bail!("current executable differs from the evaluated baseline");
    }
    let runtime_guidance_digest = if plan.source_files.is_empty() {
        let (inputs, harness) = crate::harness_generation::load_candidate(vault, id)?;
        let current = crate::harness_guidance::baseline_instructions(
            vault,
            &inputs.provider.source,
            &inputs.provider.model,
            &inputs.provider.options,
        )?;
        let hints = crate::harness_guidance::baseline_hints(
            vault,
            &inputs.provider.source,
            &inputs.provider.model,
            &inputs.provider.options,
        )?;
        if current != harness.baseline_instructions || hints != harness.baseline_hints {
            bail!("current guidance differs from the evaluated baseline");
        }
        canonical_digest(&(current, hints))?
    } else {
        // Source trials preserve the frozen policy in both lanes; the source
        // candidate and evaluation digests bind that policy and its route.
        plan.route_digest.clone()
    };
    let mut review = PromotionReview {
        schema: "ca.harness-promotion-review.v1".into(),
        cycle_id: id.into(),
        plan_digest: plan.digest,
        candidate_digest,
        evaluation_digest: canonical_digest(&(cycle.trials, decision))?,
        baseline,
        baseline_binary_digest,
        repository,
        baseline_binary: binary,
        runtime_guidance_digest,
        digest: String::new(),
    };
    codex_team::self_improvement::finalize_record(&mut review)?;
    Ok(review)
}

pub async fn confirm_review(
    vault: &LearningVault,
    id: &str,
    repository: &Path,
    binary: &Path,
    expected: &str,
) -> Result<PromotionReview> {
    let mut review = preview_review(vault, id, repository, binary).await?;
    if review.digest != expected {
        bail!("exact promotion review digest required: {}", review.digest);
    }
    let _lock = vault.acquire_lock()?;
    vault.write_immutable(&mut review)?;
    Ok(review)
}

/// Called under the existing transition owner before the first side effect.
/// Ongoing activation/rollback uses its durable transaction recovery instead.
pub async fn require_review(
    vault: &LearningVault,
    id: &str,
    repository: &Path,
    binary: &Path,
) -> Result<()> {
    let approved = vault
        .read_optional_immutable::<PromotionReview>("harness/reviews", id)?
        .context(
            "Needs review: preview the exact candidate and confirm its promotion review digest",
        )?;
    let current = preview_review(vault, id, repository, binary).await?;
    if approved != current {
        bail!("promotion review is stale; candidate, evaluation or current baseline changed");
    }
    Ok(())
}

/// Compatibility commands join ca's machine-wide resource namespace. Test and
/// explicitly supplied vaults retain an injected local namespace.
pub(crate) fn compatibility_resources(
    vault: &LearningVault,
    repository: &Path,
) -> Result<Vec<ca_automation::runtime::ResourceLease>> {
    if ca_automation::runtime::current_run_store().is_some() {
        return Ok(Vec::new());
    }
    let lease_root = compatibility_lease_root(vault)?;
    let repository = repository.canonicalize()?;
    let mut resources = harness_resource_keys(vault.root())?;
    resources.push(format!("repository:{}", repository.display()));
    resources.sort();
    resources.dedup();
    resources
        .iter()
        .map(|resource| ca_automation::runtime::ResourceLease::acquire(&lease_root, resource))
        .collect()
}

pub(crate) fn harness_resource_keys(vault_root: &Path) -> Result<Vec<String>> {
    let mut keys = vec![format!(
        "harness:{}",
        crate::automation_runtime::storage_path(vault_root).display(),
    )];
    if let Some(origin) = migration_source(vault_root)? {
        keys.push(format!(
            "harness:{}",
            crate::automation_runtime::storage_path(&origin).display(),
        ));
    }
    keys.sort();
    keys.dedup();
    Ok(keys)
}

pub(crate) fn compatibility_harness_resources(
    vault: &LearningVault,
) -> Result<Vec<ca_automation::runtime::ResourceLease>> {
    if ca_automation::runtime::current_run_store().is_some() {
        return Ok(Vec::new());
    }
    let root = compatibility_lease_root(vault)?;
    harness_resource_keys(vault.root())?
        .iter()
        .map(|key| ca_automation::runtime::ResourceLease::acquire(&root, key))
        .collect()
}

pub(crate) fn compatibility_lease_root(vault: &LearningVault) -> Result<PathBuf> {
    let parent = vault.root().parent().context("vault parent missing")?;
    let lease_root = if parent.file_name().is_some_and(|name| name == ".codex-team") {
        parent
            .parent()
            .context("vault home missing")?
            .join(".local/share/ca/automation-resources/v2")
    } else if vault
        .root()
        .file_name()
        .is_some_and(|name| name == "harness")
        && parent.file_name().is_some_and(|name| name == "automation")
    {
        let profile_data = parent.parent().context("profile data root missing")?;
        let data = if profile_data
            .parent()
            .is_some_and(|path| path.file_name().is_some_and(|name| name == "profiles"))
        {
            profile_data
                .parent()
                .and_then(Path::parent)
                .context("shared profile data root missing")?
        } else {
            profile_data
        };
        data.join("automation-resources/v2")
    } else {
        parent.join("automation-resources/v2")
    };
    Ok(lease_root)
}

pub(crate) fn compatibility_repository_lease<'a>(
    resources: &'a [ca_automation::runtime::ResourceLease],
    vault: &LearningVault,
    repository: &Path,
) -> Result<Option<&'a ca_automation::runtime::ResourceLease>> {
    if resources.is_empty() {
        return Ok(None);
    }
    let root = compatibility_lease_root(vault)?;
    let resource = format!("repository:{}", repository.canonicalize()?.display());
    resources
        .iter()
        .find(|lease| lease.validate_binding(&root, &resource).is_ok())
        .map(Some)
        .context("harness repository ownership proof is unavailable")
}

pub async fn discover(data_dir: PathBuf, vault_root: PathBuf) -> Result<serde_json::Value> {
    tokio::task::spawn_blocking(move || {
        let vault = LearningVault::existing_at(vault_root)?;
        Ok(serde_json::to_value(crate::harness_discovery::discover(
            &data_dir,
            vault.as_ref(),
        )?)?)
    })
    .await?
}

pub async fn evaluate(
    vault_root: PathBuf,
    cycle_id: String,
    expected_plan_digest: String,
    repository: PathBuf,
    baseline_binary: PathBuf,
    config: crate::config::CaConfig,
) -> Result<serde_json::Value> {
    let vault = LearningVault::existing_at(vault_root)?
        .context("harness experiment vault is unavailable")?;
    let (plan, before) = load_harness_cycle(&vault, &cycle_id)?;
    if plan.digest != expected_plan_digest {
        bail!("harness evaluation plan changed since automation confirmation");
    }
    crate::harness_control::run_background(
        vault.clone(),
        cycle_id.clone(),
        repository,
        baseline_binary,
        config,
        None,
    )
    .await?;
    let (_, cycle) = load_harness_cycle(&vault, &cycle_id)?;
    let usage = current_usage(&vault)?.unwrap_or(usage_delta(
        std::slice::from_ref(&before),
        std::slice::from_ref(&cycle),
    )?);
    Ok(
        serde_json::json!({"schema":"ca.harness-automation-result.v1","cycle":cycle,"needs_review":cycle.stage == HarnessStage::ReadyToPromote,"usage":usage}),
    )
}

const HARNESS_AUTOMATION_ID: &str = "harness-self-improvement";

/// Admission and the worker both check the repository behind the granted
/// workspace. A profile edit cannot redirect a live cycle to an unowned repo.
pub(crate) fn validate_repository_binding(
    schedule: &crate::schedule_v2::ScheduleDefinitionV2,
    config: &crate::config::CaConfig,
) -> Result<()> {
    use crate::schedule_v2::ScheduleTargetV2;
    let repository = match &schedule.target {
        ScheduleTargetV2::HarnessEvaluation { repository, .. } => repository.clone(),
        ScheduleTargetV2::HarnessCycle { .. } => config
            .harness_improvement
            .repository
            .clone()
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
        _ => return Ok(()),
    };
    if repository
        .canonicalize()
        .context("harness repository is unavailable")?
        != schedule
            .workspace
            .canonicalize()
            .context("harness workspace is unavailable")?
    {
        bail!("harness repository binding differs from the granted workspace; review the target repository before admission");
    }
    Ok(())
}

/// Only a target's exact frozen route may contribute environment credentials.
/// Discovery and already settled evaluations do not resolve provider secrets.
pub(crate) fn credential_providers(
    schedule: &crate::schedule_v2::ScheduleDefinitionV2,
    config: &crate::config::CaConfig,
    profile: &crate::runtime::ActiveProfile,
) -> Result<Vec<String>> {
    use crate::schedule_v2::ScheduleTargetV2;
    validate_repository_binding(schedule, config)?;
    let route = match &schedule.target {
        ScheduleTargetV2::HarnessDiscovery { .. } => return Ok(Vec::new()),
        ScheduleTargetV2::HarnessEvaluation {
            vault_root,
            cycle_id,
            expected_plan_digest,
            ..
        } => {
            let vault = LearningVault::existing_at(vault_root.clone())?
                .context("harness experiment vault is unavailable")?;
            let (plan, cycle) = load_harness_cycle(&vault, cycle_id)?;
            if &plan.digest != expected_plan_digest {
                bail!("harness evaluation plan changed since automation confirmation");
            }
            if !matches!(
                cycle.stage,
                HarnessStage::EvaluationFrozen
                    | HarnessStage::CandidateReady
                    | HarnessStage::Evaluating
            ) {
                return Ok(Vec::new());
            }
            let inputs = crate::harness_freeze::load_inputs(&vault, cycle_id).or_else(|_| {
                crate::harness_generation::load_candidate(&vault, cycle_id)
                    .map(|(inputs, _)| inputs)
            })?;
            (inputs.provider.source.provider, inputs.provider.model)
        }
        ScheduleTargetV2::HarnessCycle { vault_root } => {
            let source = vault_migration::migration_source(vault_root)?
                .unwrap_or_else(|| vault_root.clone());
            let vault = LearningVault::existing_at(source)?;
            let repository = config
                .harness_improvement
                .repository
                .clone()
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
            if let Some(vault) = vault.as_ref() {
                if let Some(route) =
                    crate::harness_idle::resumable_provider(vault, profile.label(), &repository)?
                {
                    if schedule.provider.as_deref() != Some(&route.0)
                        || schedule.model.as_deref() != Some(&route.1)
                    {
                        bail!("resumed harness route differs from the automation's granted route; review the frozen experiment route before native admission");
                    }
                    return Ok(vec![route.0]);
                }
                let abandoned =
                    crate::harness_idle::abandonable_owner(vault, profile.label(), &repository)?;
                if codex_team::self_improvement::list_harness_cycles(vault)?
                    .iter()
                    .any(|cycle| {
                        Some(cycle.id.as_str()) != abandoned.as_deref()
                            && crate::harness_control::blocks_new_cycle(cycle)
                    })
                {
                    return Ok(Vec::new());
                }
            }
            (
                config.defaults.provider.clone(),
                config.defaults.model.clone(),
            )
        }
        _ => return Ok(Vec::new()),
    };
    if schedule.provider.as_deref() != Some(&route.0) || schedule.model.as_deref() != Some(&route.1)
    {
        bail!("harness provider credentials require the exact frozen route in the automation definition");
    }
    Ok(vec![route.0])
}

/// Externally created evaluation targets must grant the frozen route and budget
/// before a ca-owned worker starts. Review-only settled candidates need no model.
pub(crate) fn validate_target(
    schedule: &crate::schedule_v2::ScheduleDefinitionV2,
    budget: &crate::automation::AutomationBudgetV1,
    pricing: &crate::automation::PricingBindingV1,
) -> Result<()> {
    let crate::schedule_v2::ScheduleTargetV2::HarnessEvaluation {
        vault_root,
        cycle_id,
        expected_plan_digest,
        repository,
        ..
    } = &schedule.target
    else {
        return Ok(());
    };
    if repository.canonicalize()? != schedule.workspace.canonicalize()? {
        bail!("harness evaluation repository differs from the granted workspace");
    }
    let vault = LearningVault::existing_at(vault_root.clone())?
        .context("harness experiment vault is unavailable")?;
    let (plan, cycle) = load_harness_cycle(&vault, cycle_id)?;
    if &plan.digest != expected_plan_digest {
        bail!("harness evaluation plan changed since automation confirmation");
    }
    if cycle.stage == HarnessStage::ReadyToPromote
        || !crate::harness_control::blocks_new_cycle(&cycle)
    {
        return Ok(());
    }
    let inputs = crate::harness_freeze::load_inputs(&vault, cycle_id).or_else(|_| {
        // Earlier V1 candidates carry their frozen route in the candidate
        // record rather than the later creation-input checkpoint.
        crate::harness_generation::load_candidate(&vault, cycle_id).map(|(inputs, _)| inputs)
    })?;
    let frozen = inputs.provider;
    let price_digest = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&frozen.price)?)
    );
    if schedule.provider.as_deref() != Some(&frozen.source.provider)
        || schedule.model.as_deref() != Some(&frozen.model)
        || !matches!(pricing, crate::automation::PricingBindingV1::Metered { price_card_sha256 } if price_card_sha256 == &price_digest)
        || budget.provider_requests == 0
        || budget.input_tokens < u64::from(frozen.context_tokens)
        || budget.output_tokens < u64::from(frozen.output_tokens)
        || budget.runtime_secs < plan.settings.maximum_active_seconds
        || schedule.runtime_secs < plan.settings.maximum_active_seconds
        || budget.max_cost_usd < run_budget::cost_usd(plan.settings.maximum_microusd)?
    {
        bail!("harness target route, verified price or automation budget does not cover the frozen evaluation plan");
    }
    Ok(())
}

fn policy_digest(
    config: &crate::config::CaConfig,
    frozen: &crate::harness_budget::FrozenHarnessProvider,
) -> Result<String> {
    canonical_digest(&(
        &config.harness_improvement,
        &frozen.source,
        &frozen.model,
        &frozen.options,
        &frozen.price,
    ))
}

/// The explicitly enabled legacy setting supplies the existing experiment grant.
/// A paused or previously registered record is never silently enabled/rewritten.
pub(crate) fn register_enabled(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    config: &crate::config::CaConfig,
) -> Result<()> {
    use crate::automation::{
        AutomationBudgetV1, AutomationDefinitionV1, AutomationKindV1, AutomationNotificationPolicy,
        AutomationStore, PricingBindingV1,
    };
    use crate::schedule_v2::{
        MissedRunPolicy, ScheduleDefinitionV2, ScheduleDeliveryV2, ScheduleTargetV2,
        ScheduleTriggerV2, WorkspacePolicy,
    };
    if matches!(profile, crate::runtime::ActiveProfile::Bare) {
        return Ok(());
    }
    let store = AutomationStore::for_paths(paths, profile);
    let existing = store
        .records()?
        .into_iter()
        .find(|record| record.definition.id == HARNESS_AUTOMATION_ID);
    if !config.harness_improvement.enabled {
        if let Some(record) = existing {
            if record.enabled && record.active_run_id.is_none() {
                store.set_enabled(HARNESS_AUTOMATION_ID, false, chrono::Utc::now())?;
            }
        }
        return Ok(());
    }
    let native_vault = paths.profile_data_dir(profile).join("automation/harness");
    if let Some(legacy) = LearningVault::existing_machine()? {
        let pending = codex_team::self_improvement::list_harness_cycles(&legacy)?
            .iter()
            .any(crate::harness_control::blocks_new_cycle);
        if pending && migration_source(&native_vault)?.as_deref() != Some(legacy.root()) {
            if existing
                .as_ref()
                .is_some_and(|record| record.enabled && record.active_run_id.is_none())
            {
                store.block(
                    HARNESS_AUTOMATION_ID,
                    "pending legacy experiments require exact vault migration review",
                )?;
            }
            bail!("pending legacy harness experiments require exact vault migration review before native admission");
        }
    }
    if existing.is_some() {
        return Ok(());
    }
    let frozen = crate::harness_control::configured_provider(config, paths, profile)?;
    let workspace = config
        .harness_improvement
        .repository
        .clone()
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let mut per_run = AutomationBudgetV1::per_run_default();
    per_run.provider_requests = 64;
    per_run.input_tokens = 2_000_000;
    per_run.output_tokens = 262_144;
    per_run.tool_calls = 512;
    per_run.runtime_secs = 7_200;
    per_run.max_cost_usd = "20".parse().map_err(anyhow::Error::msg)?;
    let mut rolling = AutomationBudgetV1::rolling_default();
    rolling.max_cost_usd = "600".parse().map_err(anyhow::Error::msg)?;
    rolling.input_tokens = 60_000_000;
    rolling.output_tokens = 8_000_000;
    rolling.provider_requests = 1_920;
    rolling.tool_calls = 15_360;
    let now = chrono::Utc::now();
    let definition = AutomationDefinitionV1 {
        schema: "ca.automation.v1".into(),
        id: HARNESS_AUTOMATION_ID.into(),
        name: "Harness self-improvement".into(),
        automation_kind: AutomationKindV1::Standalone,
        schedule: ScheduleDefinitionV2 {
            version: 2,
            id: HARNESS_AUTOMATION_ID.into(),
            provider: Some(frozen.source.provider.clone()),
            model: Some(frozen.model.clone()),
            workspace,
            runtime_secs: per_run.runtime_secs,
            output_bytes: per_run.output_bytes,
            tools: Vec::new(),
            grants: Vec::new(),
            allow_network: true,
            skills: Vec::new(),
            workspace_policy: WorkspacePolicy::ExistingReadOnly,
            pinned_revision: None,
            trigger: ScheduleTriggerV2::Interval {
                seconds: 300,
                anchor: Some(now),
            },
            missed_run_policy: MissedRunPolicy::Skip,
            target: ScheduleTargetV2::HarnessCycle {
                vault_root: native_vault,
            },
            delivery: ScheduleDeliveryV2::None,
        },
        reasoning_effort: None,
        pricing: PricingBindingV1::Metered {
            price_card_sha256: format!(
                "{:x}",
                sha2::Sha256::digest(serde_json::to_vec(&frozen.price)?)
            ),
        },
        context_sha256: vec![policy_digest(config, &frozen)?],
        per_run_budget: per_run,
        rolling_30_day_budget: rolling,
        notification_policy: AutomationNotificationPolicy::MeaningfulChanges,
    }
    .normalize(now)?;
    let digest = crate::automation::definition_digest(&definition)?;
    let count =
        crate::schedule::configured_schedule_count(paths, profile).map_err(anyhow::Error::msg)?;
    store.add_confirmed(definition, &digest, count, now)?;
    Ok(())
}

pub(crate) async fn cycle(
    paths: crate::runtime::RuntimePaths,
    profile: crate::runtime::ActiveProfile,
    config: crate::config::CaConfig,
    vault_root: PathBuf,
) -> Result<serde_json::Value> {
    let requested_vault = vault_root;
    let vault_root = tokio::task::spawn_blocking(move || {
        vault_migration::reconcile_vault_migration(&requested_vault)
    })
    .await??;
    let record = crate::automation::AutomationStore::for_paths(&paths, &profile)
        .records()?
        .into_iter()
        .find(|record| record.definition.id == HARNESS_AUTOMATION_ID)
        .context("harness automation is not registered")?;
    let frozen = crate::harness_control::configured_provider(&config, &paths, &profile)?;
    if record.definition.context_sha256 != [policy_digest(&config, &frozen)?] {
        bail!(
            "harness automation policy or route changed; preview and confirm an updated automation"
        );
    }
    let before = LearningVault::existing_at(vault_root.clone())?
        .as_ref()
        .map(codex_team::self_improvement::list_harness_cycles)
        .transpose()?
        .unwrap_or_default();
    let foreground = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let message =
        crate::harness_idle::run_once_at(config, paths, profile, foreground, vault_root.clone())
            .await?;
    let result_vault = LearningVault::existing_at(vault_root)?;
    let cycles = result_vault
        .as_ref()
        .map(codex_team::self_improvement::list_harness_cycles)
        .transpose()?
        .unwrap_or_default();
    let usage = result_vault
        .as_ref()
        .map(current_usage)
        .transpose()?
        .flatten()
        .unwrap_or(usage_delta(&before, &cycles)?);
    let needs_review = cycles
        .iter()
        .any(|cycle| cycle.stage == HarnessStage::ReadyToPromote);
    let unchanged = message.is_none() && canonical_digest(&before)? == canonical_digest(&cycles)?;
    Ok(
        serde_json::json!({"schema":"ca.harness-automation-result.v1", "status":if unchanged {"unchanged"} else if needs_review {"needs_review"} else {"changed"}, "summary":message, "cycles":cycles,"needs_review":needs_review, "usage":usage}),
    )
}

fn usage_delta(
    before: &[codex_team::self_improvement::HarnessCycle],
    after: &[codex_team::self_improvement::HarnessCycle],
) -> Result<crate::automation::AutomationUsageCountersV1> {
    let total_cost = |cycles: &[codex_team::self_improvement::HarnessCycle]| {
        cycles.iter().fold(0u64, |total, cycle| {
            total.saturating_add(cycle.settled_microusd)
        })
    };
    let total_requests = |cycles: &[codex_team::self_improvement::HarnessCycle]| {
        cycles.iter().fold(0usize, |total, cycle| {
            total.saturating_add(cycle.charged_requests.len())
        })
    };
    let cost = total_cost(after).saturating_sub(total_cost(before));
    Ok(crate::automation::AutomationUsageCountersV1 {
        provider_requests: u32::try_from(
            total_requests(after).saturating_sub(total_requests(before)),
        )
        .unwrap_or(u32::MAX),
        cost_usd: format!("{}.{:06}", cost / 1_000_000, cost % 1_000_000)
            .parse()
            .map_err(anyhow::Error::msg)?,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential_schedule(
        workspace: PathBuf,
        target: crate::schedule_v2::ScheduleTargetV2,
    ) -> crate::schedule_v2::ScheduleDefinitionV2 {
        crate::schedule_v2::ScheduleDefinitionV2 {
            version: 2,
            id: "harness-credential-fixture".into(),
            provider: None,
            model: None,
            workspace,
            runtime_secs: 900,
            output_bytes: 4096,
            tools: Vec::new(),
            grants: Vec::new(),
            allow_network: true,
            skills: Vec::new(),
            workspace_policy: crate::schedule_v2::WorkspacePolicy::ExistingReadOnly,
            pinned_revision: None,
            trigger: crate::schedule_v2::ScheduleTriggerV2::Interval {
                seconds: 300,
                anchor: None,
            },
            missed_run_policy: crate::schedule_v2::MissedRunPolicy::Skip,
            target,
            delivery: crate::schedule_v2::ScheduleDeliveryV2::None,
        }
    }

    #[tokio::test]
    async fn harness_repository_binding_rejects_redirects_before_native_storage_or_dispatch() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let workspace = root.join("granted-repository");
        let other = root.join("other-repository");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&other).unwrap();
        let mut schedule = credential_schedule(
            workspace.clone(),
            crate::schedule_v2::ScheduleTargetV2::HarnessEvaluation {
                vault_root: root.join("unopened-vault"),
                cycle_id: "repository-binding".into(),
                expected_plan_digest: "a".repeat(64),
                repository: other.clone(),
                baseline_binary: root.join("unavailable-binary"),
            },
        );
        let mut config = crate::config::CaConfig::default();
        assert!(validate_repository_binding(&schedule, &config).is_err());
        assert!(validate_target(
            &schedule,
            &crate::automation::AutomationBudgetV1::per_run_default(),
            &crate::automation::PricingBindingV1::ModelFree,
        )
        .unwrap_err()
        .to_string()
        .contains("repository differs"));
        assert!(!root.join("unopened-vault").exists());
        schedule.target = crate::schedule_v2::ScheduleTargetV2::HarnessCycle {
            vault_root: root.join("unopened-vault"),
        };
        config.harness_improvement.repository = Some(workspace.clone());
        validate_repository_binding(&schedule, &config).unwrap();
        config.harness_improvement.repository = Some(other.clone());
        assert!(validate_repository_binding(&schedule, &config).is_err());
        let paths = crate::runtime::RuntimePaths::from_home(root.join("home"));
        std::fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
        std::fs::write(&paths.config_file, toml::to_string(&config).unwrap()).unwrap();
        let definition = crate::automation::AutomationDefinitionV1 {
            schema: "ca.automation.v1".into(),
            id: schedule.id.clone(),
            name: "Repository binding fixture".into(),
            automation_kind: crate::automation::AutomationKindV1::Standalone,
            schedule,
            reasoning_effort: None,
            pricing: crate::automation::PricingBindingV1::ModelFree,
            context_sha256: Vec::new(),
            per_run_budget: crate::automation::AutomationBudgetV1::per_run_default(),
            rolling_30_day_budget: crate::automation::AutomationBudgetV1::rolling_default(),
            notification_policy: crate::automation::AutomationNotificationPolicy::AllResults,
        };
        let profile = crate::runtime::ActiveProfile::Default;
        let error = crate::automation_runtime::execute(
            &paths,
            &profile,
            &definition,
            uuid::Uuid::new_v4(),
            Vec::new(),
        )
        .await
        .unwrap_err();
        assert!(error.contains("repository binding"), "{error}");
        assert!(!paths
            .profile_data_dir(&profile)
            .join("automation-engine")
            .exists());
        assert!(!root.join("unopened-vault").exists());
        assert_eq!(std::fs::read_dir(&workspace).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(&other).unwrap().count(), 0);
    }

    #[test]
    fn legacy_harness_claims_and_publication_proofs_use_the_same_exact_resources() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let repository = root.join("repository");
        let other = root.join("other");
        std::fs::create_dir(&repository).unwrap();
        std::fs::create_dir(&other).unwrap();
        let vault = LearningVault::open_at(root.join("vault")).unwrap();
        let resources = compatibility_resources(&vault, &repository).unwrap();
        assert!(compatibility_harness_resources(&vault).is_err());
        assert!(
            compatibility_repository_lease(&resources, &vault, &repository)
                .unwrap()
                .is_some()
        );
        assert!(compatibility_repository_lease(&resources, &vault, &other).is_err());
        drop(resources);
        let rollback = compatibility_harness_resources(&vault).unwrap();
        assert!(compatibility_resources(&vault, &repository).is_err());
        drop(rollback);
        assert!(compatibility_resources(&vault, &repository).is_ok());
    }

    #[test]
    fn harness_credentials_select_only_the_exact_active_frozen_route() {
        let temp = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(temp.path().join("vault")).unwrap();
        let (plan, inputs, proposal) = crate::harness_generation::tests::inputs();
        let provider = inputs.provider.source.provider.clone();
        let model = inputs.provider.model.clone();
        let plan = crate::harness_generation::tests::save_fixture(&vault, plan, inputs, proposal);
        let mut schedule = credential_schedule(
            temp.path().into(),
            crate::schedule_v2::ScheduleTargetV2::HarnessEvaluation {
                vault_root: vault.root().into(),
                cycle_id: plan.id.clone(),
                expected_plan_digest: plan.digest,
                repository: temp.path().into(),
                baseline_binary: "/unavailable-binary".into(),
            },
        );
        let config = crate::config::CaConfig::default();
        let profile = crate::runtime::ActiveProfile::Default;
        assert!(credential_providers(&schedule, &config, &profile).is_err());
        schedule.provider = Some(provider.clone());
        schedule.model = Some(model);
        assert_eq!(
            credential_providers(&schedule, &config, &profile).unwrap(),
            vec![provider]
        );
        schedule.provider = Some("unrelated-key".into());
        assert!(credential_providers(&schedule, &config, &profile).is_err());
        codex_team::self_improvement::update_harness_cycle(&vault, &plan.id, |cycle| {
            cycle.stage = HarnessStage::ReadyToPromote;
            Ok(())
        })
        .unwrap();
        schedule.provider = None;
        schedule.model = None;
        assert!(credential_providers(&schedule, &config, &profile)
            .unwrap()
            .is_empty());
        schedule.target = crate::schedule_v2::ScheduleTargetV2::HarnessDiscovery {
            vault_root: vault.root().into(),
        };
        assert!(credential_providers(&schedule, &config, &profile)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn disabled_harness_registration_is_inert() {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(temp.path());
        register_enabled(
            &paths,
            &crate::runtime::ActiveProfile::Default,
            &crate::config::CaConfig::default(),
        )
        .unwrap();
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn resumed_evaluation_needs_review_without_provider_or_publication() {
        let temp = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(temp.path().join("vault")).unwrap();
        let (plan, inputs, proposal) = crate::harness_generation::tests::inputs();
        let plan = crate::harness_generation::tests::save_fixture(&vault, plan, inputs, proposal);
        codex_team::self_improvement::update_harness_cycle(&vault, &plan.id, |cycle| {
            cycle.stage = HarnessStage::ReadyToPromote;
            Ok(())
        })
        .unwrap();
        let result = evaluate(
            vault.root().into(),
            plan.id.clone(),
            plan.digest.clone(),
            temp.path().into(),
            "/unavailable-binary".into(),
            crate::config::CaConfig::default(),
        )
        .await
        .unwrap();
        assert_eq!(result["needs_review"], true);
        assert_eq!(result["usage"]["provider_requests"], 0);
        assert!(crate::harness_guidance::report(&vault, &plan.id)
            .unwrap()
            .is_none());
        assert!(vault
            .read_optional_immutable::<PromotionReview>("harness/reviews", &plan.id)
            .unwrap()
            .is_none());
        assert!(evaluate(
            vault.root().into(),
            plan.id,
            "0".repeat(64),
            temp.path().into(),
            "/unavailable-binary".into(),
            crate::config::CaConfig::default()
        )
        .await
        .is_err());
    }
}
