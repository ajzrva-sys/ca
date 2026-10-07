use crate::agent::subagent::{AgentId, AgentStatus};
use crate::app::ModelInfo;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub const DEFAULT_CANDIDATES: usize = 2;
pub const MAX_CANDIDATES: usize = 5;
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_SUMMARY_BYTES: usize = 32 * 1024;
const MAX_RATIONALE_BYTES: usize = 16 * 1024;
const MAX_PLAN_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CandidateRoute {
    pub provider: String,
    pub model: String,
}

impl CandidateRoute {
    pub fn label(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }
}

impl From<&ModelInfo> for CandidateRoute {
    fn from(model: &ModelInfo) -> Self {
        Self {
            provider: model.provider.clone(),
            model: model.id.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CandidateId(pub u8);

impl CandidateId {
    pub fn label(self) -> String {
        format!("candidate-{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiRunDefinition {
    pub request: String,
    pub candidates: Vec<CandidateDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateDefinition {
    pub id: CandidateId,
    pub route: CandidateRoute,
    pub worktree_label: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MultiRunError {
    #[error("multi-run requires a non-empty request")]
    EmptyRequest,
    #[error("multi-run request exceeds {MAX_REQUEST_BYTES} bytes")]
    RequestTooLarge,
    #[error("multi-run requires 2-{MAX_CANDIDATES} candidates")]
    CandidateCount,
    #[error("multi-run requires at least one available provider/model route")]
    NoRoutes,
    #[error("unknown multi-run candidate")]
    UnknownCandidate,
    #[error("candidate result is incomplete")]
    IncompleteResult,
    #[error("multi-run structured result is invalid: {0}")]
    InvalidReport(String),
}

impl MultiRunDefinition {
    pub fn build(
        request: impl Into<String>,
        count: usize,
        current: Option<&ModelInfo>,
        available: &[ModelInfo],
    ) -> Result<Self, MultiRunError> {
        let request = request.into();
        if request.trim().is_empty() {
            return Err(MultiRunError::EmptyRequest);
        }
        if request.len() > MAX_REQUEST_BYTES {
            return Err(MultiRunError::RequestTooLarge);
        }
        if !(2..=MAX_CANDIDATES).contains(&count) {
            return Err(MultiRunError::CandidateCount);
        }
        let routes = ordered_routes(current, available);
        if routes.is_empty() {
            return Err(MultiRunError::NoRoutes);
        }
        let candidates = (0..count)
            .map(|index| CandidateDefinition {
                id: CandidateId((index + 1) as u8),
                route: routes[index % routes.len()].clone(),
                worktree_label: format!("multi-run-{}", index + 1),
            })
            .collect();
        Ok(Self {
            request: request.trim().to_string(),
            candidates,
        })
    }

    pub fn set_route(
        &mut self,
        id: CandidateId,
        route: CandidateRoute,
    ) -> Result<(), MultiRunError> {
        let candidate = self
            .candidates
            .iter_mut()
            .find(|candidate| candidate.id == id)
            .ok_or(MultiRunError::UnknownCandidate)?;
        candidate.route = route;
        Ok(())
    }
}

pub fn ordered_routes(current: Option<&ModelInfo>, available: &[ModelInfo]) -> Vec<CandidateRoute> {
    let mut routes = Vec::new();
    let mut seen = HashSet::new();
    if let Some(current) = current {
        let route = CandidateRoute::from(current);
        if seen.insert(route.clone()) {
            routes.push(route);
        }
    }
    let mut remaining = available
        .iter()
        .map(CandidateRoute::from)
        .collect::<Vec<_>>();
    remaining.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| left.model.cmp(&right.model))
    });
    for route in remaining {
        if seen.insert(route.clone()) {
            routes.push(route);
        }
    }
    routes
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiRunStatus {
    Preview,
    CreatingWorktrees,
    Running,
    ReadyToJudge,
    Judging,
    Judged,
    Fusing,
    Complete,
    Cancelled,
    Failed,
}

impl MultiRunStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::CreatingWorktrees => "creating worktrees",
            Self::Running => "running",
            Self::ReadyToJudge => "ready to judge",
            Self::Judging => "judging",
            Self::Judged => "judged",
            Self::Fusing => "creating proposal",
            Self::Complete => "complete",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateStatus {
    Planned,
    WorktreeReady,
    Queued,
    Running,
    WaitingApproval,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

impl CandidateStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::WorktreeReady => "worktree ready",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::WaitingApproval => "waiting approval",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed out",
        }
    }

    pub fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::TimedOut
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateState {
    pub definition: CandidateDefinition,
    pub worktree_id: Option<Uuid>,
    pub agent_id: Option<AgentId>,
    pub status: CandidateStatus,
    pub summary: Option<String>,
    pub successful_tools: Vec<String>,
    pub evidence: Option<CandidateEvidence>,
    pub estimated_cost: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateEvidence {
    pub digest: String,
    pub changed_entries: usize,
    pub omitted_bytes: usize,
}

impl CandidateEvidence {
    pub fn from_bounded_diff(text: &str, changed_entries: usize, omitted_bytes: usize) -> Self {
        let digest = Sha256::digest(text.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Self {
            digest,
            changed_entries,
            omitted_bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiRunJudgment {
    pub winner: CandidateId,
    pub rationale: String,
    pub evidence_digests: HashMap<CandidateId, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FusionProposal {
    pub source: CandidateId,
    pub worktree_id: Uuid,
    pub plan: String,
    pub evidence_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiRunState {
    pub id: Uuid,
    pub status: MultiRunStatus,
    pub request: String,
    pub context_summary: String,
    pub candidates: Vec<CandidateState>,
    pub selected_idx: usize,
    pub error: Option<String>,
    pub judgment: Option<MultiRunJudgment>,
    pub fusion: Option<FusionProposal>,
}

impl MultiRunState {
    pub fn preview(definition: MultiRunDefinition, context_summary: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            status: MultiRunStatus::Preview,
            request: definition.request,
            context_summary: bound_text(context_summary, MAX_SUMMARY_BYTES),
            candidates: definition
                .candidates
                .into_iter()
                .map(|definition| CandidateState {
                    definition,
                    worktree_id: None,
                    agent_id: None,
                    status: CandidateStatus::Planned,
                    summary: None,
                    successful_tools: Vec::new(),
                    evidence: None,
                    estimated_cost: "unknown".into(),
                })
                .collect(),
            selected_idx: 0,
            error: None,
            judgment: None,
            fusion: None,
        }
    }

    pub fn set_worktree(
        &mut self,
        id: CandidateId,
        worktree_id: Uuid,
    ) -> Result<(), MultiRunError> {
        let candidate = self.candidate_mut(id)?;
        candidate.worktree_id = Some(worktree_id);
        candidate.status = CandidateStatus::WorktreeReady;
        Ok(())
    }

    pub fn apply_cost_estimates(&mut self, pricing: &HashMap<String, crate::usage::UsagePricing>) {
        let estimated_input = crate::context::estimate_text_tokens(&format!(
            "{}\n{}",
            self.request, self.context_summary
        ));
        for candidate in &mut self.candidates {
            let key = candidate.definition.route.label();
            candidate.estimated_cost = pricing
                .get(&key)
                .and_then(|pricing| {
                    let prompt_tokens = u32::try_from(estimated_input).ok()?;
                    crate::usage::cost_for(
                        pricing,
                        &crate::agent::stream::UsageInfo {
                            prompt_tokens,
                            completion_tokens: 4_096,
                            total_tokens: prompt_tokens.saturating_add(4_096),
                            cached_prompt_tokens: None,
                            cache_write_tokens: None,
                            reasoning_tokens: None,
                            response_metadata: None,
                            cache_usage_override: None,
                        },
                    )
                })
                .map_or_else(|| "unknown".into(), |cost| format!("~${cost}"));
        }
    }

    pub fn mark_spawned(
        &mut self,
        id: CandidateId,
        agent_id: AgentId,
    ) -> Result<(), MultiRunError> {
        let candidate = self.candidate_mut(id)?;
        candidate.agent_id = Some(agent_id);
        candidate.status = CandidateStatus::Queued;
        Ok(())
    }

    pub fn observe_agent(
        &mut self,
        id: CandidateId,
        snapshot: &crate::agent::subagent::AgentSnapshot,
    ) -> Result<(), MultiRunError> {
        let candidate = self.candidate_mut(id)?;
        if candidate.agent_id != Some(snapshot.id) {
            return Err(MultiRunError::UnknownCandidate);
        }
        candidate.status = match snapshot.status {
            AgentStatus::Queued => CandidateStatus::Queued,
            AgentStatus::Running => CandidateStatus::Running,
            AgentStatus::WaitingApproval => CandidateStatus::WaitingApproval,
            AgentStatus::Completed => CandidateStatus::Completed,
            AgentStatus::Failed | AgentStatus::Interrupted => CandidateStatus::Failed,
            AgentStatus::Cancelled => CandidateStatus::Cancelled,
            AgentStatus::TimedOut => CandidateStatus::TimedOut,
        };
        candidate.summary = snapshot
            .result_summary
            .as_deref()
            .map(|value| bound_text(value.to_string(), MAX_SUMMARY_BYTES));
        candidate.successful_tools = snapshot.successful_tools.clone();
        candidate.successful_tools.sort();
        candidate.successful_tools.dedup();
        if self
            .candidates
            .iter()
            .all(|candidate| candidate.status.terminal())
        {
            self.status = MultiRunStatus::ReadyToJudge;
        } else if self
            .candidates
            .iter()
            .any(|candidate| matches!(candidate.status, CandidateStatus::Running))
        {
            self.status = MultiRunStatus::Running;
        }
        Ok(())
    }

    pub fn set_evidence(
        &mut self,
        id: CandidateId,
        evidence: CandidateEvidence,
    ) -> Result<(), MultiRunError> {
        let candidate = self.candidate_mut(id)?;
        let changed = candidate.evidence.as_ref() != Some(&evidence);
        candidate.evidence = Some(evidence);
        if changed {
            let invalidated_judgment = self.judgment.take().is_some();
            self.fusion = None;
            if invalidated_judgment
                && self
                    .candidates
                    .iter()
                    .all(|candidate| candidate.evidence.is_some())
            {
                self.status = MultiRunStatus::ReadyToJudge;
            }
        }
        Ok(())
    }

    pub fn apply_judgment(&mut self, report: JudgmentReport) -> Result<(), MultiRunError> {
        let winner = CandidateId(report.winner);
        if !self
            .candidates
            .iter()
            .any(|candidate| candidate.definition.id == winner)
        {
            return Err(MultiRunError::InvalidReport("unknown winner".into()));
        }
        let evidence_digests = self
            .candidates
            .iter()
            .map(|candidate| {
                candidate
                    .evidence
                    .as_ref()
                    .map(|evidence| (candidate.definition.id, evidence.digest.clone()))
                    .ok_or(MultiRunError::IncompleteResult)
            })
            .collect::<Result<HashMap<_, _>, _>>()?;
        let rationale = checked_text(report.rationale, MAX_RATIONALE_BYTES, "rationale")?;
        self.judgment = Some(MultiRunJudgment {
            winner,
            rationale,
            evidence_digests,
        });
        self.fusion = None;
        self.status = MultiRunStatus::Judged;
        Ok(())
    }

    pub fn apply_fusion(&mut self, report: FusionReport) -> Result<(), MultiRunError> {
        let source = CandidateId(report.source);
        let candidate = self
            .candidates
            .iter()
            .find(|candidate| candidate.definition.id == source)
            .ok_or_else(|| MultiRunError::InvalidReport("unknown fusion source".into()))?;
        let worktree_id = candidate
            .worktree_id
            .ok_or(MultiRunError::IncompleteResult)?;
        let evidence_digest = candidate
            .evidence
            .as_ref()
            .map(|evidence| evidence.digest.clone())
            .ok_or(MultiRunError::IncompleteResult)?;
        let plan = checked_text(report.plan, MAX_PLAN_BYTES, "fusion plan")?;
        self.fusion = Some(FusionProposal {
            source,
            worktree_id,
            plan,
            evidence_digest,
        });
        self.status = MultiRunStatus::Complete;
        Ok(())
    }

    pub fn judgment_is_fresh(&self) -> bool {
        let Some(judgment) = &self.judgment else {
            return false;
        };
        self.candidates.iter().all(|candidate| {
            candidate.evidence.as_ref().is_some_and(|evidence| {
                judgment.evidence_digests.get(&candidate.definition.id) == Some(&evidence.digest)
            })
        })
    }

    fn candidate_mut(&mut self, id: CandidateId) -> Result<&mut CandidateState, MultiRunError> {
        self.candidates
            .iter_mut()
            .find(|candidate| candidate.definition.id == id)
            .ok_or(MultiRunError::UnknownCandidate)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JudgmentReport {
    pub winner: u8,
    pub rationale: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FusionReport {
    pub source: u8,
    pub plan: String,
}

pub fn parse_judgment(raw: &str) -> Result<JudgmentReport, MultiRunError> {
    serde_json::from_str(raw).map_err(|_| MultiRunError::InvalidReport("malformed JSON".into()))
}

pub fn parse_fusion(raw: &str) -> Result<FusionReport, MultiRunError> {
    serde_json::from_str(raw).map_err(|_| MultiRunError::InvalidReport("malformed JSON".into()))
}

fn checked_text(value: String, max: usize, label: &str) -> Result<String, MultiRunError> {
    let value = crate::terminal_text::sanitize_terminal_text(value.trim());
    if value.is_empty() || value.len() > max {
        return Err(MultiRunError::InvalidReport(format!(
            "{label} must be 1-{max} bytes"
        )));
    }
    Ok(value)
}

fn bound_text(mut value: String, max: usize) -> String {
    if value.len() <= max {
        return value;
    }
    let mut split = max;
    while !value.is_char_boundary(split) {
        split -= 1;
    }
    value.truncate(split);
    value.push_str("\n[older content omitted]");
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(provider: &str, id: &str) -> ModelInfo {
        ModelInfo {
            provider: provider.into(),
            id: id.into(),
            display_name: id.into(),
        }
    }

    #[test]
    fn definition_defaults_current_then_sorted_and_reuses_routes_without_identity_collision() {
        let current = model("z", "current");
        let available = vec![model("b", "two"), model("a", "one"), current.clone()];
        let definition = MultiRunDefinition::build("same task", 5, Some(&current), &available)
            .expect("valid definition");
        let labels = definition
            .candidates
            .iter()
            .map(|candidate| candidate.route.label())
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            ["z/current", "a/one", "b/two", "z/current", "a/one"]
        );
        assert_eq!(
            definition
                .candidates
                .iter()
                .map(|candidate| candidate.worktree_label.as_str())
                .collect::<HashSet<_>>()
                .len(),
            5
        );
    }

    #[test]
    fn definition_rejects_invalid_counts_empty_requests_and_missing_routes() {
        assert_eq!(
            MultiRunDefinition::build("", 2, None, &[model("p", "m")]),
            Err(MultiRunError::EmptyRequest)
        );
        assert_eq!(
            MultiRunDefinition::build("task", 1, None, &[model("p", "m")]),
            Err(MultiRunError::CandidateCount)
        );
        assert_eq!(
            MultiRunDefinition::build("task", 2, None, &[]),
            Err(MultiRunError::NoRoutes)
        );
    }

    #[test]
    fn judgment_and_fusion_accept_only_existing_candidates_with_evidence() {
        let definition = MultiRunDefinition::build("task", 2, None, &[model("p", "m")]).unwrap();
        let mut run = MultiRunState::preview(definition, "context".into());
        for candidate in run.candidates.clone() {
            run.set_worktree(candidate.definition.id, Uuid::new_v4())
                .unwrap();
            run.set_evidence(
                candidate.definition.id,
                CandidateEvidence::from_bounded_diff(&candidate.definition.id.label(), 1, 0),
            )
            .unwrap();
        }
        run.apply_judgment(JudgmentReport {
            winner: 1,
            rationale: "Candidate 1 has the strongest bounded diff evidence".into(),
        })
        .unwrap();
        assert!(run.judgment_is_fresh());
        run.apply_fusion(FusionReport {
            source: 1,
            plan: "Inspect the proposed source worktree, then integrate deliberately.".into(),
        })
        .unwrap();
        assert_eq!(run.status, MultiRunStatus::Complete);
        assert_eq!(run.fusion.unwrap().source, CandidateId(1));
    }

    #[test]
    fn changed_evidence_invalidates_judgment_and_fusion() {
        let definition = MultiRunDefinition::build("task", 2, None, &[model("p", "m")]).unwrap();
        let mut run = MultiRunState::preview(definition, String::new());
        for candidate in run.candidates.clone() {
            run.set_worktree(candidate.definition.id, Uuid::new_v4())
                .unwrap();
            run.set_evidence(
                candidate.definition.id,
                CandidateEvidence::from_bounded_diff("old", 1, 0),
            )
            .unwrap();
        }
        run.apply_judgment(JudgmentReport {
            winner: 1,
            rationale: "bounded reason".into(),
        })
        .unwrap();
        run.set_evidence(
            CandidateId(2),
            CandidateEvidence::from_bounded_diff("new", 1, 0),
        )
        .unwrap();
        assert!(run.judgment.is_none());
        assert_eq!(run.status, MultiRunStatus::ReadyToJudge);
    }

    #[test]
    fn cost_estimates_are_route_specific_and_unknown_without_pricing() {
        let definition = MultiRunDefinition::build(
            "bounded task",
            2,
            None,
            &[model("priced", "model"), model("unknown", "model")],
        )
        .unwrap();
        let mut run = MultiRunState::preview(definition, "frozen completed context".into());
        let pricing = HashMap::from([(
            "priced/model".into(),
            crate::usage::UsagePricing {
                input_per_million_usd: "1".parse().unwrap(),
                output_per_million_usd: "2".parse().unwrap(),
                cached_input_per_million_usd: None,
                cache_write_input_per_million_usd: None,
                reasoning_output_per_million_usd: None,
            },
        )]);
        run.apply_cost_estimates(&pricing);
        assert!(run.candidates[0].estimated_cost.starts_with("~$"));
        assert_ne!(run.candidates[0].estimated_cost, "~$0");
        assert_eq!(run.candidates[1].estimated_cost, "unknown");
    }
}
