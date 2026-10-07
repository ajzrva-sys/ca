//! Guard and scope enforcement subsystem.
//!
//! Owns repository snapshots, changed-file detection, glob matching, diff
//! summaries, and guard reports. It enforces whether Worker changes stayed
//! inside task scope; it does not decide role prompts or commits.
//!
//! Read when changing allowed-file policy or diff/risk inputs. Do not read for
//! Codex streaming, queue rendering, or deterministic command execution.

pub mod diff;
pub mod glob;
pub mod report;
pub mod snapshot;

use std::collections::HashMap;

use anyhow::Result;

use crate::config::Config;
use crate::domain::TaskSpec;

pub(crate) use diff::{diff_control_plane, diff_snapshots};
pub(crate) use glob::{compile_globs, expand_allowed_file_patterns, normalize_glob_pattern};
pub(crate) use report::{FileFingerprint, GuardReport};
pub(crate) use snapshot::{fingerprint_file, snapshot_control_plane, snapshot_repo};

pub(crate) fn build_guard_report(
    cfg: &Config,
    task: &TaskSpec,
    before: &HashMap<String, FileFingerprint>,
    after: &HashMap<String, FileFingerprint>,
) -> Result<GuardReport> {
    if !cfg.guard.enforce_worker_allowed_files {
        return Ok(GuardReport {
            enabled: false,
            status: "disabled".to_string(),
            changed_files: Vec::new(),
            allowed_patterns: task.allowed_files.clone(),
            ignored_patterns: cfg.guard.ignored_paths.clone(),
            violations: Vec::new(),
        });
    }

    let changed_files = diff_snapshots(before, after);
    let allowed_patterns = expand_allowed_file_patterns(&task.allowed_files);
    let allowed = compile_globs(&allowed_patterns)?;
    let violations = changed_files
        .iter()
        .filter(|path| !allowed.is_match(path.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    Ok(GuardReport {
        enabled: true,
        status: if violations.is_empty() {
            "ok"
        } else {
            "violation"
        }
        .to_string(),
        changed_files,
        allowed_patterns,
        ignored_patterns: cfg.guard.ignored_paths.clone(),
        violations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(len: u64, modified_ns: u128, content_hash: u64) -> FileFingerprint {
        FileFingerprint {
            len,
            modified_ns,
            content_hash,
        }
    }

    fn snap(entries: &[(&str, FileFingerprint)]) -> HashMap<String, FileFingerprint> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn default_task() -> TaskSpec {
        TaskSpec {
            task_id: "test-1".to_string(),
            parent_task: None,
            status: "approved".to_string(),
            priority: "normal".to_string(),
            review_tier: crate::domain::policy::ReviewTier::default(),
            automation_level: crate::domain::policy::AutomationLevel::default(),
            dependencies: Vec::new(),
            title: "Test task".to_string(),
            kind: "feature".to_string(),
            objective: "test".to_string(),
            why_unblocks_progress: "test".to_string(),
            allowed_files: Vec::new(),
            read_only_files: Vec::new(),
            forbidden_actions: Vec::new(),
            exact_steps: Vec::new(),
            success_criteria: Vec::new(),
            tests_to_run: Vec::new(),
            stop_conditions: Vec::new(),
            discovery_budget: crate::domain::task::default_discovery_budget(),
            estimated_minutes: 30,
            requires_network: false,
            prefer_worktree: None,
        }
    }

    // ── build_guard_report: disabled ───────────────────────────────────

    #[test]
    fn guard_report_disabled_when_config_says_so() {
        let mut cfg = Config::default();
        cfg.guard.enforce_worker_allowed_files = false;

        let task = default_task();
        let before = snap(&[("src/main.rs", fp(100, 1000, 111))]);
        let after = snap(&[("src/main.rs", fp(200, 2000, 999))]);

        let report = build_guard_report(&cfg, &task, &before, &after).unwrap();
        assert!(!report.enabled);
        assert_eq!(report.status, "disabled");
        assert!(report.changed_files.is_empty());
        assert!(report.violations.is_empty());
    }

    #[test]
    fn guard_report_disabled_preserves_allowed_patterns_from_task() {
        let mut cfg = Config::default();
        cfg.guard.enforce_worker_allowed_files = false;

        let mut task = default_task();
        task.allowed_files = vec!["src/**/*.rs".to_string()];

        let before = HashMap::new();
        let after = HashMap::new();

        let report = build_guard_report(&cfg, &task, &before, &after).unwrap();
        assert_eq!(report.allowed_patterns, vec![s("src/**/*.rs")]);
    }

    // ── build_guard_report: ok (within scope) ──────────────────────────

    #[test]
    fn guard_report_ok_when_changes_within_allowed() {
        let cfg = Config::default();

        let mut task = default_task();
        task.allowed_files = vec!["src/**/*.rs".to_string()];

        let before = snap(&[("src/main.rs", fp(100, 1000, 111))]);
        let after = snap(&[
            ("src/main.rs", fp(100, 1000, 111)), // unchanged
            ("src/lib.rs", fp(50, 3000, 222)),   // new, but within allowed
        ]);

        let report = build_guard_report(&cfg, &task, &before, &after).unwrap();
        assert!(report.enabled);
        assert_eq!(report.status, "ok");
        assert!(report.violations.is_empty());
        assert!(report.changed_files.contains(&s("src/lib.rs")));
    }

    #[test]
    fn guard_report_ok_when_no_changes() {
        let cfg = Config::default();

        let mut task = default_task();
        task.allowed_files = vec!["src/**/*.rs".to_string()];

        let before = snap(&[("src/main.rs", fp(100, 1000, 111))]);
        let after = snap(&[("src/main.rs", fp(100, 1000, 111))]);

        let report = build_guard_report(&cfg, &task, &before, &after).unwrap();
        assert_eq!(report.status, "ok");
        assert!(report.changed_files.is_empty());
        assert!(report.violations.is_empty());
    }

    #[test]
    fn guard_report_ok_when_tracking_docs_alias_used() {
        let cfg = Config::default();

        let mut task = default_task();
        task.allowed_files = vec!["tracking docs".to_string()];

        let before = HashMap::new();
        let after = snap(&[("NEXT_STEPS.md", fp(100, 1000, 111))]);

        let report = build_guard_report(&cfg, &task, &before, &after).unwrap();
        assert_eq!(report.status, "ok");
        assert!(report.violations.is_empty());
    }

    // ── build_guard_report: violation ──────────────────────────────────

    #[test]
    fn guard_report_violation_when_changes_outside_allowed() {
        let cfg = Config::default();

        let mut task = default_task();
        task.allowed_files = vec!["src/main.rs".to_string()];

        let before = HashMap::new();
        let after = snap(&[
            ("src/main.rs", fp(100, 1000, 111)),        // allowed
            ("config/secrets.toml", fp(50, 2000, 222)), // NOT allowed
        ]);

        let report = build_guard_report(&cfg, &task, &before, &after).unwrap();
        assert!(report.enabled);
        assert_eq!(report.status, "violation");
        assert!(report.violations.contains(&s("config/secrets.toml")));
        assert!(!report.violations.contains(&s("src/main.rs")));
    }

    #[test]
    fn guard_report_violation_all_changes_outside_scope() {
        let cfg = Config::default();

        let mut task = default_task();
        task.allowed_files = vec!["docs/**".to_string()];

        let before = HashMap::new();
        let after = snap(&[
            ("src/hack.rs", fp(100, 1000, 111)),
            ("etc/passwd", fp(50, 2000, 222)),
        ]);

        let report = build_guard_report(&cfg, &task, &before, &after).unwrap();
        assert_eq!(report.status, "violation");
        assert_eq!(report.violations.len(), 2);
    }

    #[test]
    fn guard_report_enforced_by_default() {
        let cfg = Config::default();
        // GuardConfig::default() sets enforce_worker_allowed_files = true
        assert!(cfg.guard.enforce_worker_allowed_files);

        let mut task = default_task();
        task.allowed_files = vec!["src/**/*.rs".to_string()];

        let before = HashMap::new();
        let after = snap(&[("src/lib.rs", fp(50, 1000, 111))]);

        let report = build_guard_report(&cfg, &task, &before, &after).unwrap();
        assert!(report.enabled);
        assert_eq!(report.status, "ok");
    }

    // helper
    fn s(v: &str) -> String {
        v.to_string()
    }
}
