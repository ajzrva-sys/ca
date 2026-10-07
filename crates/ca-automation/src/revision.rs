//! Stable, lock-scoped repository revision calculation.
//!
//! Callers are responsible for holding the repository lock for mutations. The
//! revision deliberately includes user-visible Git and control state while
//! excluding volatile `.orchestrator` observations and job records.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::paths::Paths;
use crate::state::HumanDispositionPause;

const REVISION_DOMAIN: &[u8] = b"codex-team-revision-v1";
const MAX_REVISION_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DIRTY_PROJECT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DIRTY_PROJECT_PATHS: usize = 4_096;
const MAX_GIT_PATH_BYTES: usize = 4_096;
const MAX_PORCELAIN_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MCP_PARTIAL_STATE_LATCH: &str = ".orchestrator/mcp/approval-required.json";
const ROOT_CONTROL_FILES: &[&str] = &[
    "AGENTS.md",
    "ARCHITECTURE.md",
    "CONTINUITY.md",
    "NEXT_STEPS.md",
    "NEXT_STEPS_non-ai.md",
    "PARITY.md",
    "PLAN.md",
    "QUALITY.md",
    "ROADMAP.md",
    "TASK.md",
    "TESTING.md",
];

/// Return the stable SHA-256 revision of the repository's approval-relevant
/// state. This function never reads credential configuration.
pub(crate) fn repository_revision(paths: &Paths) -> Result<String> {
    ensure_pinned_git_root(&paths.root)?;

    let mut hasher = Sha256::new();
    hash_field(&mut hasher, REVISION_DOMAIN);
    hash_field(&mut hasher, canonical_path_bytes(&paths.root)?.as_slice());
    hash_field(&mut hasher, git_head_or_unborn(paths)?.as_bytes());

    let porcelain = git_porcelain(paths)?;
    for entry in &porcelain.entries {
        hash_field(&mut hasher, entry);
    }
    hash_field(&mut hasher, b"staged-index-delta-v1");
    hash_field(&mut hasher, &git_staged_index_delta(paths)?);
    hash_dirty_project_files(&mut hasher, paths, &porcelain.dirty_paths)?;

    hash_bounded_file(&mut hasher, "orchestrator-config", &paths.config)?;
    hash_bounded_file(&mut hasher, "orchestrator-state", &paths.state)?;
    // Job records and active-child markers are volatile observations, but this
    // bounded latch is durable approval evidence. A partial/failed MCP job must
    // invalidate the revision that authorized it before any later mutation.
    hash_bounded_file(
        &mut hasher,
        "mcp-partial-state-approval-latch",
        &paths.root.join(MCP_PARTIAL_STATE_LATCH),
    )?;
    for relative in ROOT_CONTROL_FILES {
        hash_bounded_file(&mut hasher, relative, &paths.root.join(relative))?;
    }

    Ok(hex_digest(hasher.finalize().as_slice()))
}

/// Bind a human decision to both the exact repository revision and the exact
/// canonical pause payload that the operator reviewed.
pub(crate) fn human_disposition_pause_fingerprint(
    paths: &Paths,
    pause: &HumanDispositionPause,
) -> Result<String> {
    let revision = repository_revision(paths)?;
    let canonical_pause =
        serde_json::to_vec(pause).context("serialize canonical human-disposition pause")?;
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, b"codex-team-pause-fingerprint-v1");
    hash_field(&mut hasher, revision.as_bytes());
    hash_field(&mut hasher, &canonical_pause);
    Ok(hex_digest(hasher.finalize().as_slice()))
}

/// Full Git object ID, or `None` for a valid unborn repository.
pub(crate) fn git_head(paths: &Paths) -> Result<Option<String>> {
    ensure_pinned_git_root(&paths.root)?;
    let output = crate::git_metadata::query(&paths.root, &["rev-parse", "--verify", "HEAD"])?
        .current_dir(&paths.root)
        .output()
        .context("read Git HEAD for repository revision")?;
    if output.status.success() {
        let head = String::from_utf8(output.stdout).context("Git HEAD was not UTF-8")?;
        let head = head.trim();
        if !matches!(head.len(), 40 | 64) || !head.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("git returned an invalid full HEAD object ID")
        }
        return Ok(Some(head.to_ascii_lowercase()));
    }

    let symbolic = crate::git_metadata::query(&paths.root, &["symbolic-ref", "-q", "HEAD"])?
        .current_dir(&paths.root)
        .output()
        .context("inspect unborn Git HEAD")?;
    if symbolic.status.success() {
        Ok(None)
    } else {
        bail!("repository has neither a commit nor a valid unborn HEAD")
    }
}

/// Fail unless the worktree and index are clean outside orchestrator runtime
/// state. Generated/cache-root exemptions are intentionally not accepted here;
/// callers that need them must use the configured clean-start policy instead.
pub(crate) fn require_clean_outside_orchestrator(paths: &Paths) -> Result<()> {
    let entries = git_porcelain(paths)?.entries;
    if entries.is_empty() {
        return Ok(());
    }
    let summaries = entries
        .iter()
        .take(12)
        .map(|entry| sanitize_porcelain_entry(entry))
        .collect::<Vec<_>>();
    let remaining = entries.len().saturating_sub(summaries.len());
    let suffix = if remaining == 0 {
        String::new()
    } else {
        format!(" (+{remaining} more)")
    };
    bail!(
        "repository must be clean outside .orchestrator before this mutation: {}{suffix}",
        summaries.join(", ")
    )
}

fn ensure_pinned_git_root(root: &Path) -> Result<()> {
    let canonical = fs::canonicalize(root)
        .with_context(|| format!("canonicalize repository {}", root.display()))?;
    let output = crate::git_metadata::query(&canonical, &["rev-parse", "--show-toplevel"])?
        .current_dir(&canonical)
        .output()
        .context("locate Git repository root")?;
    if !output.status.success() {
        bail!("{} is not a Git repository", canonical.display())
    }
    let reported = String::from_utf8(output.stdout).context("Git root was not UTF-8")?;
    let reported = fs::canonicalize(reported.trim()).context("canonicalize Git root")?;
    if reported != canonical {
        bail!(
            "repository path {} is not the canonical Git root {}",
            canonical.display(),
            reported.display()
        )
    }
    Ok(())
}

fn git_head_or_unborn(paths: &Paths) -> Result<String> {
    Ok(match git_head(paths)? {
        Some(head) => format!("head:{head}"),
        None => "head:<unborn>".to_string(),
    })
}

#[derive(Debug)]
struct GitPorcelain {
    entries: Vec<Vec<u8>>,
    dirty_paths: Vec<Vec<u8>>,
}

fn git_porcelain(paths: &Paths) -> Result<GitPorcelain> {
    ensure_pinned_git_root(&paths.root)?;
    let output = crate::git_metadata::command_no_filters(&paths.root)?
        .args([
            "status",
            "--porcelain=v1",
            "-z",
            "--no-renames",
            "--untracked-files=all",
            "--",
            ".",
            ":(exclude).orchestrator",
            ":(exclude).orchestrator/**",
        ])
        .current_dir(&paths.root)
        .output()
        .context("read Git porcelain state")?;
    if !output.status.success() {
        bail!(
            "git status failed while calculating repository revision: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
    if output.stdout.len() > MAX_PORCELAIN_BYTES {
        bail!("Git porcelain state exceeds the 8 MiB revision limit");
    }
    let fields = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    let mut entries = Vec::with_capacity(fields.len());
    let mut dirty_paths = BTreeSet::new();
    let mut index = 0;
    while index < fields.len() {
        let entry = fields[index];
        if entry.len() < 4 || entry[2] != b' ' {
            bail!("Git returned malformed porcelain state");
        }
        entries.push(entry.to_vec());
        insert_dirty_git_path(&mut dirty_paths, &entry[3..])?;
        let renamed_or_copied = matches!(entry[0], b'R' | b'C') || matches!(entry[1], b'R' | b'C');
        if renamed_or_copied {
            index += 1;
            let original = fields
                .get(index)
                .copied()
                .context("Git rename/copy porcelain entry omitted its original path")?;
            entries.push(original.to_vec());
            insert_dirty_git_path(&mut dirty_paths, original)?;
        }
        index += 1;
    }
    entries.sort();
    if dirty_paths.len() > MAX_DIRTY_PROJECT_PATHS {
        bail!("dirty project path count exceeds the 4,096-path revision limit");
    }
    Ok(GitPorcelain {
        entries,
        dirty_paths: dirty_paths.into_iter().collect(),
    })
}

/// Bind staged blob IDs, modes, and conflict stages without reading or
/// persisting staged file content. Rename detection is disabled so repository
/// or user diff configuration cannot change the representation; renames remain
/// bound deterministically as a deletion plus an addition. `--full-index`
/// prevents object ID abbreviation.
fn git_staged_index_delta(paths: &Paths) -> Result<Vec<u8>> {
    let output = crate::git_metadata::command_no_filters(&paths.root)?
        .args([
            "diff",
            "--cached",
            "--raw",
            "--full-index",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--",
            ".",
            ":(exclude).orchestrator",
            ":(exclude).orchestrator/**",
        ])
        .current_dir(&paths.root)
        .output()
        .context("read staged Git index delta for repository revision")?;
    if !output.status.success() {
        bail!("Git failed to describe the staged index for repository revision");
    }
    if output.stdout.len() > MAX_PORCELAIN_BYTES {
        bail!("staged Git index delta exceeds the 8 MiB revision limit");
    }
    Ok(output.stdout)
}

fn insert_dirty_git_path(paths: &mut BTreeSet<Vec<u8>>, path: &[u8]) -> Result<()> {
    if path.is_empty() || path.len() > MAX_GIT_PATH_BYTES {
        bail!("Git dirty path is empty or exceeds the 4 KiB limit");
    }
    let relative = git_path(path)?;
    if !relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        bail!("Git dirty path is not a contained repository-relative path");
    }
    if relative
        .components()
        .next()
        .is_some_and(|component| component.as_os_str() == ".orchestrator")
    {
        bail!("Git returned excluded orchestrator runtime state");
    }
    paths.insert(path.to_vec());
    Ok(())
}

fn hash_dirty_project_files(
    hasher: &mut Sha256,
    paths: &Paths,
    dirty_paths: &[Vec<u8>],
) -> Result<()> {
    hash_field(hasher, b"dirty-project-files-v1");
    let mut aggregate_bytes = 0_u64;
    for raw_path in dirty_paths {
        hash_field(hasher, raw_path);
        let relative = git_path(raw_path)?;
        ensure_no_symlink_ancestors(&paths.root, &relative)?;
        let candidate = paths.root.join(&relative);
        let metadata = match fs::symlink_metadata(&candidate) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                hash_field(hasher, b"absent");
                continue;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("inspect dirty project path {}", candidate.display()))
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!(
                "dirty project path {} must be a regular, non-symlink file",
                candidate.display()
            );
        }
        if metadata.len() > MAX_REVISION_FILE_BYTES {
            bail!(
                "dirty project path {} exceeds the {}-byte per-file revision limit",
                candidate.display(),
                MAX_REVISION_FILE_BYTES
            );
        }
        aggregate_bytes = aggregate_bytes
            .checked_add(metadata.len())
            .context("dirty project byte total overflowed")?;
        if aggregate_bytes > MAX_DIRTY_PROJECT_BYTES {
            bail!("dirty project content exceeds the 64 MiB aggregate revision limit");
        }
        hash_field(hasher, b"regular");
        hash_file_mode(hasher, &metadata);
        let bytes = fs::read(&candidate)
            .with_context(|| format!("read dirty project path {}", candidate.display()))?;
        if bytes.len() as u64 != metadata.len() {
            bail!("dirty project path changed while its revision was calculated");
        }
        let after = fs::symlink_metadata(&candidate)?;
        if after.file_type().is_symlink()
            || !after.is_file()
            || !same_file_snapshot(&metadata, &after)
        {
            bail!("dirty project path changed while its revision was calculated");
        }
        // Project bytes feed the digest only; receipts and diagnostics never
        // serialize or print their contents.
        hash_field(hasher, &bytes);
    }
    Ok(())
}

fn ensure_no_symlink_ancestors(root: &Path, relative: &Path) -> Result<()> {
    let components = relative.components().collect::<Vec<_>>();
    let mut current = root.to_path_buf();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                bail!(
                    "dirty project path traverses non-directory or symlink ancestor {}",
                    current.display()
                )
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| format!("inspect {}", current.display()))
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn git_path(bytes: &[u8]) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt as _;
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec())))
}

#[cfg(not(unix))]
fn git_path(bytes: &[u8]) -> Result<PathBuf> {
    Ok(PathBuf::from(
        String::from_utf8(bytes.to_vec()).context("Git path was not valid UTF-8")?,
    ))
}

#[cfg(unix)]
fn hash_file_mode(hasher: &mut Sha256, metadata: &fs::Metadata) {
    use std::os::unix::fs::MetadataExt as _;
    hash_field(hasher, &metadata.mode().to_be_bytes());
}

#[cfg(unix)]
fn same_file_snapshot(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.mode() == after.mode()
        && before.len() == after.len()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
}

#[cfg(not(unix))]
fn hash_file_mode(hasher: &mut Sha256, metadata: &fs::Metadata) {
    hash_field(hasher, &[u8::from(metadata.permissions().readonly())]);
}

#[cfg(not(unix))]
fn same_file_snapshot(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    before.len() == after.len()
        && before.permissions().readonly() == after.permissions().readonly()
        && before.modified().ok() == after.modified().ok()
}

fn hash_bounded_file(hasher: &mut Sha256, label: &str, path: &Path) -> Result<()> {
    hash_field(hasher, label.as_bytes());
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            hash_field(hasher, b"absent");
            return Ok(());
        }
        Err(err) => return Err(err).with_context(|| format!("inspect {}", path.display())),
    };
    if !metadata.file_type().is_file() {
        bail!("revision input {} must be a regular file", path.display())
    }
    if metadata.len() > MAX_REVISION_FILE_BYTES {
        bail!(
            "revision input {} exceeds {} bytes",
            path.display(),
            MAX_REVISION_FILE_BYTES
        )
    }
    hash_field(hasher, b"present");
    let bytes =
        fs::read(path).with_context(|| format!("read revision input {}", path.display()))?;
    hash_field(hasher, &bytes);
    Ok(())
}

fn canonical_path_bytes(path: &Path) -> Result<Vec<u8>> {
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("canonicalize repository {}", path.display()))?;
    Ok(canonical.to_string_lossy().as_bytes().to_vec())
}

fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn sanitize_porcelain_entry(entry: &[u8]) -> String {
    let text = String::from_utf8_lossy(entry);
    let text = text.replace(['\r', '\n', '\0'], "?");
    let mut bounded = text.chars().take(180).collect::<String>();
    if text.chars().count() > 180 {
        bounded.push_str("...");
    }
    bounded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::process::Stdio;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo(name: &str) -> Result<(std::path::PathBuf, Paths)> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("codex-team-revision-{name}-{nonce}"));
        fs::create_dir_all(&root)?;
        run_git(&root, &["init", "-q"])?;
        run_git(&root, &["config", "user.email", "test@example.invalid"])?;
        run_git(&root, &["config", "user.name", "Test"])?;
        fs::write(root.join("tracked.txt"), "one\n")?;
        run_git(&root, &["add", "tracked.txt"])?;
        run_git(&root, &["commit", "-qm", "initial"])?;
        let paths = Paths::new(&root)?;
        fs::create_dir_all(&paths.orch)?;
        Ok((root, paths))
    }

    fn run_git(root: &Path, args: &[&str]) -> Result<()> {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            bail!("git {:?} failed", args)
        }
        Ok(())
    }

    #[test]
    fn revision_is_stable_and_ignores_runtime_observations() -> Result<()> {
        let (root, paths) = temp_repo("stable")?;
        let first = repository_revision(&paths)?;
        fs::create_dir_all(paths.orch.join("mcp/jobs"))?;
        fs::write(paths.orch.join("mcp/jobs/job.json"), "volatile")?;
        assert_eq!(repository_revision(&paths)?, first);
        fs::create_dir_all(paths.orch.join("mcp/active"))?;
        fs::write(paths.orch.join("mcp/active/job.json"), "volatile")?;
        assert_eq!(repository_revision(&paths)?, first);
        fs::write(root.join("tracked.txt"), "two\n")?;
        assert_ne!(repository_revision(&paths)?, first);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn partial_state_approval_latch_invalidates_prior_revision() -> Result<()> {
        let (root, paths) = temp_repo("partial-latch")?;
        let approved = repository_revision(&paths)?;
        fs::create_dir_all(paths.orch.join("mcp"))?;
        fs::write(
            paths.root.join(MCP_PARTIAL_STATE_LATCH),
            b"{\"schema\":\"codex-team.mcp_partial_state.v1\"}\n",
        )?;
        let fresh = repository_revision(&paths)?;
        assert_ne!(fresh, approved);
        assert_eq!(repository_revision(&paths)?, fresh);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn config_state_and_root_controls_change_revision() -> Result<()> {
        let (root, paths) = temp_repo("controls")?;
        let first = repository_revision(&paths)?;
        fs::write(&paths.config, "mission = 'safe'\n")?;
        let second = repository_revision(&paths)?;
        assert_ne!(second, first);
        fs::write(root.join("NEXT_STEPS.md"), "# queue\n")?;
        assert_ne!(repository_revision(&paths)?, second);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn same_porcelain_content_change_invalidates_revision_and_pause_fingerprint() -> Result<()> {
        let (root, paths) = temp_repo("same-porcelain-content")?;
        let pause = HumanDispositionPause {
            reason: "review exact dirty bytes".to_string(),
            options: vec!["preserve".to_string()],
            source: "test".to_string(),
        };
        fs::write(root.join("tracked.txt"), "AAAA\n")?;
        let porcelain_a = git_porcelain(&paths)?.entries;
        let revision_a = repository_revision(&paths)?;
        let pause_a = human_disposition_pause_fingerprint(&paths, &pause)?;

        fs::write(root.join("tracked.txt"), "BBBB\n")?;
        let porcelain_b = git_porcelain(&paths)?.entries;
        assert_eq!(
            porcelain_b, porcelain_a,
            "the Git status/path set must stay identical"
        );
        let revision_b = repository_revision(&paths)?;
        let pause_b = human_disposition_pause_fingerprint(&paths, &pause)?;
        assert_ne!(revision_b, revision_a);
        assert_ne!(
            pause_b, pause_a,
            "the prior pause approval must become stale"
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn same_porcelain_staged_blob_change_invalidates_revision() -> Result<()> {
        let (root, paths) = temp_repo("same-porcelain-index")?;
        let pause = HumanDispositionPause {
            reason: "review exact staged bytes".to_string(),
            options: vec!["preserve".to_string()],
            source: "test".to_string(),
        };
        fs::write(root.join("tracked.txt"), "INDEX-A\n")?;
        run_git(&root, &["add", "tracked.txt"])?;
        fs::write(root.join("tracked.txt"), "WORKTREE\n")?;
        let porcelain_a = git_porcelain(&paths)?.entries;
        let revision_a = repository_revision(&paths)?;
        let pause_a = human_disposition_pause_fingerprint(&paths, &pause)?;

        fs::write(root.join("tracked.txt"), "INDEX-B\n")?;
        run_git(&root, &["add", "tracked.txt"])?;
        fs::write(root.join("tracked.txt"), "WORKTREE\n")?;
        let porcelain_b = git_porcelain(&paths)?.entries;
        assert_eq!(porcelain_b, porcelain_a);
        assert_ne!(repository_revision(&paths)?, revision_a);
        assert_ne!(
            human_disposition_pause_fingerprint(&paths, &pause)?,
            pause_a,
            "the prior staged-state pause approval must become stale"
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn unborn_repository_revision_binds_staged_content() -> Result<()> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("codex-team-revision-unborn-{nonce}"));
        fs::create_dir_all(&root)?;
        run_git(&root, &["init", "-q"])?;
        fs::write(root.join("initial.txt"), "first\n")?;
        run_git(&root, &["add", "initial.txt"])?;
        let paths = Paths::new(&root)?;
        fs::create_dir_all(&paths.orch)?;
        let first = repository_revision(&paths)?;
        fs::write(root.join("initial.txt"), "second\n")?;
        run_git(&root, &["add", "initial.txt"])?;
        assert_ne!(repository_revision(&paths)?, first);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn revision_handles_untracked_deleted_and_staged_rename_paths_deterministically() -> Result<()>
    {
        let (root, paths) = temp_repo("dirty-path-kinds")?;
        fs::write(root.join("rename-me.txt"), "rename bytes\n")?;
        run_git(&root, &["add", "rename-me.txt"])?;
        run_git(&root, &["commit", "-qm", "add rename fixture"])?;
        run_git(&root, &["mv", "rename-me.txt", "renamed.txt"])?;
        fs::remove_file(root.join("tracked.txt"))?;
        fs::write(root.join("untracked.txt"), "untracked bytes\n")?;

        let first = repository_revision(&paths)?;
        assert_eq!(repository_revision(&paths)?, first);
        let porcelain = git_porcelain(&paths)?;
        assert!(porcelain
            .dirty_paths
            .iter()
            .any(|path| path.as_slice() == b"rename-me.txt"));
        assert!(porcelain
            .dirty_paths
            .iter()
            .any(|path| path.as_slice() == b"renamed.txt"));
        assert!(porcelain
            .dirty_paths
            .iter()
            .any(|path| path.as_slice() == b"tracked.txt"));
        assert!(porcelain
            .dirty_paths
            .iter()
            .any(|path| path.as_slice() == b"untracked.txt"));
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn revision_rejects_dirty_symlink_and_oversized_file() -> Result<()> {
        let (root, paths) = temp_repo("unsafe-dirty-types")?;
        std::os::unix::fs::symlink("tracked.txt", root.join("unsafe-link"))?;
        assert!(repository_revision(&paths).is_err());
        fs::remove_file(root.join("unsafe-link"))?;

        fs::write(
            root.join("oversized.bin"),
            vec![0_u8; MAX_REVISION_FILE_BYTES as usize + 1],
        )?;
        assert!(repository_revision(&paths).is_err());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn clean_check_ignores_only_orchestrator_runtime() -> Result<()> {
        let (root, paths) = temp_repo("clean")?;
        fs::write(paths.orch.join("state.json"), "{}")?;
        require_clean_outside_orchestrator(&paths)?;
        fs::write(root.join("untracked.txt"), "dirty")?;
        assert!(require_clean_outside_orchestrator(&paths).is_err());
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
