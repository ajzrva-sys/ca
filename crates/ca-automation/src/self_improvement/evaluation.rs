use super::{EvaluationCaseResultV1, EvaluationPlanV1, JudgeVoteV1, EVALUATION_PLAN_SCHEMA};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PromotionDecisionV1 {
    pub schema: String,
    pub promoted: bool,
    pub deterministic_decision: bool,
    pub reasons: Vec<String>,
}

pub fn validate_frozen_evaluation_plan(plan: &EvaluationPlanV1) -> Result<()> {
    if plan.schema != EVALUATION_PLAN_SCHEMA {
        bail!("unsupported evaluation plan schema");
    }
    if plan.plan_id.is_empty() || plan.objective_id.is_empty() {
        bail!("evaluation plan identifiers must be nonempty");
    }
    if plan.cases.is_empty() {
        bail!("evaluation plan must freeze at least one case");
    }
    let unique = plan
        .cases
        .iter()
        .map(|case| case.case_id.as_str())
        .collect::<BTreeSet<_>>();
    if unique.len() != plan.cases.len() {
        bail!("evaluation case identifiers must be unique");
    }
    let adversarial = plan
        .cases
        .iter()
        .filter(|case| case.adversarial)
        .filter_map(|case| case.input_evidence_ids.first())
        .collect::<BTreeSet<_>>()
        .len();
    let historical = plan
        .cases
        .iter()
        .filter(|case| !case.adversarial)
        .filter_map(|case| case.input_evidence_ids.first())
        .collect::<BTreeSet<_>>()
        .len();
    if historical < 5 && adversarial < 3 {
        bail!("fewer than five historical cases requires at least three adversarial cases");
    }
    Ok(())
}

pub fn decide_promotion(
    plan: &EvaluationPlanV1,
    baseline: &[EvaluationCaseResultV1],
    candidate: &[EvaluationCaseResultV1],
    votes: &[JudgeVoteV1],
    human_disposition: Option<&str>,
) -> Result<PromotionDecisionV1> {
    validate_frozen_evaluation_plan(plan)?;
    let baseline = index_results(plan, baseline, "baseline")?;
    let candidate = index_results(plan, candidate, "candidate")?;
    let mut reasons = Vec::new();

    if matches!(human_disposition, Some("reject" | "rollback" | "corrected")) {
        reasons.push("human disposition supersedes machine-positive evidence".to_string());
        return Ok(decision(false, true, reasons));
    }

    let mut corrected_failure = false;
    let mut equivalent_secondary_improvement = false;
    let mut deterministic_tie = true;
    for case in &plan.cases {
        let before = baseline[case.case_id.as_str()];
        let after = candidate[case.case_id.as_str()];
        if before.deterministic_pass && !after.deterministic_pass {
            reasons.push(format!(
                "candidate regressed deterministic baseline pass {}",
                case.case_id
            ));
            return Ok(decision(false, true, reasons));
        }
        if !before
            .acceptance_properties
            .is_subset(&after.acceptance_properties)
        {
            reasons.push(format!(
                "candidate removed a higher-priority acceptance property in {}",
                case.case_id
            ));
            return Ok(decision(false, true, reasons));
        }
        if !before.deterministic_pass && after.deterministic_pass {
            corrected_failure = true;
            deterministic_tie = false;
        }
        if before.deterministic_pass != after.deterministic_pass
            || before.acceptance_properties != after.acceptance_properties
            || before.output_equivalence_digest != after.output_equivalence_digest
        {
            deterministic_tie = false;
        }
        if before.output_equivalence_digest == after.output_equivalence_digest {
            let latency_improved =
                at_least_ten_percent_better(before.elapsed_millis, after.elapsed_millis);
            let cost_improved = before
                .estimated_microusd
                .zip(after.estimated_microusd)
                .is_some_and(|(before, after)| at_least_ten_percent_better(before, after));
            equivalent_secondary_improvement |= latency_improved || cost_improved;
        }
    }

    if corrected_failure || equivalent_secondary_improvement {
        reasons.push(if corrected_failure {
            "candidate corrects at least one deterministic baseline failure".to_string()
        } else {
            "candidate improves latency or cost by at least 10% with equivalent output".to_string()
        });
        return Ok(decision(true, true, reasons));
    }

    if deterministic_tie || (!corrected_failure && !equivalent_secondary_improvement) {
        let available = plan
            .available_non_author_judges
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let non_author_votes = votes
            .iter()
            .filter(|vote| !vote.author_route && available.contains(&vote.judge_route))
            .collect::<Vec<_>>();
        if available.is_empty() {
            reasons.push(
                "no deterministic improvement and no independent judge is available".to_string(),
            );
            return Ok(decision(false, false, reasons));
        }
        let voting = non_author_votes
            .iter()
            .map(|vote| vote.judge_route.clone())
            .collect::<BTreeSet<_>>();
        if voting != available || non_author_votes.iter().any(|vote| !vote.approves) {
            reasons.push("independent provider judges were missing or disagreed".to_string());
            return Ok(decision(false, false, reasons));
        }
        reasons.push("unanimous available non-author provider judges approved".to_string());
        return Ok(decision(true, false, reasons));
    }

    reasons.push("candidate produced no admissible correctness-first improvement".to_string());
    Ok(decision(false, true, reasons))
}

fn index_results<'a>(
    plan: &EvaluationPlanV1,
    results: &'a [EvaluationCaseResultV1],
    lane: &str,
) -> Result<BTreeMap<&'a str, &'a EvaluationCaseResultV1>> {
    let mut indexed = BTreeMap::new();
    for result in results {
        if indexed.insert(result.case_id.as_str(), result).is_some() {
            bail!("duplicate {lane} evaluation result for {}", result.case_id);
        }
    }
    for case in &plan.cases {
        if !indexed.contains_key(case.case_id.as_str()) {
            bail!("missing {lane} evaluation result for {}", case.case_id);
        }
    }
    if indexed.len() != plan.cases.len() {
        bail!("{lane} evaluation results contain cases outside the frozen plan");
    }
    Ok(indexed)
}

fn at_least_ten_percent_better(before: u64, after: u64) -> bool {
    before > 0 && after.saturating_mul(100) <= before.saturating_mul(90)
}

fn decision(
    promoted: bool,
    deterministic_decision: bool,
    reasons: Vec<String>,
) -> PromotionDecisionV1 {
    PromotionDecisionV1 {
        schema: "codex-team.promotion_decision.v1".to_string(),
        promoted,
        deterministic_decision,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::self_improvement::{EvaluationCaseV1, EVALUATION_PLAN_SCHEMA};

    fn plan() -> EvaluationPlanV1 {
        EvaluationPlanV1 {
            schema: EVALUATION_PLAN_SCHEMA.to_string(),
            plan_id: "plan-1".to_string(),
            objective_id: "objective-1".to_string(),
            baseline_revisions: vec!["base".to_string()],
            cases: (0..5)
                .map(|index| EvaluationCaseV1 {
                    case_id: format!("case-{index}"),
                    input_evidence_ids: vec![format!("input-{index}")],
                    repository_fingerprint: "repo".to_string(),
                    route: "codex/model".to_string(),
                    grants_digest: "grants".to_string(),
                    acceptance_contract_digest: "acceptance".to_string(),
                    adversarial: false,
                })
                .collect(),
            deterministic_acceptance: vec!["tests".to_string()],
            available_non_author_judges: vec!["mimo/judge".to_string()],
            frozen_at: "now".to_string(),
            digest: String::new(),
        }
    }

    fn results(pass: bool, elapsed: u64) -> Vec<EvaluationCaseResultV1> {
        (0..5)
            .map(|index| EvaluationCaseResultV1 {
                case_id: format!("case-{index}"),
                deterministic_pass: pass,
                acceptance_properties: ["safe".to_string()].into_iter().collect(),
                output_equivalence_digest: format!("output-{index}"),
                elapsed_millis: elapsed,
                estimated_microusd: Some(100),
            })
            .collect()
    }

    #[test]
    fn baseline_pass_regression_is_never_promoted() -> Result<()> {
        let plan = plan();
        let baseline = results(true, 100);
        let mut candidate = results(true, 80);
        candidate[2].deterministic_pass = false;
        let decision = decide_promotion(&plan, &baseline, &candidate, &[], None)?;
        assert!(!decision.promoted);
        assert!(decision.deterministic_decision);
        Ok(())
    }

    #[test]
    fn equivalent_ten_percent_latency_improvement_promotes() -> Result<()> {
        let plan = plan();
        let decision = decide_promotion(&plan, &results(true, 100), &results(true, 90), &[], None)?;
        assert!(decision.promoted);
        Ok(())
    }

    #[test]
    fn unknown_cost_cannot_claim_a_zero_cost_improvement() -> Result<()> {
        let mut baseline = results(true, 100);
        let mut candidate = baseline.clone();
        for result in &mut baseline {
            result.estimated_microusd = None;
        }
        for result in &mut candidate {
            result.estimated_microusd = Some(0);
        }
        let decision = decide_promotion(&plan(), &baseline, &candidate, &[], None)?;
        assert!(!decision.promoted);
        Ok(())
    }

    #[test]
    fn human_correction_overrides_positive_results() -> Result<()> {
        let plan = plan();
        let decision = decide_promotion(
            &plan,
            &results(false, 100),
            &results(true, 80),
            &[],
            Some("corrected"),
        )?;
        assert!(!decision.promoted);
        Ok(())
    }

    #[test]
    fn judge_disagreement_blocks_nondeterministic_tie() -> Result<()> {
        let plan = plan();
        let results = results(true, 100);
        let votes = vec![JudgeVoteV1 {
            judge_route: "mimo/judge".to_string(),
            author_route: false,
            approves: false,
            rationale_hash: "hash".to_string(),
        }];
        let decision = decide_promotion(&plan, &results, &results, &votes, None)?;
        assert!(!decision.promoted);
        assert!(!decision.deterministic_decision);
        Ok(())
    }
}
