//! Frozen, repeated harness experiments. The host owns receipts and promotion.
use super::{canonical_digest, LearningVault, SourceRepositorySnapshotV1};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const HARNESS_SCHEMA: &str = "ca.harness-cycle.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HarnessSettings {
    pub enabled: bool,
    pub automatic: bool,
    pub source_candidates: bool,
    pub automatic_promotion: bool,
    pub maximum_microusd: u64,
    pub maximum_active_seconds: u64,
    pub idle_seconds: u64,
    pub cooldown_seconds: u64,
}
impl Default for HarnessSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            automatic: true,
            source_candidates: true,
            automatic_promotion: true,
            maximum_microusd: 20_000_000,
            maximum_active_seconds: 7200,
            idle_seconds: 300,
            cooldown_seconds: 86400,
        }
    }
}
impl HarnessSettings {
    pub fn validate(&self) -> Result<()> {
        if self.maximum_microusd == 0 || self.maximum_active_seconds == 0 || self.idle_seconds == 0
        {
            bail!("harness budgets and idle interval must be positive");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HarnessCase {
    pub id: String,
    pub input_digest: String,
    pub verifier_digest: String,
    pub grants_digest: String,
    pub held_out: bool,
    pub predicted_fix: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GuidanceSource {
    pub origin: std::path::PathBuf,
    pub repository: std::path::PathBuf,
    pub source_digest: String,
    pub freezing_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HarnessPlan {
    /// Absent only for legacy host-sandbox experiments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vm: Option<super::FrozenHarnessVm>,
    /// Host discovery/freezing time. Absent legacy plans retain their original accounting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preparation_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guidance_source: Option<GuidanceSource>,
    /// Frozen before generation; legacy plans cannot replace an unspecified binary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation_executable: Option<std::path::PathBuf>,
    /// Empty means guidance; otherwise exact source preimages are frozen here.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub source_files: BTreeMap<String, String>,
    pub schema: String,
    pub id: String,
    pub baseline: SourceRepositorySnapshotV1,
    pub baseline_binary_digest: String,
    pub route_digest: String,
    pub price_digest: String,
    pub evidence_ids: Vec<String>,
    pub cases: Vec<HarnessCase>,
    pub repetitions: u8,
    pub settings: HarnessSettings,
    pub digest: String,
}
impl HarnessPlan {
    pub fn seal(&mut self) -> Result<()> {
        self.digest.clear();
        self.digest = canonical_digest(self)?;
        self.validate()
    }
    pub fn validate(&self) -> Result<()> {
        self.settings.validate()?;
        if let Some(vm) = &self.vm {
            vm.validate()?;
        }
        if self.preparation_seconds.is_some_and(|seconds| {
            seconds == 0
                || seconds > self.settings.maximum_active_seconds
                || self
                    .guidance_source
                    .as_ref()
                    .is_some_and(|source| source.freezing_seconds > seconds)
        }) {
            bail!("invalid experiment preparation time");
        }
        if let Some(source) = &self.guidance_source {
            validate_digest(&source.source_digest)?;
            if !self.source_files.is_empty()
                || source.freezing_seconds == 0
                || source.freezing_seconds > self.settings.maximum_active_seconds
                || [&source.origin, &source.repository].iter().any(|path| {
                    !path.is_absolute()
                        || path.components().any(|p| {
                            matches!(
                                p,
                                std::path::Component::ParentDir | std::path::Component::CurDir
                            )
                        })
                })
            {
                bail!("invalid private guidance source binding");
            }
        }
        if self.activation_executable.as_ref().is_some_and(|path| {
            !path.is_absolute()
                || path.file_name().is_none()
                || path.components().any(|part| {
                    matches!(
                        part,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
        }) {
            bail!("activation executable must be an absolute normalized file path");
        }
        if self.schema != HARNESS_SCHEMA
            || self.repetitions != 3
            || self.evidence_ids.is_empty()
            || self.cases.iter().filter(|c| c.held_out).count() < 2
            || !self.cases.iter().any(|c| c.predicted_fix && !c.held_out)
        {
            bail!(
                "evaluation requires evidence, a target, two held-out cases and three repetitions"
            );
        }
        if self.source_files.len() > 8
            || (!self.source_files.is_empty() && !self.settings.source_candidates)
        {
            bail!("source candidates require enabled settings and at most eight files");
        }
        for (path, digest) in &self.source_files {
            let relative = std::path::Path::new(path);
            if path.is_empty()
                || path.len() > 512
                || path.contains('\0')
                || path.contains('\\')
                || !relative
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
                || relative
                    .components()
                    .collect::<std::path::PathBuf>()
                    .to_str()
                    != Some(path.as_str())
                || super::HARNESS_PROTECTED_SOURCE.iter().any(|protected| {
                    relative == std::path::Path::new(protected) || relative.starts_with(protected)
                })
            {
                bail!("invalid or protected frozen source path");
            }
            validate_digest(digest)?;
        }
        let mut ids = BTreeSet::new();
        for case in &self.cases {
            if !valid_id(&case.id) || !ids.insert(&case.id) {
                bail!("invalid or duplicate case ID");
            }
            for digest in [
                &case.input_digest,
                &case.verifier_digest,
                &case.grants_digest,
            ] {
                validate_digest(digest)?;
            }
        }
        for digest in [
            &self.baseline_binary_digest,
            &self.route_digest,
            &self.price_digest,
        ] {
            validate_digest(digest)?;
        }
        let mut copy = self.clone();
        copy.digest.clear();
        if canonical_digest(&copy)? != self.digest {
            bail!("frozen evaluation plan changed");
        }
        if !valid_id(&self.id) {
            bail!("invalid cycle ID");
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum HarnessLane {
    Baseline,
    Candidate,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HarnessTrial {
    pub plan_digest: String,
    pub candidate_digest: String,
    pub route_digest: String,
    pub case_id: String,
    pub repetition: u8,
    pub lane: HarnessLane,
    pub verified_pass: bool,
    pub infrastructure_failure: bool,
    pub false_completion: bool,
    pub output_digest: String,
    pub cost_microusd: u64,
    pub elapsed_millis: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HarnessVerdict {
    Promote,
    Reject,
    Incomplete,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessDecision {
    pub verdict: HarnessVerdict,
    pub reasons: Vec<String>,
    pub fixed_cases: Vec<String>,
    pub regressed_cases: Vec<String>,
    pub trials_digest: String,
}

pub fn evaluate_harness(
    plan: &HarnessPlan,
    candidate_digest: &str,
    trials: &[HarnessTrial],
    engineering_checks_passed: bool,
) -> Result<HarnessDecision> {
    plan.validate()?;
    validate_digest(candidate_digest)?;
    let mut indexed = BTreeMap::new();
    let mut decision = HarnessDecision {
        verdict: HarnessVerdict::Incomplete,
        reasons: Vec::new(),
        fixed_cases: Vec::new(),
        regressed_cases: Vec::new(),
        trials_digest: canonical_digest(&trials)?,
    };
    for trial in trials {
        if trial.plan_digest != plan.digest
            || trial.candidate_digest != candidate_digest
            || trial.route_digest != plan.route_digest
            || trial.repetition >= plan.repetitions
            || !plan.cases.iter().any(|c| c.id == trial.case_id)
        {
            bail!("trial does not belong to the frozen experiment");
        }
        validate_digest(&trial.output_digest)?;
        if indexed
            .insert((&trial.case_id, trial.repetition, trial.lane), trial)
            .is_some()
        {
            bail!("duplicate trial");
        }
    }
    if indexed.len() != plan.cases.len() * usize::from(plan.repetitions) * 2
        || trials.iter().any(|t| t.infrastructure_failure)
    {
        decision
            .reasons
            .push("missing trials or infrastructure failure; no causal verdict".into());
        return Ok(decision);
    }
    let mut equivalent = true;
    for case in &plan.cases {
        let lane = |which| {
            trials
                .iter()
                .filter(|t| t.case_id == case.id && t.lane == which)
                .collect::<Vec<_>>()
        };
        let before = lane(HarnessLane::Baseline);
        let after = lane(HarnessLane::Candidate);
        let passes = |items: &[&HarnessTrial]| {
            items
                .iter()
                .filter(|t| t.verified_pass && !t.false_completion)
                .count()
        };
        let bp = passes(&before);
        let ap = passes(&after);
        if ap < bp
            || (case.held_out && ap != usize::from(plan.repetitions))
            || after.iter().any(|t| t.false_completion)
        {
            decision.regressed_cases.push(case.id.clone());
        }
        if case.predicted_fix && ap.saturating_sub(bp) >= 2 {
            decision.fixed_cases.push(case.id.clone());
        }
        equivalent &= before
            .iter()
            .all(|b| after.iter().any(|a| a.output_digest == b.output_digest))
            && after
                .iter()
                .all(|a| before.iter().any(|b| a.output_digest == b.output_digest))
            && bp == ap
            && ap == usize::from(plan.repetitions);
    }
    if !engineering_checks_passed || !decision.regressed_cases.is_empty() {
        decision.verdict = HarnessVerdict::Reject;
        decision
            .reasons
            .push("engineering checks, held-out checks or regression requirements failed".into());
        return Ok(decision);
    }
    let medians = |lane: HarnessLane, cost: bool| {
        let mut values: Vec<_> = trials
            .iter()
            .filter(|t| t.lane == lane)
            .map(|t| {
                if cost {
                    t.cost_microusd
                } else {
                    t.elapsed_millis
                }
            })
            .collect();
        values.sort();
        values[values.len() / 2]
    };
    let cheaper = [true, false].into_iter().any(|cost| {
        let b = medians(HarnessLane::Baseline, cost);
        let a = medians(HarnessLane::Candidate, cost);
        b > 0 && u128::from(a) * 100 <= u128::from(b) * 90
    });
    decision.verdict = if !decision.fixed_cases.is_empty() || (equivalent && cheaper) {
        HarnessVerdict::Promote
    } else {
        HarnessVerdict::Reject
    };
    decision.reasons.push(
        if decision.verdict == HarnessVerdict::Promote {
            "frozen matched experiment demonstrated improvement"
        } else {
            "no verified predicted improvement or equivalent efficiency improvement"
        }
        .into(),
    );
    Ok(decision)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HarnessStage {
    WaitingForEvidence,
    EvaluationFrozen,
    CandidateReady,
    Evaluating,
    ReadyToPromote,
    SourceAdopting,
    SourceAdopted,
    Promoted,
    RolledBack,
    Rejected,
    Paused,
    BudgetExhausted,
    WaitingForCleanSource,
    RecoveryRequired,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessPauseCause {
    User,
    Foreground,
}

/// A closed interrupted experiment retains all of its unsettled reservations.
/// They are unknown charges/durations, not actual spend and not refunded budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessAbandonment {
    pub recorded_at: u64,
    pub previous_stage: HarnessStage,
    pub previous_state_digest: String,
    pub closed_state_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessCycle {
    pub schema: String,
    pub id: String,
    pub plan_digest: String,
    pub stage: HarnessStage,
    #[serde(default)]
    pub resume_stage: Option<HarnessStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_cause: Option<HarnessPauseCause>,
    pub candidate_digest: Option<String>,
    pub candidate_commit: Option<String>,
    pub created_at: u64,
    pub active_seconds: u64,
    /// Maximum duration committed before starting work. Interrupted work keeps
    /// its whole reservation until a host-owned completion can settle it.
    #[serde(default)]
    pub active_reservations: BTreeMap<String, u64>,
    #[serde(default)]
    pub completed_work: BTreeSet<String>,
    /// Usage-accounted budget at the frozen price basis (which may be an upper
    /// bound). This is distinct from unknown reservations and provider invoices.
    pub settled_microusd: u64,
    pub reservations: BTreeMap<String, u64>,
    pub charged_requests: BTreeSet<String>,
    pub trials: Vec<HarnessTrial>,
    pub engineering_checks_passed: bool,
    pub decision: Option<HarnessDecision>,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abandonment: Option<HarnessAbandonment>,
}
impl HarnessCycle {
    pub fn new(plan: &HarnessPlan, now: u64) -> Result<Self> {
        plan.validate()?;
        Ok(Self {
            schema: HARNESS_SCHEMA.into(),
            id: plan.id.clone(),
            plan_digest: plan.digest.clone(),
            stage: HarnessStage::EvaluationFrozen,
            resume_stage: None,
            pause_cause: None,
            candidate_digest: None,
            candidate_commit: None,
            created_at: now,
            active_seconds: plan.preparation_seconds.unwrap_or_else(|| {
                plan.guidance_source
                    .as_ref()
                    .map_or(0, |source| source.freezing_seconds)
            }),
            active_reservations: BTreeMap::new(),
            completed_work: BTreeSet::new(),
            settled_microusd: 0,
            reservations: BTreeMap::new(),
            charged_requests: BTreeSet::new(),
            trials: Vec::new(),
            engineering_checks_passed: false,
            decision: None,
            reason: String::new(),
            abandonment: None,
        })
    }
    pub fn can_abandon(&self, automatic: bool) -> bool {
        if self.abandonment.is_some() || self.decision.is_some() {
            return false;
        }
        let eligible = |stage: &HarnessStage| {
            matches!(
                stage,
                HarnessStage::EvaluationFrozen
                    | HarnessStage::CandidateReady
                    | HarnessStage::Evaluating
                    | HarnessStage::BudgetExhausted
                    | HarnessStage::RecoveryRequired
                    | HarnessStage::Rejected
            )
        };
        eligible(&self.stage)
            || (self.stage == HarnessStage::Paused
                && (!automatic || self.pause_cause == Some(HarnessPauseCause::Foreground))
                && self.resume_stage.as_ref().is_some_and(eligible)
                && (!automatic
                    || !self.reservations.is_empty()
                    || !self.active_reservations.is_empty()))
    }

    /// Call only while controller/worker ownership and VM inventory prove that
    /// execution has stopped. This rejects the candidate without replay/refund.
    pub fn abandon_interrupted(
        &mut self,
        plan: &HarnessPlan,
        now: u64,
        automatic: bool,
    ) -> Result<()> {
        self.require_plan(plan)?;
        if !self.can_abandon(automatic) || now < self.created_at {
            bail!("cycle cannot be abandoned in its current state");
        }
        let previous_stage = self.stage.clone();
        let previous_state_digest = canonical_digest(self)?;
        self.stage = HarnessStage::Rejected;
        self.abandonment = Some(HarnessAbandonment {
            recorded_at: now,
            previous_stage,
            previous_state_digest,
            closed_state_digest: canonical_digest(self)?,
        });
        Ok(())
    }
    pub fn reserve(&mut self, plan: &HarnessPlan, request_id: &str, maximum: u64) -> Result<()> {
        self.require_plan(plan)?;
        if !valid_id(request_id)
            || maximum == 0
            || self.reservations.contains_key(request_id)
            || self.charged_requests.contains(request_id)
        {
            bail!("invalid or reused spend reservation");
        }
        self.require_work_stage()?;
        let reserved: u128 = self.reservations.values().map(|v| u128::from(*v)).sum();
        if u128::from(self.settled_microusd) + reserved + u128::from(maximum)
            > u128::from(plan.settings.maximum_microusd)
            || self.active_seconds >= plan.settings.maximum_active_seconds
        {
            self.stage = HarnessStage::BudgetExhausted;
            bail!("cycle budget exhausted");
        }
        self.reservations.insert(request_id.into(), maximum);
        Ok(())
    }
    fn require_work_stage(&self) -> Result<()> {
        if matches!(
            self.stage,
            HarnessStage::Paused
                | HarnessStage::Promoted
                | HarnessStage::RolledBack
                | HarnessStage::Rejected
                | HarnessStage::BudgetExhausted
                | HarnessStage::RecoveryRequired
                | HarnessStage::SourceAdopting
                | HarnessStage::SourceAdopted
                | HarnessStage::ReadyToPromote
                | HarnessStage::WaitingForCleanSource
        ) {
            bail!("cycle cannot start provider calls in its current state");
        }
        Ok(())
    }

    /// Persist this reservation before starting a timed activity. A caller must
    /// enforce this duration using a monotonic deadline; wall-clock restart time
    /// never releases it. Work IDs are single-use, like physical request IDs.
    pub fn reserve_active_time(
        &mut self,
        plan: &HarnessPlan,
        work_id: &str,
        maximum_seconds: u64,
    ) -> Result<()> {
        self.require_plan(plan)?;
        // Promotion preparation performs host-only inspection/transfer work.
        // Provider reservations remain forbidden once the decision is frozen.
        if self.stage != HarnessStage::ReadyToPromote {
            self.require_work_stage()?;
        }
        self.reserve_time(plan, work_id, maximum_seconds)
    }

    pub fn reserve_transition_time(
        &mut self,
        plan: &HarnessPlan,
        work_id: &str,
        maximum_seconds: u64,
    ) -> Result<()> {
        self.require_plan(plan)?;
        if !matches!(
            self.stage,
            HarnessStage::ReadyToPromote
                | HarnessStage::SourceAdopting
                | HarnessStage::SourceAdopted
                | HarnessStage::Promoted
                | HarnessStage::RecoveryRequired
        ) || !self.reservations.is_empty()
            || !self.active_reservations.is_empty()
        {
            bail!("source/binary transition requires settled work and an unpaused cycle");
        }
        self.reserve_time(plan, work_id, maximum_seconds)
    }

    fn reserve_time(
        &mut self,
        plan: &HarnessPlan,
        work_id: &str,
        maximum_seconds: u64,
    ) -> Result<()> {
        if !valid_id(work_id)
            || maximum_seconds == 0
            || self.active_reservations.contains_key(work_id)
            || self.completed_work.contains(work_id)
        {
            bail!("invalid or reused active-time reservation");
        }
        let reserved: u128 = self
            .active_reservations
            .values()
            .map(|v| u128::from(*v))
            .sum();
        if u128::from(self.active_seconds) + reserved + u128::from(maximum_seconds)
            > u128::from(plan.settings.maximum_active_seconds)
        {
            self.stage = HarnessStage::BudgetExhausted;
            bail!("cycle active-time budget exhausted");
        }
        self.active_reservations
            .insert(work_id.into(), maximum_seconds);
        Ok(())
    }

    pub fn settle_active_time(
        &mut self,
        plan: &HarnessPlan,
        work_id: &str,
        elapsed_seconds: u64,
    ) -> Result<()> {
        self.require_plan(plan)?;
        let reserved = *self
            .active_reservations
            .get(work_id)
            .context("unknown active-time reservation")?;
        if elapsed_seconds > reserved {
            self.stage = HarnessStage::RecoveryRequired;
            bail!("activity exceeded its reserved deadline");
        }
        self.active_seconds = self
            .active_seconds
            .checked_add(elapsed_seconds)
            .context("active-time overflow")?;
        self.active_reservations.remove(work_id);
        self.completed_work.insert(work_id.into());
        if self.active_seconds >= plan.settings.maximum_active_seconds {
            self.stage = HarnessStage::BudgetExhausted;
        }
        Ok(())
    }

    pub fn settle(&mut self, request_id: &str, actual: u64) -> Result<()> {
        let reserved = *self
            .reservations
            .get(request_id)
            .context("unknown reservation")?;
        if actual > reserved {
            self.stage = HarnessStage::RecoveryRequired;
            bail!("actual charge exceeds frozen reservation");
        }
        self.settled_microusd = self
            .settled_microusd
            .checked_add(actual)
            .context("charge overflow")?;
        self.reservations.remove(request_id);
        self.charged_requests.insert(request_id.into());
        Ok(())
    }
    pub fn add_active_time(&mut self, seconds: u64, plan: &HarnessPlan) -> Result<()> {
        self.require_plan(plan)?;
        self.active_seconds = self.active_seconds.saturating_add(seconds);
        if self.active_seconds >= plan.settings.maximum_active_seconds {
            self.stage = HarnessStage::BudgetExhausted;
        }
        Ok(())
    }
    pub fn decide(&mut self, plan: &HarnessPlan) -> Result<()> {
        self.require_plan(plan)?;
        if !self.reservations.is_empty() || !self.active_reservations.is_empty() {
            bail!("unsettled provider charges prevent promotion");
        }
        if !matches!(
            self.stage,
            HarnessStage::CandidateReady | HarnessStage::Evaluating | HarnessStage::ReadyToPromote
        ) {
            bail!("cycle is not ready to decide");
        }
        let decision = evaluate_harness(
            plan,
            self.candidate_digest
                .as_deref()
                .context("missing candidate")?,
            &self.trials,
            self.engineering_checks_passed,
        )?;
        self.stage = match decision.verdict {
            HarnessVerdict::Promote => HarnessStage::ReadyToPromote,
            HarnessVerdict::Reject => HarnessStage::Rejected,
            HarnessVerdict::Incomplete => HarnessStage::Evaluating,
        };
        self.decision = Some(decision);
        Ok(())
    }
    fn require_plan(&self, plan: &HarnessPlan) -> Result<()> {
        plan.validate()?;
        if self.id != plan.id || self.plan_digest != plan.digest || self.schema != HARNESS_SCHEMA {
            bail!("cycle/plan binding changed");
        }
        if let Some(receipt) = &self.abandonment {
            let mut closed = self.clone();
            closed.abandonment = None;
            let mut previous = closed.clone();
            previous.stage = receipt.previous_stage.clone();
            if self.stage != HarnessStage::Rejected
                || self.decision.is_some()
                || receipt.closed_state_digest != canonical_digest(&closed)?
                || receipt.previous_state_digest != canonical_digest(&previous)?
                || receipt.recorded_at < self.created_at
            {
                bail!("abandoned cycle evidence changed");
            }
        }
        Ok(())
    }
}

/// Serialize each mutation under the existing vault lock; reservations reach
/// atomic durable storage before a caller is permitted to issue a physical POST.
pub fn update_harness_cycle<T>(
    vault: &LearningVault,
    id: &str,
    update: impl FnOnce(&mut HarnessCycle) -> Result<T>,
) -> Result<T> {
    if !valid_id(id) {
        bail!("invalid cycle ID");
    }
    let _lock = vault.acquire_lock()?;
    let path = format!("harness/cycles/{id}.json");
    let mut state: HarnessCycle = vault.read_mutable(&path)?.context("cycle not found")?;
    if state.abandonment.is_some() {
        bail!("abandoned cycle is immutable; its unknown reservations are retained");
    }
    let result = update(&mut state);
    vault.write_mutable(&path, &state)?;
    result
}

pub fn start_harness_cycle(
    vault: &LearningVault,
    plan: &HarnessPlan,
    now: u64,
) -> Result<HarnessCycle> {
    let _lock = vault.acquire_lock()?;
    plan.validate()?;
    let path = format!("harness/cycles/{}.json", plan.id);
    if vault.read_mutable::<HarnessCycle>(&path)?.is_some() {
        bail!("cycle already exists; resume it without resetting budgets");
    }
    let state = HarnessCycle::new(plan, now)?;
    let plan_path = format!("harness/plans/{}.json", plan.id);
    if let Some(existing) = vault.read_mutable::<HarnessPlan>(&plan_path)? {
        existing.validate()?;
        if existing.digest != plan.digest {
            bail!("existing frozen plan cannot be replaced after interrupted creation");
        }
    } else {
        vault.write_mutable(&plan_path, plan)?;
    }
    vault.write_mutable(&path, &state)?;
    Ok(state)
}

pub fn harness_idle_eligible(
    settings: &HarnessSettings,
    idle_seconds: u64,
    now: u64,
    last_started: Option<u64>,
    evidence_tasks: &[String],
    active_cycle: bool,
) -> bool {
    settings.enabled
        && settings.automatic
        && !active_cycle
        && idle_seconds >= settings.idle_seconds
        && last_started.is_none_or(|last| now.saturating_sub(last) >= settings.cooldown_seconds)
        && evidence_tasks.len() >= 3
        && evidence_tasks.iter().collect::<BTreeSet<_>>().len() >= 2
}
fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
fn validate_digest(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid digest");
    }
    Ok(())
}

/// A deterministic schedule alternates the first lane on every repetition.
/// Resumption uses persisted receipts, never wall-clock ordering.
pub fn harness_trial_schedule(plan: &HarnessPlan) -> Result<Vec<(String, u8, HarnessLane)>> {
    plan.validate()?;
    let mut schedule = Vec::new();
    for repetition in 0..plan.repetitions {
        for case in &plan.cases {
            let lanes = if repetition % 2 == 0 {
                [HarnessLane::Baseline, HarnessLane::Candidate]
            } else {
                [HarnessLane::Candidate, HarnessLane::Baseline]
            };
            for lane in lanes {
                schedule.push((case.id.clone(), repetition, lane));
            }
        }
    }
    Ok(schedule)
}

pub fn load_harness_cycle(vault: &LearningVault, id: &str) -> Result<(HarnessPlan, HarnessCycle)> {
    if !valid_id(id) {
        bail!("invalid cycle ID");
    }
    let plan: HarnessPlan = vault
        .read_mutable(&format!("harness/plans/{id}.json"))?
        .context("plan not found")?;
    let cycle: HarnessCycle = vault
        .read_mutable(&format!("harness/cycles/{id}.json"))?
        .context("cycle not found")?;
    cycle.require_plan(&plan)?;
    Ok((plan, cycle))
}

pub fn list_harness_cycles(vault: &LearningVault) -> Result<Vec<HarnessCycle>> {
    let root = vault.resolve_relative("harness/cycles")?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut cycles = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|name| name.to_str())
            .context("invalid cycle filename")?;
        cycles.push(load_harness_cycle(vault, id)?.1);
    }
    cycles.sort_by_key(|c| (c.created_at, c.id.clone()));
    Ok(cycles)
}

pub fn pause_harness_cycle(vault: &LearningVault, id: &str) -> Result<()> {
    update_harness_cycle(vault, id, |state| {
        if matches!(
            state.stage,
            HarnessStage::Promoted
                | HarnessStage::RolledBack
                | HarnessStage::Rejected
                | HarnessStage::BudgetExhausted
                | HarnessStage::RecoveryRequired
        ) {
            bail!("terminal or recovery state cannot be paused");
        }
        if state.stage != HarnessStage::Paused {
            state.resume_stage = Some(state.stage.clone());
            state.stage = HarnessStage::Paused;
        }
        state.pause_cause = Some(HarnessPauseCause::User);
        Ok(())
    })
}

pub fn resume_harness_cycle(vault: &LearningVault, id: &str) -> Result<()> {
    let (plan, _) = load_harness_cycle(vault, id)?;
    update_harness_cycle(vault, id, |state| {
        state.require_plan(&plan)?;
        if state.stage != HarnessStage::Paused {
            bail!("only a paused cycle can resume");
        }
        state.stage = state.resume_stage.take().context("missing resume state")?;
        state.pause_cause = None;
        Ok(())
    })
}

/// Host-side receipt admission. Candidate text never writes a pass flag into
/// the evaluation: the host verifier constructs this receipt after execution.
pub fn record_harness_trial(vault: &LearningVault, id: &str, receipt: HarnessTrial) -> Result<()> {
    let (plan, _) = load_harness_cycle(vault, id)?;
    update_harness_cycle(vault, id, |state| {
        let paused_trial = state.stage == HarnessStage::Paused
            && matches!(
                state.resume_stage,
                Some(HarnessStage::CandidateReady | HarnessStage::Evaluating)
            );
        if !paused_trial
            && !matches!(
                state.stage,
                HarnessStage::CandidateReady
                    | HarnessStage::Evaluating
                    | HarnessStage::BudgetExhausted
            )
        {
            bail!("cycle cannot admit trials in its current state");
        }
        let candidate = state
            .candidate_digest
            .as_deref()
            .context("missing candidate identity")?;
        let mut trials = state.trials.clone();
        trials.push(receipt.clone());
        // Also validates binding, unique lane/repetition and output identity.
        evaluate_harness(&plan, candidate, &trials, false)?;
        let expected =
            harness_trial_schedule(&plan)?
                .into_iter()
                .find(|(case, repetition, lane)| {
                    !state.trials.iter().any(|t| {
                        &t.case_id == case && t.repetition == *repetition && t.lane == *lane
                    })
                });
        if expected != Some((receipt.case_id.clone(), receipt.repetition, receipt.lane)) {
            bail!("trial is out of frozen alternating order");
        }
        let digest = canonical_digest(&receipt)?;
        let path = format!("harness/receipts/{digest}.json");
        if let Some(existing) = vault.read_mutable::<HarnessTrial>(&path)? {
            if existing != receipt {
                bail!("immutable receipt collision");
            }
        } else {
            vault.write_mutable(&path, &receipt)?;
        }
        state.trials = trials;
        if paused_trial {
            state.resume_stage = Some(HarnessStage::Evaluating);
        } else if state.stage != HarnessStage::BudgetExhausted {
            state.stage = HarnessStage::Evaluating;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> HarnessPlan {
        let mut plan = HarnessPlan {
            vm: None,
            preparation_seconds: None,
            guidance_source: None,
            activation_executable: None,
            source_files: Default::default(),
            schema: HARNESS_SCHEMA.into(),
            id: "fixture".into(),
            baseline: SourceRepositorySnapshotV1 {
                schema: "test".into(),
                branch: "main".into(),
                head: "a".repeat(40),
                tracked_tree_digest: "a".repeat(64),
                submodule_digest: "b".repeat(64),
                repository_fingerprint: "c".repeat(64),
            },
            baseline_binary_digest: "a".repeat(64),
            route_digest: "b".repeat(64),
            price_digest: "c".repeat(64),
            evidence_ids: vec!["event".into()],
            cases: (0..3)
                .map(|i| HarnessCase {
                    id: format!("case-{i}"),
                    input_digest: "d".repeat(64),
                    verifier_digest: "e".repeat(64),
                    grants_digest: "f".repeat(64),
                    held_out: i > 0,
                    predicted_fix: i == 0,
                })
                .collect(),
            repetitions: 3,
            settings: HarnessSettings::default(),
            digest: String::new(),
        };
        plan.seal().unwrap();
        plan
    }
    #[test]
    fn hrs253_preparation_time_is_sealed_charged_and_legacy_compatible() {
        let mut plan = plan();
        let legacy = serde_json::to_value(&plan).unwrap();
        assert!(legacy.get("preparation_seconds").is_none());
        plan.preparation_seconds = Some(17);
        plan.seal().unwrap();
        assert_eq!(HarnessCycle::new(&plan, 0).unwrap().active_seconds, 17);
        plan.preparation_seconds = Some(plan.settings.maximum_active_seconds + 1);
        assert!(plan.seal().is_err());
        plan.preparation_seconds = Some(0);
        assert!(plan.seal().is_err());
    }

    #[test]
    fn hrs253_source_selection_is_sealed_and_legacy_guidance_stays_readable() {
        let original = plan();
        let json = serde_json::to_value(&original).unwrap();
        assert!(json.get("source_files").is_none());
        let decoded: HarnessPlan = serde_json::from_value(json).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded.digest, original.digest);
        let mut source = original.clone();
        source.source_files.insert(
            "src/example.rs".into(),
            canonical_digest(&"before").unwrap(),
        );
        source.seal().unwrap();
        assert_ne!(source.digest, original.digest);
        source.source_files.insert(
            "src/example.rs".into(),
            canonical_digest(&"changed").unwrap(),
        );
        assert!(source.validate().is_err());
        source
            .source_files
            .insert("src/harness_budget.rs".into(), "a".repeat(64));
        assert!(source.seal().is_err());
    }

    fn trials(plan: &HarnessPlan) -> Vec<HarnessTrial> {
        plan.cases
            .iter()
            .flat_map(|case| {
                (0..3).flat_map(move |r| {
                    [HarnessLane::Baseline, HarnessLane::Candidate].map(move |lane| HarnessTrial {
                        plan_digest: plan.digest.clone(),
                        candidate_digest: "f".repeat(64),
                        route_digest: plan.route_digest.clone(),
                        case_id: case.id.clone(),
                        repetition: r,
                        lane,
                        verified_pass: case.held_out || lane == HarnessLane::Candidate,
                        infrastructure_failure: false,
                        false_completion: false,
                        output_digest: "a".repeat(64),
                        cost_microusd: 10,
                        elapsed_millis: 100,
                    })
                })
            })
            .collect()
    }
    #[test]
    fn hrs253_transition_time_preserves_limits_and_unsettled_work() {
        let plan = plan();
        let mut cycle = HarnessCycle::new(&plan, 0).unwrap();
        cycle.stage = HarnessStage::Promoted;
        assert!(cycle.reserve(&plan, "provider", 10).is_err());
        cycle
            .reserve_transition_time(&plan, "rollback", 20)
            .unwrap();
        assert!(cycle.reserve_transition_time(&plan, "overlap", 1).is_err());
        cycle.settle_active_time(&plan, "rollback", 20).unwrap();
        cycle.stage = HarnessStage::Paused;
        assert!(cycle.reserve_transition_time(&plan, "paused", 1).is_err());
        cycle.stage = HarnessStage::RecoveryRequired;
        cycle.reservations.insert("unknown".into(), 100);
        assert!(cycle
            .reserve_transition_time(&plan, "unknown-charge", 1)
            .is_err());
        assert_eq!(cycle.reservations["unknown"], 100);
        cycle.reservations.clear();
        cycle.active_seconds = plan.settings.maximum_active_seconds - 5;
        assert!(cycle
            .reserve_transition_time(&plan, "over-budget", 6)
            .is_err());
        assert_eq!(cycle.stage, HarnessStage::BudgetExhausted);
        assert!(cycle.active_reservations.is_empty());
    }

    #[test]
    fn hrs253_activation_destination_is_frozen_and_legacy_plans_remain_readable() {
        let mut frozen = plan();
        let legacy = serde_json::to_value(&frozen).unwrap();
        assert!(legacy.get("activation_executable").is_none());
        serde_json::from_value::<HarnessPlan>(legacy)
            .unwrap()
            .validate()
            .unwrap();
        frozen.activation_executable = Some("relative/ca".into());
        assert!(frozen.seal().is_err());
        frozen.activation_executable = Some("/tmp/installed/ca".into());
        frozen.seal().unwrap();
        frozen.activation_executable = Some("/tmp/another/ca".into());
        assert!(frozen.validate().is_err());
    }

    #[test]
    fn hrs253_promotion_preparation_charges_time_without_reopening_provider_calls() {
        let plan = plan();
        let mut cycle = HarnessCycle::new(&plan, 0).unwrap();
        cycle.stage = HarnessStage::ReadyToPromote;
        assert!(cycle.reserve(&plan, "provider", 10).is_err());
        cycle.reserve_active_time(&plan, "import", 120).unwrap();
        cycle.settle_active_time(&plan, "import", 30).unwrap();
        assert_eq!(cycle.active_seconds, 30);
        assert_eq!(cycle.stage, HarnessStage::ReadyToPromote);
        cycle.stage = HarnessStage::Paused;
        assert!(cycle.reserve_active_time(&plan, "paused", 1).is_err());
    }

    #[test]
    fn durable_trials_pause_resume_and_decision() {
        let root = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(root.path().join("vault")).unwrap();
        let plan = plan();
        start_harness_cycle(&vault, &plan, 1).unwrap();
        assert!(start_harness_cycle(&vault, &plan, 2).is_err());
        update_harness_cycle(&vault, &plan.id, |cycle| {
            cycle.candidate_digest = Some("f".repeat(64));
            cycle.stage = HarnessStage::CandidateReady;
            cycle.reserve(&plan, "request", 100)?;
            cycle.settle("request", 50)?;
            Ok(())
        })
        .unwrap();
        pause_harness_cycle(&vault, &plan.id).unwrap();
        record_harness_trial(&vault, &plan.id, trials(&plan)[0].clone()).unwrap();
        let (_, paused) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert_eq!(paused.stage, HarnessStage::Paused);
        assert_eq!(paused.resume_stage, Some(HarnessStage::Evaluating));
        resume_harness_cycle(&vault, &plan.id).unwrap();
        let rows = trials(&plan);
        for (case, repetition, lane) in harness_trial_schedule(&plan).unwrap().into_iter().skip(1) {
            let receipt = rows
                .iter()
                .find(|t| t.case_id == case && t.repetition == repetition && t.lane == lane)
                .unwrap();
            record_harness_trial(&vault, &plan.id, receipt.clone()).unwrap();
        }
        update_harness_cycle(&vault, &plan.id, |cycle| {
            cycle.engineering_checks_passed = true;
            cycle.decide(&plan)
        })
        .unwrap();
        let (_, cycle) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert_eq!(cycle.stage, HarnessStage::ReadyToPromote);
        assert_eq!(cycle.settled_microusd, 50);
        assert_eq!(list_harness_cycles(&vault).unwrap().len(), 1);
    }
    #[test]
    fn hrs253_finished_trial_preserves_exhausted_budget_state() {
        let root = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(root.path().join("vault")).unwrap();
        let plan = plan();
        start_harness_cycle(&vault, &plan, 0).unwrap();
        update_harness_cycle(&vault, &plan.id, |cycle| {
            cycle.candidate_digest = Some("f".repeat(64));
            cycle.stage = HarnessStage::BudgetExhausted;
            Ok(())
        })
        .unwrap();
        record_harness_trial(&vault, &plan.id, trials(&plan)[0].clone()).unwrap();
        let (_, mut cycle) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert_eq!(cycle.stage, HarnessStage::BudgetExhausted);
        assert_eq!(cycle.trials.len(), 1);
        assert!(cycle.decide(&plan).is_err());
    }

    #[test]
    fn matched_improvement_requires_complete_nonregressing_evidence() {
        let plan = plan();
        let mut rows = trials(&plan);
        assert_eq!(
            evaluate_harness(&plan, &"f".repeat(64), &rows, true)
                .unwrap()
                .verdict,
            HarnessVerdict::Promote
        );
        rows.pop();
        assert_eq!(
            evaluate_harness(&plan, &"f".repeat(64), &rows, true)
                .unwrap()
                .verdict,
            HarnessVerdict::Incomplete
        );
        rows = trials(&plan);
        rows[7].verified_pass = false;
        assert_eq!(
            evaluate_harness(&plan, &"f".repeat(64), &rows, true)
                .unwrap()
                .verdict,
            HarnessVerdict::Reject
        );
        rows = trials(&plan);
        rows[0].route_digest = "0".repeat(64);
        assert!(evaluate_harness(&plan, &"f".repeat(64), &rows, true).is_err());
    }
    #[test]
    fn budget_resume_does_not_release_unknown_spend_or_reset_time() {
        let mut plan = plan();
        plan.settings.maximum_microusd = 100;
        plan.seal().unwrap();
        let mut state = HarnessCycle::new(&plan, 0).unwrap();
        state.reserve(&plan, "request-1", 70).unwrap();
        let serialized = serde_json::to_vec(&state).unwrap();
        let mut resumed: HarnessCycle = serde_json::from_slice(&serialized).unwrap();
        assert!(resumed.reserve(&plan, "request-2", 40).is_err());
        assert_eq!(resumed.reservations["request-1"], 70);
        resumed.settle("request-1", 60).unwrap();
        assert_eq!(resumed.settled_microusd, 60);
        assert!(resumed.reserve(&plan, "request-1", 1).is_err());
        resumed.add_active_time(7200, &plan).unwrap();
        assert_eq!(resumed.stage, HarnessStage::BudgetExhausted);
    }
    #[test]
    fn hrs253_active_time_reservations_survive_crash_and_block_reuse() {
        let mut plan = plan();
        plan.settings.maximum_active_seconds = 100;
        plan.seal().unwrap();
        let mut state = HarnessCycle::new(&plan, 0).unwrap();
        state.reserve_active_time(&plan, "work-1", 80).unwrap();
        let mut resumed: HarnessCycle =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(resumed.reserve_active_time(&plan, "work-2", 21).is_err());
        assert_eq!(resumed.active_reservations["work-1"], 80);
        resumed.settle_active_time(&plan, "work-1", 60).unwrap();
        assert_eq!(resumed.active_seconds, 60);
        assert!(resumed.reserve_active_time(&plan, "work-1", 1).is_err());
        // Exhaustion is terminal even if a later settlement frees headroom.
        assert!(resumed.reserve_active_time(&plan, "work-3", 20).is_err());
    }

    #[test]
    fn hrs253_active_work_prevents_promotion_and_overrun_is_retained() {
        let plan = plan();
        let mut state = HarnessCycle::new(&plan, 0).unwrap();
        state.reserve_active_time(&plan, "trial", 10).unwrap();
        state.stage = HarnessStage::Evaluating;
        assert!(state.decide(&plan).is_err());
        assert!(state.settle_active_time(&plan, "trial", 11).is_err());
        assert_eq!(state.stage, HarnessStage::RecoveryRequired);
        assert_eq!(state.active_reservations["trial"], 10);
    }

    #[test]
    fn frozen_plan_and_duplicate_trials_cannot_be_rewritten() {
        let mut plan = plan();
        let mut rows = trials(&plan);
        rows.push(rows[0].clone());
        assert!(evaluate_harness(&plan, &"f".repeat(64), &rows, true).is_err());
        plan.cases[0].verifier_digest = "1".repeat(64);
        assert!(plan.validate().is_err());
    }
    #[test]
    fn idle_scheduler_requires_new_cross_task_evidence_and_cooldown() {
        let settings = HarnessSettings {
            enabled: true,
            ..Default::default()
        };
        let tasks = vec!["a".into(), "a".into(), "b".into()];
        assert!(harness_idle_eligible(
            &settings,
            300,
            100000,
            Some(0),
            &tasks,
            false
        ));
        assert!(!harness_idle_eligible(
            &settings,
            300,
            100000,
            Some(99999),
            &tasks,
            false
        ));
        assert!(!harness_idle_eligible(
            &settings, 299, 100000, None, &tasks, false
        ));
        assert!(!harness_idle_eligible(
            &settings, 300, 100000, None, &tasks, true
        ));
    }
    #[test]
    fn hrs253_vm_binding_is_sealed_without_changing_legacy_plan_digests() {
        let original = plan();
        let legacy = serde_json::to_value(&original).unwrap();
        assert!(legacy.get("vm").is_none());
        let decoded: HarnessPlan = serde_json::from_value(legacy).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded.digest, original.digest);
        let mut selected = original.clone();
        selected.vm = Some(super::super::FrozenHarnessVm {
            schema: super::super::HARNESS_VM_SCHEMA.into(),
            profile: super::super::HarnessVmProfile {
                tart: "/vm-tools/tart".into(),
                home: "/vm-store".into(),
                template: "ca-base".into(),
                guest_agent_sha256: "a".repeat(64),
            },
            adapter_digest: "b".repeat(64),
            files: super::super::HARNESS_VM_FILES
                .into_iter()
                .map(|name| {
                    (
                        name.into(),
                        super::super::HarnessVmFile {
                            sha256: "c".repeat(64),
                            bytes: 1,
                            device: 1,
                            inode: 1,
                            uid: 501,
                            mode: if name == "tart" { 0o700 } else { 0o600 },
                            modified_seconds: 1,
                            modified_nanoseconds: 0,
                            changed_seconds: 1,
                            changed_nanoseconds: 0,
                        },
                    )
                })
                .collect(),
        });
        selected.seal().unwrap();
        assert_ne!(selected.digest, original.digest);
        let frozen_digest = selected.digest.clone();
        selected
            .vm
            .as_mut()
            .unwrap()
            .files
            .get_mut("disk.img")
            .unwrap()
            .sha256 = "d".repeat(64);
        assert!(selected.validate().is_err());
        selected.seal().unwrap();
        assert_ne!(selected.digest, frozen_digest);
        selected.vm.as_mut().unwrap().profile.template = "../other-vm".into();
        assert!(selected.seal().is_err());
    }
}
