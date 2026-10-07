//! Git worktree lifecycle management for isolated Worker tasks.
//!
//! Provides create/status/diff/destroy/list operations for managed worktrees.
//! All git operations are bounded (15s timeout, 1 MiB output cap) following
//! the same safety patterns as the `git` module.
//!
//! Read when changing how Workers get isolated workspaces. Do not read for
//! task routing, guard reports, or verification logic.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_OUTPUT_BYTES: usize = 1024 * 1024; // 1 MiB

/// Status of a managed worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeStatus {
    Clean,
    Dirty,
    Unknown,
}

/// A managed worktree with its metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedWorktree {
    pub id: String,
    pub path: PathBuf,
    pub branch: String,
    pub base_commit: String,
    pub created_at: DateTime<Utc>,
}

/// Manages git worktrees for isolated Worker task execution.
#[derive(Debug)]
pub struct WorktreeManager {
    repo_root: PathBuf,
    worktree_root: PathBuf,
}

impl WorktreeManager {
    /// Create a new WorktreeManager. `worktree_root` is where worktrees
    /// are created (e.g., `.orchestrator/worktrees/`).
    pub fn new(repo_root: PathBuf, worktree_root: PathBuf) -> Self {
        Self {
            repo_root,
            worktree_root,
        }
    }

    /// Create a new worktree branched from `base_ref`.
    pub fn create(&self, id: &str, base_ref: &str) -> Result<ManagedWorktree> {
        let branch = format!("worktree/{id}");
        let path = self.worktree_root.join(id);
        if path.exists() {
            bail!("worktree path already exists: {}", path.display());
        }
        fs::create_dir_all(&self.worktree_root)
            .with_context(|| format!("create worktree root: {}", self.worktree_root.display()))?;

        let output = run_git(
            &self.repo_root,
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                &path.to_string_lossy(),
                base_ref,
            ],
        )?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git worktree add failed: {stderr}");
        }

        let base_commit = self.current_commit(Some(&path))?;
        Ok(ManagedWorktree {
            id: id.to_string(),
            path,
            branch,
            base_commit,
            created_at: Utc::now(),
        })
    }

    /// Get the status of a worktree (clean/dirty + latest commit).
    pub fn status(&self, worktree: &ManagedWorktree) -> Result<WorktreeStatus> {
        let output = run_git(&worktree.path, &["status", "--porcelain"])?;
        if !output.status.success() {
            return Ok(WorktreeStatus::Unknown);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() {
            Ok(WorktreeStatus::Clean)
        } else {
            Ok(WorktreeStatus::Dirty)
        }
    }

    /// Get the diff between the base commit and HEAD in the worktree.
    pub fn diff(&self, worktree: &ManagedWorktree) -> Result<String> {
        let output = run_git(
            &worktree.path,
            &["diff", &format!("{}..HEAD", worktree.base_commit)],
        )?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git diff failed: {stderr}");
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get the list of changed files between base commit and HEAD.
    pub fn changed_files(&self, worktree: &ManagedWorktree) -> Result<Vec<String>> {
        let output = run_git(
            &worktree.path,
            &[
                "diff",
                "--name-only",
                &format!("{}..HEAD", worktree.base_commit),
            ],
        )?;
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

    /// Destroy a worktree: remove the worktree and delete the branch.
    pub fn destroy(&self, worktree: &ManagedWorktree) -> Result<()> {
        // Remove the worktree
        let output = run_git(
            &self.repo_root,
            &[
                "worktree",
                "remove",
                "--force",
                &worktree.path.to_string_lossy(),
            ],
        )?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Non-fatal: worktree may already be gone
            eprintln!("warning: git worktree remove failed: {stderr}");
        }

        // Delete the branch
        let output = run_git(&self.repo_root, &["branch", "-D", &worktree.branch])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("warning: git branch -D failed: {stderr}");
        }

        Ok(())
    }

    /// List all worktrees under the worktree root by scanning for
    /// directories with a `.git` file (worktree marker).
    pub fn list(&self) -> Result<Vec<ManagedWorktree>> {
        if !self.worktree_root.exists() {
            return Ok(Vec::new());
        }
        let mut worktrees = Vec::new();
        for entry in fs::read_dir(&self.worktree_root)
            .with_context(|| format!("read worktree root: {}", self.worktree_root.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let git_marker = entry.path().join(".git");
            if !git_marker.exists() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            let branch = format!("worktree/{id}");
            let base_commit = self.current_commit(Some(&entry.path())).unwrap_or_default();
            worktrees.push(ManagedWorktree {
                id,
                path: entry.path(),
                branch,
                base_commit,
                created_at: Utc::now(), // approximation for discovered worktrees
            });
        }
        Ok(worktrees)
    }

    /// Clean up orphaned worktrees that no longer have a valid git worktree reference.
    pub fn cleanup_orphans(&self) -> Result<Vec<String>> {
        let output = run_git(&self.repo_root, &["worktree", "prune"])?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect())
    }

    fn current_commit(&self, cwd: Option<&Path>) -> Result<String> {
        let dir = cwd.unwrap_or(&self.repo_root);
        let output = run_git_in(dir, &["rev-parse", "HEAD"])?;
        if !output.status.success() {
            bail!("git rev-parse HEAD failed");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

/// Run a git command in the repo root with bounded output.
fn run_git(cwd: &Path, args: &[&str]) -> Result<std::process::Output> {
    run_git_in(cwd, args)
}

fn run_git_in(cwd: &Path, args: &[&str]) -> Result<std::process::Output> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))?;
    // Cap output size
    if output.stdout.len() > MAX_OUTPUT_BYTES || output.stderr.len() > MAX_OUTPUT_BYTES {
        bail!("git output exceeded {MAX_OUTPUT_BYTES} bytes");
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn setup_test_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(root)
            .output()
            .unwrap();
        // Create an initial commit
        std::fs::write(root.join("README.md"), "test").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .unwrap();
        dir
    }

    #[test]
    fn create_and_destroy_worktree() {
        let repo = setup_test_repo();
        let root = repo.path();
        let worktree_root = root.join(".orchestrator").join("worktrees");
        let manager = WorktreeManager::new(root.to_path_buf(), worktree_root.clone());

        // Create
        let wt = manager.create("test-1", "HEAD").unwrap();
        assert_eq!(wt.id, "test-1");
        assert!(wt.path.exists());
        assert!(!wt.base_commit.is_empty());

        // Status (clean)
        let status = manager.status(&wt).unwrap();
        assert_eq!(status, WorktreeStatus::Clean);

        // List
        let list = manager.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "test-1");

        // Make a change and commit it so diff shows it
        std::fs::write(wt.path.join("new_file.txt"), "hello").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(&wt.path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "add file"])
            .current_dir(&wt.path)
            .output()
            .unwrap();
        let status = manager.status(&wt).unwrap();
        assert_eq!(status, WorktreeStatus::Clean);

        // Diff (should show the new commit vs base)
        let diff = manager.diff(&wt).unwrap();
        assert!(diff.contains("hello"), "diff should contain hello: {diff}");

        // Changed files
        let files = manager.changed_files(&wt).unwrap();
        assert!(files.contains(&"new_file.txt".to_string()));

        // Destroy
        manager.destroy(&wt).unwrap();
        assert!(!wt.path.exists());
    }

    #[test]
    fn list_empty_worktree_root() {
        let repo = setup_test_repo();
        let root = repo.path();
        let worktree_root = root.join("nonexistent");
        let manager = WorktreeManager::new(root.to_path_buf(), worktree_root);
        let list = manager.list().unwrap();
        assert!(list.is_empty());
    }
}
