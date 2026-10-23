use crate::agent::client::ChatBackend;
use crate::changes::ReviewInput;
use crate::config::{CaConfig, ProviderConfig, ReviewRouteConfig};
use crate::review::{
    ReviewBudgetSnapshot, ReviewExecutionBudget, ReviewFinding, ReviewPricing, ReviewRunConfig,
};
use crate::tool_selection::ToolSelectionPolicy;
use crate::usage::{UsageKind, UsageLedger, UsageRecorder};
use crate::workspace::WorkspaceContext;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

const MAX_REVIEW_ERROR_BYTES: usize = 8 * 1024;

#[derive(Clone)]
pub struct ResolvedReviewRoute {
    pub provider: String,
    pub model: String,
    pub provider_config: ProviderConfig,
    pub pricing: Option<ReviewPricing>,
    pub images_supported: bool,
}

impl std::fmt::Debug for ResolvedReviewRoute {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedReviewRoute")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("provider_config", &"[REDACTED]")
            .field("pricing", &self.pricing)
            .field("images_supported", &self.images_supported)
            .finish()
    }
}

impl ResolvedReviewRoute {
    pub fn label(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }
}

#[derive(Debug, Clone)]
pub struct ReviewConvergencePlan {
    pub routes: [ResolvedReviewRoute; 2],
    pub max_rounds: u8,
    pub max_provider_requests: u32,
    pub max_output_bytes: usize,
    pub max_cost_usd: crate::usage::DecimalUsd,
    pub unpriced_acknowledged: bool,
}

impl ReviewConvergencePlan {
    pub fn budget(&self) -> Result<ReviewExecutionBudget, String> {
        ReviewExecutionBudget::new(
            self.max_provider_requests,
            self.max_output_bytes,
            self.max_cost_usd,
        )
    }
}

pub fn resolve_review_convergence(
    config: &CaConfig,
    official: Option<&crate::pricing::OfficialPriceCatalog>,
    acknowledge_unpriced: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<ReviewConvergencePlan, String> {
    let configured = &config.review.convergence;
    if configured.routes.len() != 2 {
        return Err("convergent review requires exactly two configured routes".into());
    }
    if configured.routes[0] == configured.routes[1] {
        return Err("convergent review routes must be distinct provider/model pairs".into());
    }
    if configured.max_rounds == 0 || configured.max_rounds > 3 {
        return Err("convergent review permits between one and three rounds".into());
    }
    let max_cost_usd = configured
        .max_cost_usd
        .ok_or_else(|| "convergent review requires an exact cost ceiling".to_string())?;
    let budget = ReviewExecutionBudget::new(
        configured.max_provider_requests,
        configured.max_output_bytes,
        max_cost_usd,
    )?;
    drop(budget);

    let routes = configured
        .routes
        .iter()
        .map(|route| resolve_route(config, route, official, now))
        .collect::<Result<Vec<_>, _>>()?;
    let routes: [ResolvedReviewRoute; 2] = routes
        .try_into()
        .map_err(|_| "convergent review requires exactly two configured routes".to_string())?;
    let unpriced = routes
        .iter()
        .filter(|route| route.pricing.is_none())
        .count();
    if unpriced > 0 && !acknowledge_unpriced {
        return Err(format!(
            "convergent review has {unpriced} unpriced route(s); explicit one-run acknowledgment is required"
        ));
    }
    Ok(ReviewConvergencePlan {
        routes,
        max_rounds: configured.max_rounds,
        max_provider_requests: configured.max_provider_requests,
        max_output_bytes: configured.max_output_bytes,
        max_cost_usd,
        unpriced_acknowledged: unpriced > 0,
    })
}

fn resolve_route(
    config: &CaConfig,
    route: &ReviewRouteConfig,
    official: Option<&crate::pricing::OfficialPriceCatalog>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<ResolvedReviewRoute, String> {
    if route.provider.is_empty()
        || route.provider.len() > 64
        || !route
            .provider
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("convergent review provider ID is invalid".into());
    }
    if route.model.is_empty()
        || route.model.len() > 256
        || route.model.trim() != route.model
        || route.model.chars().any(char::is_control)
    {
        return Err("convergent review model ID is invalid".into());
    }
    let provider_config = config
        .providers
        .get(&route.provider)
        .cloned()
        .ok_or_else(|| {
            format!(
                "convergent review provider is unavailable: {}",
                route.provider
            )
        })?;
    if !provider_config
        .models
        .iter()
        .any(|model| model == &route.model)
    {
        return Err(format!(
            "convergent review model is unavailable: {}/{}",
            route.provider, route.model
        ));
    }
    let profile = config.effective_model_profile(&route.provider, &route.model);
    if profile.as_ref().and_then(|profile| profile.supports_tools) == Some(false) {
        return Err(format!(
            "convergent review route is incompatible because tools are disabled: {}/{}",
            route.provider, route.model
        ));
    }
    let key = format!("{}/{}", route.provider, route.model);
    let pricing = config
        .pricing
        .get(&key)
        .cloned()
        .map(ReviewPricing::Configured)
        .or_else(|| {
            official
                .and_then(|catalog| catalog.find(&route.provider, &route.model))
                .filter(|card| card.fresh_at(now))
                .filter(|card| {
                    matches!(card.pricing, crate::pricing::CardPricing::MeteredUsd { .. })
                })
                .cloned()
                .map(ReviewPricing::Official)
        });
    let images_supported = profile
        .and_then(|profile| profile.image_input_support())
        .unwrap_or(false);
    Ok(ResolvedReviewRoute {
        provider: route.provider.clone(),
        model: route.model.clone(),
        provider_config,
        pricing,
        images_supported,
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerVerdict {
    Pass,
    Findings,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewerOutcome {
    pub provider: String,
    pub model: String,
    pub verdict: ReviewerVerdict,
    pub summary: String,
    pub findings: Vec<ReviewFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewConvergenceRound {
    pub round: u8,
    pub scope: String,
    pub reviewers: [ReviewerOutcome; 2],
    pub budget: ReviewBudgetSnapshot,
    pub passed: bool,
}

impl ReviewConvergenceRound {
    pub fn render(&self, max_cost_usd: crate::usage::DecimalUsd) -> String {
        let mut output = format!(
            "Convergent review · {} · round {}\n\nBoth reviewers must pass. Cost ceiling: ${max_cost_usd}.\nRequests: {} · output: {} bytes · known cost: ${}",
            self.scope,
            self.round,
            self.budget.provider_requests,
            self.budget.output_bytes,
            self.budget.known_cost_usd,
        );
        if self.budget.unpriced_requests > 0 {
            output.push_str(&format!(
                " · {} unpriced request(s) explicitly acknowledged",
                self.budget.unpriced_requests
            ));
        }
        for reviewer in &self.reviewers {
            output.push_str(&format!(
                "\n\n{} / {} · {}\n{}",
                reviewer.provider,
                reviewer.model,
                match reviewer.verdict {
                    ReviewerVerdict::Pass => "PASS",
                    ReviewerVerdict::Findings => "BLOCKED — findings",
                    ReviewerVerdict::Error => "BLOCKED — reviewer error",
                },
                reviewer.summary
            ));
            for finding in &reviewer.findings {
                output.push_str(&format!(
                    "\n- [{:?}] {} · {}:{}-{}",
                    finding.severity,
                    finding.title,
                    finding.file,
                    finding.line_start,
                    finding.line_end
                ));
            }
        }
        output.push_str(if self.passed {
            "\n\nConvergence: PASS"
        } else {
            "\n\nConvergence: BLOCKED"
        });
        output
    }
}

pub struct ReviewPairRun<'a> {
    pub input: &'a ReviewInput,
    pub project_instructions: &'a str,
    pub workspace: &'a WorkspaceContext,
    pub timeout_secs: u64,
    pub usage_ledger: Arc<Mutex<UsageLedger>>,
    pub tool_policy: &'a ToolSelectionPolicy,
    pub budget: &'a ReviewExecutionBudget,
    pub round: u8,
}

pub async fn run_review_pair<B1, B2>(
    first: &B1,
    second: &B2,
    plan: &ReviewConvergencePlan,
    run: ReviewPairRun<'_>,
) -> ReviewConvergenceRound
where
    B1: ChatBackend + ?Sized,
    B2: ChatBackend + ?Sized,
{
    let first_route = &plan.routes[0];
    let second_route = &plan.routes[1];
    let first_recorder = UsageRecorder::new(
        run.usage_ledger.clone(),
        UsageKind::Review,
        first_route.provider.clone(),
        first_route.model.clone(),
    );
    let second_recorder = UsageRecorder::new(
        run.usage_ledger.clone(),
        UsageKind::Review,
        second_route.provider.clone(),
        second_route.model.clone(),
    );
    let first_run = ReviewRunConfig {
        project_instructions: run.project_instructions,
        workspace: run.workspace,
        timeout_secs: run.timeout_secs,
        usage_recorder: first_recorder,
        tool_policy: run.tool_policy,
        images_supported: first_route.images_supported,
        execution_budget: Some(run.budget),
        pricing: first_route.pricing.as_ref(),
    };
    let second_run = ReviewRunConfig {
        project_instructions: run.project_instructions,
        workspace: run.workspace,
        timeout_secs: run.timeout_secs,
        usage_recorder: second_recorder,
        tool_policy: run.tool_policy,
        images_supported: second_route.images_supported,
        execution_budget: Some(run.budget),
        pricing: second_route.pricing.as_ref(),
    };
    let (first_result, second_result) = tokio::join!(
        crate::review::run_review(
            first,
            (&first_route.provider, &first_route.model),
            run.input,
            first_run,
        ),
        crate::review::run_review(
            second,
            (&second_route.provider, &second_route.model),
            run.input,
            second_run,
        )
    );
    let reviewers = [
        reviewer_outcome(first_route, first_result),
        reviewer_outcome(second_route, second_result),
    ];
    let passed = reviewers
        .iter()
        .all(|reviewer| reviewer.verdict == ReviewerVerdict::Pass);
    ReviewConvergenceRound {
        round: run.round,
        scope: run.input.scope.clone(),
        reviewers,
        budget: run.budget.snapshot(),
        passed,
    }
}

fn reviewer_outcome(
    route: &ResolvedReviewRoute,
    result: Result<crate::review::ReviewResult, String>,
) -> ReviewerOutcome {
    match result {
        Ok(result) => ReviewerOutcome {
            provider: route.provider.clone(),
            model: route.model.clone(),
            verdict: if result.findings.is_empty() {
                ReviewerVerdict::Pass
            } else {
                ReviewerVerdict::Findings
            },
            summary: bounded_safe(&result.summary),
            findings: result.findings,
        },
        Err(error) => ReviewerOutcome {
            provider: route.provider.clone(),
            model: route.model.clone(),
            verdict: ReviewerVerdict::Error,
            summary: bounded_safe(&error),
            findings: Vec::new(),
        },
    }
}

fn bounded_safe(value: &str) -> String {
    let value = crate::terminal_text::sanitize_terminal_text(value);
    if value.len() <= MAX_REVIEW_ERROR_BYTES {
        return value;
    }
    let mut end = MAX_REVIEW_ERROR_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[truncated]", &value[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::{ChatMessage, ChatStreamItem, ToolDef};
    use crate::agent::stream::{FinishReason, ProviderError, ProviderEvent, UsageInfo};
    use crate::config::{ModelProfile, PromptCacheMode, ProviderTimeoutConfig};
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::{mpsc, Barrier};

    struct FakeBackend {
        turns: Mutex<VecDeque<Vec<ChatStreamItem>>>,
        requests: Mutex<Vec<Vec<ChatMessage>>>,
        entered: Arc<AtomicUsize>,
        barrier: Option<Arc<Barrier>>,
    }

    #[async_trait::async_trait]
    impl ChatBackend for FakeBackend {
        async fn stream_chat(
            &self,
            messages: &[ChatMessage],
            _tools: &[ToolDef],
        ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
            self.requests.lock().unwrap().push(messages.to_vec());
            self.entered.fetch_add(1, Ordering::SeqCst);
            if let Some(barrier) = &self.barrier {
                barrier.wait().await;
            }
            let events = self.turns.lock().unwrap().pop_front().unwrap();
            let (tx, rx) = mpsc::unbounded_channel();
            for event in events {
                tx.send(event).unwrap();
            }
            Ok(rx)
        }
    }

    fn report(summary: &str, finding: bool, usage: bool) -> Vec<ChatStreamItem> {
        let findings = if finding {
            vec![json!({
                "severity":"high", "title":"bug", "rationale":"broken",
                "file":"src/lib.rs", "line_start":1, "line_end":1,
                "confidence":99, "suggested_validation":"test it"
            })]
        } else {
            Vec::new()
        };
        let mut events = vec![
            Ok(ProviderEvent::ToolCallStart {
                index: 0,
                id: "report".into(),
                name: "report_review_findings".into(),
            }),
            Ok(ProviderEvent::ToolCallArg {
                index: 0,
                arg: json!({"summary":summary,"findings":findings}).to_string(),
            }),
        ];
        if usage {
            events.push(Ok(ProviderEvent::Usage(UsageInfo {
                prompt_tokens: 100,
                completion_tokens: 10,
                total_tokens: 110,
                cached_prompt_tokens: Some(0),
                cache_write_tokens: Some(0),
                reasoning_tokens: Some(0),
                response_metadata: None,
                cache_usage_override: None,
            })));
        }
        events.extend([
            Ok(ProviderEvent::Finish {
                reason: FinishReason::ToolCalls,
                usage: None,
            }),
            Ok(ProviderEvent::Done),
        ]);
        events
    }

    fn fake(turn: Vec<ChatStreamItem>, barrier: Option<Arc<Barrier>>) -> FakeBackend {
        FakeBackend {
            turns: Mutex::new(VecDeque::from([turn])),
            requests: Mutex::new(Vec::new()),
            entered: Arc::new(AtomicUsize::new(0)),
            barrier,
        }
    }

    fn configured(priced: bool) -> CaConfig {
        let mut config = CaConfig::default();
        for (provider, model, port) in [("first", "review-a", 1), ("second", "review-b", 2)] {
            config.providers.insert(
                provider.into(),
                ProviderConfig {
                    google: None,
                    tool_schema_policy: Default::default(),
                    base_url: format!("http://127.0.0.1:{port}/v1"),
                    api_key: "test".into(),
                    protocol: crate::agent::ProviderProtocol::default(),
                    prompt_cache_mode: PromptCacheMode::Implicit,
                    models: vec![model.into()],
                    timeout: ProviderTimeoutConfig::default(),
                },
            );
            config.model_profiles.insert(
                format!("{provider}/{model}"),
                ModelProfile {
                    supports_tools: Some(true),
                    ..ModelProfile::default()
                },
            );
            if priced {
                config.pricing.insert(
                    format!("{provider}/{model}"),
                    crate::usage::UsagePricing {
                        input_per_million_usd: "1".parse().unwrap(),
                        output_per_million_usd: "2".parse().unwrap(),
                        cached_input_per_million_usd: Some("1".parse().unwrap()),
                        cache_write_input_per_million_usd: None,
                        reasoning_output_per_million_usd: Some("2".parse().unwrap()),
                    },
                );
            }
        }
        config.review.convergence.routes = vec![
            ReviewRouteConfig {
                provider: "first".into(),
                model: "review-a".into(),
            },
            ReviewRouteConfig {
                provider: "second".into(),
                model: "review-b".into(),
            },
        ];
        config
    }

    fn input() -> ReviewInput {
        ReviewInput {
            scope: "working tree".into(),
            diff: "diff --git a/src/lib.rs b/src/lib.rs\n+change".into(),
            included_files: vec!["src/lib.rs".into()],
            omitted_files: Vec::new(),
            untracked_files: Vec::new(),
            custom_instructions: None,
        }
    }

    #[test]
    fn preflight_rejects_missing_duplicate_incompatible_and_unpriced_routes() {
        let now = chrono::Utc::now();
        let mut config = configured(true);
        config.review.convergence.routes.pop();
        assert!(resolve_review_convergence(&config, None, false, now)
            .unwrap_err()
            .contains("exactly two"));

        let mut config = configured(true);
        config.review.convergence.routes[1] = config.review.convergence.routes[0].clone();
        assert!(resolve_review_convergence(&config, None, false, now)
            .unwrap_err()
            .contains("distinct"));

        let mut config = configured(true);
        config
            .model_profiles
            .get_mut("second/review-b")
            .unwrap()
            .supports_tools = Some(false);
        assert!(resolve_review_convergence(&config, None, false, now)
            .unwrap_err()
            .contains("incompatible"));

        let config = configured(false);
        assert!(resolve_review_convergence(&config, None, false, now)
            .unwrap_err()
            .contains("acknowledgment"));
        assert!(resolve_review_convergence(&config, None, true, now).is_ok());
    }

    #[tokio::test]
    async fn reviewer_pair_runs_concurrently_with_identical_objective_and_requires_both_passes() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/lib.rs"), "change\n").unwrap();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let plan =
            resolve_review_convergence(&configured(true), None, false, chrono::Utc::now()).unwrap();
        let budget = plan.budget().unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let first = fake(report("clean one", false, true), Some(barrier.clone()));
        let second = fake(report("clean two", false, true), Some(barrier));
        let ledger = Arc::new(Mutex::new(UsageLedger::default()));
        let outcome = run_review_pair(
            &first,
            &second,
            &plan,
            ReviewPairRun {
                input: &input(),
                project_instructions: "SAME-RUBRIC",
                workspace: &workspace,
                timeout_secs: 5,
                usage_ledger: ledger,
                tool_policy: &ToolSelectionPolicy::default(),
                budget: &budget,
                round: 1,
            },
        )
        .await;
        assert!(outcome.passed);
        assert_eq!(outcome.budget.provider_requests, 2);
        let first_request = serde_json::to_vec(&first.requests.lock().unwrap()[0]).unwrap();
        let second_request = serde_json::to_vec(&second.requests.lock().unwrap()[0]).unwrap();
        assert_eq!(first_request, second_request);
        assert!(String::from_utf8(first_request)
            .unwrap()
            .contains("SAME-RUBRIC"));

        let budget = plan.budget().unwrap();
        let first = fake(report("clean", false, true), None);
        let second = fake(report("found one", true, true), None);
        let outcome = run_review_pair(
            &first,
            &second,
            &plan,
            ReviewPairRun {
                input: &input(),
                project_instructions: "SAME-RUBRIC",
                workspace: &workspace,
                timeout_secs: 5,
                usage_ledger: Arc::new(Mutex::new(UsageLedger::default())),
                tool_policy: &ToolSelectionPolicy::default(),
                budget: &budget,
                round: 1,
            },
        )
        .await;
        assert!(!outcome.passed);
        assert_eq!(outcome.reviewers[0].verdict, ReviewerVerdict::Pass);
        assert_eq!(outcome.reviewers[1].verdict, ReviewerVerdict::Findings);
    }

    #[tokio::test]
    async fn shared_request_output_and_cost_budgets_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let mut config = configured(true);
        config.review.convergence.max_provider_requests = 2;
        config.review.convergence.max_output_bytes = 1024;
        config.review.convergence.max_cost_usd = Some("0.000001".parse().unwrap());
        let plan = resolve_review_convergence(&config, None, false, chrono::Utc::now()).unwrap();
        let budget = plan.budget().unwrap();
        let first = fake(report("clean", false, true), None);
        let second = fake(report("clean", false, true), None);
        let outcome = run_review_pair(
            &first,
            &second,
            &plan,
            ReviewPairRun {
                input: &input(),
                project_instructions: "rubric",
                workspace: &workspace,
                timeout_secs: 5,
                usage_ledger: Arc::new(Mutex::new(UsageLedger::default())),
                tool_policy: &ToolSelectionPolicy::default(),
                budget: &budget,
                round: 1,
            },
        )
        .await;
        assert!(!outcome.passed);
        assert!(outcome
            .reviewers
            .iter()
            .any(|reviewer| reviewer.summary.contains("cost ceiling")));
        assert_eq!(outcome.budget.provider_requests, 2);
    }
}
