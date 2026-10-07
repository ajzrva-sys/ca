//! Status display, doctor output, and mission summary rendering.
//!
//! High-level presentation helpers that read state and emit formatted terminal
//! output.  They delegate to theme / formatting for the actual drawing.

use crate::app::receipts::DoctorReceipt;
use crate::paths::Paths;
use crate::state::State;
use crate::telemetry::read_model_call_records;

use super::formatting::wrap_terminal_text;
use super::theme::{frame_rule, frame_width, print_frame_row, Theme};

// ---------------------------------------------------------------------------
// Mission status display
// ---------------------------------------------------------------------------

pub(crate) fn print_status(paths: &Paths, state: &State) {
    let theme = Theme::new(true);
    let width = frame_width();
    println!("{}", theme.section("Mission Summary"));
    for (label, value) in mission_summary_rows(paths, state) {
        print_summary_row(&theme, width, &label, &value);
    }
    println!("{}", theme.border(&frame_rule(width)));
}

pub(crate) fn mission_summary_rows(paths: &Paths, state: &State) -> Vec<(String, String)> {
    use crate::queue::next_steps::next_steps_state_from_text;

    let next_steps = next_steps_state_from_text(&crate::app::read_text_or_empty(&paths.next_steps));
    let next_steps_non_ai =
        next_steps_state_from_text(&crate::app::read_text_or_empty(&paths.next_steps_non_ai));
    let derived = crate::app::build_derived_status(paths, state);
    let records = read_model_call_records(paths).unwrap_or_default();
    let cooldowns = crate::app::current_tool_cooldowns(paths, &records);
    let mut rows = vec![
        ("MISSION".to_string(), state.mission.clone()),
        (
            "COMPLETE".to_string(),
            if state.project_complete { "yes" } else { "no" }.to_string(),
        ),
        (
            "DERIVED".to_string(),
            format!("{} - {}", derived.status.as_str(), derived.summary),
        ),
        (
            "TASKS".to_string(),
            format!(
            "completed={} queued={} next_steps_unfinished={} approved={} future_work={}",
            state.completed_tasks,
            state.queue.len(),
            next_steps.unfinished || next_steps_non_ai.unfinished,
            next_steps.approved || next_steps_non_ai.approved,
            next_steps.future_work || next_steps_non_ai.future_work
        ),
        ),
        (
            "FLAGS".to_string(),
            format!(
            "ceo_due={} pm_due={} retry_worker={} senior_repair={} senior_batch={} needs_reconciliation={}",
            state.master_due,
            state.planner_due,
            state.retry_worker_same_task.as_deref().unwrap_or("none"),
            state.senior_dev_repair_order.as_deref().unwrap_or("none"),
            state
                .senior_batch_approval
                .as_ref()
                .map(|batch| batch.batch_id.as_str())
                .unwrap_or("none"),
            state.needs_reconciliation
        ),
        ),
    ];
    let advisory_count = cooldowns
        .iter()
        .filter(|summary| summary.cooldown_advisory)
        .count();
    if advisory_count > 0 {
        rows.push((
            "TOOL HEALTH".to_string(),
            format!("advisory_cooldowns={advisory_count}"),
        ));
    }
    if let Some(pause) = &state.human_disposition_required {
        rows.push((
            "PAUSED".to_string(),
            format!(
                "human_disposition source={} reason={}",
                pause.source, pause.reason
            ),
        ));
    }
    rows.push((
        "LAST COMMIT".to_string(),
        crate::git::git_head_short(paths).unwrap_or_else(|| "unknown".to_string()),
    ));
    if let Some(review) = &state.last_review_summary {
        rows.push(("LAST REVIEW".to_string(), review.clone()));
    }
    rows
}

fn print_summary_row(theme: &Theme, width: usize, label: &str, value: &str) {
    let label_width = 13usize.max(label.chars().count());
    let inner_width = width.saturating_sub(4);
    let value_width = inner_width
        .saturating_sub(label_width)
        .saturating_sub(2)
        .max(24);
    let lines = wrap_terminal_text(value, value_width);
    for (idx, line) in lines.iter().enumerate() {
        let label_text = if idx == 0 {
            format!("{label:<label_width$}")
        } else {
            " ".repeat(label_width)
        };
        print_frame_row(
            theme,
            width,
            &[
                (theme.label(&label_text), label_width),
                (theme.meta(line), line.chars().count()),
            ],
        );
    }
}

// ---------------------------------------------------------------------------
// Doctor receipt display
// ---------------------------------------------------------------------------

pub(crate) fn print_doctor_receipt(receipt: &DoctorReceipt) {
    println!("codex-team doctor");
    println!("repo: {}", receipt.repo);
    println!("initialized: {}", receipt.initialized);
    println!("healthy: {}", receipt.healthy);
    for check in &receipt.checks {
        let mark = if check.ok { "ok" } else { "FAIL" };
        println!("  [{mark}] {}: {}", check.name, check.detail);
    }
}

// ---------------------------------------------------------------------------
// Role tier warnings
// ---------------------------------------------------------------------------

pub(crate) fn report_role_tier_warnings(cfg: &crate::config::Config) {
    let mut warnings = Vec::new();
    if cfg.roles.planner.model == cfg.roles.master.model
        && cfg.roles.planner.effort == cfg.roles.master.effort
    {
        warnings.push(
            "project manager tier matches CEO tier exactly; task compilation may be too expensive"
                .to_string(),
        );
    }
    if role_strength_score(&cfg.roles.planner) > role_strength_score(&cfg.roles.foreman) {
        warnings.push(format!(
            "project manager tier (`{}`/`{}`) is stronger than senior dev (`{}`/`{}`); PM should stay directional while Senior Dev owns technical batch approval",
            cfg.roles.planner.model,
            cfg.roles.planner.effort,
            cfg.roles.foreman.model,
            cfg.roles.foreman.effort
        ));
    }
    if cfg.roles.worker.model == cfg.roles.master.model {
        warnings.push(format!(
            "worker model matches CEO model (`{}`); bounded execution should be cheaper",
            cfg.roles.worker.model
        ));
    }
    if cfg.roles.foreman.model == cfg.roles.master.model
        && cfg.roles.foreman.effort == cfg.roles.master.effort
    {
        warnings.push(
            "senior dev tier matches CEO tier exactly; review/decomposition should usually be cheaper"
                .to_string(),
        );
    }
    if !warnings.is_empty() {
        println!("role tier warnings:");
        for warning in warnings {
            println!("  - {warning}");
        }
    }
}

pub(crate) fn role_strength_score(role: &crate::config::RoleConfig) -> u32 {
    model_strength_score(&role.model) * 10 + effort_strength_score(&role.effort)
}

fn model_strength_score(model: &str) -> u32 {
    let lower = model.to_ascii_lowercase();
    if lower.contains("5.5") {
        55
    } else if lower.contains("5.4") && !lower.contains("mini") {
        54
    } else if lower.contains("5.4") && lower.contains("mini") {
        44
    } else if lower.contains("5.3") {
        43
    } else {
        40
    }
}

fn effort_strength_score(effort: &str) -> u32 {
    match effort.to_ascii_lowercase().as_str() {
        "xhigh" | "extra-high" | "extra_high" => 4,
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

pub(crate) fn report_agent_role_warnings() {
    use std::fs;
    use std::path::PathBuf;

    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let agents_dir = PathBuf::from(home).join(".codex/agents");
    let Ok(entries) = fs::read_dir(&agents_dir) else {
        return;
    };

    let mut warnings = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let label = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown agent role");
        if text
            .lines()
            .any(|line| line.trim_start().starts_with("reasoning_effort"))
        {
            warnings.push(format!(
                "{label}: contains unsupported `reasoning_effort`; Codex emits a malformed role warning"
            ));
        }
        if text.contains("LOW_INTEL_MODEL_HERE") {
            warnings.push(format!(
                "{label}: contains placeholder model `LOW_INTEL_MODEL_HERE`"
            ));
        }
    }

    if !warnings.is_empty() {
        println!("agent role warnings:");
        for warning in warnings {
            println!("  - {warning}");
        }
    }
}
