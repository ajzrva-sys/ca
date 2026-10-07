use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{acceptance_contract_from_task, AcceptanceContract, TaskSpec};
use crate::paths::Paths;
use crate::queue::next_steps::{
    clean_markdown_value, next_steps_state_from_text, parse_approved_next_steps_tasks,
};
use crate::repair::compiler_errors_from_text;

use super::{
    evidence_report_from_value, git_diff_stat_for_changed_files, is_tracking_doc_path,
    markdown_line_has_label, read_text_or_empty, truncate_inline, CommandKind, Config,
    DeterministicCommandResult, DeterministicResultCategory, GuardReport, State,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TaskSupportBrief {
    pub(super) task_id: String,
    pub(super) summary: String,
    pub(super) objective: String,
    pub(super) relevant_files: Vec<String>,
    pub(super) suggested_first_reads: Vec<String>,
    pub(super) allowed_files: Vec<String>,
    pub(super) read_only_files: Vec<String>,
    pub(super) forbidden_actions: Vec<String>,
    pub(super) verification_commands: Vec<String>,
    pub(super) stop_conditions: Vec<String>,
    pub(super) acceptance_contract: AcceptanceContract,
    pub(super) risks: Vec<String>,
    pub(super) warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DeterministicFailureSummary {
    pub(super) category: FailureSummaryCategory,
    pub(super) summary: String,
    pub(super) likely_files_or_tests: Vec<String>,
    pub(super) top_diagnostics: Vec<String>,
    pub(super) failed_tests: Vec<String>,
    pub(super) rejected_commands: Vec<String>,
    pub(super) suggested_route: FailureSummaryRoute,
    pub(super) needs_senior_dev: bool,
    pub(super) warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) enum FailureSummaryCategory {
    EnvironmentBlocker,
    TimeoutFailure,
    NoMatches,
    SchemaFailure,
    SafetyFailure,
    CompileFailure,
    TestFailure,
    Unknown,
}

impl FailureSummaryCategory {
    fn as_str(self) -> &'static str {
        match self {
            Self::EnvironmentBlocker => "environmentBlocker",
            Self::TimeoutFailure => "timeoutFailure",
            Self::NoMatches => "noMatches",
            Self::SchemaFailure => "schemaFailure",
            Self::SafetyFailure => "safetyFailure",
            Self::CompileFailure => "compileFailure",
            Self::TestFailure => "testFailure",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum FailureSummaryRoute {
    RetrySameTask,
    Planner,
    SeniorReview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct QueueSnapshotSummary {
    pub(super) approved_worker_count: usize,
    pub(super) approved_non_ai_count: usize,
    pub(super) active_task_ids: Vec<String>,
    pub(super) done_count: usize,
    pub(super) blocked_count: usize,
    pub(super) pending_count: usize,
    pub(super) next_steps_unfinished: bool,
    pub(super) non_ai_unfinished: bool,
    pub(super) future_work: bool,
    pub(super) stale_project_complete: bool,
    pub(super) deterministic_queue_status: DeterministicQueueStatus,
    pub(super) next_runnable_task: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum DeterministicQueueStatus {
    Empty,
    ApprovedWorkAvailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ReviewAssistPacket {
    pub(super) changed_files_by_kind: BTreeMap<String, Vec<String>>,
    pub(super) guard_status: String,
    pub(super) guard_violations: Vec<String>,
    pub(super) diff_stat: String,
    pub(super) verification_results: Vec<String>,
    pub(super) rejected_commands: Vec<String>,
    pub(super) risk_flags: Vec<String>,
    pub(super) tracking_docs_bookkeeping_only: bool,
}

pub(super) fn build_task_support_brief(
    paths: &Paths,
    _cfg: &Config,
    task: &TaskSpec,
) -> TaskSupportBrief {
    let mut relevant_files = task
        .allowed_files
        .iter()
        .chain(task.read_only_files.iter())
        .cloned()
        .collect::<Vec<_>>();
    relevant_files.sort();
    relevant_files.dedup();
    let mut suggested_first_reads = task.read_only_files.clone();
    suggested_first_reads.extend(task.allowed_files.iter().take(6).cloned());
    suggested_first_reads.sort();
    suggested_first_reads.dedup();
    suggested_first_reads.truncate(8);
    let mut risks = task
        .stop_conditions
        .iter()
        .filter(|condition| {
            let lower = condition.to_ascii_lowercase();
            lower.contains("stop")
                || lower.contains("block")
                || lower.contains("forbidden")
                || lower.contains("outside")
        })
        .take(5)
        .cloned()
        .collect::<Vec<_>>();
    if task.requires_network {
        risks.push(
            "Task declares network access; Worker should confirm this is explicitly approved."
                .to_string(),
        );
    }
    let mut warnings = Vec::new();
    for file in relevant_files
        .iter()
        .filter(|file| support_file_should_exist(file))
    {
        if !paths.root.join(file).exists() {
            warnings.push(format!("Named support file does not exist: {file}"));
        }
    }
    warnings.push("Generated by Rust support layer; no AI Intern role was launched.".to_string());
    TaskSupportBrief {
        task_id: task.task_id.clone(),
        summary: format!(
            "Rust support brief for {}: {}. Use the task card as authority; edit only allowed files, inspect only read-only files, run the listed checks, and stop on declared blockers.",
            task.task_id, task.title
        ),
        objective: task.objective.clone(),
        relevant_files,
        suggested_first_reads,
        allowed_files: task.allowed_files.clone(),
        read_only_files: task.read_only_files.clone(),
        forbidden_actions: task.forbidden_actions.clone(),
        verification_commands: task.tests_to_run.clone(),
        stop_conditions: task.stop_conditions.clone(),
        acceptance_contract: acceptance_contract_from_task(task),
        risks,
        warnings,
    }
}

fn support_file_should_exist(file: &str) -> bool {
    let lower = file.to_ascii_lowercase();
    !file.contains('*')
        && !file.contains('?')
        && !file.contains('[')
        && lower != "tracking docs"
        && lower != "tracking doc"
}

pub(super) fn rust_result_summary(
    task: &TaskSpec,
    result: &DeterministicCommandResult,
) -> DeterministicFailureSummary {
    rust_result_summary_with_rejections(task, result, &[])
}

pub(super) fn rust_result_summary_with_rejections(
    task: &TaskSpec,
    result: &DeterministicCommandResult,
    rejected_commands: &[String],
) -> DeterministicFailureSummary {
    let combined = format!("{}\n{}", result.stdout_tail, result.stderr_tail);
    let category = deterministic_failure_summary_category(result, &combined);
    let likely_files_or_tests = task
        .allowed_files
        .iter()
        .chain(task.read_only_files.iter())
        .take(8)
        .cloned()
        .collect::<Vec<_>>();
    let top_diagnostics = compiler_errors_from_text(&combined)
        .into_iter()
        .map(|diagnostic| {
            let location = match (diagnostic.file, diagnostic.line, diagnostic.column) {
                (Some(file), Some(line), Some(column)) => format!("{file}:{line}:{column}"),
                (Some(file), Some(line), None) => format!("{file}:{line}"),
                (Some(file), None, _) => file,
                _ => "unknown location".to_string(),
            };
            format!("{location}: {}", diagnostic.message)
        })
        .collect::<Vec<_>>();
    let failed_tests = failed_test_names_from_text(&combined);
    let suggested_route = deterministic_failure_route(result, &top_diagnostics);
    DeterministicFailureSummary {
        category,
        summary: format!(
            "Rust classified `{}` as `{}` for {}: {}",
            truncate_inline(&result.command.display(), 120),
            category.as_str(),
            task.task_id,
            result.evidence
        ),
        likely_files_or_tests,
        top_diagnostics,
        failed_tests,
        rejected_commands: rejected_commands.to_vec(),
        suggested_route,
        needs_senior_dev: suggested_route == FailureSummaryRoute::SeniorReview,
        warnings: vec![
            "Generated by Rust support layer; no AI Intern role was launched.".to_string(),
        ],
    }
}

fn deterministic_failure_summary_category(
    result: &DeterministicCommandResult,
    output: &str,
) -> FailureSummaryCategory {
    if result.category == DeterministicResultCategory::KnownEnvironmentBlocker {
        FailureSummaryCategory::EnvironmentBlocker
    } else if result.category == DeterministicResultCategory::TimedOut {
        FailureSummaryCategory::TimeoutFailure
    } else if result.category == DeterministicResultCategory::NoMatches {
        FailureSummaryCategory::NoMatches
    } else if output.to_ascii_lowercase().contains("schema") {
        FailureSummaryCategory::SchemaFailure
    } else if output.to_ascii_lowercase().contains("safety")
        || output.to_ascii_lowercase().contains("policy")
        || output.to_ascii_lowercase().contains("forbidden")
    {
        FailureSummaryCategory::SafetyFailure
    } else if !compiler_errors_from_text(output).is_empty()
        || output.to_ascii_lowercase().contains("compil")
    {
        FailureSummaryCategory::CompileFailure
    } else if matches!(result.command.kind, CommandKind::Test)
        || result.command.display().contains("test")
    {
        FailureSummaryCategory::TestFailure
    } else {
        FailureSummaryCategory::Unknown
    }
}

fn deterministic_failure_route(
    result: &DeterministicCommandResult,
    diagnostics: &[String],
) -> FailureSummaryRoute {
    if result.category == DeterministicResultCategory::KnownEnvironmentBlocker
        || result.category == DeterministicResultCategory::TimedOut
    {
        FailureSummaryRoute::Planner
    } else if !diagnostics.is_empty()
        || matches!(result.command.kind, CommandKind::Test | CommandKind::Build)
    {
        FailureSummaryRoute::RetrySameTask
    } else {
        FailureSummaryRoute::SeniorReview
    }
}

fn failed_test_names_from_text(text: &str) -> Vec<String> {
    let mut names = BTreeSet::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed
            .strip_prefix("Test Case '")
            .and_then(|rest| rest.split('\'').next())
        {
            names.insert(name.to_string());
        }
        if let Some(name) = trimmed
            .split(" --filter ")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
        {
            names.insert(name.trim_matches('\'').trim_matches('"').to_string());
        }
    }
    names.into_iter().take(8).collect()
}

pub(super) fn build_queue_snapshot_summary(paths: &Paths, state: &State) -> QueueSnapshotSummary {
    let next_steps = read_text_or_empty(&paths.next_steps);
    let non_ai = read_text_or_empty(&paths.next_steps_non_ai);
    let next_state = next_steps_state_from_text(&next_steps);
    let non_ai_state = next_steps_state_from_text(&non_ai);
    let approved_worker = parse_approved_next_steps_tasks(&next_steps);
    let approved_non_ai = parse_approved_next_steps_tasks(&non_ai);
    let mut active_task_ids = state
        .queue
        .iter()
        .map(|task| task.task_id.clone())
        .collect::<Vec<_>>();
    if active_task_ids.is_empty() {
        active_task_ids.extend(approved_worker.iter().map(|task| task.task_id.clone()));
    }
    active_task_ids.sort();
    active_task_ids.dedup();
    let next_runnable_task = state
        .queue
        .first()
        .map(|task| task.task_id.clone())
        .or_else(|| approved_worker.first().map(|task| task.task_id.clone()))
        .or_else(|| approved_non_ai.first().map(|task| task.task_id.clone()));
    QueueSnapshotSummary {
        approved_worker_count: approved_worker.len(),
        approved_non_ai_count: approved_non_ai.len(),
        active_task_ids,
        done_count: count_task_statuses(&next_steps, &["done"])
            + count_task_statuses(&non_ai, &["done"]),
        blocked_count: count_task_statuses(&next_steps, &["blocked"])
            + count_task_statuses(&non_ai, &["blocked"]),
        pending_count: count_task_statuses(&next_steps, &["pending approval", "pending"])
            + count_task_statuses(&non_ai, &["pending approval", "pending"]),
        next_steps_unfinished: next_state.unfinished,
        non_ai_unfinished: non_ai_state.unfinished,
        future_work: next_state.future_work || non_ai_state.future_work,
        stale_project_complete: state.project_complete
            && (next_state.unfinished
                || non_ai_state.unfinished
                || next_state.future_work
                || non_ai_state.future_work),
        deterministic_queue_status: if approved_non_ai.is_empty() {
            DeterministicQueueStatus::Empty
        } else {
            DeterministicQueueStatus::ApprovedWorkAvailable
        },
        next_runnable_task,
    }
}

fn count_task_statuses(text: &str, statuses: &[&str]) -> usize {
    let mut count = 0usize;
    let mut in_task = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("### ") {
            in_task = true;
            continue;
        }
        if !in_task || !markdown_line_has_label(trimmed, "Status") {
            continue;
        }
        if let Some((_, status)) = trimmed.split_once(':') {
            let status = clean_markdown_value(status).to_ascii_lowercase();
            if statuses.iter().any(|needle| status.starts_with(needle)) {
                count += 1;
            }
        }
        in_task = false;
    }
    count
}

pub(super) fn build_review_assist_packet(
    paths: &Paths,
    task: &TaskSpec,
    guard_report: &GuardReport,
    rust_verification: &Value,
) -> ReviewAssistPacket {
    let mut changed_files_by_kind: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in &guard_report.changed_files {
        changed_files_by_kind
            .entry(review_assist_file_kind(file).to_string())
            .or_default()
            .push(file.clone());
    }
    for files in changed_files_by_kind.values_mut() {
        files.sort();
        files.dedup();
    }
    let mut verification_results = Vec::new();
    let mut rejected_commands = Vec::new();
    if let Some(report) = evidence_report_from_value(rust_verification) {
        for command in report.commands {
            verification_results.push(format!(
                "{} => {} exit {}",
                command.command_display, command.category, command.exit_code
            ));
        }
        rejected_commands = report.rejected_commands;
    }
    if let Some(commands) = rust_verification.get("commands").and_then(Value::as_array) {
        verification_results.extend(
            commands
                .iter()
                .take(12)
                .map(|command| truncate_inline(&command.to_string(), 180)),
        );
    }
    if let Some(rejected) = rust_verification
        .get("rejected_commands")
        .and_then(Value::as_array)
    {
        rejected_commands.extend(
            rejected
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string),
        );
    }
    rejected_commands.sort();
    rejected_commands.dedup();
    let risk_flags = task
        .forbidden_actions
        .iter()
        .chain(task.stop_conditions.iter())
        .filter(|item| {
            let lower = item.to_ascii_lowercase();
            lower.contains("forbid")
                || lower.contains("outside")
                || lower.contains("public")
                || lower.contains("network")
                || lower.contains("security")
                || lower.contains("secret")
                || lower.contains("unsafe")
        })
        .take(12)
        .cloned()
        .collect::<Vec<_>>();
    ReviewAssistPacket {
        changed_files_by_kind,
        guard_status: guard_report.status.clone(),
        guard_violations: guard_report.violations.clone(),
        diff_stat: git_diff_stat_for_changed_files(paths, &guard_report.changed_files),
        verification_results,
        rejected_commands,
        risk_flags,
        tracking_docs_bookkeeping_only: !guard_report.changed_files.is_empty()
            && guard_report
                .changed_files
                .iter()
                .all(|file| is_tracking_doc_path(file)),
    }
}

fn review_assist_file_kind(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("sources/") || lower.starts_with("src/") {
        "source"
    } else if lower.starts_with("tests/") || lower.starts_with("test/") {
        "test"
    } else if is_tracking_doc_path(path) || lower.starts_with("docs/") || lower.ends_with(".md") {
        "docs"
    } else if lower == "cargo.toml"
        || lower == "cargo.lock"
        || lower == "package.swift"
        || lower == "package.resolved"
        || lower == "package.json"
        || lower == "pnpm-lock.yaml"
        || lower == "yarn.lock"
    {
        "config"
    } else {
        "other"
    }
}
