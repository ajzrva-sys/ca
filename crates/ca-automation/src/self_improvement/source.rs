use super::canonical_digest;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceRepositorySnapshotV1 {
    pub schema: String,
    pub branch: String,
    pub head: String,
    pub tracked_tree_digest: String,
    pub submodule_digest: String,
    pub repository_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceCandidateInspectionV1 {
    pub schema: String,
    pub base_head: String,
    pub candidate_commit: String,
    pub changed_paths: Vec<String>,
    pub diff_check_passed: bool,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceTransitionReceiptV1 {
    pub schema: String,
    pub branch: String,
    pub prior_head: String,
    pub active_head: String,
    pub candidate_inspection_digest: String,
    pub clean_after: bool,
}

pub fn observe_clean_repository(path: &Path) -> Result<SourceRepositorySnapshotV1> {
    let root = canonical_repository(path)?;
    let branch = git_text(&root, &["branch", "--show-current"])?;
    if branch.is_empty() {
        bail!("autonomous source improvement requires a named current branch");
    }
    let head = full_object_id(&root, "HEAD")?;
    let status = git_bytes(
        &root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    if !status.is_empty() {
        bail!("autonomous source improvement requires a clean worktree and index");
    }
    let tracked = git_bytes(&root, &["ls-files", "-s", "-z"])?;
    let submodules = git_bytes(&root, &["submodule", "status", "--recursive"])?;
    let tracked_tree_digest = format!("{:x}", Sha256::digest(&tracked));
    let submodule_digest = format!("{:x}", Sha256::digest(&submodules));
    let repository_fingerprint = canonical_digest(&serde_json::json!({
        "branch": branch,
        "head": head,
        "tracked_tree_digest": tracked_tree_digest,
        "submodule_digest": submodule_digest,
    }))?;
    Ok(SourceRepositorySnapshotV1 {
        schema: "codex-team.source_repository_snapshot.v1".to_string(),
        branch,
        head,
        tracked_tree_digest,
        submodule_digest,
        repository_fingerprint,
    })
}

pub fn inspect_source_candidate(
    path: &Path,
    expected: &SourceRepositorySnapshotV1,
    candidate: &str,
    safety_kernel_paths: &[String],
) -> Result<SourceCandidateInspectionV1> {
    let root = canonical_repository(path)?;
    require_unchanged(&root, expected)?;
    let candidate = full_object_id(&root, candidate)?;
    let parent = full_object_id(&root, &format!("{candidate}^"))?;
    if parent != expected.head {
        bail!("source candidate must be exactly one commit rooted at the observed HEAD");
    }
    let kind = git_text(&root, &["cat-file", "-t", &candidate])?;
    if kind != "commit" {
        bail!("source candidate must resolve to a commit");
    }
    let raw = git_bytes(
        &root,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "-z",
            &expected.head,
            &candidate,
        ],
    )?;
    let mut changed_paths = parse_git_paths(&raw)?;
    if changed_paths.is_empty() {
        bail!("source candidate must contain a source change");
    }
    changed_paths.sort();
    changed_paths.dedup();
    for changed in &changed_paths {
        validate_candidate_path(changed, safety_kernel_paths)?;
    }
    let raw_modes = git_text(
        &root,
        &[
            "diff-tree",
            "--no-commit-id",
            "--raw",
            "-r",
            &expected.head,
            &candidate,
        ],
    )?;
    if raw_modes.split_whitespace().any(|part| part == "160000") {
        bail!("autonomous source candidates cannot change submodules");
    }
    let diff_check = Command::new("git")
        .args(["diff", "--check", &expected.head, &candidate])
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    if !diff_check.status.success() {
        bail!("source candidate failed git diff --check");
    }
    let mut inspection = SourceCandidateInspectionV1 {
        schema: "codex-team.source_candidate_inspection.v1".to_string(),
        base_head: expected.head.clone(),
        candidate_commit: candidate,
        changed_paths,
        diff_check_passed: true,
        digest: String::new(),
    };
    inspection.digest = canonical_digest(&inspection)?;
    Ok(inspection)
}

pub fn fast_forward_source_candidate(
    path: &Path,
    expected: &SourceRepositorySnapshotV1,
    candidate: &str,
    safety_kernel_paths: &[String],
) -> Result<SourceTransitionReceiptV1> {
    let root = canonical_repository(path)?;
    let inspection = inspect_source_candidate(&root, expected, candidate, safety_kernel_paths)?;
    require_unchanged(&root, expected)?;
    let output = Command::new("git")
        .args([
            "merge",
            "--ff-only",
            "--no-edit",
            &inspection.candidate_commit,
        ])
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    if !output.status.success() {
        bail!("source fast-forward failed without adopting user changes");
    }
    let after = observe_clean_repository(&root)
        .context("source transition completed but post-transition health failed")?;
    if after.branch != expected.branch || after.head != inspection.candidate_commit {
        bail!("source transition postcondition does not match the inspected candidate");
    }
    Ok(SourceTransitionReceiptV1 {
        schema: "codex-team.source_transition_receipt.v1".to_string(),
        branch: after.branch,
        prior_head: expected.head.clone(),
        active_head: after.head,
        candidate_inspection_digest: inspection.digest,
        clean_after: true,
    })
}

fn require_unchanged(root: &Path, expected: &SourceRepositorySnapshotV1) -> Result<()> {
    let current = observe_clean_repository(root)?;
    if &current != expected {
        bail!("repository branch, HEAD, tree, index, or submodule state drifted; no source transition was made");
    }
    Ok(())
}

fn validate_candidate_path(path: &str, safety_kernel_paths: &[String]) -> Result<()> {
    let parsed = Path::new(path);
    if parsed.is_absolute()
        || path.contains('\\')
        || parsed
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("source candidate contains an unsafe path");
    }
    let lower = path.to_ascii_lowercase();
    let first = lower.split('/').next().unwrap_or_default();
    if matches!(first, ".git" | ".orchestrator" | "oracles" | "reference")
        || matches!(lower.as_str(), "cargo.toml" | "cargo.lock" | ".gitmodules")
    {
        bail!("source candidate changes a protected tree or dependency manifest");
    }
    for protected in safety_kernel_paths {
        let protected = protected.trim_matches('/').to_ascii_lowercase();
        if !protected.is_empty()
            && (lower == protected || lower.starts_with(&format!("{protected}/")))
        {
            bail!("source candidate changes the immutable safety kernel");
        }
    }
    Ok(())
}

fn canonical_repository(path: &Path) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(path).context("inspect source repository")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("source repository must be a real directory");
    }
    let root = fs::canonicalize(path)?;
    let top = git_text(&root, &["rev-parse", "--show-toplevel"])?;
    if Path::new(&top) != root.as_path() {
        bail!("source repository path must be the canonical Git top level");
    }
    Ok(root)
}

fn full_object_id(root: &Path, revision: &str) -> Result<String> {
    let value = git_text(
        root,
        &["rev-parse", "--verify", &format!("{revision}^{{object}}")],
    )?;
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("Git object id is not a full SHA-1 object name");
    }
    Ok(value.to_ascii_lowercase())
}

fn parse_git_paths(bytes: &[u8]) -> Result<Vec<String>> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            std::str::from_utf8(entry)
                .map(str::to_string)
                .context("source candidate contains a non-UTF-8 path")
        })
        .collect()
}

fn git_text(root: &Path, args: &[&str]) -> Result<String> {
    String::from_utf8(git_bytes(root, args)?)
        .context("Git emitted non-UTF-8 control output")
        .map(|value| value.trim().to_string())
}

fn git_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    if !output.status.success() {
        bail!(
            "Git source inspection failed for {}",
            args.first().unwrap_or(&"operation")
        );
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-team-source-{name}-{nonce}"))
    }

    fn repository(name: &str) -> Result<PathBuf> {
        let root = temp_root(name);
        fs::create_dir_all(root.join("src"))?;
        run(&root, &["init", "-q"])?;
        run(&root, &["config", "user.email", "tests@example.invalid"])?;
        run(&root, &["config", "user.name", "Tests"])?;
        fs::write(root.join("src/main.rs"), "fn main() {}\n")?;
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\n",
        )?;
        run(&root, &["add", "."])?;
        run(&root, &["commit", "-q", "-m", "initial"])?;
        Ok(root)
    }

    fn candidate(root: &Path, relative: &str, content: &str) -> Result<String> {
        let worktree = root.join("candidate-worktree");
        run(
            root,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                worktree.to_str().unwrap(),
                "HEAD",
            ],
        )?;
        let target = worktree.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, content)?;
        run(&worktree, &["add", "."])?;
        run(&worktree, &["commit", "-q", "-m", "candidate"])?;
        let commit = git_text(&worktree, &["rev-parse", "HEAD"])?;
        run(root, &["worktree", "remove", worktree.to_str().unwrap()])?;
        Ok(commit)
    }

    fn run(root: &Path, args: &[&str]) -> Result<()> {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            bail!("test Git command failed");
        }
        Ok(())
    }

    #[test]
    fn exact_clean_candidate_fast_forwards_current_branch() -> Result<()> {
        let root = repository("fast-forward")?;
        let snapshot = observe_clean_repository(&root)?;
        let commit = candidate(&root, "src/main.rs", "fn main() { println!(\"ok\"); }\n")?;
        let inspection =
            inspect_source_candidate(&root, &snapshot, &commit, &["src/guard.rs".into()])?;
        assert_eq!(inspection.changed_paths, ["src/main.rs"]);
        let receipt =
            fast_forward_source_candidate(&root, &snapshot, &commit, &["src/guard.rs".into()])?;
        assert_eq!(receipt.active_head, commit);
        assert!(receipt.clean_after);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn dirty_stale_and_protected_candidates_fail_without_moving_head() -> Result<()> {
        let root = repository("refuse")?;
        let snapshot = observe_clean_repository(&root)?;
        let protected = candidate(
            &root,
            "Cargo.toml",
            "[package]\nname='changed'\nversion='0.1.0'\n",
        )?;
        assert!(inspect_source_candidate(&root, &snapshot, &protected, &[]).is_err());
        assert_eq!(git_text(&root, &["rev-parse", "HEAD"])?, snapshot.head);

        let safe = candidate(&root, "src/main.rs", "fn main() { println!(\"safe\"); }\n")?;
        fs::write(root.join("untracked.txt"), "user work\n")?;
        assert!(fast_forward_source_candidate(&root, &snapshot, &safe, &[]).is_err());
        assert_eq!(git_text(&root, &["rev-parse", "HEAD"])?, snapshot.head);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn executable_mode_is_fingerprinted_as_tracked_state() -> Result<()> {
        let root = repository("mode")?;
        let before = observe_clean_repository(&root)?;
        let path = root.join("src/main.rs");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        assert!(observe_clean_repository(&root).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
        assert_eq!(observe_clean_repository(&root)?, before);
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
