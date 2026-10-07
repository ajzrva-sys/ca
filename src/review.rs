use crate::agent::client::{ChatBackend, ChatMessage, FunctionCall, ToolCallMsg};
use crate::agent::registry::{ToolExecutionResult, ToolOrigin, ToolProfile, ToolRegistry};
use crate::agent::stream::AssistantTurnAssembler;
use crate::changes::ReviewInput;
use crate::terminal_text::sanitize_terminal_text;
use crate::workspace::{prepare_tool_invocation, WorkspaceContext};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};

const MAX_REVIEW_TURNS: usize = 8;
const REVIEW_READ_TOOLS: &[&str] = &[
    "read_file",
    "view_image",
    "list_dir",
    "search_files",
    "glob",
    "read_todos",
    "blame",
];

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewSeverity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReviewFinding {
    pub severity: ReviewSeverity,
    pub title: String,
    pub rationale: String,
    pub file: String,
    pub line_start: u32,
    pub line_end: u32,
    #[schemars(range(min = 0, max = 100))]
    pub confidence: u8,
    pub suggested_validation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReportReviewFindingsArgs {
    #[serde(default)]
    pub findings: Vec<ReviewFinding>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewResult {
    pub findings: Vec<ReviewFinding>,
    pub summary: String,
}

pub struct ReviewRunConfig<'a> {
    pub project_instructions: &'a str,
    pub workspace: &'a WorkspaceContext,
    pub timeout_secs: u64,
    pub usage_recorder: crate::usage::UsageRecorder,
    pub tool_policy: &'a crate::tool_selection::ToolSelectionPolicy,
    pub images_supported: bool,
    pub execution_budget: Option<&'a ReviewExecutionBudget>,
    pub pricing: Option<&'a ReviewPricing>,
}

#[derive(Debug, Clone)]
pub enum ReviewPricing {
    Configured(crate::usage::UsagePricing),
    Official(crate::pricing::OfficialPriceCard),
}

impl ReviewPricing {
    fn cost_for(
        &self,
        usage: &crate::agent::stream::UsageInfo,
    ) -> Option<crate::pricing::ExactRational> {
        match self {
            Self::Configured(pricing) => crate::usage::cost_for(pricing, usage)
                .and_then(crate::pricing::ExactRational::from_decimal),
            Self::Official(card) => {
                let now = chrono::Utc::now();
                match crate::pricing::actual_cost_for(card, None, usage, now, now)? {
                    cost @ crate::pricing::ActualCost::IncrementalUsd(_) => cost.upper_bound_usd(),
                    // This review receipt's known_cost_usd is exact; a peak
                    // upper bound cannot be silently presented as paid cost.
                    crate::pricing::ActualCost::UpperBoundIncrementalUsd(_)
                    | crate::pricing::ActualCost::SubscriptionAllocation { .. } => None,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewBudgetSnapshot {
    pub provider_requests: u32,
    pub output_bytes: usize,
    pub known_cost_usd: String,
    pub unpriced_requests: u32,
}

#[derive(Debug)]
struct ReviewBudgetState {
    provider_requests: u32,
    output_bytes: usize,
    known_cost_usd: crate::pricing::ExactRational,
    unpriced_requests: u32,
}

impl Default for ReviewBudgetState {
    fn default() -> Self {
        Self {
            provider_requests: 0,
            output_bytes: 0,
            known_cost_usd: crate::pricing::ExactRational::new(0, 1)
                .expect("one is a nonzero denominator"),
            unpriced_requests: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReviewExecutionBudget {
    state: Arc<Mutex<ReviewBudgetState>>,
    max_provider_requests: u32,
    max_output_bytes: usize,
    max_cost_usd: crate::usage::DecimalUsd,
}

impl ReviewExecutionBudget {
    pub fn new(
        max_provider_requests: u32,
        max_output_bytes: usize,
        max_cost_usd: crate::usage::DecimalUsd,
    ) -> Result<Self, String> {
        if !(2..=48).contains(&max_provider_requests) {
            return Err("review provider-request budget must be between 2 and 48".into());
        }
        if max_output_bytes == 0 || max_output_bytes > 1024 * 1024 {
            return Err("review output budget must be between 1 and 1048576 bytes".into());
        }
        if max_cost_usd == crate::usage::DecimalUsd::ZERO {
            return Err("review cost ceiling must be greater than zero".into());
        }
        Ok(Self {
            state: Arc::new(Mutex::new(ReviewBudgetState::default())),
            max_provider_requests,
            max_output_bytes,
            max_cost_usd,
        })
    }

    fn reserve_request(&self) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.provider_requests >= self.max_provider_requests {
            return Err("review provider-request budget exhausted".into());
        }
        state.provider_requests += 1;
        Ok(())
    }

    fn record_output(&self, bytes: usize) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let next = state
            .output_bytes
            .checked_add(bytes)
            .ok_or_else(|| "review output budget overflowed".to_string())?;
        if next > self.max_output_bytes {
            return Err("review output budget exhausted".into());
        }
        state.output_bytes = next;
        Ok(())
    }

    fn record_usage(
        &self,
        pricing: Option<&ReviewPricing>,
        usage: Option<&crate::agent::stream::UsageInfo>,
    ) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let Some(pricing) = pricing else {
            state.unpriced_requests = state.unpriced_requests.saturating_add(1);
            return Ok(());
        };
        let usage = usage.ok_or_else(|| {
            "priced review response omitted exact token usage; cost ceiling cannot be verified"
                .to_string()
        })?;
        let cost = pricing.cost_for(usage).ok_or_else(|| {
            "priced review response has incomplete usage; cost ceiling cannot be verified"
                .to_string()
        })?;
        let total = state
            .known_cost_usd
            .checked_add(cost)
            .ok_or_else(|| "review cost accounting overflowed".to_string())?;
        let max_cost = crate::pricing::ExactRational::from_decimal(self.max_cost_usd)
            .ok_or_else(|| "review cost ceiling could not be represented exactly".to_string())?;
        if total > max_cost {
            return Err(format!(
                "review cost ceiling exceeded: ${total} > ${}",
                self.max_cost_usd
            ));
        }
        state.known_cost_usd = total;
        Ok(())
    }

    pub fn snapshot(&self) -> ReviewBudgetSnapshot {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        ReviewBudgetSnapshot {
            provider_requests: state.provider_requests,
            output_bytes: state.output_bytes,
            known_cost_usd: state.known_cost_usd.to_string(),
            unpriced_requests: state.unpriced_requests,
        }
    }
}

impl ReviewResult {
    pub fn sanitize(mut self) -> Result<Self, String> {
        self.summary = sanitize_terminal_text(&self.summary);
        for finding in &mut self.findings {
            if finding.line_start == 0 || finding.line_end < finding.line_start {
                return Err(format!("invalid line range for finding: {}", finding.title));
            }
            if finding.confidence > 100 {
                return Err(format!("invalid confidence for finding: {}", finding.title));
            }
            finding.title = sanitize_terminal_text(&finding.title);
            finding.rationale = sanitize_terminal_text(&finding.rationale);
            finding.file = sanitize_terminal_text(&finding.file);
            finding.suggested_validation = sanitize_terminal_text(&finding.suggested_validation);
        }
        Ok(self)
    }

    pub fn render(&self, scope: &str) -> String {
        let mut output = format!("Fresh-context review · {scope}\n\n{}", self.summary);
        if self.findings.is_empty() {
            output.push_str("\n\nNo findings reported.");
        }
        for finding in &self.findings {
            output.push_str(&format!(
                "\n\n[{:?}] {}\n{}:{}-{} · confidence {}%\n{}\nValidate: {}",
                finding.severity,
                finding.title,
                finding.file,
                finding.line_start,
                finding.line_end,
                finding.confidence,
                finding.rationale,
                finding.suggested_validation
            ));
        }
        output
    }
}

pub async fn run_review<B: ChatBackend + ?Sized>(
    backend: &B,
    request_target: (&str, &str),
    input: &ReviewInput,
    run: ReviewRunConfig<'_>,
) -> Result<ReviewResult, String> {
    let (provider, model) = request_target;
    let registry = ToolRegistry::builtins();
    let allowed = REVIEW_READ_TOOLS
        .iter()
        .copied()
        .chain(std::iter::once("report_review_findings"));
    let tools = crate::tool_selection::resolve_internal_tools(
        registry,
        ToolProfile::Review,
        run.tool_policy,
        &crate::tool_selection::ToolSelectionConstraints {
            images_supported: run.images_supported,
            ..crate::tool_selection::ToolSelectionConstraints::default()
        }
        .restricted_to(allowed),
    )
    .map_err(|error| format!("tool selection failed: {error}"))?
    .into_definitions();
    let advertised_tool_names = tools
        .iter()
        .map(|tool| tool.function.name.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut messages = vec![
        ChatMessage {
            role: "system".into(),
            content: crate::image_content::ModelContent::text(
                "You are a fresh-context code reviewer. Use only the supplied project instructions, review input, and read-only tools. Report all results exactly once with report_review_findings. Do not discuss plans, chat history, skills, MCP, or subagents.",
            ),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
        ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text(review_prompt(
                input,
                run.project_instructions,
            )),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
    ];

    for _ in 0..MAX_REVIEW_TURNS {
        if let Some(budget) = run.execution_budget {
            budget.reserve_request()?;
        }
        let mut usage_span = run.usage_recorder.begin();
        let request = async {
            let prepared = crate::request::PreparedModelRequest::new(
                crate::request::RequestKind::Review,
                provider,
                model,
                messages.clone(),
                tools.clone(),
                crate::agent::client::ChatRequestOptions::default(),
            );
            let mut stream = backend
                .stream_prepared(&prepared)
                .await
                .map_err(|error| error.to_string())?;
            let mut assembler = AssistantTurnAssembler::new();
            while let Some(item) = stream.recv().await {
                let event = item.map_err(|error| error.to_string())?;
                if let Some(budget) = run.execution_budget {
                    budget.record_output(provider_event_output_bytes(&event)?)?;
                }
                if let crate::agent::stream::ProviderEvent::Usage(usage) = &event {
                    usage_span.observe(usage);
                }
                assembler.push(&event).map_err(|error| error.to_string())?;
            }
            assembler.finish().map_err(|error| error.to_string())
        }
        .await;
        let turn = match request {
            Ok(turn) => {
                usage_span.complete(turn.usage.clone());
                if let Some(budget) = run.execution_budget {
                    budget.record_usage(run.pricing, turn.usage.as_ref())?;
                }
                turn
            }
            Err(error) => {
                usage_span.fail(None);
                return Err(error);
            }
        };
        if turn.tool_calls.is_empty() {
            return Err("review response did not call report_review_findings".into());
        }
        let report_count = turn
            .tool_calls
            .iter()
            .filter(|call| call.name == "report_review_findings")
            .count();
        if report_count > 1 || (report_count == 1 && turn.tool_calls.len() != 1) {
            return Err("report_review_findings must be the sole final tool call".into());
        }

        let assistant_calls = turn
            .tool_calls
            .iter()
            .map(|call| ToolCallMsg {
                id: call.id.clone(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                },
            })
            .collect::<Vec<_>>();
        messages.push(ChatMessage {
            role: "assistant".into(),
            content: crate::image_content::ModelContent::text(turn.content),
            tool_calls: Some(assistant_calls),
            tool_call_id: None,
            response_items: turn.response_items,
        });

        let mut reported = None;
        for call in turn.tool_calls {
            if !advertised_tool_names.contains(&call.name) {
                return Err(format!(
                    "review provider attempted unavailable tool: {}",
                    call.name
                ));
            }
            let arguments: Value = serde_json::from_str(&call.arguments)
                .map_err(|error| format!("invalid review tool arguments: {error}"))?;
            registry
                .validate(&call.name, &arguments)
                .map_err(|error| error.to_string())?;
            if call.name == "report_review_findings" {
                if reported.is_some() {
                    return Err("review reported findings more than once".into());
                }
                let report: ReportReviewFindingsArgs = serde_json::from_value(arguments)
                    .map_err(|error| format!("invalid review findings: {error}"))?;
                reported = Some(
                    ReviewResult {
                        findings: report.findings,
                        summary: report.summary,
                    }
                    .sanitize()?,
                );
                continue;
            }
            if !REVIEW_READ_TOOLS.contains(&call.name.as_str()) {
                return Err(format!("review tool is not read-only: {}", call.name));
            }
            let prepared =
                prepare_tool_invocation(run.workspace, &call.name, &arguments, ToolOrigin::BuiltIn)
                    .map_err(|error| error.to_string())?;
            let outcome = crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned(
                &call.name,
                &prepared.arguments,
                run.workspace,
                run.timeout_secs,
                crate::sandbox::SandboxMode::PolicyOnly,
                false,
                crate::jobs::JobOwner::process(),
            )
            .await;
            messages.push(tool_result_message(&call.id, &outcome));
            if let Some(image) = outcome.image() {
                messages.push(ChatMessage {
                    role: "user".into(),
                    content: crate::image_content::ModelContent::image_from_tool(
                        &call.id,
                        image.clone(),
                    ),
                    tool_calls: None,
                    tool_call_id: None,
                    response_items: Vec::new(),
                });
            }
        }
        if let Some(report) = reported {
            return Ok(report);
        }
    }
    Err("review exceeded the maximum number of turns".into())
}

fn provider_event_output_bytes(
    event: &crate::agent::stream::ProviderEvent,
) -> Result<usize, String> {
    use crate::agent::stream::ProviderEvent;
    match event {
        ProviderEvent::Text(value) | ProviderEvent::Reasoning(value) => Ok(value.len()),
        ProviderEvent::ResponseItem(item) => serde_json::to_vec(item)
            .map(|bytes| bytes.len())
            .map_err(|_| "review provider state could not be bounded".to_string()),
        ProviderEvent::ToolCallStart { id, name, .. } => id
            .len()
            .checked_add(name.len())
            .ok_or_else(|| "review output size overflowed".to_string()),
        ProviderEvent::ToolCallArg { arg, .. } => Ok(arg.len()),
        ProviderEvent::Finish { .. } | ProviderEvent::Usage(_) | ProviderEvent::Done => Ok(0),
    }
}

fn tool_result_message(id: &str, outcome: &ToolExecutionResult) -> ChatMessage {
    ChatMessage {
        role: "tool".into(),
        content: crate::image_content::ModelContent::text(outcome.model_text()),
        tool_calls: None,
        tool_call_id: Some(id.to_string()),
        response_items: Vec::new(),
    }
}

fn review_prompt(input: &ReviewInput, project_instructions: &str) -> String {
    format!(
        "<project_instructions>\n{}\n</project_instructions>\n<review_scope>{}</review_scope>\n<custom_instructions>{}</custom_instructions>\n<included_files>\n{}\n</included_files>\n<omitted_files>\n{}\n</omitted_files>\n<untracked_names_only>\n{}\n</untracked_names_only>\n<diff>\n{}\n</diff>",
        project_instructions,
        input.scope,
        input.custom_instructions.as_deref().unwrap_or(""),
        input.included_files.join("\n"),
        input.omitted_files.join("\n"),
        input.untracked_files.join("\n"),
        input.diff
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::{ChatStreamItem, ToolDef};
    use crate::agent::stream::{FinishReason, ProviderError, ProviderEvent};
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    type FakeTurn = Result<Vec<ChatStreamItem>, ProviderError>;

    fn usage_recorder() -> crate::usage::UsageRecorder {
        crate::usage::UsageRecorder::new(
            std::sync::Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default())),
            crate::usage::UsageKind::Review,
            "test",
            "test-model",
        )
    }

    struct FakeBackend {
        turns: Mutex<VecDeque<FakeTurn>>,
        requests: Mutex<Vec<(Vec<ChatMessage>, Vec<ToolDef>)>>,
        prepared: Mutex<Vec<(crate::request::RequestKind, String, String)>>,
    }

    #[async_trait::async_trait]
    impl ChatBackend for FakeBackend {
        async fn stream_chat(
            &self,
            messages: &[ChatMessage],
            tools: &[ToolDef],
        ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
            self.requests
                .lock()
                .unwrap()
                .push((messages.to_vec(), tools.to_vec()));
            let turn = self.turns.lock().unwrap().pop_front().unwrap();
            let events = turn?;
            let (tx, rx) = mpsc::unbounded_channel();
            for event in events {
                tx.send(event).unwrap();
            }
            Ok(rx)
        }

        async fn stream_prepared(
            &self,
            request: &crate::request::PreparedModelRequest,
        ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
            self.prepared.lock().unwrap().push((
                request.kind,
                request.provider.clone(),
                request.model.clone(),
            ));
            self.stream_chat(&request.messages, &request.tools).await
        }
    }

    fn backend(turns: Vec<FakeTurn>) -> FakeBackend {
        FakeBackend {
            turns: Mutex::new(turns.into()),
            requests: Mutex::new(Vec::new()),
            prepared: Mutex::new(Vec::new()),
        }
    }

    fn tool_turn(id: &str, name: &str, arguments: Value) -> FakeTurn {
        Ok(vec![
            Ok(ProviderEvent::ToolCallStart {
                index: 0,
                id: id.into(),
                name: name.into(),
            }),
            Ok(ProviderEvent::ToolCallArg {
                index: 0,
                arg: arguments.to_string(),
            }),
            Ok(ProviderEvent::Finish {
                reason: FinishReason::ToolCalls,
                usage: None,
            }),
            Ok(ProviderEvent::Done),
        ])
    }

    fn input() -> ReviewInput {
        ReviewInput {
            scope: "working tree".into(),
            diff: "diff --git a/src/lib.rs b/src/lib.rs\n+bug".into(),
            included_files: vec!["src/lib.rs".into()],
            omitted_files: Vec::new(),
            untracked_files: vec!["secret.txt".into()],
            custom_instructions: None,
        }
    }

    #[tokio::test]
    async fn review_uses_fresh_context_and_structured_report_tool() {
        let root = TempDir::new().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/lib.rs"), "bug\n").unwrap();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let report = serde_json::json!({
            "summary":"one issue",
            "findings":[{
                "severity":"high", "title":"Bug", "rationale":"unsafe behavior",
                "file":"src/lib.rs", "line_start":1, "line_end":1,
                "confidence":95, "suggested_validation":"add a regression test"
            }]
        });
        let fake = backend(vec![tool_turn(
            "report-1",
            "report_review_findings",
            report,
        )]);
        let ledger =
            std::sync::Arc::new(std::sync::Mutex::new(crate::usage::UsageLedger::default()));
        let recorder = crate::usage::UsageRecorder::new(
            ledger.clone(),
            crate::usage::UsageKind::Review,
            "review-provider",
            "review-model",
        );

        let result = run_review(
            &fake,
            ("fake", "model"),
            &input(),
            ReviewRunConfig {
                project_instructions: "PROJECT-ONLY",
                workspace: &workspace,
                timeout_secs: 5,
                usage_recorder: recorder,
                tool_policy: &crate::tool_selection::ToolSelectionPolicy::default(),
                images_supported: false,
                execution_budget: None,
                pricing: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(result.findings.len(), 1);
        assert_eq!(
            fake.prepared.lock().unwrap().as_slice(),
            &[(
                crate::request::RequestKind::Review,
                "fake".into(),
                "model".into()
            )]
        );
        let requests = fake.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0.len(), 2);
        let serialized = serde_json::to_string(&requests[0].0).unwrap();
        assert!(serialized.contains("PROJECT-ONLY"));
        for forbidden in [
            "CHAT-SENTINEL",
            "CHECKPOINT-SENTINEL",
            "PLAN-SENTINEL",
            "MCP-SENTINEL",
        ] {
            assert!(!serialized.contains(forbidden));
        }
        let names = requests[0]
            .1
            .iter()
            .map(|tool| tool.function.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"report_review_findings"));
        assert!(!names.contains(&"run_command"));
        assert!(!names.contains(&"write_file"));
        let records = ledger.lock().unwrap().snapshot().records;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, crate::usage::UsageKind::Review);
        assert_eq!(records[0].outcome, crate::usage::UsageOutcome::Completed);
    }

    #[tokio::test]
    async fn read_tool_result_preserves_call_id_before_reporting() {
        let root = TempDir::new().unwrap();
        std::fs::write(root.path().join("source.rs"), "contents\n").unwrap();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let fake = backend(vec![
            tool_turn(
                "read-1",
                "read_file",
                serde_json::json!({"path":"source.rs"}),
            ),
            tool_turn(
                "report-1",
                "report_review_findings",
                serde_json::json!({"summary":"clean", "findings":[]}),
            ),
        ]);

        let result = run_review(
            &fake,
            ("fake", "model"),
            &input(),
            ReviewRunConfig {
                project_instructions: "",
                workspace: &workspace,
                timeout_secs: 5,
                usage_recorder: usage_recorder(),
                tool_policy: &crate::tool_selection::ToolSelectionPolicy::default(),
                images_supported: false,
                execution_budget: None,
                pricing: None,
            },
        )
        .await
        .unwrap();
        assert!(result.findings.is_empty());
        let requests = fake.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let tool_result = requests[1]
            .0
            .iter()
            .find(|message| message.role == "tool")
            .unwrap();
        assert_eq!(tool_result.tool_call_id.as_deref(), Some("read-1"));
        assert!(tool_result.content.as_str().unwrap().contains("contents"));
    }

    #[tokio::test]
    async fn terminal_text_without_report_fails_protocol() {
        let root = TempDir::new().unwrap();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let fake = backend(vec![Ok(vec![
            Ok(ProviderEvent::Text("looks good".into())),
            Ok(ProviderEvent::Finish {
                reason: FinishReason::Stop,
                usage: None,
            }),
            Ok(ProviderEvent::Done),
        ])]);
        assert!(run_review(
            &fake,
            ("fake", "model"),
            &input(),
            ReviewRunConfig {
                project_instructions: "",
                workspace: &workspace,
                timeout_secs: 5,
                usage_recorder: usage_recorder(),
                tool_policy: &crate::tool_selection::ToolSelectionPolicy::default(),
                images_supported: false,
                execution_budget: None,
                pricing: None,
            },
        )
        .await
        .unwrap_err()
        .contains("report_review_findings"));
    }

    #[tokio::test]
    async fn unknown_run_policy_fails_before_review_provider_traffic() {
        let root = TempDir::new().unwrap();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let fake = backend(Vec::new());
        let error = run_review(
            &fake,
            ("fake", "model"),
            &input(),
            ReviewRunConfig {
                project_instructions: "",
                workspace: &workspace,
                timeout_secs: 5,
                usage_recorder: usage_recorder(),
                tool_policy: &crate::tool_selection::ToolSelectionPolicy::replacing(&[
                    "missing_tool",
                ]),
                images_supported: false,
                execution_budget: None,
                pricing: None,
            },
        )
        .await
        .unwrap_err();

        assert!(error.contains("tool selection failed"));
        assert!(fake.requests.lock().unwrap().is_empty());
        assert!(fake.prepared.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn review_provider_cannot_call_a_read_tool_removed_by_policy() {
        let root = TempDir::new().unwrap();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let fake = backend(vec![tool_turn(
            "read-hidden",
            "read_file",
            serde_json::json!({"path":"source.rs"}),
        )]);
        let error = run_review(
            &fake,
            ("fake", "model"),
            &input(),
            ReviewRunConfig {
                project_instructions: "",
                workspace: &workspace,
                timeout_secs: 5,
                usage_recorder: usage_recorder(),
                tool_policy: &crate::tool_selection::ToolSelectionPolicy::replacing(&[
                    "report_review_findings",
                ]),
                images_supported: false,
                execution_budget: None,
                pricing: None,
            },
        )
        .await
        .unwrap_err();

        assert!(error.contains("unavailable tool: read_file"));
        let requests = fake.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].1.len(), 1);
        assert_eq!(requests[0].1[0].function.name, "report_review_findings");
    }
}
