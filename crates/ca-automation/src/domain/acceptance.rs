//! Rust-owned acceptance contract derived from task cards.
//!
//! Owns characterization/risky-refactor classification and the pure
//! `AcceptanceContract` built from `TaskSpec` fields. It does not launch Worker
//! or write files; `app` enforces blocking before Worker.
//!
//! Read when changing definition-of-done / characterization requirements. Do not
//! read for queue Markdown or verification spawn.

use serde::{Deserialize, Serialize};

use super::policy::{AutomationLevel, ReviewTier};
use super::task::TaskSpec;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct AcceptanceContract {
    pub(crate) task_id: String,
    pub(crate) definition_of_done: Vec<String>,
    pub(crate) verification_commands: Vec<String>,
    pub(crate) review_tier: ReviewTier,
    pub(crate) automation_level: AutomationLevel,
    pub(crate) allowed_files: Vec<String>,
    pub(crate) read_only_files: Vec<String>,
    pub(crate) stop_conditions: Vec<String>,
    pub(crate) characterization_required: bool,
    pub(crate) characterization_satisfied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) blocker_reason: Option<String>,
}

impl AcceptanceContract {
    pub(crate) fn blocks_worker_launch(&self) -> bool {
        self.characterization_required && !self.characterization_satisfied
    }
}

pub(crate) fn acceptance_contract_from_task(task: &TaskSpec) -> AcceptanceContract {
    let characterization_required = risky_migration_or_refactor_task(task);
    let characterization_satisfied = task_has_characterization_evidence(task);
    let blocker_reason = if characterization_required && !characterization_satisfied {
        Some("missing_characterization_contract".to_string())
    } else {
        None
    };
    AcceptanceContract {
        task_id: task.task_id.clone(),
        definition_of_done: task.success_criteria.clone(),
        verification_commands: task.tests_to_run.clone(),
        review_tier: task.review_tier,
        automation_level: task.automation_level,
        allowed_files: task.allowed_files.clone(),
        read_only_files: task.read_only_files.clone(),
        stop_conditions: task.stop_conditions.clone(),
        characterization_required,
        characterization_satisfied,
        blocker_reason,
    }
}

pub(crate) fn risky_migration_or_refactor_task(task: &TaskSpec) -> bool {
    let text = task_contract_text(task);
    let risky = [
        "migration",
        "migrate",
        "refactor",
        "rewrite",
        "port ",
        "parity",
        "golden",
        "architecture",
        "cross-module",
        "replace ",
    ];
    let source_or_many_files = task.allowed_files.iter().any(|path| {
        let lower = path.to_ascii_lowercase();
        lower.starts_with("src/")
            || lower.starts_with("sources/")
            || lower.starts_with("crates/")
            || lower.ends_with(".rs")
            || lower.ends_with(".swift")
            || lower.ends_with(".ts")
            || lower.ends_with(".tsx")
    }) || task.allowed_files.len() > 6;
    source_or_many_files && risky.iter().any(|needle| text.contains(needle))
}

pub(crate) fn task_has_characterization_evidence(task: &TaskSpec) -> bool {
    let text = task_contract_text(task);
    [
        "characterization",
        "golden",
        "snapshot",
        "approval test",
        "baseline",
        "parity",
        "fixture",
        "known-good",
        "before/after",
    ]
    .iter()
    .any(|needle| text.contains(needle))
        && (!task.tests_to_run.is_empty()
            || task.success_criteria.iter().any(|item| {
                let lower = item.to_ascii_lowercase();
                lower.contains("evidence")
                    || lower.contains("baseline")
                    || lower.contains("snapshot")
                    || lower.contains("golden")
            }))
}

fn task_contract_text(task: &TaskSpec) -> String {
    [
        task.kind.as_str(),
        task.title.as_str(),
        task.objective.as_str(),
        task.why_unblocks_progress.as_str(),
        &task.exact_steps.join("\n"),
        &task.success_criteria.join("\n"),
        &task.tests_to_run.join("\n"),
        &task.stop_conditions.join("\n"),
    ]
    .join("\n")
    .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{default_discovery_budget, AutomationLevel, ReviewTier};

    fn task(objective: &str) -> TaskSpec {
        TaskSpec {
            task_id: "TASK-1".to_string(),
            parent_task: None,
            status: "approved".to_string(),
            priority: "normal".to_string(),
            review_tier: ReviewTier::AiReviewOnFailure,
            automation_level: AutomationLevel::AiWorkerRustReview,
            dependencies: Vec::new(),
            title: objective.to_string(),
            kind: "implementation".to_string(),
            objective: objective.to_string(),
            why_unblocks_progress: "needed".to_string(),
            allowed_files: vec!["src/lib.rs".to_string()],
            read_only_files: Vec::new(),
            forbidden_actions: Vec::new(),
            exact_steps: Vec::new(),
            success_criteria: Vec::new(),
            tests_to_run: Vec::new(),
            stop_conditions: Vec::new(),
            discovery_budget: default_discovery_budget(),
            estimated_minutes: 30,
            requires_network: false,
            prefer_worktree: None,
        }
    }

    #[test]
    fn acceptance_contract_blocks_risky_refactor_without_characterization() {
        let task = task("Refactor parser architecture across modules.");

        let contract = acceptance_contract_from_task(&task);

        assert!(contract.characterization_required);
        assert!(!contract.characterization_satisfied);
        assert!(contract.blocks_worker_launch());
        assert_eq!(
            contract.blocker_reason.as_deref(),
            Some("missing_characterization_contract")
        );
    }

    #[test]
    fn acceptance_contract_allows_risky_refactor_with_characterization() {
        let mut task = task("Refactor parser architecture across modules.");
        task.tests_to_run = vec!["cargo test parser_golden".to_string()];
        task.success_criteria = vec!["Golden baseline evidence passes.".to_string()];

        let contract = acceptance_contract_from_task(&task);

        assert!(contract.characterization_required);
        assert!(contract.characterization_satisfied);
        assert!(!contract.blocks_worker_launch());
    }

    #[test]
    fn acceptance_contract_does_not_block_safe_docs_task() {
        let mut task = task("Update runbook wording.");
        task.allowed_files = vec!["docs/OPERATIONS.md".to_string()];
        task.tests_to_run = vec!["git diff --check".to_string()];

        let contract = acceptance_contract_from_task(&task);

        assert!(!contract.characterization_required);
        assert!(!contract.blocks_worker_launch());
    }
}
