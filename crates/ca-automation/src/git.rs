//! Git adapter helpers.
//!
//! Owns repository-root discovery, git status/diff queries, observation
//! parsing, staging, and commit execution for the orchestrator.  Higher-level
//! cycle routing and acceptance still live in `app`; this module focuses on
//! git porcelain interactions and their typed wrappers.
//!
//! Read when changing how the orchestrator detects a Git worktree, stages
//! files, or records observations.  Do not read for task schema defaults,
//! role prompts, or verification allowlists.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use globset::GlobSet;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::app::{
    append_jsonl, clean_progress_action_id, is_deterministic_command_structured_allowed,
    is_tracking_doc_path, machine_json_mode, parse_verification_command, persist_evidence_report,
    push_event, read_json_file, read_text_or_empty,
    rebuild_file_affinity_from_observation_journals, record_route_attempt, record_rust_model_skip,
    reject_disallowed_swiftstacker_report_roots, route_attempt_id, stable_hash_hex,
    swift_test_filter_from_path, truncate_inline, update_file_affinity_index, update_history_index,
    update_verification_hint_index, with_stable_command_metadata, write_text, RustCommitRequest,
};
use crate::config::{CommitMode, Config};
use crate::domain::{
    AutomationLevel, ExecutionLane, ReviewTier, RouteAttempt, RouteFeedback, RouteLifecycleStatus,
    TaskSpec, TokenUsage,
};
use crate::guard::report::GuardReport;
use crate::guard::{compile_globs, expand_allowed_file_patterns};
use crate::paths::Paths;
use crate::state::{PendingCommit, PendingDeferredFileWrite, PendingPathFingerprint, State};
use crate::telemetry::{append_event, sanitize_filename, OrchestratorEventKind};
use crate::verification::recipe::VerificationCommand;
use crate::verification::{EvidenceReport, VerificationPlan};

// Under `--json`, redirect human progress to stderr so stdout stays a single
// machine receipt object (Hermes / unattended supervisors parse stdout whole).
macro_rules! println {
    () => {{
        if machine_json_mode() {
            ::std::eprintln!()
        } else {
            ::std::println!()
        }
    }};
    ($($arg:tt)*) => {{
        if machine_json_mode() {
            ::std::eprintln!($($arg)*)
        } else {
            ::std::println!($($arg)*)
        }
    }};
}

// ===========================================================================
// Observation types
// ===========================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct ObservedChangedFile {
    pub(crate) path: String,
    pub(crate) status: String,
    pub(crate) category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GitHistoryObservation {
    pub(crate) observed_at: String,
    pub(crate) commit: String,
    pub(crate) subject: String,
    pub(crate) changed_files: Vec<ObservedChangedFile>,
    pub(crate) verification_hints: Vec<String>,
}

// ===========================================================================
// Root discovery
// ===========================================================================

pub(crate) fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut cursor = Some(start);
    while let Some(path) = cursor {
        if path.join(".git").exists() {
            return Some(path.to_path_buf());
        }
        cursor = path.parent();
    }
    None
}

// ===========================================================================
// Query utilities
// ===========================================================================

pub(crate) fn git_dirty_name_only(paths: &Paths) -> Result<Vec<String>> {
    if find_git_root(&paths.root).is_none() {
        return Ok(Vec::new());
    }
    let output = Command::new("git")
        .args(["diff", "--name-only", "HEAD", "--"])
        .current_dir(&paths.root)
        .output()
        .with_context(|| "run git diff --name-only HEAD")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

pub(crate) fn git_untracked_name_only(paths: &Paths) -> Result<Vec<String>> {
    if find_git_root(&paths.root).is_none() {
        return Ok(Vec::new());
    }
    let output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(&paths.root)
        .output()
        .with_context(|| "run git ls-files --others --exclude-standard")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

pub(crate) fn git_name_only(paths: &Paths) -> Option<Vec<String>> {
    let output = Command::new("git")
        .args(["diff", "--name-only"])
        .current_dir(&paths.root)
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect()
    })
}

#[allow(dead_code)]
pub(crate) fn git_head(paths: &Paths) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&paths.root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(crate) fn git_head_short(paths: &Paths) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(&paths.root)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if head.is_empty() {
        None
    } else {
        Some(head)
    }
}

pub(crate) fn git_tracked_file_set(paths: &Paths) -> Result<HashSet<String>> {
    let output = Command::new("git")
        .args(["ls-files"])
        .current_dir(&paths.root)
        .output()
        .context("git ls-files")?;
    if !output.status.success() {
        return Ok(HashSet::new());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

pub(crate) fn git_diff_stat_for_changed_files(paths: &Paths, changed_files: &[String]) -> String {
    if find_git_root(&paths.root).is_none() {
        return "git unavailable".to_string();
    }
    let mut args = vec!["diff".to_string(), "--stat".to_string(), "--".to_string()];
    args.extend(changed_files.iter().cloned());
    let output = Command::new("git")
        .args(&args)
        .current_dir(&paths.root)
        .output();
    let stat = output
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default();
    if !stat.is_empty() {
        return truncate_inline(&stat.replace('\n', "; "), 1_000);
    }
    if changed_files.is_empty() {
        "no changed files recorded".to_string()
    } else {
        format!(
            "{} changed file(s): {}",
            changed_files.len(),
            truncate_inline(&changed_files.join(", "), 900)
        )
    }
}

pub(crate) fn planner_git_status(paths: &Paths) -> String {
    let output = Command::new("git")
        .arg("status")
        .arg("--short")
        .current_dir(&paths.root)
        .output();
    let Ok(output) = output else {
        return "unavailable".to_string();
    };
    if !output.status.success() {
        return "unavailable".to_string();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let lines = text.lines().take(20).collect::<Vec<_>>();
    if lines.is_empty() {
        "clean".to_string()
    } else {
        truncate_inline(&lines.join("; "), 1_000)
    }
}

pub(crate) fn is_git_diff_check_command(command: &VerificationCommand) -> bool {
    command.program == "git"
        && command.args.first().is_some_and(|arg| arg == "diff")
        && command.args.iter().any(|arg| arg == "--check")
}

pub(crate) fn ensure_git_ready(paths: &Paths, cfg: &Config) -> Result<()> {
    if cfg.require_git && find_git_root(&paths.root).is_none() {
        bail!(
            "Git tracking is required but {} is not inside a Git repository. Run `git init` in the project first, or set `require_git = false` in {}.",
            paths.root.display(),
            paths.config.display()
        );
    }
    Ok(())
}

pub(crate) fn install_orchestrator_runtime_exclude(paths: &Paths) -> Result<()> {
    if find_git_root(&paths.root).is_none() {
        return Ok(());
    }
    let output = Command::new("git")
        .args(["rev-parse", "--git-path", "info/exclude"])
        .current_dir(&paths.root)
        .output()
        .context("resolve repository Git exclude file")?;
    if !output.status.success() {
        bail!("git rev-parse --git-path info/exclude failed");
    }
    let reported = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let exclude = if reported.is_absolute() {
        reported
    } else {
        paths.root.join(reported)
    };
    let begin = "# >>> codex-team runtime >>>";
    let end = "# <<< codex-team runtime <<<";
    let existing = fs::read_to_string(&exclude).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == begin)
        && existing.lines().any(|line| line.trim() == end)
        && existing
            .lines()
            .any(|line| line.trim() == "/.orchestrator/")
    {
        return Ok(());
    }
    if let Some(parent) = exclude.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(begin);
    updated.push('\n');
    updated.push_str("/.orchestrator/\n");
    updated.push_str(end);
    updated.push('\n');
    write_text(&exclude, &updated)
}

pub(crate) fn orchestrator_runtime_is_ignored(paths: &Paths) -> Result<bool> {
    if find_git_root(&paths.root).is_none() {
        return Ok(false);
    }
    let output = Command::new("git")
        .args(["check-ignore", "-q", ".orchestrator/state.json"])
        .current_dir(&paths.root)
        .status()
        .context("check whether orchestrator runtime is ignored")?;
    Ok(output.success())
}

// ===========================================================================
// Observation parsing
// ===========================================================================

pub(crate) fn observed_file_category(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("sources/") || lower.starts_with("src/") {
        "source"
    } else if lower.starts_with("tests/") || lower.starts_with("test/") {
        "test"
    } else if is_tracking_doc_path(path) {
        "tracking_doc"
    } else if lower.starts_with("docs/") || lower.ends_with(".md") {
        "doc"
    } else if lower == "cargo.toml"
        || lower == "cargo.lock"
        || lower == "package.swift"
        || lower == "package.resolved"
    {
        "manifest"
    } else {
        "other"
    }
}

pub(crate) fn observation_verification_hints(changed_files: &[ObservedChangedFile]) -> Vec<String> {
    let mut hints = BTreeSet::new();
    let paths = changed_files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    if paths.iter().any(|path| path.ends_with(".rs")) {
        hints.insert("cargo test --quiet".to_string());
        hints.insert("cargo build".to_string());
    }
    if paths
        .iter()
        .any(|path| path.ends_with(".swift") || path.starts_with("Sources/"))
    {
        for filter in paths
            .iter()
            .filter_map(|path| swift_test_filter_from_path(path))
        {
            hints.insert(format!("swift test --filter {filter}"));
        }
        if hints.is_empty() && paths.iter().any(|path| path.starts_with("Tests/")) {
            hints.insert("swift test".to_string());
        }
        if paths.iter().any(|path| {
            path.ends_with(".swift") || path.starts_with("Sources/") || path == &"Package.swift"
        }) {
            hints.insert("swift build".to_string());
        }
    }
    if paths.iter().any(|path| {
        path == &"Cargo.toml"
            || path == &"Cargo.lock"
            || path == &"Package.swift"
            || path == &"Package.resolved"
    }) {
        hints.insert("git diff --check".to_string());
    }
    hints.insert("git diff --check".to_string());
    hints.into_iter().collect()
}

pub(crate) fn parse_git_porcelain_line(line: &str) -> Option<ObservedChangedFile> {
    if line.len() < 4 {
        return None;
    }
    let status = line.get(0..2)?.trim().to_string();
    let path_part = line.get(3..)?.trim();
    if path_part.is_empty() {
        return None;
    }
    let path = path_part
        .split(" -> ")
        .last()
        .unwrap_or(path_part)
        .trim_matches('"')
        .to_string();
    Some(ObservedChangedFile {
        category: observed_file_category(&path).to_string(),
        path,
        status: if status.is_empty() {
            "modified".to_string()
        } else {
            status
        },
    })
}

pub(crate) fn parse_git_name_status_line(line: &str) -> Option<ObservedChangedFile> {
    let parts = line.split('\t').collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }
    let status = parts[0].trim().to_string();
    let path = parts.last()?.trim().trim_matches('"').to_string();
    if path.is_empty() {
        return None;
    }
    Some(ObservedChangedFile {
        category: observed_file_category(&path).to_string(),
        path,
        status,
    })
}

pub(crate) fn parse_git_history_observations(text: &str) -> Vec<GitHistoryObservation> {
    let mut observations = Vec::new();
    let mut commit: Option<String> = None;
    let mut subject = String::new();
    let mut files = Vec::new();

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("__CODEX_COMMIT__") {
            if let Some(entry) = build_git_history_observation(commit.take(), &subject, &files) {
                observations.push(entry);
            }
            let (sha, title) = rest.split_once('\t').unwrap_or((rest, ""));
            commit = Some(sha.trim().to_string());
            subject = title.trim().to_string();
            files.clear();
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Some(file) = parse_git_name_status_line(line) {
            if !file.path.starts_with(".orchestrator/") {
                files.push(file);
            }
        }
    }
    if let Some(entry) = build_git_history_observation(commit, &subject, &files) {
        observations.push(entry);
    }
    observations
}

pub(crate) fn build_git_history_observation(
    commit: Option<String>,
    subject: &str,
    files: &[ObservedChangedFile],
) -> Option<GitHistoryObservation> {
    let commit = commit?;
    let mut changed_files = files.to_vec();
    changed_files.sort_by(|a, b| a.path.cmp(&b.path).then(a.status.cmp(&b.status)));
    changed_files.dedup_by(|a, b| a.path == b.path && a.status == b.status);
    if changed_files.is_empty() {
        return None;
    }
    let verification_hints = observation_verification_hints(&changed_files);
    Some(GitHistoryObservation {
        observed_at: Utc::now().to_rfc3339(),
        commit,
        subject: subject.to_string(),
        changed_files,
        verification_hints,
    })
}

pub(crate) fn git_history_observations(
    paths: &Paths,
    max_commits: usize,
) -> Result<Vec<GitHistoryObservation>> {
    let count_arg = format!("-n{max_commits}");
    let output = Command::new("git")
        .args([
            "log",
            "--name-status",
            "--pretty=format:__CODEX_COMMIT__%H%x09%s",
            &count_arg,
            "--",
        ])
        .current_dir(&paths.root)
        .output()
        .with_context(|| "run git log --name-status")?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    Ok(parse_git_history_observations(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

pub(crate) fn merge_git_history_observations(
    local_journal: &Path,
    global_journal: &Path,
) -> Result<usize> {
    let mut seen = BTreeSet::new();
    for line in read_text_or_empty(global_journal).lines() {
        let Ok(observation) = serde_json::from_str::<GitHistoryObservation>(line) else {
            continue;
        };
        seen.insert(observation.commit);
    }
    let mut added = 0usize;
    for line in read_text_or_empty(local_journal).lines() {
        let Ok(observation) = serde_json::from_str::<GitHistoryObservation>(line) else {
            continue;
        };
        if seen.insert(observation.commit.clone()) {
            append_jsonl(global_journal, &serde_json::to_value(&observation)?)?;
            added += 1;
        }
    }
    Ok(added)
}

// ===========================================================================
// History operations
// ===========================================================================

pub(crate) fn observe_git_history(paths: &Paths, max_commits: usize) -> Result<usize> {
    if find_git_root(&paths.root).is_none() || max_commits == 0 {
        return Ok(0);
    }
    let commits = git_history_observations(paths, max_commits)?;
    if commits.is_empty() {
        return Ok(0);
    }
    fs::create_dir_all(&paths.observations)?;
    let seen_path = paths.observations.join("git_history_seen.json");
    let mut seen = read_json_file::<BTreeSet<String>>(&seen_path).unwrap_or_default();
    let mut observed = 0usize;
    for observation in commits {
        if !seen.insert(observation.commit.clone()) {
            continue;
        }
        append_jsonl(
            &paths.observations.join("git_history_journal.jsonl"),
            &serde_json::to_value(&observation)?,
        )?;
        update_file_affinity_index(paths, &observation.changed_files)?;
        update_verification_hint_index(
            paths,
            &observation.changed_files,
            &observation.verification_hints,
        )?;
        observed += 1;
    }
    write_text(
        &seen_path,
        &format!("{}\n", serde_json::to_string_pretty(&seen)?),
    )?;
    rebuild_file_affinity_from_observation_journals(paths)?;
    update_history_index(paths)?;
    Ok(observed)
}

// ===========================================================================
// Staging operations
// ===========================================================================

pub(crate) fn stage_explicit_commit_paths(
    paths: &Paths,
    files: &[String],
    context: &str,
) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let tracked = git_tracked_file_set(paths)?;
    let (tracked_paths, untracked_paths): (Vec<_>, Vec<_>) = files
        .iter()
        .cloned()
        .partition(|path| tracked.contains(path));

    if !tracked_paths.is_empty() {
        let mut add_args = vec!["add".to_string(), "-u".to_string(), "--".to_string()];
        add_args.extend(tracked_paths.iter().cloned());
        let add = Command::new("git")
            .arg("--literal-pathspecs")
            .args(&add_args)
            .current_dir(&paths.root)
            .status()
            .with_context(|| format!("git add -u {context}"))?;
        if !add.success() {
            bail!("git add -u {context} failed");
        }
    }

    if !untracked_paths.is_empty() {
        // These paths have already passed the caller's allowed-file policy; -f
        // keeps Rust-owned bookkeeping working when a target ignores root queues.
        let mut add_args = vec!["add".to_string(), "-f".to_string(), "--".to_string()];
        add_args.extend(untracked_paths.iter().cloned());
        let add = Command::new("git")
            .arg("--literal-pathspecs")
            .args(&add_args)
            .current_dir(&paths.root)
            .status()
            .with_context(|| format!("git add -f {context}"))?;
        if !add.success() {
            bail!("git add -f {context} failed");
        }
    }

    Ok(())
}

pub(crate) fn run_git_diff_check_for_selected_commit(
    paths: &Paths,
    selected_paths: &[String],
    request_id: &str,
) -> Result<()> {
    let check = Command::new("git")
        .args(["diff", "--check", "--"])
        .args(selected_paths)
        .current_dir(&paths.root)
        .status()
        .with_context(|| format!("git diff --check role-authorized commit {request_id}"))?;
    if !check.success() {
        bail!("git diff --check failed for Rust commit request {request_id}");
    }
    Ok(())
}

pub(crate) fn apply_unified_patch_with_git(
    paths: &Paths,
    task: &TaskSpec,
    patch: &str,
) -> Result<()> {
    let patch_path = std::env::temp_dir().join(format!(
        "codex-team-{}-{}.patch",
        sanitize_filename(&task.task_id),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let mut patch_text = patch.to_string();
    if !patch_text.ends_with('\n') {
        patch_text.push('\n');
    }
    write_text(&patch_path, &patch_text)?;
    for args in [
        vec!["apply", "--check", "--whitespace=nowarn"],
        vec!["apply", "--whitespace=nowarn"],
    ] {
        let output = Command::new("git")
            .args(&args)
            .arg(&patch_path)
            .current_dir(&paths.root)
            .output()
            .with_context(|| format!("git {}", args.join(" ")))?;
        if !output.status.success() {
            bail!(
                "git {} failed: {}{}",
                args.join(" "),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    let _ = fs::remove_file(&patch_path);
    Ok(())
}

// ===========================================================================
// Commit path selection
// ===========================================================================

pub(crate) fn control_doc_commit_allowlist() -> BTreeSet<String> {
    BTreeSet::from([
        "CONTINUITY.md".to_string(),
        "NEXT_STEPS.md".to_string(),
        "NEXT_STEPS_non-ai.md".to_string(),
        "ROADMAP.md".to_string(),
        "TASK.md".to_string(),
        "docs/ROADMAP.md".to_string(),
        "Docs/ROADMAP.md".to_string(),
        "Docs/.INDEX.md".to_string(),
    ])
}

pub(crate) fn selected_rust_commit_paths(paths: &Paths, allowed: &GlobSet) -> Result<Vec<String>> {
    let mut candidates = git_dirty_name_only(paths)?;
    candidates.extend(git_untracked_name_only(paths)?);
    candidates.sort();
    candidates.dedup();

    let mut selected = candidates
        .into_iter()
        .filter(|path| !path.starts_with(".orchestrator/"))
        .filter(|path| allowed.is_match(path.as_str()))
        .collect::<Vec<_>>();
    selected.sort();
    selected.dedup();
    Ok(selected)
}

pub(crate) fn concrete_dirty_commit_paths(
    paths: &Paths,
    candidates: &[String],
    allowed: &GlobSet,
) -> Result<Vec<String>> {
    let dirty_files = git_dirty_name_only(paths)?;
    let dirty_set = dirty_files.iter().cloned().collect::<HashSet<_>>();
    let mut concrete = Vec::new();
    for candidate in candidates {
        let candidate = candidate
            .trim()
            .trim_start_matches("./")
            .trim_end_matches('/');
        if candidate.is_empty() || candidate.contains('*') || candidate.contains('{') {
            continue;
        }
        let candidate_path = paths.root.join(candidate);
        if candidate_path.is_dir() {
            let prefix = format!("{candidate}/");
            concrete.extend(
                dirty_files
                    .iter()
                    .filter(|path| {
                        path.get(..prefix.len()).is_some_and(|actual_prefix| {
                            actual_prefix.eq_ignore_ascii_case(&prefix)
                        }) && allowed.is_match(path.as_str())
                    })
                    .cloned(),
            );
            continue;
        }
        if dirty_set.contains(candidate) {
            concrete.push(candidate.to_string());
        } else if let Some(actual_path) = dirty_files
            .iter()
            .find(|path| path.eq_ignore_ascii_case(candidate))
        {
            // APFS commonly resolves `Docs/foo` and `docs/foo` to the same
            // file, while Git pathspecs still require the tracked spelling.
            // Prefer the exact path reported by Git so an allowlist alias
            // cannot make an otherwise valid commit fail at the boundary.
            concrete.push(actual_path.clone());
        } else if candidate_path.exists() {
            concrete.push(candidate.to_string());
        }
    }
    concrete.sort();
    concrete.dedup();
    Ok(concrete)
}

// ===========================================================================
// Commit operations
// ===========================================================================

/// Reject repository dirt before supervised execution. Runtime state and
/// configured generated/cache roots are the only exclusions.
pub(crate) fn ensure_clean_start(paths: &Paths, cfg: &Config) -> Result<()> {
    if !cfg.require_clean_start && cfg.commit_mode != CommitMode::Manual {
        return Ok(());
    }
    if find_git_root(&paths.root).is_none() {
        bail!("clean-start enforcement requires a Git repository");
    }
    let dirty = git_dirty_paths_excluding_runtime(paths, cfg)?;
    if !dirty.is_empty() {
        bail!(
            "clean start required; repository has non-runtime changes: {}",
            dirty.join(", ")
        );
    }
    ensure_index_empty(paths)
}

pub(crate) fn ensure_no_pending_commit(state: &State) -> Result<()> {
    if let Some(pending) = &state.pending_commit {
        bail!(
            "pending commit {} blocks further work; review it and run `codex-team commit-pending`",
            pending.id
        );
    }
    Ok(())
}

fn ensure_index_empty(paths: &Paths) -> Result<()> {
    let status = Command::new("git")
        .args(["diff", "--cached", "--quiet", "--exit-code"])
        .current_dir(&paths.root)
        .status()
        .context("check Git index")?;
    match status.code() {
        Some(0) => Ok(()),
        Some(1) => bail!("Git index must be empty for supervised manual commits"),
        _ => bail!("failed to inspect Git index"),
    }
}

fn git_dirty_paths_excluding_runtime(paths: &Paths, cfg: &Config) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .current_dir(&paths.root)
        .output()
        .context("git status for clean-start enforcement")?;
    if !output.status.success() {
        bail!("git status failed during clean-start enforcement");
    }
    let ignored = compile_globs(&cfg.guard.ignored_paths)?;
    let mut dirty = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_porcelain_path)
        .filter(|path| path != ".orchestrator" && !path.starts_with(".orchestrator/"))
        .filter(|path| !ignored.is_match(path.as_str()))
        .collect::<Vec<_>>();
    dirty.sort();
    dirty.dedup();
    Ok(dirty)
}

fn parse_porcelain_path(line: &str) -> Option<String> {
    let payload = line.get(3..)?.trim();
    let path = payload.rsplit_once(" -> ").map_or(payload, |(_, new)| new);
    let path = path.trim_matches('"').replace("\\\"", "\"");
    (!path.is_empty()).then_some(path)
}

fn current_commit_paths(paths: &Paths) -> Result<Vec<String>> {
    let mut paths_out = git_dirty_name_only(paths)?;
    paths_out.extend(git_untracked_name_only(paths)?);
    paths_out.retain(|path| path != ".orchestrator" && !path.starts_with(".orchestrator/"));
    paths_out.sort();
    paths_out.dedup();
    Ok(paths_out)
}

fn fingerprint_pending_path(paths: &Paths, relative: &str) -> Result<PendingPathFingerprint> {
    let absolute = paths.root.join(relative);
    let metadata = match fs::symlink_metadata(&absolute) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PendingPathFingerprint {
                path: relative.to_string(),
                exists: false,
                file_type: "missing".to_string(),
                git_mode: None,
                content_sha256: None,
            });
        }
        Err(err) => return Err(err).with_context(|| format!("inspect pending path {relative}")),
    };
    let (file_type, git_mode, content) = if metadata.file_type().is_symlink() {
        (
            "symlink",
            Some("120000".to_string()),
            Some(
                fs::read_link(&absolute)?
                    .to_string_lossy()
                    .as_bytes()
                    .to_vec(),
            ),
        )
    } else if metadata.is_file() {
        (
            "file",
            Some(worktree_regular_file_git_mode(&metadata)),
            Some(fs::read(&absolute)?),
        )
    } else if metadata.is_dir() {
        ("directory", Some("040000".to_string()), None)
    } else {
        ("other", None, None)
    };
    Ok(PendingPathFingerprint {
        path: relative.to_string(),
        exists: true,
        file_type: file_type.to_string(),
        git_mode,
        content_sha256: content.map(|bytes| sha256_hex(&bytes)),
    })
}

#[cfg(unix)]
fn worktree_regular_file_git_mode(metadata: &fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt as _;
    if metadata.permissions().mode() & 0o111 == 0 {
        "100644".to_string()
    } else {
        "100755".to_string()
    }
}

#[cfg(not(unix))]
fn worktree_regular_file_git_mode(_metadata: &fs::Metadata) -> String {
    // Git for non-POSIX filesystems records ordinary worktree files as
    // non-executable unless a platform-specific index operation says otherwise.
    "100644".to_string()
}

fn missing_pending_path_fingerprint(relative: &str) -> PendingPathFingerprint {
    PendingPathFingerprint {
        path: relative.to_string(),
        exists: false,
        file_type: "missing".to_string(),
        git_mode: None,
        content_sha256: None,
    }
}

fn git_blob_content_sha256(paths: &Paths, object_id: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["cat-file", "blob", object_id])
        .current_dir(&paths.root)
        .output()
        .with_context(|| format!("read staged Git blob {object_id}"))?;
    if !output.status.success() {
        bail!("Git could not read staged blob {object_id}");
    }
    Ok(sha256_hex(&output.stdout))
}

fn fingerprint_index_path(paths: &Paths, relative: &str) -> Result<PendingPathFingerprint> {
    let output = Command::new("git")
        .args([
            "--literal-pathspecs",
            "ls-files",
            "--stage",
            "-z",
            "--",
            relative,
        ])
        .current_dir(&paths.root)
        .output()
        .with_context(|| format!("inspect staged Git path {relative}"))?;
    if !output.status.success() {
        bail!("Git could not inspect staged path {relative}");
    }
    let records = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect::<Vec<_>>();
    if records.is_empty() {
        return Ok(missing_pending_path_fingerprint(relative));
    }
    if records.len() != 1 {
        bail!("staged path {relative} resolved to multiple index entries");
    }
    let record = records[0];
    let tab = record
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or_else(|| anyhow::anyhow!("Git returned a malformed staged entry for {relative}"))?;
    if &record[tab + 1..] != relative.as_bytes() {
        bail!("Git returned a different staged path while inspecting {relative}");
    }
    let header = std::str::from_utf8(&record[..tab])
        .with_context(|| format!("Git returned a non-UTF-8 staged header for {relative}"))?;
    let fields = header.split_ascii_whitespace().collect::<Vec<_>>();
    if fields.len() != 3 || fields[2] != "0" {
        bail!("Git returned an unmerged or malformed staged entry for {relative}");
    }
    fingerprint_git_entry(paths, relative, fields[0], "blob", fields[1])
}

fn fingerprint_commit_path(
    paths: &Paths,
    commit: &str,
    relative: &str,
) -> Result<PendingPathFingerprint> {
    let output = Command::new("git")
        .args([
            "--literal-pathspecs",
            "ls-tree",
            "-z",
            commit,
            "--",
            relative,
        ])
        .current_dir(&paths.root)
        .output()
        .with_context(|| format!("inspect committed Git path {relative}"))?;
    if !output.status.success() {
        bail!("Git could not inspect committed path {relative}");
    }
    let records = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect::<Vec<_>>();
    if records.is_empty() {
        return Ok(missing_pending_path_fingerprint(relative));
    }
    if records.len() != 1 {
        bail!("committed path {relative} resolved to multiple tree entries");
    }
    let record = records[0];
    let tab = record
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or_else(|| anyhow::anyhow!("Git returned a malformed tree entry for {relative}"))?;
    if &record[tab + 1..] != relative.as_bytes() {
        bail!("Git returned a different tree path while inspecting {relative}");
    }
    let header = std::str::from_utf8(&record[..tab])
        .with_context(|| format!("Git returned a non-UTF-8 tree header for {relative}"))?;
    let fields = header.split_ascii_whitespace().collect::<Vec<_>>();
    if fields.len() != 3 {
        bail!("Git returned a malformed tree entry for {relative}");
    }
    fingerprint_git_entry(paths, relative, fields[0], fields[1], fields[2])
}

fn fingerprint_git_entry(
    paths: &Paths,
    relative: &str,
    mode: &str,
    object_type: &str,
    object_id: &str,
) -> Result<PendingPathFingerprint> {
    let (file_type, content_sha256) = match (mode, object_type) {
        ("100644" | "100755", "blob") => ("file", Some(git_blob_content_sha256(paths, object_id)?)),
        ("120000", "blob") => ("symlink", Some(git_blob_content_sha256(paths, object_id)?)),
        ("160000", "commit") => ("gitlink", None),
        ("040000", "tree") => ("directory", None),
        _ => bail!("Git returned unsupported mode/type {mode}/{object_type} for {relative}"),
    };
    Ok(PendingPathFingerprint {
        path: relative.to_string(),
        exists: true,
        file_type: file_type.to_string(),
        git_mode: Some(mode.to_string()),
        content_sha256,
    })
}

fn validate_staged_fingerprints(paths: &Paths, expected: &[PendingPathFingerprint]) -> Result<()> {
    for approved in expected {
        let staged = fingerprint_index_path(paths, &approved.path)?;
        if staged != *approved {
            bail!(
                "staged path {} no longer matches its approved content, type, or Git mode",
                approved.path
            );
        }
    }
    Ok(())
}

fn git_commit_changed_paths(paths: &Paths, commit: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args([
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--name-only",
            "-r",
            "--no-renames",
            "-z",
            commit,
        ])
        .current_dir(&paths.root)
        .output()
        .context("inspect committed path set")?;
    if !output.status.success() {
        bail!("Git could not inspect the committed path set");
    }
    let mut changed = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            String::from_utf8(path.to_vec()).context("committed Git path was not valid UTF-8")
        })
        .collect::<Result<Vec<_>>>()?;
    changed.sort();
    changed.dedup();
    Ok(changed)
}

fn git_commit_parents(paths: &Paths, commit: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["rev-list", "--parents", "-n", "1", commit])
        .current_dir(&paths.root)
        .output()
        .context("inspect commit ancestry")?;
    if !output.status.success() {
        bail!("Git could not inspect commit ancestry");
    }
    let fields =
        String::from_utf8(output.stdout).context("Git returned non-UTF-8 commit ancestry")?;
    let fields = fields.split_ascii_whitespace().collect::<Vec<_>>();
    if fields.first().copied() != Some(commit) {
        bail!("Git returned unexpected commit ancestry");
    }
    Ok(fields[1..]
        .iter()
        .map(|value| (*value).to_string())
        .collect())
}

fn validate_committed_fingerprints(
    paths: &Paths,
    commit: &str,
    base_head: &str,
    message: &str,
    expected: &[PendingPathFingerprint],
) -> Result<()> {
    let current_head = git_head(paths).ok_or_else(|| anyhow::anyhow!("read Git HEAD"))?;
    if current_head != commit {
        bail!("Git HEAD changed before committed content could be validated");
    }
    validate_commit_object_fingerprints(paths, commit, base_head, message, expected)
}

/// Validate the complete commit object before publishing it through a ref.
///
/// This is deliberately separate from [`validate_committed_fingerprints`]: a
/// pathspec-form `git commit` re-reads the worktree after staged fingerprints
/// were approved.  Building an object from the already-validated index and
/// checking it while it is still unreachable closes that TOCTOU window.  Only
/// the exact validated object is subsequently allowed through the ref CAS.
fn validate_commit_object_fingerprints(
    paths: &Paths,
    commit: &str,
    base_head: &str,
    message: &str,
    expected: &[PendingPathFingerprint],
) -> Result<()> {
    let expected_parents = if base_head == "unborn" {
        Vec::new()
    } else {
        vec![base_head.to_string()]
    };
    if git_commit_parents(paths, commit)? != expected_parents {
        bail!("approved commit does not have the expected base HEAD");
    }

    let object = Command::new("git")
        .args(["cat-file", "commit", commit])
        .current_dir(&paths.root)
        .output()
        .context("inspect committed object")?;
    if !object.status.success() {
        bail!("Git could not inspect the committed object");
    }
    let separator = object
        .stdout
        .windows(2)
        .position(|window| window == b"\n\n")
        .ok_or_else(|| anyhow::anyhow!("Git returned a malformed commit object"))?;
    let headers = &object.stdout[..separator];
    if headers
        .split(|byte| *byte == b'\n')
        .any(|line| line.starts_with(b"gpgsig "))
    {
        bail!("approved commit was unexpectedly signed");
    }
    let actual_message = String::from_utf8(object.stdout[separator + 2..].to_vec())
        .context("Git commit message was not valid UTF-8")?;
    if actual_message.trim_end_matches('\n') != message.trim_end_matches('\n') {
        bail!("approved commit message changed");
    }

    let mut expected_paths = expected
        .iter()
        .map(|fingerprint| fingerprint.path.clone())
        .collect::<Vec<_>>();
    expected_paths.sort();
    expected_paths.dedup();
    if git_commit_changed_paths(paths, commit)? != expected_paths {
        bail!("approved commit changed an unexpected path set");
    }
    for approved in expected {
        let committed = fingerprint_commit_path(paths, commit, &approved.path)?;
        if committed != *approved {
            bail!(
                "committed path {} no longer matches its approved content, type, or Git mode",
                approved.path
            );
        }
    }
    Ok(())
}

fn parse_git_object_id(label: &str, bytes: &[u8]) -> Result<String> {
    if bytes.len() > 256 {
        bail!("Git returned oversized {label} output");
    }
    let value = std::str::from_utf8(bytes)
        .with_context(|| format!("Git returned a non-UTF-8 {label}"))?
        .trim();
    if !matches!(value.len(), 40 | 64)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("Git returned an invalid {label}");
    }
    Ok(value.to_string())
}

/// Materialize and validate a commit from the already-approved index, then
/// publish it with an old-value compare-and-swap on HEAD.
///
/// `git commit -- <paths>` is intentionally forbidden here: that form stages
/// the named worktree paths a second time and can therefore commit bytes which
/// were never fingerprinted. `write-tree` freezes the validated index, and
/// `commit-tree` creates an unreachable object. Full path/content/parent/message
/// validation happens before the sole visible mutation, `update-ref`.
fn publish_prevalidated_index_commit(
    paths: &Paths,
    base_head: &str,
    message: &str,
    expected: &[PendingPathFingerprint],
) -> Result<String> {
    let tree_output = Command::new("git")
        .arg("write-tree")
        .current_dir(&paths.root)
        .output()
        .context("write approved Git tree")?;
    if !tree_output.status.success() {
        bail!("Git could not write the approved index tree");
    }
    let tree = parse_git_object_id("tree object ID", &tree_output.stdout)?;

    let mut commit_command = Command::new("git");
    commit_command
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgSign=false",
            "commit-tree",
            tree.as_str(),
        ])
        .current_dir(&paths.root);
    if base_head != "unborn" {
        commit_command.args(["-p", base_head]);
    }
    commit_command.args(["-m", message]);
    let commit_output = commit_command
        .output()
        .context("create approved Git commit object")?;
    if !commit_output.status.success() {
        bail!("Git could not create the approved commit object");
    }
    let commit = parse_git_object_id("commit object ID", &commit_output.stdout)?;
    validate_commit_object_fingerprints(paths, &commit, base_head, message, expected)
        .context("validate approved commit object before ref publication")?;

    let old_value = if base_head == "unborn" {
        "0".repeat(commit.len())
    } else {
        if base_head.len() != commit.len() {
            bail!("approved base HEAD uses a different Git object format");
        }
        base_head.to_string()
    };
    let update = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "update-ref",
            "-m",
            "codex-team approved commit",
            "HEAD",
            commit.as_str(),
            old_value.as_str(),
        ])
        .current_dir(&paths.root)
        .status()
        .context("publish approved Git commit")?;
    if !update.success() {
        bail!("Git HEAD changed before the approved commit could be published");
    }
    Ok(commit)
}

const MAX_DEFERRED_CONTROL_DOC_BYTES: usize = 1024 * 1024;
const MAX_DEFERRED_CONTROL_DOC_TOTAL_BYTES: usize = 2 * 1024 * 1024;

/// Exact bytes observed immediately before Rust updates a queue or tracking
/// document. Auto mode ignores this snapshot because it commits immediately;
/// manual mode restores it while human approval is pending.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct RustOwnedFileSnapshot {
    pub(crate) path: String,
    before_fingerprint: PendingPathFingerprint,
    before_content: Option<String>,
}

pub(crate) fn capture_rust_owned_file(paths: &Paths, path: &Path) -> Result<RustOwnedFileSnapshot> {
    let relative = path.strip_prefix(&paths.root).with_context(|| {
        format!(
            "Rust-owned file {} is outside the repository",
            path.display()
        )
    })?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        bail!("Rust-owned file path must be a normalized repository-relative path");
    }
    let relative = relative
        .iter()
        .map(|component| component.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if relative == ".orchestrator" || relative.starts_with(".orchestrator/") {
        bail!("orchestrator runtime files cannot be deferred into a project commit");
    }
    let before_fingerprint = fingerprint_pending_path(paths, &relative)?;
    let before_content =
        match before_fingerprint.file_type.as_str() {
            "missing" => None,
            "file" => {
                let bytes = fs::read(path)
                    .with_context(|| format!("read Rust-owned file {}", path.display()))?;
                if bytes.len() > MAX_DEFERRED_CONTROL_DOC_BYTES {
                    bail!(
                        "Rust-owned control document {} exceeds the {} byte manual-mode limit",
                        relative,
                        MAX_DEFERRED_CONTROL_DOC_BYTES
                    );
                }
                Some(String::from_utf8(bytes).with_context(|| {
                    format!("Rust-owned control document {relative} must be UTF-8")
                })?)
            }
            kind => bail!(
            "Rust-owned control document {relative} must be a regular file or missing, found {kind}"
        ),
        };
    Ok(RustOwnedFileSnapshot {
        path: relative,
        before_fingerprint,
        before_content,
    })
}

pub(crate) fn rust_owned_file_changed(
    paths: &Paths,
    snapshot: &RustOwnedFileSnapshot,
) -> Result<bool> {
    Ok(fingerprint_pending_path(paths, &snapshot.path)? != snapshot.before_fingerprint)
}

pub(crate) fn restore_rust_owned_files(
    paths: &Paths,
    snapshots: &[RustOwnedFileSnapshot],
) -> Result<()> {
    let writes = snapshots
        .iter()
        .map(|snapshot| PendingDeferredFileWrite {
            path: snapshot.path.clone(),
            before_fingerprint: snapshot.before_fingerprint.clone(),
            before_content: snapshot.before_content.clone(),
            after_content: String::new(),
        })
        .collect::<Vec<_>>();
    restore_deferred_file_writes(paths, &writes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct DeferredTaskCompletionV1 {
    schema: String,
    task_id: String,
    completed_tasks_before: u64,
    set_master_due: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingCommitRecoveryV1 {
    schema: String,
    pending_id: String,
    pending_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    approved_revision: Option<String>,
    base_head: String,
    dirty_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    path_fingerprints: Vec<PendingPathFingerprint>,
    commit_message: String,
    prepared_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commit_sha: Option<String>,
    state_after: State,
}

fn pending_commit_recovery_path(paths: &Paths) -> PathBuf {
    paths.orch.join("pending-commit-recovery.json")
}

fn write_pending_commit_recovery(paths: &Paths, recovery: &PendingCommitRecoveryV1) -> Result<()> {
    let text = serde_json::to_string_pretty(recovery)?;
    if text.len() > 4 * 1024 * 1024 {
        bail!("pending-commit recovery record exceeds the 4 MiB runtime-state limit");
    }
    fs::create_dir_all(&paths.orch)?;
    write_text(&pending_commit_recovery_path(paths), &text)
        .context("write pending-commit recovery evidence")
}

fn remove_pending_commit_recovery(paths: &Paths) {
    match fs::remove_file(pending_commit_recovery_path(paths)) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => eprintln!("[Recovery] Could not remove pending-commit evidence: {err}"),
    }
}

pub(crate) fn clear_pending_commit_recovery(paths: &Paths) {
    remove_pending_commit_recovery(paths);
}

fn read_pending_commit_recovery(paths: &Paths) -> Result<Option<PendingCommitRecoveryV1>> {
    let path = pending_commit_recovery_path(paths);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).context("inspect pending-commit recovery evidence"),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("pending-commit recovery evidence must be a regular, non-symlink file");
    }
    if metadata.len() > 4 * 1024 * 1024 {
        bail!("pending-commit recovery record exceeds the 4 MiB runtime-state limit");
    }
    let text = fs::read_to_string(&path).context("read pending-commit recovery evidence")?;
    let recovery: PendingCommitRecoveryV1 =
        serde_json::from_str(&text).context("parse pending-commit recovery evidence")?;
    if recovery.schema != "codex-team.pending_commit_recovery.v1" {
        bail!("unsupported pending-commit recovery schema");
    }
    Ok(Some(recovery))
}

fn states_serialize_equally(left: &State, right: &State) -> Result<bool> {
    Ok(serde_json::to_value(left)? == serde_json::to_value(right)?)
}

/// Finish the state half of a manual commit that already advanced Git.  This
/// is intentionally called before ordinary pending-intent validation so a
/// process restart after `git commit` but before `state.json` persistence can
/// retry the same exact approval idempotently.
fn reconcile_completed_pending_commit(
    paths: &Paths,
    state: &mut State,
    expected_id: &str,
    expected_digest: &str,
) -> Result<Option<String>> {
    let Some(mut recovery) = read_pending_commit_recovery(paths)? else {
        return Ok(None);
    };
    if recovery.pending_id != expected_id || recovery.pending_digest != expected_digest {
        bail!("a different pending-commit recovery record blocks this approval");
    }
    if recovery.dirty_paths.is_empty() {
        bail!("pending-commit recovery record has an empty path set");
    }

    let expected_state_after = if let Some(pending) = state.pending_commit.as_ref() {
        if pending.id != recovery.pending_id || pending.digest != recovery.pending_digest {
            bail!("pending-commit recovery record does not match persisted state");
        }
        if pending_commit_digest(pending)? != pending.digest {
            bail!("persisted pending commit was modified after recovery preparation");
        }
        if recovery.base_head != pending.base_head
            || recovery.dirty_paths != pending.dirty_paths
            || recovery.commit_message != pending.commit_message
            || (!recovery.path_fingerprints.is_empty()
                && recovery.path_fingerprints != pending.path_fingerprints)
        {
            bail!("pending-commit recovery evidence does not match the approved intent");
        }
        let transition = validate_deferred_task_completion(state, pending)?;
        let mut expected = state.clone();
        if let Some(transition) = transition {
            apply_deferred_task_completion(&mut expected, transition);
        }
        expected.pending_commit = None;
        if !states_serialize_equally(&expected, &recovery.state_after)? {
            bail!("pending-commit recovery state transition was modified");
        }
        expected
    } else {
        // A prior retry may have persisted state and then failed only while
        // removing recovery evidence.  Returning the same SHA is idempotent,
        // but only when the persisted state exactly equals the recorded result.
        if !states_serialize_equally(state, &recovery.state_after)? {
            bail!("pending-commit recovery evidence does not match persisted state");
        }
        state.clone()
    };

    let head = git_head(paths).unwrap_or_else(|| "unborn".to_string());
    let commit = match recovery.commit_sha.as_deref() {
        Some(commit) => {
            if head != commit {
                bail!("Git HEAD does not match the completed pending-commit recovery record");
            }
            commit.to_string()
        }
        None if head == recovery.base_head => return Ok(None),
        None => head,
    };
    if commit == "unborn" {
        bail!("pending-commit recovery claims completion without a Git commit");
    }

    let fingerprints = if let Some(pending) = state.pending_commit.as_ref() {
        pending.path_fingerprints.clone()
    } else if recovery.path_fingerprints.is_empty() {
        // Legacy v1 records did not persist fingerprints.  When state already
        // contains the exact recorded result, derive tree fingerprints only to
        // validate ancestry, message, and changed-path integrity idempotently.
        recovery
            .dirty_paths
            .iter()
            .map(|path| fingerprint_commit_path(paths, &commit, path))
            .collect::<Result<Vec<_>>>()?
    } else {
        recovery.path_fingerprints.clone()
    };
    validate_committed_fingerprints(
        paths,
        &commit,
        &recovery.base_head,
        &recovery.commit_message,
        &fingerprints,
    )
    .context("validate completed pending-commit recovery")?;

    if recovery.commit_sha.is_none() {
        recovery.commit_sha = Some(commit.clone());
        write_pending_commit_recovery(paths, &recovery)
            .context("finalize reconstructed pending-commit recovery evidence")?;
    }
    *state = expected_state_after;
    Ok(Some(commit))
}

/// Permit the one safe stale-revision exception: the caller repeats the exact
/// revision, ID, and digest that originally authorized a commit which recovery
/// evidence proves already advanced Git. Ordinary pending intents and recovery
/// records from older versions do not bypass repository revision validation.
pub(crate) fn reconcile_pending_commit_retry(
    paths: &Paths,
    state: &mut State,
    expected_id: &str,
    expected_digest: &str,
    expected_revision: &str,
) -> Result<Option<String>> {
    let Some(recovery) = read_pending_commit_recovery(paths)? else {
        return Ok(None);
    };
    if recovery.pending_id != expected_id
        || recovery.pending_digest != expected_digest
        || recovery.approved_revision.as_deref() != Some(expected_revision)
    {
        return Ok(None);
    }
    reconcile_completed_pending_commit(paths, state, expected_id, expected_digest)
}

fn pending_commit_digest(pending: &PendingCommit) -> Result<String> {
    // The stable identifier fields are outputs of the digest and therefore are
    // excluded from its material.  Recomputing this during approval detects a
    // locally edited state record before Git is touched.
    let mut material = pending.clone();
    material.id.clear();
    material.digest.clear();
    let bytes = serde_json::to_vec(&material)?;
    Ok(sha256_hex(
        &[
            b"codex-team-pending-commit-v1\0".as_slice(),
            bytes.as_slice(),
        ]
        .concat(),
    ))
}

fn validate_deferred_task_completion(
    state: &State,
    pending: &PendingCommit,
) -> Result<Option<DeferredTaskCompletionV1>> {
    let Some(value) = pending.deferred_transition.clone() else {
        return Ok(None);
    };
    let transition: DeferredTaskCompletionV1 =
        serde_json::from_value(value).context("parse deferred pending-commit state transition")?;
    if transition.schema != "codex-team.deferred_task_completion.v1" {
        bail!("pending commit has an unsupported deferred transition schema");
    }
    if pending.task_id.as_deref() != Some(transition.task_id.as_str()) {
        bail!("pending commit deferred transition task does not match its intent");
    }
    if state.completed_tasks != transition.completed_tasks_before {
        bail!("pending commit deferred transition is stale");
    }
    if state
        .queue
        .iter()
        .filter(|task| task.task_id == transition.task_id)
        .count()
        != 1
    {
        bail!("pending commit deferred task is missing or duplicated in state");
    }
    match &pending.import_digest {
        Some(digest) => {
            let imported = state
                .imported_tasks
                .get(&transition.task_id)
                .ok_or_else(|| anyhow::anyhow!("pending imported task origin is missing"))?;
            if imported.import_digest != *digest {
                bail!("pending imported task digest no longer matches its origin record");
            }
        }
        None if state.imported_tasks.contains_key(&transition.task_id) => {
            bail!("pending imported task is missing its bound import digest");
        }
        None => {}
    }
    Ok(Some(transition))
}

fn apply_deferred_task_completion(state: &mut State, transition: DeferredTaskCompletionV1) {
    state
        .queue
        .retain(|task| task.task_id != transition.task_id);
    state.completed_tasks = transition.completed_tasks_before.saturating_add(1);
    state.failed_tasks.remove(&transition.task_id);
    if state.retry_worker_same_task.as_deref() == Some(transition.task_id.as_str()) {
        state.retry_worker_same_task = None;
        state.retry_worker_brief = None;
    }
    if state.senior_dev_repair_order.as_deref() == Some(transition.task_id.as_str()) {
        state.senior_dev_repair_order = None;
    }
    state.master_due |= transition.set_master_due;
}

struct CommitIntent<'a> {
    source_lane: &'a str,
    task_id: Option<&'a str>,
    message: &'a str,
    files: &'a [String],
    guard_evidence_ids: Vec<String>,
    verification_evidence_ids: Vec<String>,
    deferred_file_snapshots: &'a [RustOwnedFileSnapshot],
    defer_task_completion: bool,
}

fn restore_deferred_file_writes(paths: &Paths, writes: &[PendingDeferredFileWrite]) -> Result<()> {
    let mut failures = Vec::new();
    for write in writes {
        let path = paths.root.join(&write.path);
        let result = match write.before_content.as_deref() {
            Some(content) => write_text(&path, content),
            None => match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err.into()),
            },
        };
        if let Err(err) = result {
            failures.push(format!("{}: {err:#}", write.path));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        bail!(
            "failed to restore Rust-owned control-document bytes: {}",
            failures.join("; ")
        )
    }
}

fn apply_deferred_file_writes(paths: &Paths, writes: &[PendingDeferredFileWrite]) -> Result<()> {
    for write in writes {
        let current = fingerprint_pending_path(paths, &write.path)?;
        if current != write.before_fingerprint {
            bail!(
                "pending Rust-owned file {} changed before approval",
                write.path
            );
        }
        write_text(&paths.root.join(&write.path), &write.after_content)
            .with_context(|| format!("apply pending Rust-owned file {}", write.path))?;
    }
    Ok(())
}

fn unstage_selected_paths(paths: &Paths, selected: &[String]) {
    let _ = Command::new("git")
        .args(["--literal-pathspecs", "reset", "--quiet", "--"])
        .args(selected)
        .current_dir(&paths.root)
        .status();
}

fn validate_preapproval_worktree(paths: &Paths, pending: &PendingCommit) -> Result<()> {
    let selected = pending.dirty_paths.iter().cloned().collect::<BTreeSet<_>>();
    let deferred = pending
        .deferred_file_writes
        .iter()
        .map(|write| (write.path.as_str(), write))
        .collect::<std::collections::HashMap<_, _>>();
    let current = current_commit_paths(paths)?;
    let current_set = current.iter().cloned().collect::<BTreeSet<_>>();
    let outside = current_set
        .difference(&selected)
        .cloned()
        .collect::<Vec<_>>();
    if !outside.is_empty() {
        bail!(
            "pending commit dirty path set changed; unexpected paths: [{}]",
            outside.join(", ")
        );
    }

    for (path, final_fingerprint) in pending.dirty_paths.iter().zip(&pending.path_fingerprints) {
        let current_fingerprint = fingerprint_pending_path(paths, path)?;
        if let Some(write) = deferred.get(path.as_str()) {
            if current_fingerprint != write.before_fingerprint {
                bail!("pending commit Rust-owned file content, type, or Git mode changed");
            }
        } else {
            if !current_set.contains(path) {
                bail!("pending commit dirty path set changed");
            }
            if current_fingerprint != *final_fingerprint {
                bail!("pending commit file content, type, or Git mode changed");
            }
        }
    }
    Ok(())
}

fn prepare_deferred_file_writes(
    paths: &Paths,
    selected: &[String],
    snapshots: &[RustOwnedFileSnapshot],
) -> Result<Vec<PendingDeferredFileWrite>> {
    let selected = selected.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut total_bytes = 0usize;
    let mut writes = Vec::new();
    for snapshot in snapshots {
        if !selected.contains(snapshot.path.as_str()) {
            bail!(
                "Rust-owned deferred file {} is not part of the proposed commit",
                snapshot.path
            );
        }
        if !seen.insert(snapshot.path.clone()) {
            bail!("duplicate Rust-owned deferred file {}", snapshot.path);
        }
        let after_fingerprint = fingerprint_pending_path(paths, &snapshot.path)?;
        if after_fingerprint.file_type != "file" {
            bail!(
                "Rust-owned deferred file {} must end as a regular file",
                snapshot.path
            );
        }
        let after_bytes = fs::read(paths.root.join(&snapshot.path))?;
        if after_bytes.len() > MAX_DEFERRED_CONTROL_DOC_BYTES {
            bail!(
                "Rust-owned control document {} exceeds the {} byte manual-mode limit",
                snapshot.path,
                MAX_DEFERRED_CONTROL_DOC_BYTES
            );
        }
        let after_content = String::from_utf8(after_bytes).with_context(|| {
            format!(
                "Rust-owned control document {} must remain UTF-8",
                snapshot.path
            )
        })?;
        total_bytes = total_bytes
            .checked_add(
                snapshot
                    .before_content
                    .as_ref()
                    .map_or(0, |content| content.len()),
            )
            .and_then(|total| total.checked_add(after_content.len()))
            .ok_or_else(|| anyhow::anyhow!("Rust-owned deferred content size overflow"))?;
        if total_bytes > MAX_DEFERRED_CONTROL_DOC_TOTAL_BYTES {
            bail!(
                "Rust-owned deferred control-document contents exceed the {} byte aggregate manual-mode limit",
                MAX_DEFERRED_CONTROL_DOC_TOTAL_BYTES
            );
        }
        writes.push(PendingDeferredFileWrite {
            path: snapshot.path.clone(),
            before_fingerprint: snapshot.before_fingerprint.clone(),
            before_content: snapshot.before_content.clone(),
            after_content,
        });
    }
    writes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(writes)
}

fn coordinate_commit(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    intent: CommitIntent<'_>,
) -> Result<Option<String>> {
    if find_git_root(&paths.root).is_none() {
        return Ok(None);
    }
    let mut selected = intent.files.to_vec();
    selected.sort();
    selected.dedup();
    if selected.is_empty() {
        return Ok(None);
    }

    if cfg.commit_mode == CommitMode::Auto {
        let base_head = git_head(paths).unwrap_or_else(|| "unborn".to_string());
        let approved_fingerprints = selected
            .iter()
            .map(|path| fingerprint_pending_path(paths, path))
            .collect::<Result<Vec<_>>>()?;
        if let Err(err) = stage_explicit_commit_paths(paths, &selected, intent.source_lane) {
            unstage_selected_paths(paths, &selected);
            restore_rust_owned_files(paths, intent.deferred_file_snapshots)
                .context("restore Rust-owned files after staging failure")?;
            return Err(err);
        }
        if let Err(err) = validate_staged_fingerprints(paths, &approved_fingerprints) {
            unstage_selected_paths(paths, &selected);
            restore_rust_owned_files(paths, intent.deferred_file_snapshots)
                .context("restore Rust-owned files after staged-content validation failure")?;
            return Err(err).context("validate approved staged commit content");
        }
        let final_worktree_fingerprints = selected
            .iter()
            .map(|path| fingerprint_pending_path(paths, path))
            .collect::<Result<Vec<_>>>();
        if let Err(err) = final_worktree_fingerprints.and_then(|fingerprints| {
            if fingerprints == approved_fingerprints {
                Ok(())
            } else {
                bail!("approved worktree content, type, or Git mode changed before commit")
            }
        }) {
            unstage_selected_paths(paths, &selected);
            restore_rust_owned_files(paths, intent.deferred_file_snapshots)
                .context("restore Rust-owned files after final worktree validation failure")?;
            return Err(err);
        }
        let sha = match publish_prevalidated_index_commit(
            paths,
            &base_head,
            intent.message,
            &approved_fingerprints,
        ) {
            Ok(sha) => sha,
            Err(err) => {
                unstage_selected_paths(paths, &selected);
                restore_rust_owned_files(paths, intent.deferred_file_snapshots)
                    .context("restore Rust-owned files after commit publication failure")?;
                return Err(err)
                    .with_context(|| format!("publish Git commit {}", intent.source_lane));
            }
        };
        validate_committed_fingerprints(
            paths,
            &sha,
            &base_head,
            intent.message,
            &approved_fingerprints,
        )
        .with_context(|| {
            format!(
                "Git commit {} succeeded but did not match the approved fingerprint set",
                truncate_inline(&sha, 12)
            )
        })?;
        let _ = append_event(
            paths,
            OrchestratorEventKind::CommitCreated,
            intent.task_id,
            json!({"commit_sha": sha, "message": intent.message, "mode": intent.source_lane}),
        );
        return Ok(Some(sha));
    }

    let preparation = (|| -> Result<_> {
        ensure_no_pending_commit(state)?;
        ensure_index_empty(paths)?;
        let current_dirty = current_commit_paths(paths)?;
        let selected_set = selected.iter().cloned().collect::<BTreeSet<_>>();
        let dirty_set = current_dirty.iter().cloned().collect::<BTreeSet<_>>();
        if selected_set != dirty_set {
            let outside = dirty_set
                .difference(&selected_set)
                .cloned()
                .collect::<Vec<_>>();
            let not_dirty = selected_set
                .difference(&dirty_set)
                .cloned()
                .collect::<Vec<_>>();
            bail!(
                "manual pending commit path set mismatch; outside selected: [{}]; selected but clean: [{}]",
                outside.join(", "),
                not_dirty.join(", ")
            );
        }
        let base_head = git_head(paths).unwrap_or_else(|| "unborn".to_string());
        let mut path_fingerprints = selected
            .iter()
            .map(|path| fingerprint_pending_path(paths, path))
            .collect::<Result<Vec<_>>>()?;
        path_fingerprints.sort_by(|a, b| a.path.cmp(&b.path));
        let deferred_file_writes =
            prepare_deferred_file_writes(paths, &selected, intent.deferred_file_snapshots)?;
        Ok((base_head, path_fingerprints, deferred_file_writes))
    })();
    let restore = restore_rust_owned_files(paths, intent.deferred_file_snapshots);
    let (base_head, path_fingerprints, deferred_file_writes) = match (preparation, restore) {
        (Ok(prepared), Ok(())) => prepared,
        (Err(err), Ok(())) => return Err(err),
        (Ok(_), Err(restore_err)) => {
            return Err(restore_err).context("restore Rust-owned files while creating manual intent")
        }
        (Err(err), Err(restore_err)) => {
            return Err(err).context(format!(
                "manual intent validation failed and Rust-owned files could not be restored: {restore_err:#}"
            ))
        }
    };

    let created_at = Utc::now().to_rfc3339();
    let imported = intent.task_id.and_then(|id| state.imported_tasks.get(id));
    let deferred_transition = if intent.defer_task_completion {
        intent.task_id.map(|id| {
            json!({
                "schema": "codex-team.deferred_task_completion.v1",
                "task_id": id,
                "completed_tasks_before": state.completed_tasks,
                "set_master_due": cfg.master_every_completed_tasks > 0
                    && state
                        .completed_tasks
                        .saturating_add(1)
                        .is_multiple_of(cfg.master_every_completed_tasks)
                    && state.queue.is_empty()
                    && state.senior_batch_approval.is_none(),
            })
        })
    } else {
        None
    };
    let mut pending = PendingCommit {
        id: String::new(),
        digest: String::new(),
        source_lane: intent.source_lane.to_string(),
        task_id: intent.task_id.map(str::to_string),
        import_digest: imported.map(|record| record.import_digest.clone()),
        base_head,
        dirty_paths: selected,
        path_fingerprints,
        deferred_file_writes,
        commit_message: intent.message.to_string(),
        guard_evidence_ids: intent.guard_evidence_ids,
        verification_evidence_ids: intent.verification_evidence_ids,
        deferred_transition,
        created_at,
    };
    pending.digest = pending_commit_digest(&pending)?;
    pending.id = format!("pending-{}", &pending.digest[..16]);
    let pending_id = pending.id.clone();
    let pending_digest = pending.digest.clone();
    state.pending_commit = Some(pending);
    if let Err(err) = append_event(
        paths,
        OrchestratorEventKind::StateTransitionApplied,
        intent.task_id,
        json!({
            "transition": "pending_manual_commit_created",
            "pending_id": pending_id,
            "digest": pending_digest,
            "source_lane": intent.source_lane,
        }),
    ) {
        state.pending_commit = None;
        return Err(err).context("record pending manual commit creation");
    }
    Ok(None)
}

#[cfg(test)]
fn apply_pending_commit(
    paths: &Paths,
    state: &mut State,
    expected_id: &str,
    expected_digest: &str,
) -> Result<String> {
    if let Some(commit) =
        reconcile_completed_pending_commit(paths, state, expected_id, expected_digest)?
    {
        return Ok(commit);
    }
    apply_pending_commit_with_approved_revision(paths, state, expected_id, expected_digest, None)
}

pub(crate) fn apply_pending_commit_for_revision(
    paths: &Paths,
    state: &mut State,
    expected_id: &str,
    expected_digest: &str,
    approved_revision: &str,
) -> Result<String> {
    apply_pending_commit_with_approved_revision(
        paths,
        state,
        expected_id,
        expected_digest,
        Some(approved_revision),
    )
}

fn apply_pending_commit_with_approved_revision(
    paths: &Paths,
    state: &mut State,
    expected_id: &str,
    expected_digest: &str,
    approved_revision: Option<&str>,
) -> Result<String> {
    let pending = state
        .pending_commit
        .clone()
        .ok_or_else(|| anyhow::anyhow!("no pending commit exists"))?;
    if pending.id != expected_id {
        bail!("pending commit ID is stale or does not match");
    }
    if pending.digest != expected_digest {
        bail!("pending commit digest is stale or does not match");
    }
    let recomputed_digest = pending_commit_digest(&pending)?;
    if recomputed_digest != pending.digest
        || pending.id != format!("pending-{}", &recomputed_digest[..16])
    {
        bail!("pending commit state was modified after intent creation");
    }
    let deferred_transition = validate_deferred_task_completion(state, &pending)?;
    ensure_index_empty(paths)?;
    let head = git_head(paths).unwrap_or_else(|| "unborn".to_string());
    if head != pending.base_head {
        bail!("pending commit base HEAD changed");
    }
    validate_preapproval_worktree(paths, &pending)?;
    let mut state_after = state.clone();
    if let Some(transition) = deferred_transition.clone() {
        apply_deferred_task_completion(&mut state_after, transition);
    }
    state_after.pending_commit = None;
    let mut recovery = PendingCommitRecoveryV1 {
        schema: "codex-team.pending_commit_recovery.v1".to_string(),
        pending_id: pending.id.clone(),
        pending_digest: pending.digest.clone(),
        approved_revision: approved_revision.map(str::to_string),
        base_head: pending.base_head.clone(),
        dirty_paths: pending.dirty_paths.clone(),
        path_fingerprints: pending.path_fingerprints.clone(),
        commit_message: pending.commit_message.clone(),
        prepared_at: Utc::now().to_rfc3339(),
        commit_sha: None,
        state_after,
    };
    write_pending_commit_recovery(paths, &recovery)?;
    if let Err(err) = apply_deferred_file_writes(paths, &pending.deferred_file_writes) {
        let restore = restore_deferred_file_writes(paths, &pending.deferred_file_writes);
        remove_pending_commit_recovery(paths);
        return match restore {
            Ok(()) => Err(err).context("apply Rust-owned pending files"),
            Err(restore_err) => Err(err).context(format!(
                "apply Rust-owned pending files; restoration also failed: {restore_err:#}"
            )),
        };
    }
    let final_validation = (|| -> Result<()> {
        if current_commit_paths(paths)? != pending.dirty_paths {
            bail!("pending commit final dirty path set changed");
        }
        let fingerprints = pending
            .dirty_paths
            .iter()
            .map(|path| fingerprint_pending_path(paths, path))
            .collect::<Result<Vec<_>>>()?;
        if fingerprints != pending.path_fingerprints {
            bail!("pending commit final file content, type, or Git mode changed");
        }
        Ok(())
    })();
    if let Err(err) = final_validation {
        let restore = restore_deferred_file_writes(paths, &pending.deferred_file_writes);
        remove_pending_commit_recovery(paths);
        return match restore {
            Ok(()) => Err(err),
            Err(restore_err) => Err(err).context(format!(
                "restore Rust-owned files after final validation failure: {restore_err:#}"
            )),
        };
    }

    if let Err(err) =
        stage_explicit_commit_paths(paths, &pending.dirty_paths, "manual pending commit")
    {
        unstage_selected_paths(paths, &pending.dirty_paths);
        let restore = restore_deferred_file_writes(paths, &pending.deferred_file_writes);
        remove_pending_commit_recovery(paths);
        return match restore {
            Ok(()) => Err(err).context("stage pending manual intent"),
            Err(restore_err) => Err(err).context(format!(
                "stage pending manual intent; Rust-owned file restoration failed: {restore_err:#}"
            )),
        };
    }
    if let Err(err) = validate_staged_fingerprints(paths, &pending.path_fingerprints) {
        unstage_selected_paths(paths, &pending.dirty_paths);
        let restore = restore_deferred_file_writes(paths, &pending.deferred_file_writes);
        remove_pending_commit_recovery(paths);
        return match restore {
            Ok(()) => Err(err).context("validate staged pending manual intent"),
            Err(restore_err) => Err(err).context(format!(
                "validate staged pending manual intent; Rust-owned file restoration failed: {restore_err:#}"
            )),
        };
    }
    let sha = match publish_prevalidated_index_commit(
        paths,
        &pending.base_head,
        &pending.commit_message,
        &pending.path_fingerprints,
    ) {
        Ok(sha) => sha,
        Err(err) => {
            unstage_selected_paths(paths, &pending.dirty_paths);
            restore_deferred_file_writes(paths, &pending.deferred_file_writes).context(
                "Git commit could not be published and Rust-owned pending files could not be restored",
            )?;
            remove_pending_commit_recovery(paths);
            return Err(err).context("publish pending manual intent");
        }
    };
    recovery.commit_sha = Some(sha.clone());
    if let Err(err) = write_pending_commit_recovery(paths, &recovery) {
        eprintln!(
            "[Recovery] Commit {} succeeded, but recovery evidence could not be finalized: {err:#}",
            truncate_inline(&sha, 12)
        );
    }
    validate_committed_fingerprints(
        paths,
        &sha,
        &pending.base_head,
        &pending.commit_message,
        &pending.path_fingerprints,
    )
    .with_context(|| {
        format!(
            "Git commit {} succeeded but did not match the approved pending intent; recovery evidence was retained",
            truncate_inline(&sha, 12)
        )
    })?;
    *state = recovery.state_after.clone();
    let _ = append_event(
        paths,
        OrchestratorEventKind::CommitCreated,
        pending.task_id.as_deref(),
        json!({
            "commit_sha": sha,
            "message": pending.commit_message,
            "mode": "manual_pending_commit",
            "pending_id": pending.id,
        }),
    );
    Ok(sha)
}

pub(crate) struct AcceptedWorkerCommitRequest<'a> {
    pub(crate) task: &'a TaskSpec,
    pub(crate) guard_report: &'a GuardReport,
    pub(crate) message: &'a str,
    pub(crate) rust_owned_file_snapshots: &'a [RustOwnedFileSnapshot],
    pub(crate) verification_evidence_id: &'a str,
}

pub(crate) fn commit_accepted_worker_task(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    request: AcceptedWorkerCommitRequest<'_>,
) -> Result<Option<String>> {
    // Rust commits exactly the verified/allowed patch.  If Git is unavailable
    // this degrades to "accepted but uncommitted" rather than failing local usage.
    if find_git_root(&paths.root).is_none() {
        return Ok(None);
    }
    let task = request.task;
    let imported = state.imported_tasks.get(&task.task_id);
    let mut allowed_patterns = expand_allowed_file_patterns(&task.allowed_files);
    if imported.is_none() {
        allowed_patterns.push("NEXT_STEPS.md".to_string());
    }
    allowed_patterns.sort();
    allowed_patterns.dedup();
    let allowed = compile_globs(&allowed_patterns)?;

    let mut paths_to_add = request.guard_report.changed_files.clone();
    if imported.is_none() {
        paths_to_add.push("NEXT_STEPS.md".to_string());
    }
    paths_to_add.extend(
        request
            .rust_owned_file_snapshots
            .iter()
            .map(|snapshot| snapshot.path.clone()),
    );
    // Commit only the verified Worker/guard delta plus queue/tracking docs.
    // Pre-existing allowed dirty paths stay unstaged until explicit adoption.
    paths_to_add = concrete_dirty_commit_paths(paths, &paths_to_add, &allowed)?;
    paths_to_add.sort();
    paths_to_add.dedup();

    let disallowed = paths_to_add
        .iter()
        .filter(|path| !allowed.is_match(path.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !disallowed.is_empty() {
        bail!(
            "refusing Rust-owned commit for {}; disallowed changed files: {}",
            task.task_id,
            disallowed.join(", ")
        );
    }

    let result = coordinate_commit(
        paths,
        cfg,
        state,
        CommitIntent {
            source_lane: "accepted_worker_task",
            task_id: Some(&task.task_id),
            message: request.message,
            files: &paths_to_add,
            guard_evidence_ids: vec![format!("guard:{}", task.task_id)],
            verification_evidence_ids: vec![request.verification_evidence_id.to_string()],
            deferred_file_snapshots: request.rust_owned_file_snapshots,
            defer_task_completion: true,
        },
    )?;
    if cfg.commit_mode == CommitMode::Manual
        && state.pending_commit.is_some()
        && !state
            .queue
            .iter()
            .any(|queued| queued.task_id == task.task_id)
    {
        state.queue.insert(0, task.clone());
    }
    Ok(result)
}

pub(crate) fn execute_rust_commit_request(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    request: &RustCommitRequest,
    source: &str,
) -> Result<Option<String>> {
    if find_git_root(&paths.root).is_none() {
        return Ok(None);
    }
    if request.allowed_files.is_empty() {
        bail!(
            "Rust commit request {} has no allowed files",
            request.request_id
        );
    }
    if request.commit_message.trim().is_empty() {
        bail!(
            "Rust commit request {} has an empty commit message",
            request.request_id
        );
    }

    let allowed_patterns = expand_allowed_file_patterns(&request.allowed_files);
    let allowed = compile_globs(&allowed_patterns)?;
    let selected_paths = selected_rust_commit_paths(paths, &allowed)?;
    if selected_paths.is_empty() {
        bail!(
            "Rust commit request {} matched no dirty allowed files",
            request.request_id
        );
    }

    run_git_diff_check_for_selected_commit(paths, &selected_paths, &request.request_id)?;
    let verification_report = run_rust_commit_request_verification(paths, cfg, request, source)?;
    if let Some(report) = verification_report.as_ref() {
        if !rust_commit_request_report_is_clean(report) {
            bail!(
                "Rust commit request {} verification failed with status {}",
                request.request_id,
                report.status
            );
        }
    }

    let verification_ids = verification_report
        .as_ref()
        .map(|report| vec![report.report_id.clone()])
        .unwrap_or_default();
    let source_lane = format!("{source}.role_authorized_rust_commit");
    let commit_sha = coordinate_commit(
        paths,
        cfg,
        state,
        CommitIntent {
            source_lane: &source_lane,
            task_id: Some(&request.request_id),
            message: request.commit_message.trim(),
            files: &selected_paths,
            guard_evidence_ids: Vec::new(),
            verification_evidence_ids: verification_ids,
            deferred_file_snapshots: &[],
            defer_task_completion: false,
        },
    )?;
    if let Some(sha) = commit_sha.as_deref() {
        record_role_authorized_commit_route(
            paths,
            request,
            source,
            sha,
            verification_report.as_ref(),
        )?;
        let _ = record_rust_model_skip(
            paths,
            "worker",
            Some(&request.request_id),
            cfg,
            "role_authorized_rust_commit",
        );
    }
    Ok(commit_sha)
}

pub(crate) fn run_rust_commit_request_verification(
    paths: &Paths,
    cfg: &Config,
    request: &RustCommitRequest,
    source: &str,
) -> Result<Option<EvidenceReport>> {
    let mut commands = Vec::new();
    for (index, command_text) in request.verification_commands.iter().enumerate() {
        let trimmed = command_text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(command) = parse_verification_command(trimmed, "rust.commit_request") else {
            bail!(
                "Rust commit request {} has unsafe verification command: {}",
                request.request_id,
                trimmed
            );
        };
        if !is_deterministic_command_structured_allowed(&command) {
            bail!(
                "Rust commit request {} has disallowed verification command: {}",
                request.request_id,
                command.display()
            );
        }
        commands.push(with_stable_command_metadata(
            &request.request_id,
            "rust-commit-request",
            index,
            command,
        ));
    }
    if commands.is_empty() {
        return Ok(None);
    }

    let plan_fingerprint = commands
        .iter()
        .map(VerificationCommand::fingerprint)
        .collect::<Vec<_>>()
        .join("\n");
    let mut plan = VerificationPlan {
        plan_id: format!(
            "{}-{}",
            sanitize_filename(&request.request_id),
            &stable_hash_hex(&plan_fingerprint)[..12]
        ),
        task_id: clean_progress_action_id(&request.request_id),
        parent_task: None,
        source: format!("{source}.rust_commit_request"),
        commands,
        rejected_commands: Vec::new(),
    };
    let envelope = rust_commit_request_task_envelope(request);
    reject_disallowed_swiftstacker_report_roots(paths, &envelope, &mut plan)?;
    let report = crate::app::execute_verification_plan_with_policy(
        paths,
        &plan,
        cfg.requires_verification_isolation(),
    )?;
    persist_evidence_report(paths, &report)?;
    Ok(Some(report))
}

pub(crate) fn rust_commit_request_report_is_clean(report: &EvidenceReport) -> bool {
    report.status == "passed"
        && report.rejected_commands.is_empty()
        && report
            .commands
            .iter()
            .all(|command| command.category == "passed")
}

pub(crate) fn rust_commit_request_task_envelope(request: &RustCommitRequest) -> TaskSpec {
    TaskSpec {
        task_id: clean_progress_action_id(&request.request_id),
        parent_task: None,
        status: "approved".to_string(),
        priority: "normal".to_string(),
        review_tier: ReviewTier::AiReviewOnFailure,
        automation_level: AutomationLevel::AiWorkerRustReview,
        dependencies: Vec::new(),
        title: request.title.clone(),
        kind: "rust_commit_request".to_string(),
        objective: request.summary.clone(),
        why_unblocks_progress: request.summary.clone(),
        allowed_files: request.allowed_files.clone(),
        read_only_files: Vec::new(),
        forbidden_actions: vec!["Do not run git directly from an AI role.".to_string()],
        exact_steps: vec![request.summary.clone()],
        success_criteria: vec!["Rust creates the requested commit.".to_string()],
        tests_to_run: request.verification_commands.clone(),
        stop_conditions: request.stop_conditions.clone(),
        discovery_budget:
            "Use only the named allowed files and deterministic verification commands.".to_string(),
        estimated_minutes: 10,
        requires_network: false,
        prefer_worktree: None,
    }
}

pub(crate) fn record_role_authorized_commit_route(
    paths: &Paths,
    request: &RustCommitRequest,
    source: &str,
    commit_sha: &str,
    report: Option<&EvidenceReport>,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let duration_ms = report.map(|report| {
        report
            .commands
            .iter()
            .map(|command| command.duration_ms)
            .sum()
    });
    record_route_attempt(
        paths,
        &RouteAttempt {
            id: route_attempt_id(
                "rust_commit_request",
                Some(&request.request_id),
                ExecutionLane::RustBookkeeping,
            ),
            task_id: Some(request.request_id.clone()),
            attempt: 1,
            status: RouteLifecycleStatus::Succeeded,
            started_at: report
                .map(|report| report.started_at.clone())
                .unwrap_or_else(|| now.clone()),
            finished_at: Some(now),
            execution_lane: ExecutionLane::RustBookkeeping,
            route_kind: format!("{source}.rust_commit_request"),
            model_tier: Some("rust".to_string()),
            model: None,
            prompt_receipt_id: None,
            evidence_receipt_id: report.map(|report| report.report_id.clone()),
            failure_fingerprint: None,
            token_usage: TokenUsage::default(),
            scope_group: Some(request.allowed_files.join("|")),
            scope_overlap: false,
            final_feedback: Some(RouteFeedback {
                outcome: "committed".to_string(),
                summary: format!(
                    "Rust committed role-authorized request {}; commit {}.",
                    request.request_id,
                    truncate_inline(commit_sha, 12)
                ),
                regret_kind: None,
                better_lane: None,
                verification_status: report.map(|report| report.status.clone()),
                commit_sha: Some(commit_sha.to_string()),
                duration_ms,
            }),
        },
    )
}

pub(crate) fn commit_rust_patch_task(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    task: &TaskSpec,
    queue_path: &Path,
    patch_files: &[String],
    queue_before: &RustOwnedFileSnapshot,
) -> Result<Option<String>> {
    if find_git_root(&paths.root).is_none() {
        return Ok(None);
    }
    let queue_rel = crate::app::relative_slash_path(&paths.root, queue_path)
        .unwrap_or_else(|| queue_path.to_string_lossy().to_string());
    let mut files = patch_files.to_vec();
    files.push(queue_rel.clone());
    files.sort();
    files.dedup();
    let allowed = compile_globs(&expand_allowed_file_patterns(&task.allowed_files))?;
    let disallowed = patch_files
        .iter()
        .filter(|path| !allowed.is_match(path.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !disallowed.is_empty() {
        bail!(
            "refusing Rust patch commit for {}; disallowed changed files: {}",
            task.task_id,
            disallowed.join(", ")
        );
    }
    coordinate_commit(
        paths,
        cfg,
        state,
        CommitIntent {
            source_lane: "rust_patch_task",
            task_id: Some(&task.task_id),
            message: &format!("Apply {} non-AI patch", task.task_id),
            files: &files,
            guard_evidence_ids: Vec::new(),
            verification_evidence_ids: Vec::new(),
            deferred_file_snapshots: std::slice::from_ref(queue_before),
            defer_task_completion: true,
        },
    )
}

pub(crate) fn commit_control_doc_edits(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    message: &str,
) -> Result<Option<String>> {
    if find_git_root(&paths.root).is_none() {
        return Ok(None);
    }
    let allowed = control_doc_commit_allowlist();
    let files = git_dirty_name_only(paths)?
        .into_iter()
        .filter(|path| allowed.contains(path))
        .collect::<Vec<_>>();
    commit_selected_control_doc_edits(paths, cfg, state, message, files, "ceo_control_docs")
}

pub(crate) fn commit_selected_control_doc_edits(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    message: &str,
    mut files: Vec<String>,
    mode: &str,
) -> Result<Option<String>> {
    if find_git_root(&paths.root).is_none() {
        return Ok(None);
    }
    let allowed = control_doc_commit_allowlist();
    files.sort();
    files.dedup();
    if files.iter().any(|path| !allowed.contains(path)) {
        bail!("selected control-doc commit includes non-control-doc file");
    }
    if files.is_empty() {
        return Ok(None);
    }
    let check = Command::new("git")
        .args(["diff", "--check", "--"])
        .args(&files)
        .current_dir(&paths.root)
        .status()
        .context("git diff --check control docs")?;
    if !check.success() {
        bail!("git diff --check failed for CEO control docs");
    }
    coordinate_commit(
        paths,
        cfg,
        state,
        CommitIntent {
            source_lane: mode,
            task_id: None,
            message,
            files: &files,
            guard_evidence_ids: Vec::new(),
            verification_evidence_ids: Vec::new(),
            deferred_file_snapshots: &[],
            defer_task_completion: false,
        },
    )
}

pub(crate) fn try_commit_control_doc_edits_with_log(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    message: &str,
) {
    match commit_control_doc_edits(paths, cfg, state, message) {
        Ok(Some(sha)) => {
            println!(
                "[Rust] Committed control-doc updates: {}",
                truncate_inline(&sha, 12)
            );
            let _ = append_event(
                paths,
                OrchestratorEventKind::StateTransitionApplied,
                None,
                json!({
                    "mode": "control_doc_commit",
                    "commit_sha": sha,
                }),
            );
        }
        Ok(None) => {}
        Err(err) => {
            println!(
                "[Rust] Control-doc commit skipped: {}",
                truncate_inline(&format!("{err:#}"), 180)
            );
        }
    }
}

pub(crate) fn commit_deterministic_task(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    task: &TaskSpec,
    queue_path: &Path,
    queue_before: &RustOwnedFileSnapshot,
) -> Result<Option<String>> {
    if find_git_root(&paths.root).is_none() {
        return Ok(None);
    }
    let queue_rel = crate::app::relative_slash_path(&paths.root, queue_path)
        .unwrap_or_else(|| queue_path.to_string_lossy().to_string());
    coordinate_commit(
        paths,
        cfg,
        state,
        CommitIntent {
            source_lane: "deterministic_task",
            task_id: Some(&task.task_id),
            message: &format!("Complete {} mechanically", task.task_id),
            files: std::slice::from_ref(&queue_rel),
            guard_evidence_ids: Vec::new(),
            verification_evidence_ids: Vec::new(),
            deferred_file_snapshots: std::slice::from_ref(queue_before),
            defer_task_completion: true,
        },
    )
}

pub(crate) fn commit_queue_bookkeeping(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    queue_path: &Path,
    message: &str,
    queue_before: &RustOwnedFileSnapshot,
) -> Result<Option<String>> {
    commit_queue_bookkeeping_inner(paths, cfg, state, queue_path, message, queue_before, None)
}

pub(crate) fn commit_completed_queue_bookkeeping(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    task: &TaskSpec,
    queue_path: &Path,
    message: &str,
    queue_before: &RustOwnedFileSnapshot,
) -> Result<Option<String>> {
    commit_queue_bookkeeping_inner(
        paths,
        cfg,
        state,
        queue_path,
        message,
        queue_before,
        Some(&task.task_id),
    )
}

#[allow(clippy::too_many_arguments)]
fn commit_queue_bookkeeping_inner(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    queue_path: &Path,
    message: &str,
    queue_before: &RustOwnedFileSnapshot,
    completed_task_id: Option<&str>,
) -> Result<Option<String>> {
    if find_git_root(&paths.root).is_none() {
        return Ok(None);
    }
    let queue_rel = crate::app::relative_slash_path(&paths.root, queue_path)
        .unwrap_or_else(|| queue_path.to_string_lossy().to_string());
    coordinate_commit(
        paths,
        cfg,
        state,
        CommitIntent {
            source_lane: "queue_bookkeeping",
            task_id: completed_task_id,
            message,
            files: std::slice::from_ref(&queue_rel),
            guard_evidence_ids: Vec::new(),
            verification_evidence_ids: Vec::new(),
            deferred_file_snapshots: std::slice::from_ref(queue_before),
            defer_task_completion: completed_task_id.is_some(),
        },
    )
}

pub(crate) fn handle_pm_commit_requests(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    plan: &crate::app::ProjectManagerOutput,
) -> Result<usize> {
    let mut committed = 0usize;
    for request in &plan.commit_requests {
        match execute_rust_commit_request(paths, cfg, state, request, "project_manager") {
            Ok(Some(sha)) => {
                committed += 1;
                let summary = format!(
                    "Rust committed Project Manager request {}: {}.",
                    request.request_id,
                    truncate_inline(&sha, 12)
                );
                println!("[Rust] {summary}");
                state.last_review_summary = Some(summary.clone());
                state.last_worker_summary = Some(summary.clone());
                push_event(state, "deterministic", "pm-rust-commit", &summary);
                append_event(
                    paths,
                    OrchestratorEventKind::StateTransitionApplied,
                    Some(&request.request_id),
                    json!({
                        "mode": "pm_role_authorized_rust_commit",
                        "commit_sha": sha,
                    }),
                )?;
            }
            Ok(None) if state.pending_commit.is_some() => {
                let summary = format!(
                    "Project Manager request {} is paused for explicit commit approval.",
                    request.request_id
                );
                state.last_review_summary = Some(summary.clone());
                push_event(state, "deterministic", "pm-rust-commit-pending", &summary);
                break;
            }
            Ok(None) => {}
            Err(err) => {
                let summary = format!(
                    "Project Manager Rust commit request {} rejected before commit: {}",
                    request.request_id,
                    truncate_inline(&format!("{err:#}"), 220)
                );
                println!("[Rust] {summary}");
                state.last_review_summary = Some(summary.clone());
                state.last_worker_summary = Some(
                    "Worker skipped because a Project Manager Rust commit request was rejected."
                        .to_string(),
                );
                push_event(state, "policy", "pm-rust-commit-rejected", &summary);
                append_event(
                    paths,
                    OrchestratorEventKind::PolicyViolation,
                    Some(&request.request_id),
                    json!({
                        "mode": "pm_role_authorized_rust_commit_rejected",
                        "reason": format!("{err:#}"),
                    }),
                )?;
                record_rust_model_skip(
                    paths,
                    "worker",
                    Some(&request.request_id),
                    cfg,
                    "pm_role_authorized_rust_commit_rejected",
                )?;
            }
        }
    }
    Ok(committed)
}

pub(crate) fn try_auto_commit_pm_control_doc_reconciliation(
    paths: &Paths,
    cfg: &Config,
    state: &mut State,
    candidate: &TaskSpec,
) -> Result<bool> {
    if !candidate
        .allowed_files
        .iter()
        .all(|path| is_tracking_doc_path(path))
    {
        return Ok(false);
    }
    let allowed = candidate
        .allowed_files
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let dirty = git_dirty_name_only(paths)?;
    if dirty.is_empty() || dirty.iter().any(|path| !allowed.contains(path)) {
        return Ok(false);
    }
    let files = dirty
        .into_iter()
        .filter(|path| allowed.contains(path))
        .collect::<Vec<_>>();
    if files.is_empty() {
        return Ok(false);
    }
    match commit_selected_control_doc_edits(
        paths,
        cfg,
        state,
        &format!("Reconcile PM control docs for {}", candidate.task_id),
        files,
        "pm_control_doc_reconciliation",
    ) {
        Ok(Some(sha)) => {
            let summary = format!(
                "Rust committed PM control-doc reconciliation {} without Senior/Worker; commit {}.",
                candidate.task_id,
                truncate_inline(&sha, 12)
            );
            println!("[Rust] {summary}");
            state.last_review_summary = Some(summary.clone());
            state.last_worker_summary = Some(summary.clone());
            push_event(
                state,
                "deterministic",
                "pm-control-doc-reconciliation",
                &summary,
            );
            record_rust_model_skip(
                paths,
                "foreman-review",
                Some(&candidate.task_id),
                cfg,
                "pm_control_doc_reconciliation_auto_committed",
            )?;
            record_rust_model_skip(
                paths,
                "worker",
                Some(&candidate.task_id),
                cfg,
                "pm_control_doc_reconciliation_auto_committed",
            )?;
            append_event(
                paths,
                OrchestratorEventKind::StateTransitionApplied,
                Some(&candidate.task_id),
                json!({
                    "mode": "pm_control_doc_reconciliation_auto_committed",
                    "commit_sha": sha,
                }),
            )?;
            Ok(true)
        }
        Ok(None) => Ok(state.pending_commit.is_some()),
        Err(err) => {
            let summary = format!(
                "PM control-doc auto-commit skipped; falling back to Senior gate: {}",
                truncate_inline(&format!("{err:#}"), 180)
            );
            println!("[Rust] {summary}");
            append_event(
                paths,
                OrchestratorEventKind::PolicyViolation,
                Some(&candidate.task_id),
                json!({
                    "mode": "pm_control_doc_reconciliation_auto_commit_failed",
                    "reason": format!("{err:#}"),
                }),
            )?;
            Ok(false)
        }
    }
}

#[cfg(test)]
mod manual_commit_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    type PendingMutation = (&'static str, Box<dyn Fn(&mut PendingCommit)>);

    fn temp_repo(label: &str) -> (PathBuf, Paths) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("codex-team-{label}-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.name", "Codex Team Test"],
            vec!["config", "user.email", "codex-team@example.invalid"],
        ] {
            assert!(Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success());
        }
        fs::write(root.join("NEXT_STEPS.md"), "# NEXT_STEPS\n\ninitial\n").unwrap();
        fs::write(root.join("src.txt"), "initial\n").unwrap();
        assert!(Command::new("git")
            .args(["add", "NEXT_STEPS.md", "src.txt"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-q", "-m", "initial"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        let paths = Paths::new(&root).unwrap();
        fs::create_dir_all(&paths.orch).unwrap();
        (root, paths)
    }

    fn task() -> TaskSpec {
        TaskSpec {
            task_id: "TEST-1".to_string(),
            parent_task: None,
            status: "approved".to_string(),
            priority: "normal".to_string(),
            review_tier: ReviewTier::RustOnly,
            automation_level: AutomationLevel::RustOnly,
            dependencies: Vec::new(),
            title: "test task".to_string(),
            kind: "test".to_string(),
            objective: "exercise commit lane".to_string(),
            why_unblocks_progress: "characterization".to_string(),
            allowed_files: vec!["src.txt".to_string(), "NEXT_STEPS.md".to_string()],
            read_only_files: Vec::new(),
            forbidden_actions: vec!["no scope expansion".to_string()],
            exact_steps: vec!["edit".to_string()],
            success_criteria: vec!["committed".to_string()],
            tests_to_run: Vec::new(),
            stop_conditions: vec!["stop on error".to_string()],
            discovery_budget: "named files".to_string(),
            estimated_minutes: 1,
            requires_network: false,
            prefer_worktree: None,
        }
    }

    #[test]
    fn default_auto_mode_keeps_queue_commit_behavior() {
        let (root, paths) = temp_repo("auto-commit");
        let before = git_head(&paths).unwrap();
        let queue_before = capture_rust_owned_file(&paths, &paths.next_steps).unwrap();
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\ndone\n").unwrap();
        let cfg = Config::default();
        let mut state = State::default();

        let committed = commit_queue_bookkeeping(
            &paths,
            &cfg,
            &mut state,
            &paths.next_steps,
            "complete queue item",
            &queue_before,
        )
        .unwrap()
        .unwrap();

        assert_ne!(before, committed);
        assert!(state.pending_commit.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concrete_commit_paths_prefer_git_spelling_over_case_aliases() {
        let (root, paths) = temp_repo("commit-path-case-alias");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/ROADMAP.md"), "initial\n").unwrap();
        assert!(Command::new("git")
            .args(["add", "docs/ROADMAP.md"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-q", "-m", "add roadmap"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        fs::write(root.join("docs/ROADMAP.md"), "changed\n").unwrap();

        let allowed =
            compile_globs(&["docs/ROADMAP.md".to_string(), "Docs/ROADMAP.md".to_string()]).unwrap();
        let selected = concrete_dirty_commit_paths(
            &paths,
            &["Docs/ROADMAP.md".to_string(), "docs/ROADMAP.md".to_string()],
            &allowed,
        )
        .unwrap();

        assert_eq!(selected, vec!["docs/ROADMAP.md"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn default_auto_mode_commits_accepted_worker_lane() {
        let (root, paths) = temp_repo("auto-worker");
        let queue_before = capture_rust_owned_file(&paths, &paths.next_steps).unwrap();
        fs::write(root.join("src.txt"), "worker edit\n").unwrap();
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\ndone\n").unwrap();
        let cfg = Config::default();
        let mut state = State::default();
        let task = task();
        let guard = GuardReport {
            enabled: true,
            status: "passed".to_string(),
            changed_files: vec!["src.txt".to_string()],
            allowed_patterns: task.allowed_files.clone(),
            ignored_patterns: Vec::new(),
            violations: Vec::new(),
        };

        let sha = commit_accepted_worker_task(
            &paths,
            &cfg,
            &mut state,
            AcceptedWorkerCommitRequest {
                task: &task,
                guard_report: &guard,
                message: "accept worker",
                rust_owned_file_snapshots: &[queue_before],
                verification_evidence_id: "evidence-worker",
            },
        )
        .unwrap();

        assert!(sha.is_some());
        assert!(state.pending_commit.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn default_auto_mode_commits_role_authorized_lane() {
        let (root, paths) = temp_repo("auto-role-request");
        fs::write(root.join("src.txt"), "role edit\n").unwrap();
        let cfg = Config::default();
        let mut state = State::default();
        let request = RustCommitRequest {
            request_id: "REQ-1".to_string(),
            title: "request".to_string(),
            summary: "commit source".to_string(),
            allowed_files: vec!["src.txt".to_string()],
            verification_commands: Vec::new(),
            commit_message: "role authorized commit".to_string(),
            stop_conditions: vec!["stop on error".to_string()],
        };

        let sha =
            execute_rust_commit_request(&paths, &cfg, &mut state, &request, "project_manager")
                .unwrap();

        assert!(sha.is_some());
        assert!(state.pending_commit.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn auto_commit_disables_hooks_and_preserves_exact_approved_tree() {
        let (root, paths) = temp_repo("auto-hooks-disabled");
        let before = git_head(&paths).unwrap();
        fs::write(root.join("src.txt"), "approved auto bytes\n").unwrap();
        let hook = root.join(".git/hooks/pre-commit");
        fs::write(
            &hook,
            "#!/bin/sh\nprintf invoked > hook-ran\nprintf hostile > outside.txt\nchmod +x src.txt\ngit add outside.txt src.txt\nexit 1\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&hook).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&hook, permissions).unwrap();
        let post_hook = root.join(".git/hooks/post-commit");
        fs::write(
            &post_hook,
            "#!/bin/sh\nprintf invoked > post-hook-ran\nprintf hostile > post-hook-outside.txt\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&post_hook).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&post_hook, permissions).unwrap();
        let cfg = Config::default();
        let mut state = State::default();
        let files = vec!["src.txt".to_string()];

        let committed = coordinate_commit(
            &paths,
            &cfg,
            &mut state,
            CommitIntent {
                source_lane: "auto-hook-regression",
                task_id: None,
                message: "approved auto commit",
                files: &files,
                guard_evidence_ids: Vec::new(),
                verification_evidence_ids: Vec::new(),
                deferred_file_snapshots: &[],
                defer_task_completion: false,
            },
        )
        .unwrap()
        .unwrap();

        assert_ne!(committed, before);
        assert!(!root.join("hook-ran").exists());
        assert!(!root.join("outside.txt").exists());
        assert!(!root.join("post-hook-ran").exists());
        assert!(!root.join("post-hook-outside.txt").exists());
        let committed_fingerprint = fingerprint_commit_path(&paths, &committed, "src.txt").unwrap();
        assert_eq!(committed_fingerprint.git_mode.as_deref(), Some("100644"));
        assert_eq!(
            committed_fingerprint.content_sha256,
            Some(sha256_hex(b"approved auto bytes\n"))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stateful_clean_filter_cannot_replace_prevalidated_commit_bytes() {
        let (root, paths) = temp_repo("auto-stateful-clean-filter");
        let filter = paths.orch.join("stateful-clean-filter.sh");
        let counter = paths.orch.join("stateful-clean-filter-count");
        fs::write(
            &filter,
            "#!/bin/sh\ncount=.orchestrator/stateful-clean-filter-count\nn=0\nif test -f \"$count\"; then n=$(cat \"$count\"); fi\nn=$((n + 1))\nprintf '%s\\n' \"$n\" > \"$count\"\nif test \"$n\" -le 2; then cat; else cat >/dev/null; printf 'unapproved post-validation filter output\\n'; fi\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&filter).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&filter, permissions).unwrap();
        fs::write(
            root.join(".gitattributes"),
            "src.txt filter=codex-team-stateful-clean\n",
        )
        .unwrap();
        assert!(Command::new("git")
            .args([
                "config",
                "filter.codex-team-stateful-clean.clean",
                filter.to_str().unwrap(),
            ])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args([
                "config",
                "filter.codex-team-stateful-clean.required",
                "true"
            ])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["add", ".gitattributes"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-q", "-m", "configure stateful filter"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        let _ = fs::remove_file(&counter);

        let before = git_head(&paths).unwrap();
        fs::write(root.join("src.txt"), "approved filter bytes\n").unwrap();
        let cfg = Config::default();
        let mut state = State::default();
        let files = vec!["src.txt".to_string()];

        let committed = coordinate_commit(
            &paths,
            &cfg,
            &mut state,
            CommitIntent {
                source_lane: "stateful-clean-filter-regression",
                task_id: None,
                message: "commit prevalidated filter bytes",
                files: &files,
                guard_evidence_ids: Vec::new(),
                verification_evidence_ids: Vec::new(),
                deferred_file_snapshots: &[],
                defer_task_completion: false,
            },
        )
        .unwrap()
        .unwrap();

        assert_ne!(committed, before);
        assert_eq!(fs::read_to_string(&counter).unwrap().trim(), "2");
        assert_eq!(
            fingerprint_commit_path(&paths, &committed, "src.txt")
                .unwrap()
                .content_sha256,
            Some(sha256_hex(b"approved filter bytes\n"))
        );
        assert!(Command::new("git")
            .args(["diff", "--cached", "--quiet", "--exit-code"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clean_filter_content_mismatch_never_advances_head() {
        let (root, paths) = temp_repo("auto-clean-filter-mismatch");
        let filter = paths.orch.join("mismatching-clean-filter.sh");
        fs::write(
            &filter,
            "#!/bin/sh\ncat >/dev/null\nprintf 'unapproved filtered bytes\\n'\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&filter).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&filter, permissions).unwrap();
        fs::write(
            root.join(".gitattributes"),
            "src.txt filter=codex-team-mismatching-clean\n",
        )
        .unwrap();
        assert!(Command::new("git")
            .args([
                "config",
                "filter.codex-team-mismatching-clean.clean",
                filter.to_str().unwrap(),
            ])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args([
                "config",
                "filter.codex-team-mismatching-clean.required",
                "true",
            ])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["add", ".gitattributes"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-q", "-m", "configure mismatching filter"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());

        let before = git_head(&paths).unwrap();
        fs::write(root.join("src.txt"), "approved worktree bytes\n").unwrap();
        let cfg = Config::default();
        let mut state = State::default();
        let files = vec!["src.txt".to_string()];

        let error = coordinate_commit(
            &paths,
            &cfg,
            &mut state,
            CommitIntent {
                source_lane: "clean-filter-mismatch-regression",
                task_id: None,
                message: "must not publish mismatched bytes",
                files: &files,
                guard_evidence_ids: Vec::new(),
                verification_evidence_ids: Vec::new(),
                deferred_file_snapshots: &[],
                defer_task_completion: false,
            },
        )
        .unwrap_err()
        .to_string();

        assert!(
            error.contains("validate approved staged commit content"),
            "unexpected error: {error}"
        );
        assert_eq!(git_head(&paths).as_deref(), Some(before.as_str()));
        assert!(Command::new("git")
            .args(["diff", "--cached", "--quiet", "--exit-code"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert_eq!(
            fs::read_to_string(root.join("src.txt")).unwrap(),
            "approved worktree bytes\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn default_auto_mode_commits_rust_patch_lane() {
        let (root, paths) = temp_repo("auto-rust-patch");
        let queue_before = capture_rust_owned_file(&paths, &paths.next_steps).unwrap();
        fs::write(root.join("src.txt"), "patch edit\n").unwrap();
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\ndone\n").unwrap();
        let cfg = Config::default();
        let mut state = State::default();

        let sha = commit_rust_patch_task(
            &paths,
            &cfg,
            &mut state,
            &task(),
            &paths.next_steps,
            &["src.txt".to_string()],
            &queue_before,
        )
        .unwrap();

        assert!(sha.is_some());
        assert!(state.pending_commit.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn default_auto_mode_commits_control_doc_lane() {
        let (root, paths) = temp_repo("auto-control-doc");
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\nupdated\n").unwrap();
        let cfg = Config::default();
        let mut state = State::default();

        let sha = commit_control_doc_edits(&paths, &cfg, &mut state, "update controls").unwrap();

        assert!(sha.is_some());
        assert!(state.pending_commit.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manual_mode_creates_exact_pending_intent_without_staging() {
        let (root, paths) = temp_repo("manual-pending");
        let before = git_head(&paths).unwrap();
        let before_text = fs::read_to_string(&paths.next_steps).unwrap();
        let queue_before = capture_rust_owned_file(&paths, &paths.next_steps).unwrap();
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\nreview me\n").unwrap();
        let cfg = Config {
            commit_mode: CommitMode::Manual,
            ..Config::default()
        };
        let mut state = State::default();

        let committed = commit_queue_bookkeeping(
            &paths,
            &cfg,
            &mut state,
            &paths.next_steps,
            "complete queue item",
            &queue_before,
        )
        .unwrap();

        assert!(committed.is_none());
        assert_eq!(git_head(&paths).unwrap(), before);
        assert_eq!(fs::read_to_string(&paths.next_steps).unwrap(), before_text);
        assert_eq!(
            state.pending_commit.as_ref().unwrap().dirty_paths,
            vec!["NEXT_STEPS.md"]
        );
        assert!(Command::new("git")
            .args(["diff", "--cached", "--quiet", "--exit-code"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pending_commit_rejects_stale_content_without_mutation() {
        let (root, paths) = temp_repo("manual-stale");
        let before = git_head(&paths).unwrap();
        let queue_before = capture_rust_owned_file(&paths, &paths.next_steps).unwrap();
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\nreview me\n").unwrap();
        let cfg = Config {
            commit_mode: CommitMode::Manual,
            ..Config::default()
        };
        let mut state = State::default();
        commit_queue_bookkeeping(
            &paths,
            &cfg,
            &mut state,
            &paths.next_steps,
            "complete queue item",
            &queue_before,
        )
        .unwrap();
        let pending = state.pending_commit.clone().unwrap();
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\nchanged after review\n").unwrap();

        let error = apply_pending_commit(&paths, &mut state, &pending.id, &pending.digest)
            .unwrap_err()
            .to_string();

        assert!(error.contains("content, type, or Git mode changed"));
        assert_eq!(git_head(&paths).unwrap(), before);
        assert!(state.pending_commit.is_some());
        assert!(Command::new("git")
            .args(["diff", "--cached", "--quiet", "--exit-code"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pending_commit_rejects_executable_mode_change_without_mutation() {
        let (root, paths) = temp_repo("manual-stale-executable-mode");
        let before = git_head(&paths).unwrap();
        fs::write(root.join("src.txt"), "approved bytes\n").unwrap();
        let cfg = Config {
            commit_mode: CommitMode::Manual,
            ..Config::default()
        };
        let mut state = State::default();
        let files = vec!["src.txt".to_string()];
        coordinate_commit(
            &paths,
            &cfg,
            &mut state,
            CommitIntent {
                source_lane: "chmod-regression",
                task_id: None,
                message: "approved executable mode",
                files: &files,
                guard_evidence_ids: Vec::new(),
                verification_evidence_ids: Vec::new(),
                deferred_file_snapshots: &[],
                defer_task_completion: false,
            },
        )
        .unwrap();
        let pending = state.pending_commit.clone().unwrap();
        assert_eq!(
            pending.path_fingerprints[0].git_mode.as_deref(),
            Some("100644")
        );

        let path = root.join("src.txt");
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();

        let error = apply_pending_commit(&paths, &mut state, &pending.id, &pending.digest)
            .unwrap_err()
            .to_string();

        assert!(error.contains("content, type, or Git mode changed"));
        assert_eq!(git_head(&paths).unwrap(), before);
        assert!(state.pending_commit.is_some());
        assert!(Command::new("git")
            .args(["diff", "--cached", "--quiet", "--exit-code"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exact_pending_approval_creates_one_commit_and_clears_intent() {
        let (root, paths) = temp_repo("manual-apply");
        let before = git_head(&paths).unwrap();
        let queue_before = capture_rust_owned_file(&paths, &paths.next_steps).unwrap();
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\nreview me\n").unwrap();
        let cfg = Config {
            commit_mode: CommitMode::Manual,
            ..Config::default()
        };
        let mut state = State::default();
        commit_queue_bookkeeping(
            &paths,
            &cfg,
            &mut state,
            &paths.next_steps,
            "complete queue item",
            &queue_before,
        )
        .unwrap();
        let pending = state.pending_commit.clone().unwrap();

        let committed =
            apply_pending_commit(&paths, &mut state, &pending.id, &pending.digest).unwrap();

        assert_ne!(before, committed);
        assert!(state.pending_commit.is_none());
        let recovery: PendingCommitRecoveryV1 = serde_json::from_str(
            &fs::read_to_string(pending_commit_recovery_path(&paths)).unwrap(),
        )
        .unwrap();
        assert_eq!(recovery.commit_sha.as_deref(), Some(committed.as_str()));
        assert!(recovery.state_after.pending_commit.is_none());
        clear_pending_commit_recovery(&paths);
        assert!(!pending_commit_recovery_path(&paths).exists());
        assert!(Command::new("git")
            .args([
                "status",
                "--porcelain",
                "--",
                ".",
                ":(exclude).orchestrator"
            ])
            .current_dir(&root)
            .output()
            .unwrap()
            .stdout
            .is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn completed_pending_commit_reconciles_state_idempotently_after_restart() {
        let (root, paths) = temp_repo("manual-recovery-restart");
        let queue_before = capture_rust_owned_file(&paths, &paths.next_steps).unwrap();
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\nrecovery approved\n").unwrap();
        let cfg = Config {
            commit_mode: CommitMode::Manual,
            ..Config::default()
        };
        let mut state = State::default();
        commit_queue_bookkeeping(
            &paths,
            &cfg,
            &mut state,
            &paths.next_steps,
            "complete queue item with recovery",
            &queue_before,
        )
        .unwrap();
        let persisted_before_commit = state.clone();
        let pending = state.pending_commit.clone().unwrap();

        let committed =
            apply_pending_commit(&paths, &mut state, &pending.id, &pending.digest).unwrap();
        assert!(state.pending_commit.is_none());

        // Simulate a process crash after Git advanced but before state.json was
        // saved: a restarted process sees the old pending state and the durable
        // recovery record.
        let mut restarted = persisted_before_commit;
        let recovered =
            apply_pending_commit(&paths, &mut restarted, &pending.id, &pending.digest).unwrap();

        assert_eq!(recovered, committed);
        assert!(restarted.pending_commit.is_none());
        let count = Command::new("git")
            .args(["rev-list", "--count", "HEAD"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(count.status.success());
        assert_eq!(String::from_utf8_lossy(&count.stdout).trim(), "2");

        // A retry after state persistence but before recovery cleanup is also
        // idempotent and returns the already-created commit.
        let replay =
            apply_pending_commit(&paths, &mut restarted, &pending.id, &pending.digest).unwrap();
        assert_eq!(replay, committed);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_reconstructs_commit_sha_but_rejects_modified_state_transition() {
        let (root, paths) = temp_repo("manual-recovery-reconstruct");
        let queue_before = capture_rust_owned_file(&paths, &paths.next_steps).unwrap();
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\nreconstruct recovery\n").unwrap();
        let cfg = Config {
            commit_mode: CommitMode::Manual,
            ..Config::default()
        };
        let mut state = State::default();
        commit_queue_bookkeeping(
            &paths,
            &cfg,
            &mut state,
            &paths.next_steps,
            "reconstruct recovery SHA",
            &queue_before,
        )
        .unwrap();
        let persisted_before_commit = state.clone();
        let pending = state.pending_commit.clone().unwrap();
        let committed =
            apply_pending_commit(&paths, &mut state, &pending.id, &pending.digest).unwrap();
        let path = pending_commit_recovery_path(&paths);
        let mut recovery: PendingCommitRecoveryV1 =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        recovery.commit_sha = None;
        write_pending_commit_recovery(&paths, &recovery).unwrap();

        let mut restarted = persisted_before_commit.clone();
        let recovered =
            apply_pending_commit(&paths, &mut restarted, &pending.id, &pending.digest).unwrap();
        assert_eq!(recovered, committed);
        let finalized: PendingCommitRecoveryV1 =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(finalized.commit_sha.as_deref(), Some(committed.as_str()));

        let mut modified = recovery;
        modified.state_after.completed_tasks =
            modified.state_after.completed_tasks.saturating_add(1);
        modified.commit_sha = Some(committed.clone());
        write_pending_commit_recovery(&paths, &modified).unwrap();
        let mut stale = persisted_before_commit;
        let error = apply_pending_commit(&paths, &mut stale, &pending.id, &pending.digest)
            .unwrap_err()
            .to_string();
        assert!(error.contains("state transition was modified"));
        assert!(stale.pending_commit.is_some());
        assert_eq!(git_head(&paths).as_deref(), Some(committed.as_str()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn accepted_worker_manual_intent_defers_queue_bytes_and_preserves_worker_files() {
        let (root, paths) = temp_repo("manual-worker-deferred-queue");
        let before_head = git_head(&paths).unwrap();
        let before_queue = fs::read_to_string(&paths.next_steps).unwrap();
        let queue_before = capture_rust_owned_file(&paths, &paths.next_steps).unwrap();
        fs::write(root.join("src.txt"), "worker edit\n").unwrap();
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\ndone with evidence\n").unwrap();
        let cfg = Config {
            commit_mode: CommitMode::Manual,
            ..Config::default()
        };
        let task = task();
        let mut state = State {
            queue: vec![task.clone()],
            ..State::default()
        };
        let guard = GuardReport {
            enabled: true,
            status: "passed".to_string(),
            changed_files: vec!["src.txt".to_string()],
            allowed_patterns: task.allowed_files.clone(),
            ignored_patterns: Vec::new(),
            violations: Vec::new(),
        };

        let commit = commit_accepted_worker_task(
            &paths,
            &cfg,
            &mut state,
            AcceptedWorkerCommitRequest {
                task: &task,
                guard_report: &guard,
                message: "accept worker with deferred queue",
                rust_owned_file_snapshots: &[queue_before],
                verification_evidence_id: "evidence-worker-deferred",
            },
        )
        .unwrap();

        assert!(commit.is_none());
        assert_eq!(git_head(&paths).unwrap(), before_head);
        assert_eq!(fs::read_to_string(&paths.next_steps).unwrap(), before_queue);
        assert_eq!(
            fs::read_to_string(root.join("src.txt")).unwrap(),
            "worker edit\n"
        );
        assert_eq!(current_commit_paths(&paths).unwrap(), vec!["src.txt"]);
        let pending = state.pending_commit.clone().unwrap();
        assert_eq!(
            pending.dirty_paths,
            vec!["NEXT_STEPS.md".to_string(), "src.txt".to_string()]
        );
        assert_eq!(pending.deferred_file_writes.len(), 1);
        assert!(Command::new("git")
            .args(["diff", "--cached", "--quiet", "--exit-code"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());

        let sha = apply_pending_commit(&paths, &mut state, &pending.id, &pending.digest).unwrap();
        assert_ne!(sha, before_head);
        assert_eq!(state.completed_tasks, 1);
        assert!(state.queue.is_empty());
        assert_eq!(
            fs::read_to_string(&paths.next_steps).unwrap(),
            "# NEXT_STEPS\n\ndone with evidence\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deterministic_manual_intent_defers_queue_and_completion_until_approval() {
        let (root, paths) = temp_repo("manual-deterministic-deferred-queue");
        let before_head = git_head(&paths).unwrap();
        let before_queue = fs::read_to_string(&paths.next_steps).unwrap();
        let queue_before = capture_rust_owned_file(&paths, &paths.next_steps).unwrap();
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\ndeterministic done\n").unwrap();
        let cfg = Config {
            commit_mode: CommitMode::Manual,
            ..Config::default()
        };
        let task = task();
        let mut state = State {
            completed_tasks: 4,
            queue: vec![task.clone()],
            ..State::default()
        };

        let commit = commit_deterministic_task(
            &paths,
            &cfg,
            &mut state,
            &task,
            &paths.next_steps,
            &queue_before,
        )
        .unwrap();

        assert!(commit.is_none());
        assert_eq!(git_head(&paths).unwrap(), before_head);
        assert_eq!(fs::read_to_string(&paths.next_steps).unwrap(), before_queue);
        assert_eq!(state.completed_tasks, 4);
        let pending = state.pending_commit.clone().unwrap();
        apply_pending_commit(&paths, &mut state, &pending.id, &pending.digest).unwrap();
        assert_eq!(state.completed_tasks, 5);
        assert!(state.queue.is_empty());
        assert_eq!(
            fs::read_to_string(&paths.next_steps).unwrap(),
            "# NEXT_STEPS\n\ndeterministic done\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rust_patch_manual_intent_keeps_patch_and_defers_queue_bookkeeping() {
        let (root, paths) = temp_repo("manual-rust-patch-deferred-queue");
        let before_head = git_head(&paths).unwrap();
        let before_queue = fs::read_to_string(&paths.next_steps).unwrap();
        let queue_before = capture_rust_owned_file(&paths, &paths.next_steps).unwrap();
        fs::write(root.join("src.txt"), "Rust patch edit\n").unwrap();
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\nRust patch done\n").unwrap();
        let cfg = Config {
            commit_mode: CommitMode::Manual,
            ..Config::default()
        };
        let task = task();
        let mut state = State {
            queue: vec![task.clone()],
            ..State::default()
        };

        let commit = commit_rust_patch_task(
            &paths,
            &cfg,
            &mut state,
            &task,
            &paths.next_steps,
            &["src.txt".to_string()],
            &queue_before,
        )
        .unwrap();

        assert!(commit.is_none());
        assert_eq!(git_head(&paths).unwrap(), before_head);
        assert_eq!(fs::read_to_string(&paths.next_steps).unwrap(), before_queue);
        assert_eq!(
            fs::read_to_string(root.join("src.txt")).unwrap(),
            "Rust patch edit\n"
        );
        let pending = state.pending_commit.clone().unwrap();
        apply_pending_commit(&paths, &mut state, &pending.id, &pending.digest).unwrap();
        assert_eq!(state.completed_tasks, 1);
        assert!(state.queue.is_empty());
        assert_eq!(
            fs::read_to_string(&paths.next_steps).unwrap(),
            "# NEXT_STEPS\n\nRust patch done\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manual_commit_disables_hooks_and_signing_before_applying_exact_intent() {
        let (root, paths) = temp_repo("manual-hooks-disabled");
        let before_head = git_head(&paths).unwrap();
        let queue_before = capture_rust_owned_file(&paths, &paths.next_steps).unwrap();
        fs::write(root.join("src.txt"), "approved worker edit\n").unwrap();
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\napproved done\n").unwrap();
        let cfg = Config {
            commit_mode: CommitMode::Manual,
            ..Config::default()
        };
        let task = task();
        let mut state = State {
            queue: vec![task.clone()],
            ..State::default()
        };
        let guard = GuardReport {
            enabled: true,
            status: "passed".to_string(),
            changed_files: vec!["src.txt".to_string()],
            allowed_patterns: task.allowed_files.clone(),
            ignored_patterns: Vec::new(),
            violations: Vec::new(),
        };
        commit_accepted_worker_task(
            &paths,
            &cfg,
            &mut state,
            AcceptedWorkerCommitRequest {
                task: &task,
                guard_report: &guard,
                message: "commit despite hostile repository hooks",
                rust_owned_file_snapshots: &[queue_before],
                verification_evidence_id: "evidence-hook",
            },
        )
        .unwrap();
        let pending = state.pending_commit.clone().unwrap();
        let hook = root.join(".git/hooks/pre-commit");
        fs::write(
            &hook,
            "#!/bin/sh\nprintf invoked > hook-ran\nprintf hostile > outside.txt\nchmod +x src.txt\ngit add outside.txt src.txt\nexit 1\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&hook).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&hook, permissions).unwrap();
        let signer = paths.orch.join("hostile-signer");
        fs::write(&signer, "#!/bin/sh\nprintf invoked > signer-ran\nexit 1\n").unwrap();
        let mut permissions = fs::metadata(&signer).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&signer, permissions).unwrap();
        assert!(Command::new("git")
            .args(["config", "commit.gpgSign", "true"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["config", "gpg.program", signer.to_str().unwrap()])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());

        let committed =
            apply_pending_commit(&paths, &mut state, &pending.id, &pending.digest).unwrap();

        assert_ne!(committed, before_head);
        assert_eq!(git_head(&paths).as_deref(), Some(committed.as_str()));
        assert!(!root.join("hook-ran").exists());
        assert!(!root.join("outside.txt").exists());
        assert!(!root.join("signer-ran").exists());
        assert!(state.pending_commit.is_none());
        assert_eq!(
            fs::read_to_string(&paths.next_steps).unwrap(),
            "# NEXT_STEPS\n\napproved done\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("src.txt")).unwrap(),
            "approved worker edit\n"
        );
        assert!(Command::new("git")
            .args(["diff", "--cached", "--quiet", "--exit-code"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn aggregate_deferred_control_doc_limit_restores_bytes_without_intent() {
        let (root, paths) = temp_repo("manual-deferred-size-limit");
        let continuity = paths.root.join("CONTINUITY.md");
        let initial = format!("A{}", "a".repeat(600 * 1024));
        fs::write(&paths.next_steps, &initial).unwrap();
        fs::write(&continuity, &initial).unwrap();
        assert!(Command::new("git")
            .args(["add", "NEXT_STEPS.md", "CONTINUITY.md"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-q", "-m", "large controls"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        let next_before = capture_rust_owned_file(&paths, &paths.next_steps).unwrap();
        let continuity_before = capture_rust_owned_file(&paths, &continuity).unwrap();
        let proposed = format!("B{}", "b".repeat(600 * 1024));
        fs::write(&paths.next_steps, &proposed).unwrap();
        fs::write(&continuity, &proposed).unwrap();
        let cfg = Config {
            commit_mode: CommitMode::Manual,
            ..Config::default()
        };
        let mut state = State::default();
        let files = vec!["CONTINUITY.md".to_string(), "NEXT_STEPS.md".to_string()];
        let snapshots = vec![continuity_before, next_before];

        let error = coordinate_commit(
            &paths,
            &cfg,
            &mut state,
            CommitIntent {
                source_lane: "size-limit-test",
                task_id: None,
                message: "oversized pending controls",
                files: &files,
                guard_evidence_ids: Vec::new(),
                verification_evidence_ids: Vec::new(),
                deferred_file_snapshots: &snapshots,
                defer_task_completion: false,
            },
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("aggregate manual-mode limit"));
        assert!(state.pending_commit.is_none());
        assert_eq!(fs::read_to_string(&paths.next_steps).unwrap(), initial);
        assert_eq!(fs::read_to_string(&continuity).unwrap(), initial);
        assert!(current_commit_paths(&paths).unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn every_persisted_pending_field_is_digest_bound_before_staging() {
        let (root, paths) = temp_repo("manual-tamper");
        let before = git_head(&paths).unwrap();
        let queue_before = capture_rust_owned_file(&paths, &paths.next_steps).unwrap();
        fs::write(&paths.next_steps, "# NEXT_STEPS\n\nreview me\n").unwrap();
        let cfg = Config {
            commit_mode: CommitMode::Manual,
            ..Config::default()
        };
        let mut original_state = State::default();
        commit_queue_bookkeeping(
            &paths,
            &cfg,
            &mut original_state,
            &paths.next_steps,
            "complete queue item",
            &queue_before,
        )
        .unwrap();

        let mut cases: Vec<PendingMutation> = vec![
            ("id", Box::new(|pending| pending.id.push('x'))),
            (
                "digest",
                Box::new(|pending| pending.digest = "f".repeat(64)),
            ),
            (
                "source_lane",
                Box::new(|pending| pending.source_lane.push_str("-tampered")),
            ),
            (
                "task_id",
                Box::new(|pending| pending.task_id = Some("OTHER-1".to_string())),
            ),
            (
                "import_digest",
                Box::new(|pending| pending.import_digest = Some("a".repeat(64))),
            ),
            (
                "base_head",
                Box::new(|pending| pending.base_head = "0".repeat(40)),
            ),
            (
                "dirty_paths",
                Box::new(|pending| pending.dirty_paths.push("src.txt".to_string())),
            ),
            (
                "path_fingerprints",
                Box::new(|pending| {
                    pending.path_fingerprints[0].file_type = "tampered".to_string();
                }),
            ),
            (
                "deferred_file_writes",
                Box::new(|pending| {
                    pending.deferred_file_writes[0]
                        .after_content
                        .push_str("tampered");
                }),
            ),
            (
                "commit_message",
                Box::new(|pending| pending.commit_message.push_str(" tampered")),
            ),
            (
                "guard_evidence_ids",
                Box::new(|pending| pending.guard_evidence_ids.push("guard-x".to_string())),
            ),
            (
                "verification_evidence_ids",
                Box::new(|pending| {
                    pending
                        .verification_evidence_ids
                        .push("evidence-x".to_string());
                }),
            ),
            (
                "deferred_transition",
                Box::new(|pending| {
                    pending.deferred_transition = Some(json!({"tampered": true}));
                }),
            ),
            (
                "created_at",
                Box::new(|pending| pending.created_at.push_str("-tampered")),
            ),
        ];

        for (field, mutate) in cases.drain(..) {
            let mut state = original_state.clone();
            let pending = state.pending_commit.as_mut().unwrap();
            mutate(pending);
            let expected_id = pending.id.clone();
            let expected_digest = pending.digest.clone();

            let error = apply_pending_commit(&paths, &mut state, &expected_id, &expected_digest)
                .unwrap_err()
                .to_string();

            assert!(
                error.contains("pending commit state was modified"),
                "field {field} was not rejected by digest recomputation: {error}"
            );
            assert_eq!(git_head(&paths).unwrap(), before, "field {field}");
            assert!(Command::new("git")
                .args(["diff", "--cached", "--quiet", "--exit-code"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imported_worker_task_stays_pending_then_completes_once_on_exact_approval() {
        let (root, paths) = temp_repo("manual-imported-worker");
        let before_head = git_head(&paths).unwrap();
        let before_next_steps = fs::read_to_string(&paths.next_steps).unwrap();
        fs::write(root.join("src.txt"), "imported worker edit\n").unwrap();
        let cfg = Config {
            commit_mode: CommitMode::Manual,
            ..Config::default()
        };
        let task = task();
        let import_digest = "a".repeat(64);
        let mut state = State {
            completed_tasks: 7,
            queue: vec![task.clone()],
            ..State::default()
        };
        state.imported_tasks.insert(
            task.task_id.clone(),
            crate::task_import::ImportedTaskRecord {
                schema: "codex-team.imported_task_record.v1".to_string(),
                task_id: task.task_id.clone(),
                import_digest: import_digest.clone(),
                source: "ca".to_string(),
                base_head: before_head.clone(),
                imported_at: "2026-08-20T00:00:00Z".to_string(),
                receipt_path: ".orchestrator/imports/test.json".to_string(),
            },
        );
        let guard = GuardReport {
            enabled: true,
            status: "passed".to_string(),
            changed_files: vec!["src.txt".to_string()],
            allowed_patterns: task.allowed_files.clone(),
            ignored_patterns: Vec::new(),
            violations: Vec::new(),
        };

        let commit = commit_accepted_worker_task(
            &paths,
            &cfg,
            &mut state,
            AcceptedWorkerCommitRequest {
                task: &task,
                guard_report: &guard,
                message: "complete imported worker task",
                rust_owned_file_snapshots: &[],
                verification_evidence_id: "evidence-imported-worker",
            },
        )
        .unwrap();

        assert!(commit.is_none());
        assert_eq!(git_head(&paths).unwrap(), before_head);
        assert_eq!(state.completed_tasks, 7);
        assert_eq!(
            state
                .queue
                .iter()
                .filter(|queued| queued.task_id == task.task_id)
                .count(),
            1
        );
        let pending = state.pending_commit.clone().unwrap();
        assert_eq!(
            pending.import_digest.as_deref(),
            Some(import_digest.as_str())
        );
        assert_eq!(pending.dirty_paths, vec!["src.txt"]);
        assert!(!pending
            .dirty_paths
            .iter()
            .any(|path| path == "NEXT_STEPS.md"));
        assert_eq!(
            fs::read_to_string(&paths.next_steps).unwrap(),
            before_next_steps
        );
        assert!(Command::new("git")
            .args(["diff", "--cached", "--quiet", "--exit-code"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());

        let committed =
            apply_pending_commit(&paths, &mut state, &pending.id, &pending.digest).unwrap();

        assert_ne!(committed, before_head);
        assert!(state.pending_commit.is_none());
        assert_eq!(state.completed_tasks, 8);
        assert!(!state
            .queue
            .iter()
            .any(|queued| queued.task_id == task.task_id));
        assert_eq!(
            state
                .imported_tasks
                .get(&task.task_id)
                .map(|record| record.import_digest.as_str()),
            Some(import_digest.as_str())
        );
        let committed_paths = Command::new("git")
            .args(["show", "--pretty=format:", "--name-only", "HEAD"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(committed_paths.status.success());
        assert_eq!(
            String::from_utf8_lossy(&committed_paths.stdout)
                .lines()
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>(),
            vec!["src.txt"]
        );

        clear_pending_commit_recovery(&paths);
        let second = apply_pending_commit(&paths, &mut state, &pending.id, &pending.digest);
        assert!(second.is_err());
        assert_eq!(state.completed_tasks, 8);
        assert_eq!(git_head(&paths).unwrap(), committed);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_exclude_installation_is_idempotent() {
        let (root, paths) = temp_repo("runtime-exclude");
        install_orchestrator_runtime_exclude(&paths).unwrap();
        install_orchestrator_runtime_exclude(&paths).unwrap();
        let exclude_path = Command::new("git")
            .args(["rev-parse", "--git-path", "info/exclude"])
            .current_dir(&root)
            .output()
            .unwrap();
        let exclude_path = root.join(String::from_utf8_lossy(&exclude_path.stdout).trim());
        let text = fs::read_to_string(exclude_path).unwrap();
        assert_eq!(text.matches("# >>> codex-team runtime >>>").count(), 1);
        assert_eq!(text.matches("/.orchestrator/").count(), 1);
        assert!(orchestrator_runtime_is_ignored(&paths).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}
