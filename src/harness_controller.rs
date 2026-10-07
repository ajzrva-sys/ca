//! Production evaluation driver. Each step rechecks durable state so pause,
//! exhausted budgets and failed infrastructure stop admission of further work.
use crate::agent::client::AgentClient;
use anyhow::{bail, Result};
use codex_team::self_improvement::{
    load_harness_cycle, update_harness_cycle, HarnessCycle, HarnessStage, LearningVault,
};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationStop {
    Paused,
    BudgetExhausted,
    RecoveryRequired,
    AwaitingEngineeringChecks,
    EngineeringFailed,
    DecisionRecorded,
    NeedsReview,
    Terminal,
}

fn next_action(
    plan: &codex_team::self_improvement::HarnessPlan,
    state: &HarnessCycle,
) -> Result<Option<EvaluationStop>> {
    use HarnessStage::*;
    Ok(match state.stage {
        Paused => Some(EvaluationStop::Paused),
        BudgetExhausted => Some(EvaluationStop::BudgetExhausted),
        RecoveryRequired => Some(EvaluationStop::RecoveryRequired),
        ReadyToPromote => Some(EvaluationStop::NeedsReview),
        SourceAdopting | SourceAdopted | Promoted | RolledBack | Rejected => {
            Some(EvaluationStop::Terminal)
        }
        CandidateReady | Evaluating => {
            if state
                .trials
                .iter()
                .any(|trial| trial.infrastructure_failure)
            {
                Some(EvaluationStop::RecoveryRequired)
            } else if state.trials.len() == plan.cases.len() * usize::from(plan.repetitions) * 2
                && !state.engineering_checks_passed
            {
                Some(EvaluationStop::AwaitingEngineeringChecks)
            } else {
                None
            }
        }
        _ => bail!("cycle has no candidate ready for evaluation"),
    })
}

#[derive(Serialize)]
pub struct EvaluationResult {
    pub stop: EvaluationStop,
    pub cycle: HarnessCycle,
}

pub async fn evaluate_saved_candidate(
    vault: LearningVault,
    id: &str,
    client: AgentClient,
    baseline_binary: &Path,
    repository: &Path,
) -> Result<EvaluationResult> {
    let (plan, cycle) = load_harness_cycle(&vault, id)?;
    if matches!(
        next_action(&plan, &cycle)?,
        None | Some(EvaluationStop::AwaitingEngineeringChecks)
    ) {
        crate::harness_execution::require_native_baseline(baseline_binary).await?;
    }
    let (inputs, harness, candidate_binary) =
        if crate::harness_source_generation::load(&vault, id)?.is_some() {
            crate::harness_source_build::load_candidate(&vault, id)?
        } else {
            let (inputs, harness) = crate::harness_generation::load_candidate(&vault, id)?;
            (inputs, harness, baseline_binary.to_path_buf())
        };
    loop {
        crate::harness_idle::boundary(&vault, id).await?;
        let (plan, state) = load_harness_cycle(&vault, id)?;
        if let Some(stop) = next_action(&plan, &state)? {
            if stop == EvaluationStop::AwaitingEngineeringChecks {
                return finish_evaluation(&vault, id, &plan, repository, baseline_binary).await;
            }
            if stop == EvaluationStop::RecoveryRequired
                && state.stage != HarnessStage::RecoveryRequired
            {
                update_harness_cycle(&vault, id, |current| {
                    if matches!(
                        current.stage,
                        HarnessStage::CandidateReady | HarnessStage::Evaluating
                    ) {
                        current.stage = HarnessStage::RecoveryRequired;
                        current.reason =
                            "Trial infrastructure failed; no subsequent trials admitted".into();
                    }
                    Ok(())
                })?;
            }
            return Ok(EvaluationResult {
                stop,
                cycle: load_harness_cycle(&vault, id)?.1,
            });
        }
        let receipt = crate::harness_trials::run_next_trial(
            vault.clone(),
            id,
            client.clone(),
            inputs.provider.clone(),
            &inputs.cases,
            &harness,
            baseline_binary,
            &candidate_binary,
            repository,
        )
        .await?;
        if receipt.is_none() {
            return finish_evaluation(&vault, id, &plan, repository, baseline_binary).await;
        }
        tokio::task::yield_now().await;
    }
}

async fn finish_evaluation(
    vault: &LearningVault,
    id: &str,
    plan: &codex_team::self_improvement::HarnessPlan,
    repository: &Path,
    _baseline_binary: &Path,
) -> Result<EvaluationResult> {
    let checked = if crate::harness_source_generation::load(vault, id)?.is_some() {
        crate::harness_source_build::build_and_admit(vault.clone(), id)
            .await
            .map(|_| true)
    } else {
        crate::harness_engineering::check_guidance(vault.clone(), id, repository).await
    };
    let passed = match checked {
        Ok(passed) => passed,
        Err(error) => {
            let (_, cycle) = load_harness_cycle(vault, id)?;
            let stop = match cycle.stage {
                HarnessStage::Paused => EvaluationStop::Paused,
                HarnessStage::BudgetExhausted => EvaluationStop::BudgetExhausted,
                HarnessStage::RecoveryRequired => EvaluationStop::RecoveryRequired,
                _ => return Err(error),
            };
            return Ok(EvaluationResult { stop, cycle });
        }
    };
    let mut stop = EvaluationStop::DecisionRecorded;
    update_harness_cycle(vault, id, |state| {
        match state.stage {
            HarnessStage::Paused => stop = EvaluationStop::Paused,
            HarnessStage::BudgetExhausted => stop = EvaluationStop::BudgetExhausted,
            HarnessStage::RecoveryRequired => stop = EvaluationStop::RecoveryRequired,
            HarnessStage::CandidateReady | HarnessStage::Evaluating => {
                // The engine rechecks every trial and unresolved reservation.
                state.decide(plan)?;
                if !passed {
                    stop = EvaluationStop::EngineeringFailed;
                }
            }
            _ => bail!("cycle changed during engineering checks"),
        }
        Ok(())
    })?;
    if load_harness_cycle(vault, id)?.1.stage == HarnessStage::ReadyToPromote {
        stop = EvaluationStop::NeedsReview;
        update_harness_cycle(vault, id, |state| {
            state.reason =
                "Evaluation passed; exact candidate and evaluation require promotion review".into();
            Ok(())
        })?;
    }
    Ok(EvaluationResult {
        stop,
        cycle: load_harness_cycle(vault, id)?.1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_team::self_improvement::{HarnessLane, HarnessTrial};

    #[test]
    fn hrs253_controller_stops_on_pause_budget_infrastructure_and_missing_checks() {
        let provider = crate::harness_budget::tests::frozen();
        let (plan, _, harness) =
            crate::harness_trials::tests::fixture_plan(&provider, &"a".repeat(64));
        let mut cycle = HarnessCycle::new(&plan, 0).unwrap();
        assert!(next_action(&plan, &cycle).is_err());
        for (stage, stop) in [
            (HarnessStage::ReadyToPromote, EvaluationStop::NeedsReview),
            (HarnessStage::Paused, EvaluationStop::Paused),
            (
                HarnessStage::BudgetExhausted,
                EvaluationStop::BudgetExhausted,
            ),
            (
                HarnessStage::RecoveryRequired,
                EvaluationStop::RecoveryRequired,
            ),
        ] {
            cycle.stage = stage;
            assert_eq!(next_action(&plan, &cycle).unwrap(), Some(stop));
        }
        cycle.stage = HarnessStage::Evaluating;
        assert_eq!(next_action(&plan, &cycle).unwrap(), None);
        cycle.trials = codex_team::self_improvement::harness_trial_schedule(&plan)
            .unwrap()
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
        assert_eq!(
            next_action(&plan, &cycle).unwrap(),
            Some(EvaluationStop::AwaitingEngineeringChecks)
        );
        cycle.engineering_checks_passed = true;
        assert_eq!(next_action(&plan, &cycle).unwrap(), None);
        cycle.trials[0].infrastructure_failure = true;
        assert_eq!(
            next_action(&plan, &cycle).unwrap(),
            Some(EvaluationStop::RecoveryRequired)
        );
    }
}
