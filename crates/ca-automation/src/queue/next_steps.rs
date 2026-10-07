//! NEXT_STEPS.md parsing and rendering.
//!
//! Owns the pure Markdown parser/status contract for human-editable queue docs.
//! Filesystem IO, state mutation, verification execution, and role routing stay in `app`.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::domain::{
    default_discovery_budget, default_task_priority, AutomationLevel, ReviewTier, TaskSpec,
};

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct NextStepsQueueState {
    pub(crate) approved: bool,
    pub(crate) unfinished: bool,
    pub(crate) future_work: bool,
}

pub(crate) fn next_steps_state_from_text(text: &str) -> NextStepsQueueState {
    let mut state = NextStepsQueueState::default();
    let mut heading: Option<String> = None;
    let mut body: Vec<String> = Vec::new();
    let mut archived_sections = false;
    let (active_queue_seen, active_task_ids) = active_queue_task_ids(text);

    for line in text.lines() {
        if next_steps_archive_marker(line) {
            update_next_steps_queue_state(
                &mut state,
                heading.as_deref(),
                &body,
                archived_sections,
                active_queue_seen,
                &active_task_ids,
            );
            heading = None;
            body.clear();
            archived_sections = true;
            continue;
        }
        if is_task_section_heading(line) {
            update_next_steps_queue_state(
                &mut state,
                heading.as_deref(),
                &body,
                archived_sections,
                active_queue_seen,
                &active_task_ids,
            );
            if archived_sections {
                heading = None;
            } else {
                heading = Some(line.trim().to_string());
            }
            body.clear();
        } else if heading.is_some() {
            body.push(line.to_string());
        }
    }

    update_next_steps_queue_state(
        &mut state,
        heading.as_deref(),
        &body,
        archived_sections,
        active_queue_seen,
        &active_task_ids,
    );
    state
}

pub(crate) fn active_queue_task_ids(text: &str) -> (bool, HashSet<String>) {
    let mut seen = false;
    let mut collecting = false;
    let mut ids = HashSet::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if is_approved_queue_label(trimmed) {
            seen = true;
            collecting = true;
            ids.extend(extract_task_ids(trimmed));
            continue;
        }
        if collecting {
            if trimmed.starts_with("## ")
                || trimmed.starts_with("### ")
                || starts_top_level_field(line)
            {
                collecting = false;
                continue;
            }
            ids.extend(extract_task_ids(trimmed));
        }
    }

    (seen, ids)
}

pub(crate) fn is_approved_queue_label(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.starts_with("- approved ")
        && lower.contains("task queue")
        && (lower.contains("worker")
            || lower.contains("deterministic")
            || lower.contains("non-ai")
            || lower.contains("non ai")
            || lower.contains("rust")
            || lower.contains("intern")
            || lower.contains("bottom")
            || lower.contains("slave"))
}

fn extract_task_ids(line: &str) -> Vec<String> {
    line.split(|ch: char| ch.is_whitespace() || ch == '`' || ch == ',' || ch == ':' || ch == ';')
        .filter_map(|part| {
            let token = part.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-');
            if looks_like_task_id_token(token) {
                Some(token.to_string())
            } else {
                None
            }
        })
        .collect()
}

pub(crate) fn next_steps_archive_marker(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    lower.contains("archived continuity evidence")
        || lower.contains("do not reopen approval")
        || lower.contains("does not reopen approval")
        || lower.contains("historical task")
        || lower.contains("archived task")
}

fn update_next_steps_queue_state(
    state: &mut NextStepsQueueState,
    heading: Option<&str>,
    body: &[String],
    archived_sections: bool,
    active_queue_seen: bool,
    active_task_ids: &HashSet<String>,
) {
    if !is_next_steps_task_section(heading, body) {
        return;
    }
    if !archived_sections {
        if let Some(status) = markdown_field_value(body, "Status") {
            let status = clean_markdown_value(&status);
            if is_future_next_steps_status(&status) {
                state.future_work = true;
            }
        }
    }
    if active_queue_seen {
        let section_task_id =
            markdown_field_value(body, "Task ID").unwrap_or_else(|| heading_task_id(heading, ""));
        if !active_task_ids.contains(&section_task_id) {
            return;
        }
    }
    if let Some(status) = markdown_field_value(body, "Status") {
        let status = clean_markdown_value(&status);
        if status.eq_ignore_ascii_case("approved") {
            state.approved = true;
            state.unfinished = true;
        } else if !archived_sections && is_active_unfinished_next_steps_status(&status) {
            state.unfinished = true;
        }
    }
}

fn is_active_unfinished_next_steps_status(status: &str) -> bool {
    let status = status.trim();
    status.eq_ignore_ascii_case("pending")
        || status.eq_ignore_ascii_case("pending approval")
        || status.eq_ignore_ascii_case("ready")
        || status.eq_ignore_ascii_case("in progress")
        || status.eq_ignore_ascii_case("started")
        || status.eq_ignore_ascii_case("active")
}

fn is_future_next_steps_status(status: &str) -> bool {
    let normalized = status.trim().to_ascii_lowercase();
    normalized.starts_with("draft")
        || normalized.starts_with("proposed")
        || normalized.contains("draft handoff")
        || normalized.contains("requires explicit approval")
        || normalized.contains("do not implement")
        || normalized.contains("blocked pending")
}

pub(crate) fn is_next_steps_task_section(heading: Option<&str>, body: &[String]) -> bool {
    if markdown_field_value(body, "Status").is_none() {
        return false;
    }
    if markdown_field_value(body, "Task ID").is_some()
        || heading.and_then(extract_task_id).is_some()
    {
        return true;
    }
    for label in [
        "Allowed files",
        "Forbidden files/actions",
        "Implementation",
        "Exact steps",
        "Verification commands",
        "Tests/checks to run",
        "Definition of done",
        "Success criteria",
    ] {
        if !markdown_list_after(body, label).is_empty() {
            return true;
        }
    }
    for label in ["Title", "Objective", "Task type"] {
        if markdown_field_value(body, label).is_some() {
            return true;
        }
    }
    false
}

pub(crate) fn heading_task_id(heading: Option<&str>, title: &str) -> String {
    if let Some(id) = heading.and_then(extract_task_id) {
        return id;
    }
    let slug = slugify_task_id(title);
    if slug.is_empty() {
        "TASK".to_string()
    } else {
        format!("TASK-{slug}")
    }
}

fn slugify_task_id(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out.chars().take(48).collect()
}

fn heading_title(heading: Option<&str>) -> Option<String> {
    let line = heading?;
    let heading_text = line.trim_start_matches('#').trim();
    if let Some((_, title)) = heading_text.split_once(" - ") {
        Some(title.trim().to_string())
    } else {
        Some(heading_text.to_string())
    }
}

fn heading_contains_task_id_title(heading: Option<&str>) -> Option<String> {
    let line = heading?;
    let heading_text = line.trim_start_matches('#').trim();
    if let Some((id, title)) = heading_text
        .split_once(" - ")
        .or_else(|| heading_text.split_once(": "))
    {
        if looks_like_task_id_token(id.trim()) {
            return Some(title.trim().to_string());
        }
    }
    None
}

fn looks_like_task_id_token(token: &str) -> bool {
    let cleaned = token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-');
    cleaned.contains('-')
        && cleaned.chars().any(|ch| ch.is_ascii_digit())
        && cleaned
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn is_task_section_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("### ")
        || (trimmed.starts_with("## ") && extract_task_id(trimmed).is_some())
}

fn strip_heading_task_prefix(title: &str) -> &str {
    if let Some((id, rest)) = title.split_once(" - ").or_else(|| title.split_once(": ")) {
        if looks_like_task_id_token(id.trim()) {
            return rest.trim();
        }
    }
    title
}

pub(crate) fn parse_approved_next_steps_tasks(text: &str) -> Vec<TaskSpec> {
    parse_next_steps_tasks_with_mode(text, NextStepsTaskParseMode::Approved)
}

pub(crate) fn parse_pending_next_steps_tasks(text: &str) -> Vec<TaskSpec> {
    parse_next_steps_tasks_with_mode(text, NextStepsTaskParseMode::PendingApproval)
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum NextStepsTaskParseMode {
    Approved,
    PendingApproval,
}

fn parse_next_steps_tasks_with_mode(text: &str, mode: NextStepsTaskParseMode) -> Vec<TaskSpec> {
    let mut tasks = Vec::new();
    let mut heading: Option<String> = None;
    let mut body: Vec<String> = Vec::new();
    let mut archived_sections = false;
    let (active_queue_seen, active_task_ids) = active_queue_task_ids(text);
    let outputs = next_steps_outputs(text);
    let restrict_to_active_queue = mode == NextStepsTaskParseMode::Approved && active_queue_seen;

    for line in text.lines() {
        if next_steps_archive_marker(line) {
            if !archived_sections {
                if let Some(task) =
                    parse_next_steps_task_section(heading.as_deref(), &body, &outputs, mode)
                {
                    if !restrict_to_active_queue || active_task_ids.contains(&task.task_id) {
                        tasks.push(task);
                    }
                }
            }
            heading = None;
            body.clear();
            archived_sections = true;
            continue;
        }
        if is_task_section_heading(line) {
            if !archived_sections {
                if let Some(task) =
                    parse_next_steps_task_section(heading.as_deref(), &body, &outputs, mode)
                {
                    if !restrict_to_active_queue || active_task_ids.contains(&task.task_id) {
                        tasks.push(task);
                    }
                }
            }
            if archived_sections {
                heading = None;
            } else {
                heading = Some(line.trim().to_string());
            }
            body.clear();
        } else if heading.is_some() {
            body.push(line.to_string());
        }
    }

    if !archived_sections {
        if let Some(task) = parse_next_steps_task_section(heading.as_deref(), &body, &outputs, mode)
        {
            if !restrict_to_active_queue || active_task_ids.contains(&task.task_id) {
                tasks.push(task);
            }
        }
    }

    tasks
}

pub(crate) type NextStepsOutputMap = HashMap<String, HashMap<String, String>>;

fn parse_next_steps_task_section(
    heading: Option<&str>,
    body: &[String],
    outputs: &NextStepsOutputMap,
    mode: NextStepsTaskParseMode,
) -> Option<TaskSpec> {
    let status = clean_markdown_value(&markdown_field_value(body, "Status")?);
    let selected = match mode {
        NextStepsTaskParseMode::Approved => {
            status.eq_ignore_ascii_case("approved")
                || (status.eq_ignore_ascii_case("conditional approved")
                    && conditional_run_if_satisfied(body, outputs) == Some(true))
        }
        NextStepsTaskParseMode::PendingApproval => {
            status.eq_ignore_ascii_case("pending")
                || status.eq_ignore_ascii_case("pending approval")
                || status.eq_ignore_ascii_case("ready")
        }
    };
    if !selected {
        return None;
    }
    if !is_next_steps_task_section(heading, body) {
        return None;
    }

    let heading_title = heading_contains_task_id_title(heading).or_else(|| heading_title(heading));
    let title = markdown_field_value(body, "Title")
        .or_else(|| heading_title.map(|title| strip_heading_task_prefix(&title).to_string()))
        .unwrap_or_else(|| "Approved NEXT_STEPS task".to_string());
    let task_id =
        markdown_field_value(body, "Task ID").unwrap_or_else(|| heading_task_id(heading, &title));
    let parent_task = markdown_field_value(body, "Parent task")
        .map(|value| clean_markdown_value(&value).trim_matches('`').to_string());
    let priority = markdown_field_value(body, "Priority").unwrap_or_else(default_task_priority);
    let dependencies = parse_task_dependencies(body);
    let objective = markdown_field_value(body, "Objective").unwrap_or_else(|| title.clone());
    let task_type = markdown_field_value(body, "Task type")
        .or_else(|| markdown_field_value(body, "Type"))
        .unwrap_or_default();
    let mut allowed_files = markdown_list_after(body, "Allowed files");
    if allowed_files.is_empty() {
        allowed_files = markdown_list_after(body, "Allowed edit files");
    }
    if allowed_files.is_empty() {
        allowed_files = markdown_list_after(body, "Allowed edits");
    }
    if allowed_files.is_empty() {
        return None;
    }
    let mut read_only_files = markdown_list_after(body, "Read-only files");
    if read_only_files.is_empty() {
        read_only_files = markdown_list_after(body, "Read only files");
    }
    let mut forbidden_actions = markdown_list_after(body, "Forbidden files/actions");
    if forbidden_actions.is_empty() {
        forbidden_actions = markdown_list_after(body, "Forbidden actions");
    }
    let mut exact_steps = markdown_list_after(body, "Implementation");
    if exact_steps.is_empty() {
        exact_steps = markdown_list_after(body, "Exact steps");
    }
    if exact_steps.is_empty() {
        exact_steps.push(format!(
            "Complete the approved `{task_id}` task exactly as described in `NEXT_STEPS.md`."
        ));
    }
    let mut success_criteria = markdown_list_after(body, "Definition of done");
    if success_criteria.is_empty() {
        success_criteria = markdown_list_after(body, "Success criteria");
    }
    if success_criteria.is_empty() {
        success_criteria.push(format!("`{task_id}` is marked `Done.` with evidence."));
    }
    let mut tests_to_run = markdown_list_after(body, "Verification commands");
    if tests_to_run.is_empty() {
        tests_to_run = markdown_list_after(body, "Tests/checks to run");
    }
    if tests_to_run.is_empty() {
        tests_to_run = markdown_list_after(body, "Tests to run");
    }
    let mut stop_conditions = markdown_list_after(body, "Stop conditions");
    if stop_conditions.is_empty() {
        stop_conditions = markdown_list_after(body, "Stop if");
    }
    if stop_conditions.is_empty() {
        stop_conditions.push("Stop if required edits exceed the allowed files or the task needs architectural judgment.".to_string());
    }
    let discovery_budget = markdown_field_value(body, "Discovery budget")
        .or_else(|| markdown_field_value(body, "Discovery"))
        .unwrap_or_else(default_discovery_budget);
    let review_tier = markdown_field_value(body, "Review tier")
        .or_else(|| markdown_field_value(body, "Review"))
        .and_then(|value| parse_review_tier_value(&value))
        .unwrap_or_default();
    let automation_level = markdown_field_value(body, "Automation level")
        .or_else(|| markdown_field_value(body, "Automation"))
        .and_then(|value| parse_automation_level_value(&value))
        .unwrap_or_default();

    Some(TaskSpec {
        task_id,
        parent_task,
        status: "approved".to_string(),
        priority,
        review_tier,
        automation_level,
        dependencies,
        title,
        kind: classify_task_kind(&task_type),
        why_unblocks_progress: format!("Approved in `NEXT_STEPS.md`: {objective}"),
        objective,
        allowed_files,
        read_only_files,
        forbidden_actions,
        exact_steps,
        success_criteria,
        tests_to_run,
        stop_conditions,
        discovery_budget,
        estimated_minutes: 30,
        requires_network: false,
        prefer_worktree: None,
    })
}

fn parse_task_dependencies(body: &[String]) -> Vec<String> {
    let mut dependencies = markdown_list_after(body, "Dependencies");
    if dependencies.is_empty() {
        dependencies = markdown_list_after(body, "Depends on");
    }
    if dependencies.is_empty() {
        if let Some(value) = markdown_field_value(body, "Depends on")
            .or_else(|| markdown_field_value(body, "Dependency"))
            .or_else(|| markdown_field_value(body, "Dependencies"))
        {
            dependencies = extract_task_ids(&value);
            if dependencies.is_empty() {
                let cleaned = clean_markdown_value(&value).trim_matches('`').to_string();
                if !cleaned.is_empty() && !cleaned.eq_ignore_ascii_case("none") {
                    dependencies.push(cleaned);
                }
            }
        }
    }
    dependencies
        .into_iter()
        .flat_map(|dependency| {
            let ids = extract_task_ids(&dependency);
            if ids.is_empty() {
                vec![clean_markdown_value(&dependency)
                    .trim_matches('`')
                    .to_string()]
            } else {
                ids
            }
        })
        .filter(|dependency| !dependency.is_empty() && !dependency.eq_ignore_ascii_case("none"))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn next_steps_completed_task_ids(text: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    let mut heading: Option<String> = None;
    let mut body: Vec<String> = Vec::new();
    let mut archived_sections = false;

    for line in text.lines() {
        if next_steps_archive_marker(line) {
            collect_completed_next_steps_id(heading.as_deref(), &body, &mut ids);
            heading = None;
            body.clear();
            archived_sections = true;
            continue;
        }
        if is_task_section_heading(line) {
            if !archived_sections {
                collect_completed_next_steps_id(heading.as_deref(), &body, &mut ids);
            }
            if archived_sections {
                heading = None;
            } else {
                heading = Some(line.trim().to_string());
            }
            body.clear();
        } else if heading.is_some() {
            body.push(line.to_string());
        }
    }

    if !archived_sections {
        collect_completed_next_steps_id(heading.as_deref(), &body, &mut ids);
    }
    ids
}

fn collect_completed_next_steps_id(
    heading: Option<&str>,
    body: &[String],
    ids: &mut HashSet<String>,
) {
    let Some(status) =
        markdown_field_value(body, "Status").map(|status| clean_markdown_value(&status))
    else {
        return;
    };
    let completed = status.eq_ignore_ascii_case("done")
        || status.eq_ignore_ascii_case("blocked")
        || status.eq_ignore_ascii_case("skipped by gate");
    if !completed {
        return;
    }
    let task_id =
        markdown_field_value(body, "Task ID").unwrap_or_else(|| heading_task_id(heading, ""));
    if !task_id.is_empty() {
        ids.insert(task_id);
    }
}

pub(crate) fn pending_task_dependencies_satisfied(
    task: &TaskSpec,
    completed_ids: &HashSet<String>,
    outputs: &NextStepsOutputMap,
) -> bool {
    task.dependencies
        .iter()
        .all(|dependency| completed_ids.contains(dependency) || outputs.contains_key(dependency))
}

fn parse_review_tier_value(value: &str) -> Option<ReviewTier> {
    match clean_markdown_value(value)
        .trim_matches('`')
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
        .as_str()
    {
        "rust_only" => Some(ReviewTier::RustOnly),
        "auto_accept_if_verified" => Some(ReviewTier::AutoAcceptIfVerified),
        "ai_review_on_failure" => Some(ReviewTier::AiReviewOnFailure),
        "always_ai_review" => Some(ReviewTier::AlwaysAiReview),
        "human_review" => Some(ReviewTier::HumanReview),
        _ => None,
    }
}

fn parse_automation_level_value(value: &str) -> Option<AutomationLevel> {
    match clean_markdown_value(value)
        .trim_matches('`')
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
        .as_str()
    {
        "rust_only" => Some(AutomationLevel::RustOnly),
        "rust_patch" => Some(AutomationLevel::RustPatch),
        "ai_worker_rust_review" => Some(AutomationLevel::AiWorkerRustReview),
        "ai_worker_ai_review" => Some(AutomationLevel::AiWorkerAiReview),
        "planning_only" => Some(AutomationLevel::PlanningOnly),
        _ => None,
    }
}

pub(crate) fn next_steps_outputs(text: &str) -> NextStepsOutputMap {
    let mut outputs: NextStepsOutputMap = HashMap::new();
    let mut heading: Option<String> = None;
    let mut body: Vec<String> = Vec::new();
    let mut archived_sections = false;

    for line in text.lines() {
        if next_steps_archive_marker(line) {
            collect_next_steps_outputs(heading.as_deref(), &body, &mut outputs);
            heading = None;
            body.clear();
            archived_sections = true;
            continue;
        }
        if is_task_section_heading(line) {
            if !archived_sections {
                collect_next_steps_outputs(heading.as_deref(), &body, &mut outputs);
            }
            if archived_sections {
                heading = None;
            } else {
                heading = Some(line.trim().to_string());
            }
            body.clear();
        } else if heading.is_some() {
            body.push(line.to_string());
        }
    }

    if !archived_sections {
        collect_next_steps_outputs(heading.as_deref(), &body, &mut outputs);
    }
    outputs
}

fn collect_next_steps_outputs(
    heading: Option<&str>,
    body: &[String],
    outputs: &mut NextStepsOutputMap,
) {
    if !is_next_steps_output_evidence_section(heading, body) {
        return;
    }
    let task_id =
        markdown_field_value(body, "Task ID").unwrap_or_else(|| heading_task_id(heading, ""));
    if task_id.is_empty() {
        return;
    }
    for line in body {
        for (name, value) in parse_outputs_from_line(line) {
            outputs
                .entry(task_id.clone())
                .or_default()
                .insert(name, value);
        }
    }
}

fn is_next_steps_output_evidence_section(heading: Option<&str>, body: &[String]) -> bool {
    let heading = heading.unwrap_or("").to_ascii_lowercase();
    if heading.contains(" result") || heading.ends_with("result") {
        return true;
    }
    let Some(status) =
        markdown_field_value(body, "Status").map(|status| clean_markdown_value(&status))
    else {
        return false;
    };
    status.eq_ignore_ascii_case("done")
        || status.eq_ignore_ascii_case("blocked")
        || status.eq_ignore_ascii_case("skipped by gate")
        || status.eq_ignore_ascii_case("skipped by gate.")
}

fn parse_outputs_from_line(line: &str) -> Vec<(String, String)> {
    let mut parsed = Vec::new();
    if let Some(idx) = line.find("Outputs:") {
        let after = &line[idx + "Outputs:".len()..];
        for token in after.split(|ch: char| {
            ch.is_whitespace() || ch == ',' || ch == ';' || ch == ')' || ch == '('
        }) {
            let token = clean_output_token(token);
            let Some((name, value)) = token.split_once('=') else {
                continue;
            };
            let name = clean_output_token(name);
            let value = clean_output_token(value);
            if !name.is_empty() && !value.is_empty() {
                parsed.push((name, value));
            }
        }
    }
    parsed
}

pub(crate) fn clean_output_token(value: &str) -> String {
    value
        .trim()
        .trim_matches('`')
        .trim_matches('.')
        .trim_matches(',')
        .trim_matches(';')
        .trim_matches(':')
        .trim()
        .to_string()
}

pub(crate) fn conditional_run_if_satisfied(
    body: &[String],
    outputs: &NextStepsOutputMap,
) -> Option<bool> {
    let conditions = markdown_list_after(body, "Run if");
    if conditions.is_empty() {
        return None;
    }

    let has_or = conditions
        .iter()
        .any(|condition| condition.trim().eq_ignore_ascii_case("OR"));
    let evaluated = conditions
        .iter()
        .filter(|condition| !condition.trim().eq_ignore_ascii_case("OR"))
        .map(|condition| evaluate_run_if_condition(condition, body, outputs))
        .collect::<Vec<_>>();
    if evaluated.is_empty() {
        return None;
    }

    if has_or {
        if evaluated.contains(&Some(true)) {
            Some(true)
        } else if evaluated.iter().any(Option::is_none) {
            None
        } else {
            Some(false)
        }
    } else if evaluated.contains(&Some(false)) {
        Some(false)
    } else if evaluated.iter().any(Option::is_none) {
        None
    } else {
        Some(true)
    }
}

fn evaluate_run_if_condition(
    condition: &str,
    body: &[String],
    outputs: &NextStepsOutputMap,
) -> Option<bool> {
    let condition = clean_markdown_value(condition);
    if let Some(output_condition) = condition.strip_prefix("Outputs:") {
        let output_condition = clean_markdown_value(output_condition);
        let (name, expected) = output_condition.split_once('=')?;
        let name = clean_output_token(name);
        let expected = clean_output_token(expected);
        if name.is_empty() || expected.is_empty() {
            return None;
        }
        let dependencies = parse_task_dependencies(body);
        if dependencies.is_empty() {
            return Some(outputs.values().any(|task_outputs| {
                task_outputs
                    .get(&name)
                    .map(|actual| actual == &expected)
                    .unwrap_or(false)
            }));
        }
        let mut saw_dependency_output = false;
        for dependency in dependencies {
            if let Some(task_outputs) = outputs.get(&dependency) {
                saw_dependency_output = true;
                if task_outputs
                    .get(&name)
                    .map(|actual| actual == &expected)
                    .unwrap_or(false)
                {
                    return Some(true);
                }
            }
        }
        return saw_dependency_output.then_some(false);
    }
    let (left, expected) = condition.split_once("==")?;
    let left = left.trim().trim_matches('`');
    let expected = clean_output_token(expected);
    let (task_id, output_name) = left.split_once(".Outputs.")?;
    let actual = outputs.get(task_id.trim())?.get(output_name.trim())?;
    Some(actual == &expected)
}

pub(crate) fn markdown_field_value(lines: &[String], label: &str) -> Option<String> {
    let bullet_prefix = format!("- {label}:");
    let bare_prefix = format!("{label}:");
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed
            .strip_prefix(&bullet_prefix)
            .or_else(|| trimmed.strip_prefix(&bare_prefix))
        {
            let mut value = rest.trim().to_string();
            for continuation in lines.iter().skip(idx + 1) {
                if is_markdown_section_boundary(continuation)
                    || starts_top_level_field(continuation)
                {
                    break;
                }
                let continuation = continuation.trim();
                if !continuation.is_empty() && !continuation.starts_with("- ") {
                    if !value.is_empty() {
                        value.push(' ');
                    }
                    value.push_str(continuation);
                }
            }
            return Some(clean_markdown_value(&value));
        }
    }
    None
}

pub(crate) fn markdown_list_after(lines: &[String], label: &str) -> Vec<String> {
    let bullet_prefix = format!("- {label}:");
    let bare_prefix = format!("{label}:");
    let Some(start) = lines.iter().position(
        |line| matches!(line.trim(), value if value == bullet_prefix || value == bare_prefix),
    ) else {
        return Vec::new();
    };

    let mut items = Vec::new();
    for line in lines.iter().skip(start + 1) {
        if is_markdown_section_boundary(line) || starts_top_level_field(line) {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            items.push(clean_markdown_value(item));
        } else if let Some(item) = strip_numbered_markdown_item(trimmed) {
            items.push(clean_markdown_value(item));
        } else if let Some(last) = items.last_mut() {
            last.push(' ');
            last.push_str(&clean_markdown_value(trimmed));
        }
    }

    items
}

fn is_markdown_section_boundary(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("## ") || trimmed.starts_with("### ")
}

fn starts_top_level_field(line: &str) -> bool {
    let trimmed = line.trim();
    (line.starts_with("- ") && line.contains(':')) || looks_like_bare_markdown_field(line, trimmed)
}

fn looks_like_bare_markdown_field(line: &str, trimmed: &str) -> bool {
    if line.chars().next().is_some_and(char::is_whitespace) {
        return false;
    }
    let Some((label, _)) = trimmed.split_once(':') else {
        return false;
    };
    let label = label.trim();
    !label.is_empty()
        && label.len() <= 40
        && label
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_' | '/'))
}

pub(crate) fn strip_numbered_markdown_item(line: &str) -> Option<&str> {
    let (number, rest) = line.split_once('.')?;
    if number.chars().all(|ch| ch.is_ascii_digit()) {
        Some(rest.trim())
    } else {
        None
    }
}

fn extract_task_id(line: &str) -> Option<String> {
    line.split(|ch: char| ch.is_whitespace() || ch == '`')
        .find(|part| looks_like_task_id_token(part))
        .map(|part| {
            part.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
                .to_string()
        })
}

pub(crate) fn clean_markdown_value(value: &str) -> String {
    value
        .trim()
        .trim_matches('`')
        .trim_matches('.')
        .trim()
        .to_string()
}

fn classify_task_kind(task_type: &str) -> String {
    let normalized = task_type.to_ascii_lowercase();
    if normalized.contains("implementation") {
        "implementation"
    } else if normalized.contains("documentation") || normalized.contains("docs") {
        "documentation"
    } else if normalized.contains("verification") || normalized.contains("test") {
        "verification"
    } else if normalized.contains("discovery") || normalized.contains("research") {
        "discovery"
    } else if normalized.contains("setup") {
        "setup"
    } else if normalized.contains("repair") {
        "repair"
    } else {
        "implementation"
    }
    .to_string()
}

pub(crate) fn next_steps_task_section_bounds(
    lines: &[String],
    task_id: &str,
) -> Option<(usize, usize)> {
    let mut start = None;
    for (idx, line) in lines.iter().enumerate() {
        if is_task_section_heading(line) && extract_task_id(line).as_deref() == Some(task_id) {
            start = Some(idx);
            break;
        }
    }
    let start = start?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(idx, line)| is_task_section_heading(line).then_some(idx))
        .unwrap_or(lines.len());
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_steps_task_section_bounds_matches_exact_task_id_only() {
        let text = r#"
### TASK-1 - Short
- Status: Approved.

### TASK-10 - Longer
- Status: Approved.

### TASK-100 - Longest
- Status: Done.
"#;
        let lines = text.lines().map(str::to_string).collect::<Vec<_>>();
        let (start, end) = next_steps_task_section_bounds(&lines, "TASK-1").expect("TASK-1");
        assert!(lines[start].contains("TASK-1 - Short"));
        assert_eq!(end, start + 3);
        let (start10, _) = next_steps_task_section_bounds(&lines, "TASK-10").expect("TASK-10");
        assert!(lines[start10].contains("TASK-10 - Longer"));
        assert!(next_steps_task_section_bounds(&lines, "TASK-11").is_none());
    }

    #[test]
    fn next_steps_done_detection_uses_unfinished_task_sections() {
        let approved = r#"
## Active Queue Summary
- Approved Worker task queue:
  - `TASK-200`

### TASK-199 - Completed Thing
- Status: Done.

### TASK-200 - Next Thing
- Status: Approved.
    "#;
        let approved_state = next_steps_state_from_text(approved);
        assert!(approved_state.approved);
        assert!(approved_state.unfinished);

        let done = r#"
## Active Queue Summary
- Approved Worker task queue:
  - None currently.

### TASK-199 - Completed Thing
- Status: Done.
    "#;
        let done_state = next_steps_state_from_text(done);
        assert!(!done_state.approved);
        assert!(!done_state.unfinished);

        let prose_only = r#"
The word Approved appears in prose, but not as a task status.

### Notes
- Status: Approved.
    "#;
        let prose_state = next_steps_state_from_text(prose_only);
        assert!(!prose_state.approved);
        assert!(!prose_state.unfinished);

        let pending = "### TASK-201 - Needs Direction\n- Status: Pending approval.\n";
        let pending_state = next_steps_state_from_text(pending);
        assert!(!pending_state.approved);
        assert!(pending_state.unfinished);

        let blocked = "### TASK-202 - Blocked Historical Work\n- Status: Blocked.\n";
        let blocked_state = next_steps_state_from_text(blocked);
        assert!(!blocked_state.approved);
        assert!(!blocked_state.unfinished);
        assert!(!blocked_state.future_work);

        let draft_future = r#"
## Active Queue Summary
- Approved Worker task queue:
  - None.

### TASK-236 - Bounded Session Data-Phase Observation Session
- Task ID: `TASK-236`
- Status: Draft. Requires explicit approval before implementation.
    "#;
        let draft_future_state = next_steps_state_from_text(draft_future);
        assert!(!draft_future_state.approved);
        assert!(!draft_future_state.unfinished);
        assert!(draft_future_state.future_work);

        let generic = r#"
### WEB-004 - Fix CLI Help
- Task ID: `WEB-004`
- Status: Approved.
- Allowed files:
  - `README.md`
    "#;
        let generic_state = next_steps_state_from_text(generic);
        assert!(generic_state.approved);
        assert!(generic_state.unfinished);
    }

    #[test]
    fn next_steps_queue_state_ignores_archived_task_history() {
        let text = r#"
## Active Queue Summary
- Approved Worker task queue:
  - None. No safe approved execution slice remains.

The detailed task records below are archived continuity evidence. They remain
useful for provenance, but they do not reopen approval.

### TASK-100 - Old Blocker
- Status: Blocked.
- Task ID: `TASK-100`

### TASK-101 - Old Approved Record
- Status: Approved.
- Task ID: `TASK-101`
- Allowed files:
  - `README.md`
    "#;

        let state = next_steps_state_from_text(text);
        assert!(!state.approved);
        assert!(!state.unfinished);
        assert!(parse_approved_next_steps_tasks(text).is_empty());
    }

    #[test]
    fn active_queue_summary_limits_which_task_sections_are_live() {
        let stale_batch = r#"
## Active Queue Summary
- Approved Worker task queue:
  - `TASK-130`, `TASK-131`, and `TASK-132` are approved as one coherent intern batch.

## Next Steps

### TASK-130 - Done One
- Task ID: `TASK-130`
- Status: Done.
- Allowed files:
  - `README.md`

### TASK-131 - Done Two
- Task ID: `TASK-131`
- Status: Done.
- Allowed files:
  - `README.md`

### TASK-132 - Done Three
- Task ID: `TASK-132`
- Status: Done.
- Allowed files:
  - `README.md`

### TASK-025 - Old Pending
- Task ID: `TASK-025`
- Status: Pending approval.
- Allowed files:
  - `README.md`
    "#;

        let state = next_steps_state_from_text(stale_batch);
        assert!(!state.approved);
        assert!(!state.unfinished);
        assert!(parse_approved_next_steps_tasks(stale_batch).is_empty());
    }

    #[test]
    fn helicon_ready_sections_parse_as_pending_from_level_two_headings() {
        let text = r#"
# Next Steps

Small, executable queue items.

## DOC-001: Verify Focused Test Commands In TESTING.md

Status: Ready
Priority: High

Allowed files:
- `TESTING.md`
- `CONTINUITY.md`
- `NEXT_STEPS.md`

Instructions:
1. Run `swift test` once from repo root.

Acceptance criteria:
- `TESTING.md` distinguishes verified commands from inferred commands.

Tests:
- `swift test`

Stop conditions:
- Stop if Swift toolchain is missing.

## PAR-001: Write Current Renderer/Campaign Preflight Packet

Status: Ready
Priority: High

Allowed files:
- `CONTINUITY.md`
- `PARITY.md`
- `Docs/exec-plans/active/*.md`

Instructions:
1. Re-open the current accepted baseline report root.

Tests:
- Manual verification by checking all cited report paths exist.

## GRD-001: Propose Mechanical Check For Renderer Preflight

Status: Proposed
Priority: Medium

Allowed files:
- `Docs/QUALITY.md`

## PAR-002: Plan Diagnostics-Only Second-Pass Merge Replay

Status: Proposed
Priority: Medium

Allowed files:
- `Docs/exec-plans/active/*.md`
"#;

        let state = next_steps_state_from_text(text);
        assert!(!state.approved);
        assert!(state.unfinished);
        assert!(state.future_work);

        assert!(parse_approved_next_steps_tasks(text).is_empty());
        let pending = parse_pending_next_steps_tasks(text);
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].task_id, "DOC-001");
        assert_eq!(
            pending[0].title,
            "Verify Focused Test Commands In TESTING.md"
        );
        assert_eq!(pending[1].task_id, "PAR-001");
        assert!(pending[1]
            .allowed_files
            .contains(&"Docs/exec-plans/active/*.md".to_string()));
    }

    #[test]
    fn non_ai_deterministic_queue_aliases_parse_into_safe_task() {
        let text = r#"
# NEXT_STEPS_non-ai

## Active Queue Summary

- Current phase: verification export.
- Approved deterministic task queue:
  - `TASK-146V`

## Approved Tasks

### TASK-146V - Verify Default-Refusing Local Transport Adapter Seam

- Task ID: `TASK-146V`
- Status: Approved.
- Type: Verification.
- Priority: P1
- Objective: Run deterministic verification commands and update only tracking
  docs with the result.
- Allowed edit files:
  - `NEXT_STEPS_non-ai.md`
  - `NEXT_STEPS.md`
  - `CONTINUITY.md`
  - `TASK.md`
- Forbidden actions:
  - Do not edit source files.
- Exact instructions:
  1. Run the listed verification commands.
  2. Update only the allowed tracking docs.
- Tests to run:
  - `swift test --filter LocalTransportAdapterSeamTests`
  - `swift test --filter AppCoordinatorTests`
  - `swift test --filter ProductionSafetyFlagRegressionTests`
  - `swift build --product app-admin`
  - `git diff --check`
    "#;

        let state = next_steps_state_from_text(text);
        assert!(state.approved);
        assert!(state.unfinished);

        let tasks = parse_approved_next_steps_tasks(text);
        assert_eq!(tasks.len(), 1);
        let task = &tasks[0];
        assert_eq!(task.task_id, "TASK-146V");
        assert_eq!(task.kind, "verification");
        assert_eq!(task.allowed_files.len(), 4);
        assert_eq!(task.tests_to_run.len(), 5);
    }

    #[test]
    fn approved_next_steps_sections_parse_into_worker_tasks() {
        let text = r#"
### TASK-124 - Settings Loader Validates Missing File

- Task ID: `TASK-124`
- Title: Settings Loader Validates Missing File
- Objective: Add the smallest settings-loader validation path for a missing
  file without changing successful load behavior.
- Priority: P1
- Status: Approved.
- Task type: Local implementation and tests.
- Allowed files:
  - `src/settings.rs`
  - `tests/settings_tests.rs`
  - `NEXT_STEPS.md`
- Forbidden files/actions:
  - Do not modify files outside the allowed list.
  - Do not change CLI command surface.
- Implementation:
  1. Add one missing-file validation branch.
  2. Add one focused regression test.
- Verification commands:
  - `cargo test settings_missing_file`
  - `git diff --check`
- Definition of done:
  - Missing settings files produce a clear local error.

### TASK-125 - Later Task
- Task ID: `TASK-125`
- Status: Done.
    "#;

        let tasks = parse_approved_next_steps_tasks(text);
        assert_eq!(tasks.len(), 1);
        let task = &tasks[0];
        assert_eq!(task.task_id, "TASK-124");
        assert_eq!(task.title, "Settings Loader Validates Missing File");
        assert_eq!(task.kind, "implementation");
        assert_eq!(task.allowed_files.len(), 3);
        assert_eq!(task.forbidden_actions.len(), 2);
        assert_eq!(task.exact_steps.len(), 2);
        assert_eq!(task.tests_to_run.len(), 2);
        assert!(task.objective.contains("without changing"));
        assert!(!task.requires_network);
    }

    #[test]
    fn conditional_next_steps_tasks_queue_when_outputs_match() {
        let text = r#"
## Active Queue Summary
- Approved Worker task queue:
  - `TASK-153A`
  - `TASK-153B`

### TASK-152 Result
- Task ID: `TASK-152`
- Status: Done.
- Output: `Outputs: loopback_udp_bind=passed`

### TASK-153A - Success Branch
- Task ID: `TASK-153A`
- Status: Conditional approved.
- Run if:
  - `TASK-152.Outputs.loopback_udp_bind == passed`
- Allowed files:
  - `Sources/Success.swift`
- Implementation:
  1. Do success branch.
- Verification commands:
  - `git diff --check`
- Definition of done:
  - Mark `TASK-153A` Done and record `Outputs: loopback_bind_lifecycle=passed`.

### TASK-153B - Failure Branch
- Task ID: `TASK-153B`
- Status: Conditional approved.
- Run if:
  - `TASK-152.Outputs.loopback_udp_bind == failed`
  - OR
  - `TASK-152.Outputs.loopback_udp_bind == blocked`
- Allowed files:
  - `NEXT_STEPS.md`
- Implementation:
  1. Record failure.
- Verification commands:
  - `git diff --check`

### TASK-154 - Later Success Branch
- Task ID: `TASK-154`
- Status: Conditional approved.
- Run if:
  - `TASK-153A.Outputs.loopback_bind_lifecycle == passed`
- Allowed files:
  - `Sources/Later.swift`
- Implementation:
  1. Do later branch.
- Verification commands:
  - `git diff --check`
    "#;

        let tasks = parse_approved_next_steps_tasks(text);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].task_id, "TASK-153A");

        let state = next_steps_state_from_text(text);
        assert!(!state.approved);
        let paths_state = {
            let mut state = state;
            if !parse_approved_next_steps_tasks(text).is_empty() {
                state.approved = true;
                state.unfinished = true;
            }
            state
        };
        assert!(paths_state.approved);
        assert!(paths_state.unfinished);
    }

    #[test]
    fn conditional_next_steps_tasks_accept_output_shorthand_against_dependencies() {
        let text = r#"
## Active Queue Summary
- Approved Worker task queue:
  - `TASK-190`

### TASK-189 - Controlled Loopback Send Adapter
- Task ID: `TASK-189`
- Status: Done.
- Definition of done:
  - Record exactly: `Outputs: loopback_controlled_send_adapter=present`.

### TASK-190 - Loopback Receive Confirmation
- Task ID: `TASK-190`
- Status: Conditional approved.
- Depends on: `TASK-189`.
- Run if:
  - `Outputs: loopback_controlled_send_adapter=present`
- Allowed files:
  - `Sources/App/LoopbackReceiveConfirmation.swift`
- Forbidden files/actions:
  - Do not open public sockets.
- Implementation:
  1. Add sanitized receive confirmation.
- Verification commands:
  - `swift test --filter LoopbackReceiveConfirmationTests`
- Definition of done:
  - Record exactly: `Outputs: loopback_receive_confirmation=present`.
    "#;

        let tasks = parse_approved_next_steps_tasks(text);

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].task_id, "TASK-190");
        assert_eq!(tasks[0].dependencies, vec!["TASK-189"]);
    }

    #[test]
    fn approved_next_steps_parser_accepts_project_specific_task_ids() {
        let text = r#"
### WEB-004 - Fix CLI Help

- Task ID: `WEB-004`
- Title: Fix CLI Help
- Objective: Tighten the CLI help text without changing behavior.
- Status: Approved.
- Task type: Documentation and CLI polish.
- Allowed files:
  - `README.md`
  - `src/main.rs`
- Forbidden files/actions:
  - Do not change command behavior.
- Exact steps:
  1. Update the help text.
  2. Keep options unchanged.
- Tests/checks to run:
  - `cargo test cli_help`
- Success criteria:
  - Help text is clearer.
    "#;

        let tasks = parse_approved_next_steps_tasks(text);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].task_id, "WEB-004");
        assert_eq!(tasks[0].title, "Fix CLI Help");
        assert_eq!(tasks[0].allowed_files.len(), 2);
        assert_eq!(tasks[0].exact_steps.len(), 2);
        assert_eq!(tasks[0].tests_to_run, vec!["cargo test cli_help"]);
    }

    #[test]
    fn pending_next_steps_parser_reads_depends_on_field() {
        let text = r#"
### TASK-201 - Next Safe Slice
- Task ID: `TASK-201`
- Title: Next Safe Slice
- Objective: Do the next bounded thing.
- Status: Pending approval.
- Depends on: `TASK-200`
- Allowed files:
  - `src/main.rs`
- Forbidden files/actions:
  - Do not touch network behavior.
- Exact steps:
  1. Make the small edit.
- Verification commands:
  - `cargo test`
- Success criteria:
  - Evidence is recorded.
- Stop conditions:
  - Stop if scope expands.
    "#;

        let tasks = parse_pending_next_steps_tasks(text);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].task_id, "TASK-201");
        assert_eq!(tasks[0].dependencies, vec!["TASK-200"]);
        assert_eq!(tasks[0].status, "approved");
    }
}
