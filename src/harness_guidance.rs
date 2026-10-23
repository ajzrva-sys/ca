//! Evaluated replacement of the existing repository-work prompt paragraph.
//! Active pointers are scoped to one build, provider route, model and options.
use crate::agent::client::ChatRequestOptions;
use crate::agent::stream::ProviderStateSourceV1;
use crate::harness_activity::HarnessActivity;
use crate::request::{PreparedModelRequest, RequestKind};
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{
    canonical_digest, evaluate_harness, load_harness_cycle, update_harness_cycle, HarnessStage,
    HarnessVerdict, LearningVault,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

// This exact paragraph already exists in prompts/system_prefix.md. The rest of
// the system prompt, workspace instructions and host policy are not replaced.
pub const BASELINE: &str = "For repository work, identify likely symbols before reading whole files. Batch independent read-only searches and reads in one tool turn; prefer target symbols and narrow line ranges, and use condensed error context before requesting raw logs.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveGuidance {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub source_cycle: bool,
    pub cycle_id: String,
    pub plan_digest: String,
    pub candidate_digest: String,
    pub scope: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub hints: crate::harness_hints::HintPolicy,
    pub instructions: String,
}

/// Policy held constant in both source-trial lanes. The new build identity is
/// obtained from the signed artifact; route limits remain bound to the plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceGuidance {
    pub build_digest: String,
    pub source: ProviderStateSourceV1,
    pub model: String,
    pub options: ChatRequestOptions,
    pub context_tokens: u32,
    pub output_tokens: u32,
    pub request_seconds: u64,
    pub instructions: String,
    pub hints: crate::harness_hints::HintPolicy,
}
impl SourceGuidance {
    pub(crate) fn from_evaluated(
        build_digest: String,
        provider: &crate::harness_budget::FrozenHarnessProvider,
        harness: &crate::harness_trials::TrialHarness,
    ) -> Result<Self> {
        if harness.baseline_instructions != harness.candidate_instructions
            || harness.baseline_hints != harness.candidate_hints
            || harness.candidate_commit.is_none()
        {
            bail!("source activation must preserve the policy evaluated in both lanes");
        }
        Ok(Self {
            build_digest,
            source: provider.source.clone(),
            model: provider.model.clone(),
            options: provider.options.clone(),
            context_tokens: provider.context_tokens,
            output_tokens: provider.output_tokens,
            request_seconds: provider.request_seconds,
            instructions: harness.candidate_instructions.clone(),
            hints: harness.candidate_hints.clone(),
        })
    }

    fn candidate(
        &self,
        plan: &codex_team::self_improvement::HarnessPlan,
        promotion: &crate::harness_promotion::Promotion,
    ) -> Result<ActiveGuidance> {
        let route = canonical_digest(&serde_json::json!({
            "source": self.source, "model": self.model, "options": self.options,
            "context_tokens": self.context_tokens, "output_tokens": self.output_tokens,
            "request_seconds": self.request_seconds,
        }))?;
        let evaluated = crate::harness_trials::TrialHarness {
            baseline_instructions: self.instructions.clone(),
            candidate_instructions: self.instructions.clone(),
            baseline_hints: self.hints.clone(),
            candidate_hints: self.hints.clone(),
            candidate_binary_digest: promotion.artifacts.evaluated_digest.clone(),
            candidate_commit: Some(promotion.candidate_source.head.clone()),
        };
        evaluated.validate(plan)?;
        if route != plan.route_digest
            || evaluated.digest()? != promotion.artifacts.candidate_digest
            || self.build_digest.len() != 64
            || !self.build_digest.bytes().all(|b| b.is_ascii_hexdigit())
        {
            bail!("source guidance differs from the evaluated policy or provider");
        }
        Ok(ActiveGuidance {
            source_cycle: true,
            cycle_id: plan.id.clone(),
            plan_digest: plan.digest.clone(),
            candidate_digest: evaluated.digest()?,
            scope: scope_at(&self.build_digest, &self.source, &self.model, &self.options)?,
            hints: self.hints.clone(),
            instructions: self.instructions.clone(),
        })
    }
}

/// Publication is replayable before settling the source promotion. Readers
/// require the source transaction and its core cycle to be fully active.
pub(crate) fn publish_source(
    vault: &LearningVault,
    plan: &codex_team::self_improvement::HarnessPlan,
    promotion: &crate::harness_promotion::Promotion,
) -> Result<()> {
    let Some(policy) = &promotion.artifacts.guidance else {
        return Ok(());
    };
    let candidate = policy.candidate(plan, promotion)?;
    if let Some(existing) = read_pointer(vault, &candidate.scope)? {
        if existing != candidate {
            bail!("new source build already has a different guidance policy");
        }
        return Ok(());
    }
    vault.write_mutable(&pointer(&candidate.scope), &Some(&candidate))?;
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Prepared,
    Active,
    RolledBack,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Activation {
    pub build_digest: String,
    pub candidate: ActiveGuidance,
    pub previous: Option<ActiveGuidance>,
    pub trials_digest: String,
    pub phase: Phase,
    pub work: Option<(String, u64)>,
}
fn path(id: &str) -> String {
    format!("harness/receipts/{id}-guidance.json")
}
fn pointer(scope: &str) -> String {
    format!("harness/guidance-active-{scope}.json")
}
fn scope(
    source: &ProviderStateSourceV1,
    model: &str,
    options: &ChatRequestOptions,
) -> Result<String> {
    scope_at(env!("CA_BUILD_DIGEST"), source, model, options)
}
fn scope_at(
    build: &str,
    source: &ProviderStateSourceV1,
    model: &str,
    options: &ChatRequestOptions,
) -> Result<String> {
    canonical_digest(&(build, source, model, options))
}
fn read_pointer(vault: &LearningVault, key: &str) -> Result<Option<ActiveGuidance>> {
    Ok(vault
        .read_mutable::<Option<ActiveGuidance>>(&pointer(key))?
        .flatten())
}
fn save(vault: &LearningVault, record: &Activation) -> Result<()> {
    vault
        .write_mutable(&path(&record.candidate.cycle_id), record)
        .map(|_| ())
}
pub fn report(vault: &LearningVault, id: &str) -> Result<Option<Activation>> {
    let (plan, cycle) = load_harness_cycle(vault, id)?;
    let Some(record) = vault.read_mutable::<Activation>(&path(id))? else {
        return Ok(None);
    };
    let (inputs, harness) = crate::harness_generation::load_candidate(vault, id)?;
    if !plan.source_files.is_empty()
        || record.candidate.cycle_id != id
        || record.candidate.plan_digest != plan.digest
        || record.candidate.candidate_digest != harness.digest()?
        || record.candidate.instructions != harness.candidate_instructions
        || record.candidate.hints != harness.candidate_hints
        || record
            .previous
            .as_ref()
            .map(|p| p.hints.clone())
            .unwrap_or_default()
            != harness.baseline_hints
        || record.candidate.scope
            != scope_at(
                &record.build_digest,
                &inputs.provider.source,
                &inputs.provider.model,
                &inputs.provider.options,
            )?
        || record.trials_digest != canonical_digest(&cycle.trials)?
        || record
            .previous
            .as_ref()
            .is_some_and(|previous| previous.scope != record.candidate.scope)
        || record
            .previous
            .as_ref()
            .map_or(BASELINE, |previous| &previous.instructions)
            != harness.baseline_instructions
    {
        bail!("guidance activation binding changed");
    }
    Ok(Some(record))
}

fn active(vault: &LearningVault, key: &str) -> Result<Option<ActiveGuidance>> {
    let Some(current) = read_pointer(vault, key)? else {
        return Ok(None);
    };
    if current.scope != key
        || current.instructions.trim().is_empty()
        || current.instructions.len() > 120 * 1024
    {
        bail!("invalid active guidance");
    }
    if current.source_cycle {
        let (plan, cycle) = load_harness_cycle(vault, &current.cycle_id)?;
        let promotion = crate::harness_promotion::report(vault, &current.cycle_id)?
            .context("source guidance promotion missing")?;
        let policy = promotion
            .artifacts
            .guidance
            .as_ref()
            .context("source guidance policy missing")?;
        if policy.build_digest != env!("CA_BUILD_DIGEST")
            || policy.candidate(&plan, &promotion)? != current
            || promotion.phase != crate::harness_promotion::Phase::Active
            || promotion.work.is_some()
            || cycle.stage != HarnessStage::Promoted
            || !cycle.reservations.is_empty()
            || !cycle.active_reservations.is_empty()
            || promotion.artifacts.trials_digest != canonical_digest(&cycle.trials)?
        {
            bail!("source guidance is incomplete or changed");
        }
        return Ok(Some(current));
    }
    // Do not consume a half-written activation. The recovery command completes
    // its journal/core stage without changing the evaluated instructions.
    let record: Activation = vault
        .read_mutable(&path(&current.cycle_id))?
        .context("guidance receipt missing")?;
    let (plan, cycle) = load_harness_cycle(vault, &current.cycle_id)?;
    let evaluated = crate::harness_trials::TrialHarness {
        baseline_instructions: record
            .previous
            .as_ref()
            .map_or(BASELINE, |p| &p.instructions)
            .into(),
        baseline_hints: record
            .previous
            .as_ref()
            .map(|p| p.hints.clone())
            .unwrap_or_default(),
        candidate_instructions: current.instructions.clone(),
        candidate_hints: current.hints.clone(),
        candidate_binary_digest: plan.baseline_binary_digest.clone(),
        candidate_commit: None,
    };
    evaluated.validate(&plan)?;
    if evaluated.digest()? != current.candidate_digest
        || record.candidate != current
        || record.phase != Phase::Active
        || cycle.stage != HarnessStage::Promoted
        || !cycle.reservations.is_empty()
        || !cycle.active_reservations.is_empty()
        || plan.digest != current.plan_digest
        || cycle.candidate_digest.as_deref() != Some(&current.candidate_digest)
        || record.trials_digest != canonical_digest(&cycle.trials)?
    {
        bail!("active guidance is incomplete or changed");
    }
    Ok(Some(current))
}

fn ensure_runtime_slot() -> Result<()> {
    if include_str!("../prompts/system_prefix.md")
        .matches(BASELINE)
        .count()
        != 1
    {
        bail!("this build has no unambiguous runtime guidance slot");
    }
    Ok(())
}

pub fn baseline_instructions(
    vault: &LearningVault,
    source: &ProviderStateSourceV1,
    model: &str,
    options: &ChatRequestOptions,
) -> Result<String> {
    ensure_runtime_slot()?;
    Ok(active(vault, &scope(source, model, options)?)?
        .map_or_else(|| BASELINE.into(), |current| current.instructions))
}

pub(crate) fn baseline_hints(
    vault: &LearningVault,
    source: &ProviderStateSourceV1,
    model: &str,
    options: &ChatRequestOptions,
) -> Result<crate::harness_hints::HintPolicy> {
    Ok(active(vault, &scope(source, model, options)?)?
        .map(|current| current.hints)
        .unwrap_or_default())
}

fn apply(request: &mut PreparedModelRequest, current: &ActiveGuidance) -> bool {
    // Custom agent/system prompts without this slot keep their original policy.
    let count: usize = request
        .messages
        .iter()
        .filter(|m| m.role == "system")
        .filter_map(|m| m.content.as_str())
        .map(|text| text.matches(BASELINE).count())
        .sum();
    if count != 1 {
        return false;
    }
    if !current.hints.is_empty() {
        let component = crate::harness_registry::HarnessComponent::Config;
        let digest = canonical_digest(&(
            "execution_hints_v1",
            request.harness_components.get(&component),
            &current.hints,
        ))
        .expect("hint policy consists only of JSON strings");
        request.harness_components.insert(component, digest);
    }
    let reminder = crate::harness_hints::reminder_text(&request.messages, &current.hints);
    for message in &mut request.messages {
        if message.role == "system" {
            if let Some(text) = message.content.as_str() {
                if text.contains(BASELINE) {
                    let replacement = format!(
                        "{}{reminder}",
                        text.replacen(BASELINE, &current.instructions, 1)
                    );
                    let estimator = crate::context::TokenEstimator::generic();
                    if let Some(breakdown) = &mut request.context_breakdown {
                        let before = estimator.estimate_text(text);
                        let after = estimator.estimate_text(&replacement);
                        if after >= before {
                            breakdown.instructions =
                                breakdown.instructions.saturating_add(after - before);
                            breakdown.total = breakdown.total.saturating_add(after - before);
                        } else {
                            breakdown.instructions =
                                breakdown.instructions.saturating_sub(before - after);
                            breakdown.total = breakdown.total.saturating_sub(before - after);
                        }
                    }
                    message.content = crate::image_content::ModelContent::text(replacement);
                    return true;
                }
            }
        }
    }
    false
}

pub async fn prepare(
    request: PreparedModelRequest,
    source: ProviderStateSourceV1,
) -> PreparedModelRequest {
    prepare_with_loader(request, source, runtime_vault).await
}
async fn prepare_with_loader(
    mut request: PreparedModelRequest,
    source: ProviderStateSourceV1,
    loader: impl FnOnce() -> Result<Option<LearningVault>> + Send + 'static,
) -> PreparedModelRequest {
    if request.guidance_prepared {
        return request;
    }
    request.guidance_prepared = true;
    if !matches!(
        request.kind,
        RequestKind::MainTurn
            | RequestKind::ToolContinuation
            | RequestKind::Headless
            | RequestKind::Service
            | RequestKind::Subagent
            | RequestKind::RoleRun
            | RequestKind::WorkflowStep
            | RequestKind::Scheduled
            | RequestKind::EmailGateway
    ) {
        return request;
    }
    let model = request.model.clone();
    let options = request.options.clone();
    let loaded = tokio::task::spawn_blocking(move || -> Result<Option<ActiveGuidance>> {
        let Some(vault) = loader()? else {
            return Ok(None);
        };
        active(&vault, &scope(&source, &model, &options)?)
    });
    match tokio::time::timeout(std::time::Duration::from_secs(2), loaded).await {
        Ok(Ok(Ok(Some(current)))) => {
            apply(&mut request, &current);
        }
        Ok(Ok(Ok(None))) => {}
        _ => eprintln!("Warning: evaluated guidance is unavailable; using the built-in guidance."),
    }
    request
}

/// Called with the worker lease held. The same vault lock serializes pause and
/// pointer publication. No await separates the final checks and commit writes.
fn commit(vault: &LearningVault, record: &mut Activation, rollback: bool) -> Result<()> {
    let id = record.candidate.cycle_id.clone();
    update_harness_cycle(vault, &id, |cycle| {
        if cycle.stage == HarnessStage::Paused {
            bail!("guidance transition paused");
        }
        let (work, maximum) = record
            .work
            .as_ref()
            .context("missing transition reservation")?;
        if !cycle.reservations.is_empty()
            || cycle.active_reservations.len() != 1
            || cycle.active_reservations.get(work) != Some(maximum)
        {
            bail!("unsettled or changed work prevents guidance transition");
        }
        let current = read_pointer(vault, &record.candidate.scope)?;
        let candidate = Some(record.candidate.clone());
        if current != candidate && current != record.previous {
            bail!("newer guidance must not be overwritten");
        }
        let wanted = if rollback {
            &record.previous
        } else {
            &candidate
        };
        if current != *wanted {
            vault.write_mutable(&pointer(&record.candidate.scope), wanted)?;
        }
        record.phase = if rollback {
            Phase::RolledBack
        } else {
            Phase::Active
        };
        save(vault, record)?;
        cycle.stage = if rollback {
            HarnessStage::RolledBack
        } else {
            HarnessStage::Promoted
        };
        cycle.resume_stage = None;
        cycle.reason = if rollback {
            "Previous guidance restored"
        } else {
            "Evaluated guidance activated"
        }
        .into();
        Ok(())
    })
}

pub async fn promote(
    vault: LearningVault,
    id: &str,
    repository: &Path,
    binary: &Path,
) -> Result<Activation> {
    Box::pin(transition(vault, id, Some((repository, binary)))).await
}
/// A prepared record is not promotion authority. Only an already published
/// pointer or active receipt establishes an interrupted activation to reconcile.
pub(crate) fn recovery_started(vault: &LearningVault, id: &str) -> Result<bool> {
    Ok(report(vault, id)?.is_some_and(|record| {
        record.phase == Phase::Active
            || read_pointer(vault, &record.candidate.scope)
                .is_ok_and(|current| current == Some(record.candidate))
    }))
}
pub async fn rollback(vault: LearningVault, id: &str) -> Result<Activation> {
    Box::pin(transition(vault, id, None)).await
}
async fn transition(
    vault: LearningVault,
    id: &str,
    artifacts: Option<(&Path, &Path)>,
) -> Result<Activation> {
    let _worker = vault.acquire_worker_lock()?;
    let (plan, mut cycle) = load_harness_cycle(&vault, id)?;
    if !plan.source_files.is_empty() {
        bail!("source experiments use source rollback");
    }
    if cycle.stage == HarnessStage::Paused {
        bail!("guidance transition paused");
    }
    if let Some((repository, binary)) = artifacts {
        if !recovery_started(&vault, id)? {
            crate::harness_automation::require_review(&vault, id, repository, binary).await?;
        }
    }
    let mut record = match report(&vault, id)? {
        Some(record) => record,
        None => {
            if artifacts.is_none() || cycle.stage != HarnessStage::ReadyToPromote {
                bail!("guidance is not ready for activation");
            }
            let (inputs, harness) = crate::harness_generation::load_candidate(&vault, id)?;
            let key = scope(
                &inputs.provider.source,
                &inputs.provider.model,
                &inputs.provider.options,
            )?;
            let previous = active(&vault, &key)?;
            if previous.as_ref().map_or(BASELINE, |p| &p.instructions)
                != harness.baseline_instructions
                || previous
                    .as_ref()
                    .map(|p| p.hints.clone())
                    .unwrap_or_default()
                    != harness.baseline_hints
            {
                bail!("evaluated baseline does not match the active runtime guidance slot");
            }
            let record = Activation {
                build_digest: env!("CA_BUILD_DIGEST").into(),
                candidate: ActiveGuidance {
                    source_cycle: false,
                    cycle_id: id.into(),
                    plan_digest: plan.digest.clone(),
                    candidate_digest: harness.digest()?,
                    scope: key,
                    hints: harness.candidate_hints,
                    instructions: harness.candidate_instructions,
                },
                previous,
                trials_digest: canonical_digest(&cycle.trials)?,
                phase: Phase::Prepared,
                work: None,
            };
            save(&vault, &record)?;
            record
        }
    };
    if let Some((work, maximum)) = &record.work {
        if cycle.active_reservations.get(work) == Some(maximum) {
            if cycle.active_reservations.len() != 1 || !cycle.reservations.is_empty() {
                bail!("unrelated unsettled work prevents guidance recovery");
            }
            update_harness_cycle(&vault, id, |state| {
                state.settle_active_time(&plan, work, *maximum)
            })?;
        } else if !cycle.completed_work.contains(work) {
            bail!("guidance activity receipt changed");
        }
        record.work = None;
        save(&vault, &record)?;
        cycle = load_harness_cycle(&vault, id)?.1;
    }
    if !cycle.reservations.is_empty() || !cycle.active_reservations.is_empty() {
        bail!("unsettled work prevents guidance transition");
    }
    let rollback = artifacts.is_none();
    if !rollback && record.build_digest != env!("CA_BUILD_DIGEST") {
        bail!("guidance belongs to a different build");
    }
    if !rollback {
        ensure_runtime_slot()?;
        let verdict = evaluate_harness(
            &plan,
            &record.candidate.candidate_digest,
            &cycle.trials,
            cycle.engineering_checks_passed,
        )?;
        if verdict.verdict != HarnessVerdict::Promote
            || verdict.trials_digest != record.trials_digest
        {
            bail!("guidance promotion checks are incomplete or changed");
        }
    }
    if record.phase == Phase::RolledBack {
        if !rollback {
            bail!("guidance was rolled back; start a new experiment");
        }
        update_harness_cycle(&vault, id, |state| {
            state.stage = HarnessStage::RolledBack;
            state.resume_stage = None;
            Ok(())
        })?;
        return Ok(record);
    }
    if !rollback && record.phase == Phase::Active {
        if read_pointer(&vault, &record.candidate.scope)? != Some(record.candidate.clone()) {
            bail!("guidance has been superseded");
        }
        update_harness_cycle(&vault, id, |state| {
            state.stage = HarnessStage::Promoted;
            state.resume_stage = None;
            Ok(())
        })?;
        return Ok(record);
    }
    // A refused rollback of an older deployment must not revoke that receipt:
    // a newer deployment may still need to restore it as its previous version.
    let current = read_pointer(&vault, &record.candidate.scope)?;
    if current != Some(record.candidate.clone()) && current != record.previous {
        bail!("newer guidance must not be overwritten");
    }
    let remaining = plan
        .settings
        .maximum_active_seconds
        .saturating_sub(cycle.active_seconds);
    if !rollback && remaining < 120 {
        bail!("guidance activation needs 60 seconds plus rollback allowance");
    }
    let activity =
        HarnessActivity::begin_transition(vault.clone(), plan.clone(), remaining.min(60)).await?;
    record.work = Some((activity.reservation().0.into(), activity.reservation().1));
    save(&vault, &record)?;
    let result = activity
        .run(async {
            if let Some((repository, binary)) = artifacts {
                let repository = crate::harness_snapshot::resolve(&vault, &plan, repository)?;
                if crate::harness_engineering::source_snapshot(&repository).await? != plan.baseline
                {
                    bail!("guidance baseline source changed");
                }
                crate::harness_snapshot::verify_contents(&plan, &repository).await?;
                let binary = binary.to_path_buf();
                let expected = plan.baseline_binary_digest.clone();
                tokio::task::spawn_blocking(move || -> Result<()> {
                    let host = std::env::current_exe()?.canonicalize()?;
                    if crate::harness_engineering::artifact_digest(&binary)? != expected
                        || (binary.canonicalize()? != host
                            && crate::harness_engineering::artifact_digest(&host)? != expected)
                    {
                        bail!("guidance baseline executable differs from the running host");
                    }
                    Ok(())
                })
                .await??;
                crate::harness_engineering::verify_guidance_ready(&vault, id, &activity).await?;
            }
            activity.remaining()?;
            commit(&vault, &mut record, rollback)
        })
        .await;
    if !load_harness_cycle(&vault, id)?
        .1
        .active_reservations
        .contains_key(activity.reservation().0)
    {
        record.work = None;
        save(&vault, &record)?;
    }
    if let Err(error) = result {
        update_harness_cycle(&vault, id, |state| {
            if state.stage != HarnessStage::Paused {
                state.stage = HarnessStage::RecoveryRequired;
            }
            state.reason = "Guidance transition incomplete; resume or roll back its journal".into();
            Ok(())
        })?;
        return Err(error);
    }
    Ok(record)
}

fn runtime_vault() -> Result<Option<LearningVault>> {
    #[cfg(not(test))]
    {
        LearningVault::existing_machine()
    }
    #[cfg(test)]
    {
        Ok(None)
    } // Unit tests inject disposable vaults at the active/apply boundary.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::ChatMessage;
    use codex_team::self_improvement::{harness_trial_schedule, HarnessLane, HarnessTrial};

    fn request() -> PreparedModelRequest {
        PreparedModelRequest::new(
            RequestKind::MainTurn,
            "fake",
            "fake-model",
            vec![
                ChatMessage {
                    role: "system".into(),
                    content: crate::image_content::ModelContent::text(format!(
                        "Host prefix. {BASELINE} Host suffix."
                    )),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: crate::image_content::ModelContent::text(format!(
                        "User quoted {BASELINE}"
                    )),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                },
            ],
            Vec::new(),
            ChatRequestOptions::default(),
        )
    }
    fn candidate() -> ActiveGuidance {
        ActiveGuidance {
            source_cycle: false,
            cycle_id: "guidance-test".into(),
            plan_digest: "a".repeat(64),
            candidate_digest: "b".repeat(64),
            scope: "c".repeat(64),
            hints: Default::default(),
            instructions: "Verify the changed behavior and report the checks actually run.".into(),
        }
    }
    #[test]
    fn hrs253_guidance_changes_only_the_runtime_slot_and_its_fingerprint() {
        let mut request = request();
        let original_user = request.messages[1].content.clone();
        let original = crate::harness_registry::capture_prepared(&request);
        assert!(apply(&mut request, &candidate()));
        assert_eq!(request.messages[1].content, original_user);
        assert_eq!(
            request.messages[0].content.as_str().unwrap(),
            format!("Host prefix. {} Host suffix.", candidate().instructions)
        );
        assert_ne!(
            original.digest(),
            crate::harness_registry::capture_prepared(&request).digest()
        );
        assert!(!apply(&mut request, &candidate()));
        request.messages[0].content =
            crate::image_content::ModelContent::text(format!("{BASELINE}{BASELINE}"));
        assert!(!apply(&mut request, &candidate()));
        request.messages[0].content = crate::image_content::ModelContent::text("Custom prompt");
        assert!(!apply(&mut request, &candidate()));
    }
    #[test]
    fn hrs253_evaluated_hint_policy_changes_identity_and_only_fires_at_a_completed_trigger() {
        let mut current = candidate();
        current
            .hints
            .insert(crate::harness_hints::ExecutionHint::DependencyProbes);
        let mut request = request();
        let before = crate::harness_registry::capture_prepared(&request);
        assert!(apply(&mut request, &current));
        assert_ne!(
            before
                .components
                .get(&crate::harness_registry::HarnessComponent::Config),
            crate::harness_registry::capture_prepared(&request)
                .components
                .get(&crate::harness_registry::HarnessComponent::Config)
        );
        assert!(!request.messages[0]
            .content
            .as_str()
            .unwrap()
            .contains("Execution reminder:"));
        let mut request = self::request();
        for (index, command) in ["which cargo", "command -v rustc", "cargo --version"]
            .into_iter()
            .enumerate()
        {
            crate::harness_hints::tests::command(
                &mut request.messages,
                &index.to_string(),
                command,
            );
        }
        let mut worker_messages = request.messages.clone();
        crate::harness_hints::apply(&mut worker_messages, &current.hints);
        assert!(apply(&mut request, &current));
        let runtime = request.messages[0].content.as_str().unwrap();
        let worker = worker_messages[0].content.as_str().unwrap();
        assert_eq!(
            runtime.split("Execution reminder:").nth(1),
            worker.split("Execution reminder:").nth(1)
        );
        assert_eq!(runtime.matches("Execution reminder:").count(), 1);
        assert_eq!(request.messages[1].content, worker_messages[1].content);
    }

    #[test]
    fn hrs253_guidance_scope_changes_with_route_model_options_and_build() {
        let provider = crate::harness_budget::tests::frozen();
        let original = scope(&provider.source, &provider.model, &provider.options).unwrap();
        assert_ne!(
            original,
            scope_at(
                "other-build",
                &provider.source,
                &provider.model,
                &provider.options
            )
            .unwrap()
        );
        assert_ne!(
            original,
            scope(&provider.source, "other-model", &provider.options).unwrap()
        );
        let mut changed = provider.source.clone();
        changed.provider = "different".into();
        assert_ne!(
            original,
            scope(&changed, &provider.model, &provider.options).unwrap()
        );
        let mut options = provider.options.clone();
        options.reasoning_stream_field = Some("different".into());
        assert_ne!(
            original,
            scope(&provider.source, &provider.model, &options).unwrap()
        );
    }

    #[tokio::test]
    async fn hrs253_guidance_does_not_load_for_analysis_or_frozen_trials() {
        let provider = crate::harness_budget::tests::frozen();
        for kind in [
            RequestKind::Compatibility,
            RequestKind::Analyze,
            RequestKind::HeadlessCheck,
        ] {
            let mut request = request();
            request.kind = kind;
            let original = request.messages[0].content.clone();
            let prepared = prepare_with_loader(request, provider.source.clone(), || {
                panic!("isolated requests must not load active policy")
            })
            .await;
            assert_eq!(prepared.messages[0].content, original);
        }
    }

    #[test]
    fn hrs253_guidance_updates_context_estimate_by_the_actual_delta() {
        let mut request = request();
        request.context_breakdown = Some(crate::context::ContextBreakdown {
            source: crate::context::ContextBreakdownSource::Prepared,
            instructions: 1,
            skills: 200,
            built_in_tools: 0,
            mcp_tools: 0,
            checkpoint: 0,
            conversation: 99,
            attachments: 0,
            total: 300,
            mcp_prepared: true,
        });
        let estimator = crate::context::TokenEstimator::generic();
        let before = estimator.estimate_text(request.messages[0].content.as_str().unwrap());
        let mut candidate = candidate();
        candidate.instructions = "Longer evaluated guidance. ".repeat(100);
        assert!(apply(&mut request, &candidate));
        let after = estimator.estimate_text(request.messages[0].content.as_str().unwrap());
        let breakdown = request.context_breakdown.unwrap();
        assert_eq!(breakdown.instructions, 1 + after - before);
        assert_eq!(breakdown.total, 300 + after - before);
        assert_eq!(breakdown.skills, 200);
    }

    fn ready_trials(vault: &LearningVault, plan: &codex_team::self_improvement::HarnessPlan) {
        let (_, harness) = crate::harness_generation::load_candidate(vault, &plan.id).unwrap();
        update_harness_cycle(vault, &plan.id, |cycle| {
            cycle.trials = harness_trial_schedule(plan)?
                .into_iter()
                .map(|(case_id, repetition, lane)| HarnessTrial {
                    plan_digest: plan.digest.clone(),
                    candidate_digest: harness.digest().unwrap(),
                    route_digest: plan.route_digest.clone(),
                    case_id,
                    repetition,
                    lane,
                    verified_pass: true,
                    infrastructure_failure: false,
                    false_completion: false,
                    output_digest: "a".repeat(64),
                    cost_microusd: if lane == HarnessLane::Baseline {
                        100
                    } else {
                        80
                    },
                    elapsed_millis: 1,
                })
                .collect();
            cycle.decide(plan)
        })
        .unwrap();
    }

    #[test]
    fn hrs253_source_guidance_requires_settled_matching_promotion_and_preserves_rollback() {
        use crate::harness_activation::PreparedArtifacts;
        use crate::harness_promotion::{Phase as SourcePhase, Promotion};
        use std::os::unix::fs::PermissionsExt;

        // Synthetic receipts exercise publication/recovery binding, not coding
        // performance or executable replacement (covered by process tests).
        let root = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(root.path().join("vault")).unwrap();
        let (mut plan, inputs, _) = crate::harness_generation::tests::inputs();
        plan.source_files
            .insert("src/repair.rs".into(), "a".repeat(64));
        let directory = root.path().join(".ca-harness-source-policy");
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let directory = directory.canonicalize().unwrap();
        plan.activation_executable = Some(directory.parent().unwrap().join("ca"));
        plan.seal().unwrap();
        codex_team::self_improvement::start_harness_cycle(&vault, &plan, 0).unwrap();
        let hints = [crate::harness_hints::ExecutionHint::DependencyProbes]
            .into_iter()
            .collect();
        let harness = crate::harness_trials::TrialHarness {
            baseline_instructions: "Previously measured guidance".into(),
            candidate_instructions: "Previously measured guidance".into(),
            baseline_hints: hints,
            candidate_hints: [crate::harness_hints::ExecutionHint::DependencyProbes]
                .into_iter()
                .collect(),
            candidate_binary_digest: "b".repeat(64),
            candidate_commit: Some("c".repeat(40)),
        };
        let policy = SourceGuidance::from_evaluated(
            env!("CA_BUILD_DIGEST").into(),
            &inputs.provider,
            &harness,
        )
        .unwrap();
        let mut source = plan.baseline.clone();
        source.head = harness.candidate_commit.clone().unwrap();
        let mut promotion = Promotion {
            plan_digest: plan.digest.clone(),
            repository: root.path().into(),
            artifacts: PreparedArtifacts {
                guidance: Some(policy.clone()),
                plan_digest: plan.digest.clone(),
                candidate_digest: harness.digest().unwrap(),
                trials_digest: canonical_digest(&Vec::<HarnessTrial>::new()).unwrap(),
                destination: plan.activation_executable.clone().unwrap(),
                directory,
                baseline_digest: plan.baseline_binary_digest.clone(),
                evaluated_digest: harness.candidate_binary_digest.clone(),
                destination_mode: 0o755,
                signed_digest: Some("d".repeat(64)),
                baseline_smoke: vec!["version".into(), "help".into()],
                candidate_smoke: vec!["version".into(), "help".into()],
                ready: true,
            },
            candidate_source: source,
            phase: SourcePhase::Prepared,
            reverted_head: None,
            work: None,
        };
        let journal = format!("harness/source/{}-promotion.json", plan.id);
        update_harness_cycle(&vault, &plan.id, |cycle| {
            cycle.candidate_digest = Some(harness.digest()?);
            cycle.candidate_commit = harness.candidate_commit.clone();
            cycle.stage = HarnessStage::SourceAdopted;
            Ok(())
        })
        .unwrap();
        let candidate = policy.candidate(&plan, &promotion).unwrap();
        // The old build's pointer remains untouched, including when rollback
        // disables the newly published policy.
        let mut previous = candidate.clone();
        previous.scope = "e".repeat(64);
        vault
            .write_mutable(&pointer(&previous.scope), &Some(&previous))
            .unwrap();
        publish_source(&vault, &plan, &promotion).unwrap();
        publish_source(&vault, &plan, &promotion).unwrap();
        vault.write_mutable(&journal, &promotion).unwrap();
        assert!(active(&vault, &candidate.scope).is_err());
        promotion.phase = SourcePhase::Active;
        vault.write_mutable(&journal, &promotion).unwrap();
        assert!(active(&vault, &candidate.scope).is_err());
        update_harness_cycle(&vault, &plan.id, |cycle| {
            cycle.stage = HarnessStage::Promoted;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            active(&vault, &candidate.scope).unwrap(),
            Some(candidate.clone())
        );
        for change in ["hint", "route", "build", "artifact", "work", "rollback"] {
            let mut altered = promotion.clone();
            match change {
                "hint" => {
                    altered.artifacts.guidance.as_mut().unwrap().hints.clear();
                }
                "route" => altered
                    .artifacts
                    .guidance
                    .as_mut()
                    .unwrap()
                    .model
                    .push_str("other"),
                "build" => {
                    altered.artifacts.guidance.as_mut().unwrap().build_digest = "f".repeat(64)
                }
                "artifact" => altered.artifacts.evaluated_digest = "f".repeat(64),
                "work" => altered.work = Some(("unsettled".into(), 1)),
                "rollback" => altered.phase = SourcePhase::BinaryRestored,
                _ => unreachable!(),
            }
            vault.write_mutable(&journal, &altered).unwrap();
            assert!(active(&vault, &candidate.scope).is_err(), "{change}");
        }
        assert_eq!(
            read_pointer(&vault, &previous.scope).unwrap(),
            Some(previous)
        );
        vault.write_mutable(&journal, &promotion).unwrap();
        let mut later = candidate.clone();
        later.instructions.push_str(" later");
        vault
            .write_mutable(&pointer(&candidate.scope), &Some(&later))
            .unwrap();
        assert!(publish_source(&vault, &plan, &promotion).is_err());
        assert_eq!(read_pointer(&vault, &candidate.scope).unwrap(), Some(later));
    }

    #[tokio::test]
    async fn hrs253_guidance_rollback_restores_the_previous_promoted_version() {
        let root = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(root.path().join("vault")).unwrap();
        let mut previous: Option<ActiveGuidance> = None;
        let mut last = None;
        // These are synthetic trusted receipts for the pointer transaction.
        // The opt-in test below exercises the actual engineering/promotion path.
        for revision in 0..2 {
            let (mut plan, mut inputs, mut proposal) = crate::harness_generation::tests::inputs();
            plan.id = format!("guidance-revision-{revision}");
            inputs.baseline_instructions = previous
                .as_ref()
                .map_or(BASELINE, |p| &p.instructions)
                .into();
            inputs.baseline_hints = previous
                .as_ref()
                .map(|p| p.hints.clone())
                .unwrap_or_default();
            if revision == 0 {
                proposal.candidate_instructions = inputs.baseline_instructions.clone();
                proposal.candidate_hints = Some(
                    [crate::harness_hints::ExecutionHint::DependencyProbes]
                        .into_iter()
                        .collect(),
                );
            } else {
                proposal.candidate_instructions = format!(
                    "Evaluated guidance revision {revision}: verify the requested behavior."
                );
            }
            let provider = inputs.provider.clone();
            let plan =
                crate::harness_generation::tests::save_fixture(&vault, plan, inputs, proposal);
            update_harness_cycle(&vault, &plan.id, |cycle| {
                cycle.engineering_checks_passed = true;
                Ok(())
            })
            .unwrap();
            ready_trials(&vault, &plan);
            let (_, harness) = crate::harness_generation::load_candidate(&vault, &plan.id).unwrap();
            let cycle = load_harness_cycle(&vault, &plan.id).unwrap().1;
            let mut record = Activation {
                build_digest: env!("CA_BUILD_DIGEST").into(),
                candidate: ActiveGuidance {
                    source_cycle: false,
                    cycle_id: plan.id.clone(),
                    plan_digest: plan.digest.clone(),
                    candidate_digest: harness.digest().unwrap(),
                    scope: scope(&provider.source, &provider.model, &provider.options).unwrap(),
                    hints: harness.candidate_hints,
                    instructions: harness.candidate_instructions,
                },
                previous: previous.clone(),
                trials_digest: canonical_digest(&cycle.trials).unwrap(),
                phase: Phase::Prepared,
                work: Some(("publish-guidance".into(), 60)),
            };
            update_harness_cycle(&vault, &plan.id, |cycle| {
                cycle.reserve_transition_time(&plan, "publish-guidance", 60)
            })
            .unwrap();
            if revision == 1 {
                update_harness_cycle(&vault, &plan.id, |cycle| {
                    cycle.stage = HarnessStage::Paused;
                    Ok(())
                })
                .unwrap();
                assert!(commit(&vault, &mut record, false).is_err());
                assert_eq!(
                    read_pointer(&vault, &record.candidate.scope).unwrap(),
                    previous
                );
                update_harness_cycle(&vault, &plan.id, |cycle| {
                    cycle.stage = HarnessStage::ReadyToPromote;
                    Ok(())
                })
                .unwrap();
            }
            commit(&vault, &mut record, false).unwrap();
            assert!(
                active(&vault, &record.candidate.scope).is_err(),
                "publication is not consumable before activity settlement"
            );
            update_harness_cycle(&vault, &plan.id, |cycle| {
                cycle.settle_active_time(&plan, "publish-guidance", 1)
            })
            .unwrap();
            record.work = None;
            save(&vault, &record).unwrap();
            assert_eq!(
                active(&vault, &record.candidate.scope).unwrap(),
                Some(record.candidate.clone())
            );
            let mut altered = record.clone();
            altered
                .candidate
                .hints
                .insert(crate::harness_hints::ExecutionHint::ForegroundWaits);
            save(&vault, &altered).unwrap();
            vault
                .write_mutable(
                    &pointer(&altered.candidate.scope),
                    &Some(altered.candidate.clone()),
                )
                .unwrap();
            assert!(
                active(&vault, &altered.candidate.scope).is_err(),
                "a matching pointer and journal cannot authorize an unevaluated policy"
            );
            save(&vault, &record).unwrap();
            vault
                .write_mutable(
                    &pointer(&record.candidate.scope),
                    &Some(record.candidate.clone()),
                )
                .unwrap();
            previous = Some(record.candidate.clone());
            last = Some(record);
        }
        let record = last.unwrap();
        assert!(
            rollback(vault.clone(), &record.previous.as_ref().unwrap().cycle_id)
                .await
                .is_err()
        );
        assert_eq!(
            read_pointer(&vault, &record.candidate.scope).unwrap(),
            Some(record.candidate.clone())
        );
        // Refusal does not revoke the prior published version: restoring it must
        // remain usable even though its attempted rollback was refused.
        let restored = rollback(vault.clone(), &record.candidate.cycle_id)
            .await
            .unwrap();
        assert_eq!(
            read_pointer(&vault, &record.candidate.scope).unwrap(),
            record.previous
        );
        assert_eq!(restored.phase, Phase::RolledBack);
        assert_eq!(
            active(&vault, &record.candidate.scope).unwrap(),
            record.previous
        );
    }

    #[tokio::test]
    #[ignore = "requires macOS sandbox and local Cargo; executes full matrix in a disposable project"]
    async fn hrs253_guidance_promotes_recovers_and_rolls_back_without_touching_source() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repository");
        crate::harness_snapshot::tests::fixture(&repository);
        let vault = LearningVault::open_at(root.path().join("vault")).unwrap();
        let expected = crate::harness_snapshot::source_digest(&repository).unwrap();
        let (mut frozen, _owner) =
            crate::harness_snapshot::capture(&vault, &repository, &expected).unwrap();
        frozen.freezing_seconds = 1;
        let snapshot = crate::harness_engineering::source_snapshot(&frozen.repository)
            .await
            .unwrap();
        // Subsequent user work must neither enter the experiment nor block it.
        std::fs::write(repository.join("src/untracked.rs"), "// later user work\n").unwrap();
        let binary = std::env::current_exe().unwrap();
        let (mut plan, mut inputs, mut proposal) = crate::harness_generation::tests::inputs();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        inputs.provider.source = ProviderStateSourceV1::from_route(
            "fake",
            crate::agent::provider::ProviderProtocol::ChatCompletions,
            &url,
        )
        .unwrap();
        plan.route_digest = inputs.provider.route_digest().unwrap();
        plan.baseline = snapshot.clone();
        plan.guidance_source = Some(frozen.clone());
        plan.baseline_binary_digest = crate::harness_engineering::artifact_digest(&binary).unwrap();
        inputs.baseline_instructions = BASELINE.into();
        proposal.candidate_instructions = candidate().instructions;
        let provider = inputs.provider.clone();
        let plan = crate::harness_generation::tests::save_fixture(&vault, plan, inputs, proposal);
        // Unknown provider charges cannot be erased by activation admission.
        update_harness_cycle(&vault, &plan.id, |state| {
            state.reserve(&plan, "unknown", 100)
        })
        .unwrap();
        assert!(promote(vault.clone(), &plan.id, &repository, &binary)
            .await
            .is_err());
        assert!(report(&vault, &plan.id).unwrap().is_none());
        update_harness_cycle(&vault, &plan.id, |state| {
            state.reservations.clear();
            Ok(())
        })
        .unwrap();
        assert!(
            crate::harness_engineering::check_guidance(vault.clone(), &plan.id, &repository)
                .await
                .unwrap()
        );
        ready_trials(&vault, &plan);
        assert!(promote(vault.clone(), &plan.id, &repository, &binary)
            .await
            .is_err());
        let review =
            crate::harness_automation::preview_review(&vault, &plan.id, &repository, &binary)
                .await
                .unwrap();
        crate::harness_automation::confirm_review(
            &vault,
            &plan.id,
            &repository,
            &binary,
            &review.digest,
        )
        .await
        .unwrap();
        let record = promote(vault.clone(), &plan.id, &repository, &binary)
            .await
            .unwrap();
        assert_eq!(record.phase, Phase::Active);
        assert_eq!(
            baseline_instructions(&vault, &provider.source, &provider.model, &provider.options)
                .unwrap(),
            record.candidate.instructions
        );
        let store = vault.clone();
        let runtime_request =
            prepare_with_loader(request(), provider.source.clone(), move || Ok(Some(store))).await;
        assert!(runtime_request.guidance_prepared);
        assert!(runtime_request.messages[0]
            .content
            .as_str()
            .unwrap()
            .contains(&record.candidate.instructions));
        let store = vault.clone();
        let repeated = prepare_with_loader(
            runtime_request.clone(),
            provider.source.clone(),
            move || {
                drop(store);
                panic!("prepared requests must not reload guidance")
            },
        )
        .await;
        assert_eq!(
            repeated.messages[0].content,
            runtime_request.messages[0].content
        );
        let expected = runtime_request.messages[0]
            .content
            .as_str()
            .unwrap()
            .to_owned();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let body = loop {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
                assert!(bytes.len() < 65536);
                if let Some(boundary) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&bytes[..boundary]).unwrap();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    if bytes.len() >= boundary + 4 + length {
                        break serde_json::from_slice::<serde_json::Value>(
                            &bytes[boundary + 4..boundary + 4 + length],
                        )
                        .unwrap();
                    }
                }
            };
            assert_eq!(body["messages"][0]["content"], expected);
            assert_eq!(
                body["messages"][1]["content"],
                format!("User quoted {BASELINE}")
            );
            let response = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).as_bytes()).await.unwrap();
        });
        let configuration = crate::config::ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: "synthetic".into(),
            protocol: crate::agent::provider::ProviderProtocol::ChatCompletions,
            prompt_cache_mode: Default::default(),
            models: vec![provider.model.clone()],
            timeout: Default::default(),
        };
        let client = crate::agent::client::AgentClient::with_request_store(
            "fake",
            &configuration,
            &provider.model,
            crate::request::RequestStore::default(),
        );
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut stream = client.stream_prepared(&runtime_request).await.unwrap();
            while let Some(event) = stream.recv().await {
                event.unwrap();
            }
            server.await.unwrap();
        })
        .await
        .unwrap();

        assert_eq!(
            crate::harness_engineering::source_snapshot(&frozen.repository)
                .await
                .unwrap(),
            snapshot
        );
        assert_eq!(
            std::fs::read_to_string(repository.join("src/untracked.rs")).unwrap(),
            "// later user work\n"
        );
        crate::harness_snapshot::verify_contents(&plan, &frozen.repository)
            .await
            .unwrap();
        std::fs::write(
            frozen.repository.join("src/untracked.rs"),
            "// changed frozen source\n",
        )
        .unwrap();
        assert!(
            crate::harness_snapshot::verify_contents(&plan, &frozen.repository)
                .await
                .is_err()
        );
        std::fs::write(
            frozen.repository.join("src/untracked.rs"),
            "// uncommitted source\n",
        )
        .unwrap();

        // Crash after pointer publication: only the recorded host reservation is
        // charged at its maximum, and the existing pointer is reused.
        let mut interrupted = record.clone();
        interrupted.phase = Phase::Prepared;
        interrupted.work = Some(("interrupted-guidance".into(), 60));
        update_harness_cycle(&vault, &plan.id, |state| {
            state.stage = HarnessStage::ReadyToPromote;
            state.reserve_transition_time(&plan, "interrupted-guidance", 60)
        })
        .unwrap();
        save(&vault, &interrupted).unwrap();
        assert!(active(&vault, &record.candidate.scope).is_err());
        let before = load_harness_cycle(&vault, &plan.id)
            .unwrap()
            .1
            .active_seconds;
        promote(vault.clone(), &plan.id, &repository, &binary)
            .await
            .unwrap();
        let recovered = load_harness_cycle(&vault, &plan.id).unwrap().1;
        assert!(recovered.active_seconds >= before + 60);
        assert!(recovered.completed_work.contains("interrupted-guidance"));

        // A later slot owner is never overwritten, including during rollback.
        let mut later = record.candidate.clone();
        later.cycle_id = "later-owner".into();
        vault
            .write_mutable(&pointer(&later.scope), &Some(later.clone()))
            .unwrap();
        assert!(rollback(vault.clone(), &plan.id).await.is_err());
        assert_eq!(read_pointer(&vault, &later.scope).unwrap(), Some(later));
        vault
            .write_mutable(
                &pointer(&record.candidate.scope),
                &Some(record.candidate.clone()),
            )
            .unwrap();
        std::fs::write(repository.join("user-work.txt"), "keep this").unwrap();
        let rolled_back = rollback(vault.clone(), &plan.id).await.unwrap();
        assert_eq!(rolled_back.phase, Phase::RolledBack);
        assert_eq!(
            baseline_instructions(&vault, &provider.source, &provider.model, &provider.options)
                .unwrap(),
            BASELINE
        );
        assert_eq!(
            std::fs::read_to_string(repository.join("user-work.txt")).unwrap(),
            "keep this"
        );
        assert_eq!(
            rollback(vault.clone(), &plan.id).await.unwrap().phase,
            Phase::RolledBack
        );
        assert_eq!(
            load_harness_cycle(&vault, &plan.id).unwrap().1.stage,
            HarnessStage::RolledBack
        );
    }
}
