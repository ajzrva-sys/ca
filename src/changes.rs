use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeFile {
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub kind: ChangeKind,
    pub conflicted: bool,
    pub staged: bool,
    pub unstaged: bool,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub state_fingerprint: String,
}

impl ChangeFile {
    pub fn marker(&self) -> &'static str {
        if self.conflicted {
            return "U";
        }
        match self.kind {
            ChangeKind::Modified => "M",
            ChangeKind::Added => "A",
            ChangeKind::Deleted => "D",
            ChangeKind::Renamed => "R",
            ChangeKind::Untracked => "?",
        }
    }

    pub fn location_label(&self) -> &'static str {
        if self.conflicted {
            return "conflict";
        }
        match (self.staged, self.unstaged) {
            (true, true) => "staged+unstaged",
            (true, false) => "staged",
            (false, true) => "unstaged",
            (false, false) => "metadata",
        }
    }

    pub fn is_tracked(&self) -> bool {
        self.kind != ChangeKind::Untracked
    }

    pub fn display_path(&self) -> String {
        sanitize_terminal_text(&self.path.to_string_lossy())
    }

    pub fn display_old_path(&self) -> Option<String> {
        self.old_path
            .as_ref()
            .map(|path| sanitize_terminal_text(&path.to_string_lossy()))
    }

    fn literal_pathspecs(&self) -> Vec<OsString> {
        let mut paths = Vec::with_capacity(2);
        if let Some(old_path) = &self.old_path {
            paths.push(old_path.as_os_str().to_os_string());
        }
        paths.push(self.path.as_os_str().to_os_string());
        paths
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSet {
    pub files: Vec<ChangeFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepositoryState {
    pub root: PathBuf,
    pub branch: String,
    pub head: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeReviewLoad {
    pub repository: Option<GitRepositoryState>,
    pub changes: ChangeSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedChange {
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub kind: ChangeKind,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
}

impl StagedChange {
    pub fn display_path(&self) -> String {
        let path = sanitize_terminal_text(&self.path.to_string_lossy());
        self.old_path.as_ref().map_or(path.clone(), |old_path| {
            format!(
                "{} → {path}",
                sanitize_terminal_text(&old_path.to_string_lossy())
            )
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommitSnapshot {
    pub branch: String,
    pub head: Option<String>,
    pub index_tree: String,
    pub files: Vec<StagedChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommitResult {
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommitFailure {
    pub message: String,
    pub uncertain: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeReviewPurpose {
    Inspect,
    Commit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeViewMode {
    List,
    Diff,
    Subject,
    Confirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitReviewOperation {
    Loading,
    Staging,
    Unstaging,
    Preparing,
    Committing,
}

#[derive(Debug, Clone)]
pub struct ChangeReviewState {
    pub open: bool,
    pub purpose: ChangeReviewPurpose,
    pub loading: bool,
    pub operation: Option<GitReviewOperation>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub repository: Option<GitRepositoryState>,
    pub files: Vec<ChangeFile>,
    pub selected_index: usize,
    pub mode: ChangeViewMode,
    pub diff_path: Option<String>,
    pub diff_content: String,
    pub scroll: u16,
    pub subject: String,
    pub subject_cursor: usize,
    pub commit_snapshot: Option<GitCommitSnapshot>,
    pub commit_selected: bool,
    pub confirm_scroll: u16,
    pub cancelled_operation: bool,
    pub cancelled_commit_head: Option<Option<String>>,
    pub generation: u64,
}

impl Default for ChangeReviewState {
    fn default() -> Self {
        Self {
            open: false,
            purpose: ChangeReviewPurpose::Inspect,
            loading: false,
            operation: None,
            error: None,
            notice: None,
            repository: None,
            files: Vec::new(),
            selected_index: 0,
            mode: ChangeViewMode::List,
            diff_path: None,
            diff_content: String::new(),
            scroll: 0,
            subject: String::new(),
            subject_cursor: 0,
            commit_snapshot: None,
            commit_selected: false,
            confirm_scroll: 0,
            cancelled_operation: false,
            cancelled_commit_head: None,
            generation: 0,
        }
    }
}

impl ChangeReviewState {
    pub fn open_for(purpose: ChangeReviewPurpose) -> Self {
        Self {
            open: true,
            purpose,
            loading: true,
            operation: Some(GitReviewOperation::Loading),
            generation: 1,
            ..Self::default()
        }
    }

    pub fn is_commit(&self) -> bool {
        self.purpose == ChangeReviewPurpose::Commit
    }
}

#[derive(Debug, Clone)]
pub struct GitChangeService {
    root: PathBuf,
}

pub const MAX_REVIEW_DIFF_BYTES: usize = 1024 * 1024;
const MAX_GIT_MUTATION_OUTPUT_BYTES: usize = 1024 * 1024;
pub const MAX_COMMIT_SUBJECT_CHARS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewTarget {
    Working,
    Base(String),
    Commit(String),
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewInput {
    pub scope: String,
    pub diff: String,
    pub included_files: Vec<String>,
    pub omitted_files: Vec<String>,
    pub untracked_files: Vec<String>,
    pub custom_instructions: Option<String>,
}

impl GitChangeService {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub async fn load(&self) -> Result<ChangeSet, String> {
        self.load_internal(false, None).await
    }

    async fn load_internal(
        &self,
        capture_exact_state: bool,
        timeout_secs: Option<u64>,
    ) -> Result<ChangeSet, String> {
        let status_args = ["status", "--porcelain=v1", "-z", "--untracked-files=all"];
        let output = if let Some(timeout_secs) = timeout_secs {
            let args = status_args.iter().map(OsString::from).collect::<Vec<_>>();
            run_git_bounded_bytes(&self.root, &args, timeout_secs).await?
        } else {
            run_git_bytes(&self.root, &status_args).await?
        };
        let mut files = parse_porcelain(&output)?;
        for file in files.iter_mut().filter(|file| file.is_tracked()) {
            let mut additions = 0u64;
            let mut deletions = 0u64;
            let mut binary = false;
            if file.staged {
                let paths = std::slice::from_ref(&file.path);
                accumulate_numstat(
                    &if let Some(timeout_secs) = timeout_secs {
                        run_git_paths_bounded(
                            &self.root,
                            &["diff", "--cached", "--numstat", "--"],
                            paths,
                            timeout_secs,
                        )
                        .await?
                    } else {
                        run_git_paths(&self.root, &["diff", "--cached", "--numstat", "--"], paths)
                            .await?
                    },
                    &mut additions,
                    &mut deletions,
                    &mut binary,
                );
            }
            if file.unstaged {
                let paths = std::slice::from_ref(&file.path);
                accumulate_numstat(
                    &if let Some(timeout_secs) = timeout_secs {
                        run_git_paths_bounded(
                            &self.root,
                            &["diff", "--numstat", "--"],
                            paths,
                            timeout_secs,
                        )
                        .await?
                    } else {
                        run_git_paths(&self.root, &["diff", "--numstat", "--"], paths).await?
                    },
                    &mut additions,
                    &mut deletions,
                    &mut binary,
                );
            }
            if !binary {
                file.additions = Some(additions);
                file.deletions = Some(deletions);
            }
        }
        if capture_exact_state {
            let timeout_secs = timeout_secs.expect("exact commit state is always bounded");
            for file in &mut files {
                file.state_fingerprint =
                    change_state_fingerprint(&self.root, file, timeout_secs).await?;
            }
        }
        Ok(ChangeSet { files })
    }

    pub async fn unified_diff(&self, file: &ChangeFile) -> Result<String, String> {
        self.unified_diff_internal(file, None).await
    }

    pub async fn unified_diff_bounded(
        &self,
        file: &ChangeFile,
        timeout_secs: u64,
    ) -> Result<String, String> {
        self.unified_diff_internal(file, Some(timeout_secs)).await
    }

    async fn unified_diff_internal(
        &self,
        file: &ChangeFile,
        timeout_secs: Option<u64>,
    ) -> Result<String, String> {
        if !file.is_tracked() {
            return Err("Untracked file contents are not opened automatically".into());
        }
        let mut sections = Vec::new();
        if file.staged {
            let args = [
                "diff",
                "--cached",
                "--no-color",
                "--no-ext-diff",
                "--unified=3",
                "--",
            ];
            let paths = std::slice::from_ref(&file.path);
            let diff = if let Some(timeout_secs) = timeout_secs {
                run_git_paths_bounded(&self.root, &args, paths, timeout_secs).await?
            } else {
                run_git_paths(&self.root, &args, paths).await?
            };
            if !diff.is_empty() {
                sections.push(format!("Staged changes\n{}", sanitize_terminal_text(&diff)));
            }
        }
        if file.unstaged {
            let args = ["diff", "--no-color", "--no-ext-diff", "--unified=3", "--"];
            let paths = std::slice::from_ref(&file.path);
            let diff = if let Some(timeout_secs) = timeout_secs {
                run_git_paths_bounded(&self.root, &args, paths, timeout_secs).await?
            } else {
                run_git_paths(&self.root, &args, paths).await?
            };
            if !diff.is_empty() {
                sections.push(format!(
                    "Unstaged changes\n{}",
                    sanitize_terminal_text(&diff)
                ));
            }
        }
        if sections.is_empty() {
            Ok("No textual diff is available for this tracked path".into())
        } else {
            Ok(sections.join("\n\n"))
        }
    }

    pub async fn review_input(&self, target: ReviewTarget) -> Result<ReviewInput, String> {
        let (scope, paths, untracked_files, custom_instructions) = match &target {
            ReviewTarget::Working | ReviewTarget::Custom(_) => {
                let changes = self.load().await?;
                let tracked = changes
                    .files
                    .iter()
                    .filter(|file| file.is_tracked())
                    .map(ChangeFile::display_path)
                    .collect();
                let untracked = changes
                    .files
                    .iter()
                    .filter(|file| !file.is_tracked())
                    .map(ChangeFile::display_path)
                    .collect();
                (
                    if matches!(&target, ReviewTarget::Working) {
                        "working tree".into()
                    } else {
                        "custom working-tree review".into()
                    },
                    tracked,
                    untracked,
                    match &target {
                        ReviewTarget::Custom(instructions) => Some(instructions.clone()),
                        _ => None,
                    },
                )
            }
            ReviewTarget::Base(reference) => {
                validate_git_ref(reference)?;
                verify_commit(&self.root, reference).await?;
                let range = format!("{reference}...HEAD");
                (
                    format!("base {reference}"),
                    git_paths(&self.root, &["diff", "--name-only", "-z", &range, "--"]).await?,
                    Vec::new(),
                    None,
                )
            }
            ReviewTarget::Commit(sha) => {
                validate_commit_sha(sha)?;
                verify_commit(&self.root, sha).await?;
                (
                    format!("commit {sha}"),
                    git_paths(
                        &self.root,
                        &[
                            "diff-tree",
                            "--root",
                            "--no-commit-id",
                            "--name-only",
                            "-r",
                            "-z",
                            sha,
                        ],
                    )
                    .await?,
                    Vec::new(),
                    None,
                )
            }
        };
        let mut diff = String::new();
        let mut included_files = Vec::new();
        let mut omitted_files = Vec::new();
        for path in paths {
            let section = match &target {
                ReviewTarget::Working | ReviewTarget::Custom(_) => {
                    run_git(
                        &self.root,
                        &[
                            "diff",
                            "HEAD",
                            "--no-color",
                            "--no-ext-diff",
                            "--unified=3",
                            "--",
                            &path,
                        ],
                    )
                    .await?
                }
                ReviewTarget::Base(reference) => {
                    let range = format!("{reference}...HEAD");
                    run_git(
                        &self.root,
                        &[
                            "diff",
                            &range,
                            "--no-color",
                            "--no-ext-diff",
                            "--unified=3",
                            "--",
                            &path,
                        ],
                    )
                    .await?
                }
                ReviewTarget::Commit(sha) => {
                    run_git(
                        &self.root,
                        &[
                            "show",
                            "--format=",
                            "--no-color",
                            "--no-ext-diff",
                            "--unified=3",
                            sha,
                            "--",
                            &path,
                        ],
                    )
                    .await?
                }
            };
            let section = format!(
                "\n--- FILE: {} ---\n{}",
                sanitize_terminal_text(&path),
                sanitize_terminal_text(&section)
            );
            if diff.len().saturating_add(section.len()) > MAX_REVIEW_DIFF_BYTES {
                omitted_files.push(path);
            } else {
                diff.push_str(&section);
                included_files.push(path);
            }
        }
        Ok(ReviewInput {
            scope,
            diff,
            included_files,
            omitted_files,
            untracked_files,
            custom_instructions,
        })
    }

    pub async fn load_for_review(
        &self,
        purpose: ChangeReviewPurpose,
        timeout_secs: u64,
    ) -> Result<ChangeReviewLoad, String> {
        let (repository, changes) = if purpose == ChangeReviewPurpose::Commit {
            let repository = self.commit_repository_state(timeout_secs).await?;
            let changes = GitChangeService::new(&repository.root)
                .load_internal(true, Some(timeout_secs))
                .await?;
            (Some(repository), changes)
        } else {
            (None, self.load().await?)
        };
        if purpose == ChangeReviewPurpose::Commit {
            let conflicts = changes
                .files
                .iter()
                .filter(|file| file.conflicted)
                .map(ChangeFile::display_path)
                .collect::<Vec<_>>();
            if !conflicts.is_empty() {
                return Err(format!(
                    "Commit is unavailable while conflicts remain: {}",
                    conflicts.join(", ")
                ));
            }
        }
        Ok(ChangeReviewLoad {
            repository,
            changes,
        })
    }

    pub async fn stage_file(
        &self,
        expected: &ChangeFile,
        timeout_secs: u64,
    ) -> Result<ChangeReviewLoad, String> {
        let current = self
            .load_for_review(ChangeReviewPurpose::Commit, timeout_secs)
            .await?;
        let file = exact_current_file(&current.changes, expected)?;
        let repository = current
            .repository
            .as_ref()
            .ok_or("repository state unavailable for staging")?;
        if file.staged && !file.unstaged {
            return Err(format!("{} is already fully staged", file.display_path()));
        }
        let mut args = vec![
            OsString::from("--literal-pathspecs"),
            OsString::from("add"),
            OsString::from("-A"),
            OsString::from("--"),
        ];
        if file.kind == ChangeKind::Renamed && file.staged {
            if let Some(old_path) = &file.old_path {
                let remove_old = vec![
                    OsString::from("--literal-pathspecs"),
                    OsString::from("update-index"),
                    OsString::from("--remove"),
                    OsString::from("--"),
                    old_path.as_os_str().to_os_string(),
                ];
                run_git_mutation(&repository.root, &remove_old, timeout_secs).await?;
            }
            args.push(file.path.as_os_str().to_os_string());
        } else {
            args.extend(file.literal_pathspecs());
        }
        run_git_mutation(&repository.root, &args, timeout_secs).await?;
        GitChangeService::new(&repository.root)
            .load_for_review(ChangeReviewPurpose::Commit, timeout_secs)
            .await
    }

    pub async fn unstage_file(
        &self,
        expected: &ChangeFile,
        timeout_secs: u64,
    ) -> Result<ChangeReviewLoad, String> {
        let current = self
            .load_for_review(ChangeReviewPurpose::Commit, timeout_secs)
            .await?;
        let repository = current
            .repository
            .as_ref()
            .ok_or("repository state unavailable for unstaging")?;
        let file = exact_current_file(&current.changes, expected)?;
        if !file.staged {
            return Err(format!("{} has no staged changes", file.display_path()));
        }
        let mut args = if repository.head.is_some() {
            vec![
                OsString::from("--literal-pathspecs"),
                OsString::from("restore"),
                OsString::from("--staged"),
                OsString::from("--source=HEAD"),
                OsString::from("--"),
            ]
        } else {
            vec![
                OsString::from("--literal-pathspecs"),
                OsString::from("rm"),
                OsString::from("--cached"),
                OsString::from("-f"),
                OsString::from("--ignore-unmatch"),
                OsString::from("--"),
            ]
        };
        args.extend(file.literal_pathspecs());
        run_git_mutation(&repository.root, &args, timeout_secs).await?;
        GitChangeService::new(&repository.root)
            .load_for_review(ChangeReviewPurpose::Commit, timeout_secs)
            .await
    }

    pub async fn prepare_commit(&self, timeout_secs: u64) -> Result<GitCommitSnapshot, String> {
        let loaded = self
            .load_for_review(ChangeReviewPurpose::Commit, timeout_secs)
            .await?;
        let repository = loaded
            .repository
            .as_ref()
            .ok_or("repository state unavailable for commit preparation")?;
        let index_tree = run_git_bounded(&repository.root, &["write-tree"], timeout_secs).await?;
        let index_tree = index_tree.trim().to_string();
        let head_tree = if repository.head.is_some() {
            Some(
                run_git_bounded(
                    &repository.root,
                    &["rev-parse", "HEAD^{tree}"],
                    timeout_secs,
                )
                .await?
                .trim()
                .to_string(),
            )
        } else {
            None
        };
        let staged = loaded
            .changes
            .files
            .iter()
            .filter(|file| file.staged)
            .collect::<Vec<_>>();
        if staged.is_empty() || head_tree.as_deref() == Some(index_tree.as_str()) {
            return Err("No staged changes to commit".into());
        }
        let mut files = Vec::with_capacity(staged.len());
        for file in staged {
            let mut additions = 0;
            let mut deletions = 0;
            let mut binary = false;
            let output = run_git_paths_bounded(
                &repository.root,
                &["diff", "--cached", "--numstat", "--"],
                std::slice::from_ref(&file.path),
                timeout_secs,
            )
            .await?;
            accumulate_numstat(&output, &mut additions, &mut deletions, &mut binary);
            files.push(StagedChange {
                path: file.path.clone(),
                old_path: file.old_path.clone(),
                kind: file.kind,
                additions: (!binary).then_some(additions),
                deletions: (!binary).then_some(deletions),
            });
        }
        Ok(GitCommitSnapshot {
            branch: repository.branch.clone(),
            head: repository.head.clone(),
            index_tree,
            files,
        })
    }

    pub async fn commit(
        &self,
        expected: &GitCommitSnapshot,
        subject: &str,
        timeout_secs: u64,
    ) -> Result<GitCommitResult, GitCommitFailure> {
        let subject = validate_commit_subject(subject).map_err(commit_failure)?;
        let current = self
            .prepare_commit(timeout_secs)
            .await
            .map_err(commit_failure)?;
        if current.branch != expected.branch
            || current.head != expected.head
            || current.index_tree != expected.index_tree
        {
            return Err(commit_failure(
                "Repository state changed after confirmation; refresh before committing".into(),
            ));
        }
        let repository = self
            .commit_repository_state(timeout_secs)
            .await
            .map_err(commit_failure)?;
        let index_tree = run_git_bounded(&repository.root, &["write-tree"], timeout_secs)
            .await
            .map_err(commit_failure)?;
        if repository.branch != expected.branch
            || repository.head != expected.head
            || index_tree.trim() != expected.index_tree
        {
            return Err(commit_failure(
                "Repository state changed immediately before commit; refresh and confirm again"
                    .into(),
            ));
        }
        let args = vec![
            OsString::from("commit"),
            OsString::from("-m"),
            OsString::from(&subject),
        ];
        let command_result = run_git_mutation(&repository.root, &args, timeout_secs).await;
        let after = self.commit_repository_state(timeout_secs).await;
        let after_head = after.as_ref().ok().and_then(|state| state.head.clone());
        if let Err(error) = command_result {
            let uncertain = after_head != expected.head;
            return Err(GitCommitFailure {
                message: if uncertain {
                    format!(
                        "Git did not report success, but HEAD changed; inspect the repository before retrying: {error}"
                    )
                } else {
                    error
                },
                uncertain,
            });
        }
        let after = after.map_err(|error| GitCommitFailure {
            message: format!(
                "Git reported success, but the new HEAD could not be verified; inspect the repository: {error}"
            ),
            uncertain: true,
        })?;
        let Some(sha) = after.head.clone() else {
            return Err(GitCommitFailure {
                message: "Git reported success, but HEAD is still unborn; inspect the repository"
                    .into(),
                uncertain: true,
            });
        };
        if after.branch != expected.branch {
            return Err(GitCommitFailure {
                message:
                    "Git reported success, but the current branch changed; inspect the repository"
                        .into(),
                uncertain: true,
            });
        }
        let tree = run_git_bounded(&after.root, &["rev-parse", "HEAD^{tree}"], timeout_secs)
            .await
            .map_err(|error| GitCommitFailure {
                message: format!(
                    "Git reported success, but the commit tree could not be verified: {error}"
                ),
                uncertain: true,
            })?;
        let parents = run_git_bounded(
            &after.root,
            &["rev-list", "--parents", "-n", "1", "HEAD"],
            timeout_secs,
        )
        .await
        .map_err(|error| GitCommitFailure {
            message: format!(
                "Git reported success, but the commit parent could not be verified: {error}"
            ),
            uncertain: true,
        })?;
        let parent_fields = parents.split_whitespace().collect::<Vec<_>>();
        let parent_matches = match expected.head.as_deref() {
            Some(parent) => parent_fields.len() == 2 && parent_fields[1] == parent,
            None => parent_fields.len() == 1,
        };
        if tree.trim() != expected.index_tree || !parent_matches {
            return Err(GitCommitFailure {
                message:
                    "The new commit does not match the confirmed tree and parent; inspect the repository"
                        .into(),
                uncertain: true,
            });
        }
        let short_sha = run_git_bounded(
            &after.root,
            &["rev-parse", "--short=12", "HEAD"],
            timeout_secs,
        )
        .await
        .map_err(|error| GitCommitFailure {
            message: format!("Commit created, but its short SHA could not be read: {error}"),
            uncertain: true,
        })?
        .trim()
        .to_string();
        Ok(GitCommitResult {
            sha,
            short_sha,
            subject,
        })
    }

    async fn commit_repository_state(
        &self,
        timeout_secs: u64,
    ) -> Result<GitRepositoryState, String> {
        let root_args = ["rev-parse", "--show-toplevel"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let root_bytes = run_git_bounded_bytes(&self.root, &root_args, timeout_secs).await?;
        let root = bytes_to_path(trim_line_end(&root_bytes))?;
        let branch = run_git_bounded(
            &root,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
            timeout_secs,
        )
        .await
        .map_err(|_| "Commit is unavailable while HEAD is detached".to_string())?;
        let branch = sanitize_terminal_text(branch.trim());
        if branch.is_empty() {
            return Err("Commit is unavailable without a named branch".into());
        }
        let head =
            optional_git_bounded(&root, &["rev-parse", "--verify", "HEAD"], timeout_secs).await?;
        let git_dir_args = ["rev-parse", "--absolute-git-dir"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let git_dir_bytes = run_git_bounded_bytes(&root, &git_dir_args, timeout_secs).await?;
        let git_dir = bytes_to_path(trim_line_end(&git_dir_bytes))?;
        if let Some(operation) = active_git_operation(&git_dir) {
            return Err(format!(
                "Commit is unavailable during an active Git {operation}; use Git directly"
            ));
        }
        Ok(GitRepositoryState {
            root,
            branch,
            head: head.map(|value| value.trim().to_string()),
        })
    }
}

pub fn validate_commit_subject(subject: &str) -> Result<String, String> {
    let subject = subject.trim();
    if subject.is_empty() {
        return Err("Commit subject cannot be empty".into());
    }
    if subject.chars().count() > MAX_COMMIT_SUBJECT_CHARS {
        return Err(format!(
            "Commit subject must be at most {MAX_COMMIT_SUBJECT_CHARS} characters"
        ));
    }
    if subject.chars().any(char::is_control) {
        return Err("Commit subject must be one line without control characters".into());
    }
    Ok(subject.to_string())
}

fn commit_failure(message: String) -> GitCommitFailure {
    GitCommitFailure {
        message,
        uncertain: false,
    }
}

fn exact_current_file<'a>(
    changes: &'a ChangeSet,
    expected: &ChangeFile,
) -> Result<&'a ChangeFile, String> {
    changes
        .files
        .iter()
        .find(|file| file.path == expected.path)
        .filter(|file| {
            file.old_path == expected.old_path
                && file.kind == expected.kind
                && file.conflicted == expected.conflicted
                && file.staged == expected.staged
                && file.unstaged == expected.unstaged
                && file.state_fingerprint == expected.state_fingerprint
        })
        .ok_or_else(|| {
            format!(
                "{} changed since it was displayed; refresh before changing the index",
                expected.display_path()
            )
        })
}

fn active_git_operation(git_dir: &Path) -> Option<&'static str> {
    [
        ("MERGE_HEAD", "merge"),
        ("CHERRY_PICK_HEAD", "cherry-pick"),
        ("REVERT_HEAD", "revert"),
        ("REBASE_HEAD", "rebase"),
        ("rebase-merge", "rebase"),
        ("rebase-apply", "rebase"),
        ("sequencer", "sequencer"),
        ("BISECT_LOG", "bisect"),
    ]
    .into_iter()
    .find_map(|(path, label)| git_dir.join(path).exists().then_some(label))
}

fn validate_git_ref(reference: &str) -> Result<(), String> {
    let valid = !reference.is_empty()
        && !reference.starts_with('-')
        && !reference.contains("..")
        && !reference.contains("@{")
        && !reference.ends_with('/')
        && reference
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._/-".contains(character));
    valid
        .then_some(())
        .ok_or_else(|| "Unsafe Git reference".into())
}

fn validate_commit_sha(sha: &str) -> Result<(), String> {
    (sha.len() >= 7
        && sha.len() <= 40
        && sha.chars().all(|character| character.is_ascii_hexdigit()))
    .then_some(())
    .ok_or_else(|| "Commit must be a 7-40 character hexadecimal SHA".into())
}

async fn verify_commit(root: &Path, reference: &str) -> Result<(), String> {
    let commit = format!("{reference}^{{commit}}");
    run_git(root, &["rev-parse", "--verify", "--quiet", &commit])
        .await
        .map(|_| ())
}

async fn git_paths(root: &Path, args: &[&str]) -> Result<Vec<String>, String> {
    Ok(run_git(root, args)
        .await?
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(sanitize_terminal_text)
        .collect())
}

async fn run_git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = run_git_bytes(root, args).await?;
    Ok(String::from_utf8_lossy(&output).into_owned())
}

async fn run_git_bounded(root: &Path, args: &[&str], timeout_secs: u64) -> Result<String, String> {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    let output = run_git_bounded_bytes(root, &args, timeout_secs).await?;
    Ok(String::from_utf8_lossy(&output).into_owned())
}

async fn run_git_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    if ca_automation::runtime::current_run_store().is_some() {
        let args = args.iter().map(OsString::from).collect::<Vec<_>>();
        return run_git_bounded_bytes(root, &args, 30).await;
    }
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .await
        .map_err(|error| format!("Git could not be started: {error}"))?;
    if !output.status.success() {
        let error = sanitize_terminal_text(&String::from_utf8_lossy(&output.stderr));
        return Err(if error.trim().is_empty() {
            "Git command failed".into()
        } else {
            error.trim().to_string()
        });
    }
    Ok(output.stdout)
}

async fn optional_git_bounded(
    root: &Path,
    args: &[&str],
    timeout_secs: u64,
) -> Result<Option<String>, String> {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    let output = run_git_process(root, &args, timeout_secs).await?;
    if output.status.success() {
        Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
    } else {
        Ok(None)
    }
}

async fn run_git_paths(root: &Path, prefix: &[&str], paths: &[PathBuf]) -> Result<String, String> {
    Ok(String::from_utf8_lossy(&run_git_paths_bytes(root, prefix, paths).await?).into_owned())
}

async fn run_git_paths_bytes(
    root: &Path,
    prefix: &[&str],
    paths: &[PathBuf],
) -> Result<Vec<u8>, String> {
    let mut args = Vec::with_capacity(prefix.len() + paths.len() + 1);
    args.push(OsString::from("--literal-pathspecs"));
    args.extend(prefix.iter().map(OsString::from));
    args.extend(paths.iter().map(|path| path.as_os_str().to_os_string()));
    if ca_automation::runtime::current_run_store().is_some() {
        return run_git_bounded_bytes(root, &args, 30).await;
    }
    let output = Command::new("git")
        .args(&args)
        .current_dir(root)
        .output()
        .await
        .map_err(|error| format!("Git could not be started: {error}"))?;
    if !output.status.success() {
        let error = sanitize_terminal_text(&String::from_utf8_lossy(&output.stderr));
        return Err(if error.trim().is_empty() {
            "Git command failed".into()
        } else {
            error.trim().to_string()
        });
    }
    Ok(output.stdout)
}

async fn run_git_paths_bounded(
    root: &Path,
    prefix: &[&str],
    paths: &[PathBuf],
    timeout_secs: u64,
) -> Result<String, String> {
    let output = run_git_paths_bounded_bytes(root, prefix, paths, timeout_secs).await?;
    Ok(String::from_utf8_lossy(&output).into_owned())
}

async fn run_git_paths_bounded_bytes(
    root: &Path,
    prefix: &[&str],
    paths: &[PathBuf],
    timeout_secs: u64,
) -> Result<Vec<u8>, String> {
    let mut args = Vec::with_capacity(prefix.len() + paths.len() + 1);
    args.push(OsString::from("--literal-pathspecs"));
    args.extend(prefix.iter().map(OsString::from));
    args.extend(paths.iter().map(|path| path.as_os_str().to_os_string()));
    run_git_bounded_bytes(root, &args, timeout_secs).await
}

async fn change_state_fingerprint(
    root: &Path,
    file: &ChangeFile,
    timeout_secs: u64,
) -> Result<String, String> {
    let mut digest = Sha256::new();
    digest.update([file.kind as u8]);
    digest.update([
        u8::from(file.conflicted),
        u8::from(file.staged),
        u8::from(file.unstaged),
    ]);
    let paths = file
        .old_path
        .iter()
        .chain(std::iter::once(&file.path))
        .cloned()
        .collect::<Vec<_>>();
    let index = run_git_paths_bounded_bytes(
        root,
        &["ls-files", "--stage", "-z", "--"],
        &paths,
        timeout_secs,
    )
    .await?;
    digest.update((index.len() as u64).to_le_bytes());
    digest.update(index);
    for path in paths {
        update_os_str_digest(&mut digest, path.as_os_str());
        match tokio::fs::symlink_metadata(root.join(&path)).await {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                digest.update(b"symlink\0");
                let target = tokio::fs::read_link(root.join(&path))
                    .await
                    .map_err(|error| {
                        format!(
                            "Could not inspect {} while refreshing Git state: {error}",
                            sanitize_terminal_text(&path.to_string_lossy())
                        )
                    })?;
                update_os_str_digest(&mut digest, target.as_os_str());
            }
            Ok(metadata) if metadata.is_file() => {
                digest.update(b"file\0");
                let hash = run_git_paths_bounded_bytes(
                    root,
                    &["hash-object", "--no-filters", "--"],
                    std::slice::from_ref(&path),
                    timeout_secs,
                )
                .await?;
                digest.update(hash);
            }
            Ok(metadata) => {
                digest.update(b"other\0");
                digest.update(metadata.len().to_le_bytes());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                digest.update(b"missing\0");
            }
            Err(error) => {
                return Err(format!(
                    "Could not inspect {} while refreshing Git state: {error}",
                    sanitize_terminal_text(&path.to_string_lossy())
                ));
            }
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn update_os_str_digest(digest: &mut Sha256, value: &std::ffi::OsStr) {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let bytes = value.as_bytes();
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    #[cfg(not(unix))]
    {
        let value = value.to_string_lossy();
        digest.update((value.len() as u64).to_le_bytes());
        digest.update(value.as_bytes());
    }
}

fn parse_porcelain(output: &[u8]) -> Result<Vec<ChangeFile>, String> {
    let records: Vec<&[u8]> = output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect();
    let mut files = Vec::new();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        if record.len() < 3 {
            return Err("Git returned an invalid status record".into());
        }
        let staged_status = record[0] as char;
        let unstaged_status = record[1] as char;
        let path = bytes_to_path(&record[3..])?;
        let mut old_path = None;
        let renamed = staged_status == 'R'
            || staged_status == 'C'
            || unstaged_status == 'R'
            || unstaged_status == 'C';
        if renamed {
            index += 1;
            let Some(previous) = records.get(index) else {
                return Err("Git returned an incomplete rename record".into());
            };
            old_path = Some(bytes_to_path(previous)?);
        }
        let untracked = staged_status == '?' && unstaged_status == '?';
        let conflicted = matches!(
            (staged_status, unstaged_status),
            ('D', 'D')
                | ('A', 'U')
                | ('U', 'D')
                | ('U', 'A')
                | ('D', 'U')
                | ('A', 'A')
                | ('U', 'U')
        );
        let kind = if untracked {
            ChangeKind::Untracked
        } else if renamed {
            ChangeKind::Renamed
        } else if staged_status == 'D' || unstaged_status == 'D' {
            ChangeKind::Deleted
        } else if staged_status == 'A' || unstaged_status == 'A' {
            ChangeKind::Added
        } else {
            ChangeKind::Modified
        };
        if path.as_os_str().is_empty() {
            return Err("Git returned an empty status path".into());
        }
        files.push(ChangeFile {
            path,
            old_path,
            kind,
            conflicted,
            staged: !untracked && staged_status != ' ',
            unstaged: untracked || unstaged_status != ' ',
            additions: None,
            deletions: None,
            state_fingerprint: String::new(),
        });
        index += 1;
    }
    Ok(files)
}

fn trim_line_end(mut bytes: &[u8]) -> &[u8] {
    while matches!(bytes.last(), Some(b'\n' | b'\r')) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn bytes_to_path(bytes: &[u8]) -> Result<PathBuf, String> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(bytes.to_vec())
            .map(PathBuf::from)
            .map_err(|_| "Git returned a non-UTF-8 path on this platform".into())
    }
}

struct BoundedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

struct GitProcessOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

async fn read_bounded<R>(mut reader: R) -> std::io::Result<BoundedOutput>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = MAX_GIT_MUTATION_OUTPUT_BYTES.saturating_sub(bytes.len());
        let keep = remaining.min(read);
        bytes.extend_from_slice(&buffer[..keep]);
        truncated |= keep < read;
    }
    Ok(BoundedOutput { bytes, truncated })
}

struct ProcessGroupGuard {
    process_group: Option<i32>,
}

impl ProcessGroupGuard {
    fn disarm(&mut self) {
        self.process_group = None;
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        terminate_process_group(self.process_group);
    }
}

#[cfg(unix)]
fn terminate_process_group(process_group: Option<i32>) {
    if let Some(process_group) = process_group {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(process_group),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}

#[cfg(not(unix))]
fn terminate_process_group(_process_group: Option<i32>) {}

async fn run_git_mutation(root: &Path, args: &[OsString], timeout_secs: u64) -> Result<(), String> {
    let output = run_git_process(root, args, timeout_secs).await?;
    if !output.status.success() {
        return Err(git_process_error(&output));
    }
    Ok(())
}

async fn run_git_bounded_bytes(
    root: &Path,
    args: &[OsString],
    timeout_secs: u64,
) -> Result<Vec<u8>, String> {
    let output = run_git_process(root, args, timeout_secs).await?;
    if !output.status.success() {
        return Err(git_process_error(&output));
    }
    Ok(output.stdout)
}

async fn run_git_process(
    root: &Path,
    args: &[OsString],
    timeout_secs: u64,
) -> Result<GitProcessOutput, String> {
    let native = ca_automation::runtime::current_run_store().is_some();
    let mut command = if native {
        let root = root.to_path_buf();
        let trusted = tokio::task::spawn_blocking(move || {
            ca_automation::git_metadata::command_no_filters(&root)
        })
        .await
        .map_err(|error| format!("Git coordinator inventory stopped: {error}"))?
        .map_err(|error| format!("Git coordinator inventory failed: {error}"))?;
        Command::from(trusted)
    } else {
        Command::new("git")
    };
    let mut safe_args = args.to_vec();
    let command_index = usize::from(args.first().is_some_and(|arg| arg == "--literal-pathspecs"));
    if native && args.get(command_index).is_some_and(|arg| arg == "diff") {
        safe_args.splice(
            command_index + 1..command_index + 1,
            [
                OsString::from("--no-ext-diff"),
                OsString::from("--no-textconv"),
            ],
        );
    }
    command
        .args(&safe_args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let owned_group = !native || !crate::jobs::worker_contained();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        if owned_group {
            command.as_std_mut().process_group(0);
        }
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Git could not be started: {error}"))?;
    let process_group = owned_group
        .then(|| child.id().and_then(|id| i32::try_from(id).ok()))
        .flatten();
    let mut guard = ProcessGroupGuard { process_group };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Git stdout was unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Git stderr was unavailable".to_string())?;
    let stdout_task = tokio::spawn(read_bounded(stdout));
    let stderr_task = tokio::spawn(read_bounded(stderr));
    let status =
        match tokio::time::timeout(Duration::from_secs(timeout_secs.max(1)), child.wait()).await {
            Ok(result) => result.map_err(|error| format!("Git wait failed: {error}"))?,
            Err(_) => {
                terminate_process_group(process_group);
                let _ = child.kill().await;
                let _ = child.wait().await;
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                guard.disarm();
                return Err(format!(
                    "Git command timed out after {} seconds",
                    timeout_secs.max(1)
                ));
            }
        };
    let stdout = stdout_task
        .await
        .map_err(|error| format!("Git stdout task failed: {error}"))?
        .map_err(|error| format!("Git stdout read failed: {error}"))?;
    let stderr = stderr_task
        .await
        .map_err(|error| format!("Git stderr task failed: {error}"))?
        .map_err(|error| format!("Git stderr read failed: {error}"))?;
    guard.disarm();
    if stdout.truncated || stderr.truncated {
        return Err("Git command output exceeded the 1 MiB safety limit".into());
    }
    Ok(GitProcessOutput {
        status,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
    })
}

fn git_process_error(output: &GitProcessOutput) -> String {
    let stderr = sanitize_terminal_text(&String::from_utf8_lossy(&output.stderr));
    let stdout = sanitize_terminal_text(&String::from_utf8_lossy(&output.stdout));
    if !stderr.trim().is_empty() {
        stderr.trim().to_string()
    } else if !stdout.trim().is_empty() {
        stdout.trim().to_string()
    } else {
        "Git command failed".into()
    }
}

fn accumulate_numstat(output: &str, additions: &mut u64, deletions: &mut u64, binary: &mut bool) {
    for line in output.lines() {
        let mut fields = line.splitn(3, '\t');
        let added = fields.next().unwrap_or("-");
        let deleted = fields.next().unwrap_or("-");
        match (added.parse::<u64>(), deleted.parse::<u64>()) {
            (Ok(added), Ok(deleted)) => {
                *additions += added;
                *deletions += deleted;
            }
            _ => *binary = true,
        }
    }
}

pub use crate::terminal_text::sanitize_terminal_text;

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;
    use tempfile::TempDir;

    fn git(root: &Path, args: &[&str]) {
        let status = StdCommand::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap();
        assert!(status.success(), "git command failed: {args:?}");
    }

    fn git_output(root: &Path, args: &[&str]) -> String {
        let output = StdCommand::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success(), "git command failed: {args:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    fn dirty_repository() -> TempDir {
        let root = TempDir::new().unwrap();
        git(root.path(), &["init", "-q"]);
        std::fs::write(root.path().join("modified.txt"), "base\n").unwrap();
        std::fs::write(root.path().join("deleted.txt"), "delete me\n").unwrap();
        std::fs::write(root.path().join("rename-me.txt"), "rename me\n").unwrap();
        std::fs::write(root.path().join("long.txt"), "short\n").unwrap();
        git(root.path(), &["add", "."]);
        git(
            root.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "base",
            ],
        );

        std::fs::write(root.path().join("modified.txt"), "base\nstaged\n").unwrap();
        git(root.path(), &["add", "modified.txt"]);
        std::fs::write(root.path().join("modified.txt"), "base\nstaged\nunstaged\n").unwrap();
        std::fs::remove_file(root.path().join("deleted.txt")).unwrap();
        git(root.path(), &["mv", "rename-me.txt", "renamed.txt"]);
        std::fs::write(root.path().join("renamed.txt"), "rename me\nupdated\n").unwrap();
        std::fs::write(root.path().join("added.txt"), "added\n").unwrap();
        git(root.path(), &["add", "added.txt"]);
        std::fs::write(root.path().join("unicodé-界.txt"), "private contents\n").unwrap();
        std::fs::write(
            root.path().join("long.txt"),
            format!("{}\n", "a very long changed line ".repeat(20)),
        )
        .unwrap();
        root
    }

    fn configure_identity(root: &Path) {
        git(root, &["config", "user.name", "Test User"]);
        git(root, &["config", "user.email", "test@example.com"]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_git_callbacks_never_execute_repository_metadata_or_commit_helpers() {
        use std::os::unix::fs::PermissionsExt;
        let repository = dirty_repository();
        configure_identity(repository.path());
        let private = TempDir::new().unwrap();
        let private_root = private.path().canonicalize().unwrap();
        let marker = private.path().join("repository-helper-ran");
        let helper = private.path().join("helper");
        std::fs::write(
            &helper,
            format!("#!/bin/sh\ntouch '{}'\ncat\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let helper = helper.to_str().unwrap();
        for key in [
            "core.fsmonitor",
            "diff.external",
            "diff.fixture.textconv",
            "filter.fixture.clean",
            "filter.fixture.process",
            "filter.fixture.smudge",
            "gpg.program",
        ] {
            git(repository.path(), &["config", key, helper]);
        }
        git(repository.path(), &["config", "commit.gpgSign", "true"]);
        std::fs::write(
            repository.path().join(".gitattributes"),
            "modified.txt diff=fixture filter=fixture\n",
        )
        .unwrap();
        let context = ca_automation::runtime::ExecutionContext {
            schema: ca_automation::runtime::EXECUTION_SCHEMA.into(),
            run_id: "changes-callback".into(),
            profile: "default".into(),
            state_root: private_root.join("state"),
            lease_root: private_root.join("leases"),
            definition_digest: "a".repeat(64),
            resource_binding_digest: "b".repeat(64),
            policy_digest: "c".repeat(64),
            resources: Vec::new(),
            runtime_seconds: 30,
            output_bytes: 8192,
        };
        let store = ca_automation::runtime::RunStore::open(&context).unwrap();
        ca_automation::runtime::EXECUTION_STORE
            .scope(store, async {
                run_git_bytes(repository.path(), &["status", "--porcelain=v1"])
                    .await
                    .unwrap();
                run_git_paths_bytes(
                    repository.path(),
                    &["diff", "--numstat", "--"],
                    &[PathBuf::from("modified.txt")],
                )
                .await
                .unwrap();
                run_git_mutation(repository.path(), &["add".into(), "--all".into()], 5)
                    .await
                    .unwrap();
                run_git_mutation(
                    repository.path(),
                    &["commit".into(), "-m".into(), "candidate".into()],
                    5,
                )
                .await
                .unwrap();
            })
            .await;
        assert!(!marker.exists());
        assert_eq!(
            std::fs::read_to_string(repository.path().join("modified.txt")).unwrap(),
            "base\nstaged\nunstaged\n"
        );
    }

    fn find_change(changes: &ChangeSet, path: &str) -> ChangeFile {
        changes
            .files
            .iter()
            .find(|file| file.path == Path::new(path))
            .unwrap_or_else(|| panic!("missing change {path}"))
            .clone()
    }

    #[tokio::test]
    async fn loads_all_working_tree_statuses_and_line_counts() {
        let root = dirty_repository();
        let changes = GitChangeService::new(root.path()).load().await.unwrap();

        assert!(changes.files.iter().any(|file| {
            file.path == Path::new("modified.txt") && file.staged && file.unstaged
        }));
        assert!(changes
            .files
            .iter()
            .any(|file| file.kind == ChangeKind::Deleted));
        assert!(changes.files.iter().any(|file| {
            file.kind == ChangeKind::Renamed
                && file.path == Path::new("renamed.txt")
                && file.old_path.as_deref() == Some(Path::new("rename-me.txt"))
        }));
        assert!(changes.files.iter().any(|file| {
            file.kind == ChangeKind::Untracked && file.path == Path::new("unicodé-界.txt")
        }));
        assert!(changes
            .files
            .iter()
            .filter(|file| file.is_tracked())
            .all(|file| { file.additions.is_some() && file.deletions.is_some() }));
    }

    #[tokio::test]
    async fn tracked_diff_is_sanitized_and_untracked_content_is_not_opened() {
        let root = dirty_repository();
        let service = GitChangeService::new(root.path());
        let changes = service.load().await.unwrap();
        let modified = changes
            .files
            .iter()
            .find(|file| file.path == Path::new("modified.txt"))
            .unwrap();
        let diff = service.unified_diff(modified).await.unwrap();
        assert!(diff.contains("Staged changes"));
        assert!(diff.contains("Unstaged changes"));
        assert!(!diff.contains('\u{1b}'));

        let untracked = changes
            .files
            .iter()
            .find(|file| file.kind == ChangeKind::Untracked)
            .unwrap();
        assert_eq!(
            service.unified_diff(untracked).await,
            Err("Untracked file contents are not opened automatically".into())
        );
    }

    #[tokio::test]
    async fn non_repository_error_is_deterministic() {
        let root = TempDir::new().unwrap();
        let error = GitChangeService::new(root.path()).load().await.unwrap_err();
        assert!(!error.is_empty());
        assert!(!error.contains('\u{1b}'));
    }

    #[tokio::test]
    async fn working_review_includes_tracked_diff_but_only_names_untracked_files() {
        let root = dirty_repository();
        let input = GitChangeService::new(root.path())
            .review_input(ReviewTarget::Working)
            .await
            .unwrap();

        assert!(input.diff.contains("modified.txt"));
        assert!(input.untracked_files.contains(&"unicodé-界.txt".into()));
        assert!(!input.diff.contains("private contents"));
        assert!(input.omitted_files.is_empty());
    }

    #[tokio::test]
    async fn base_commit_and_custom_targets_are_argument_safe() {
        let root = TempDir::new().unwrap();
        git(root.path(), &["init", "-q"]);
        std::fs::write(root.path().join("source.rs"), "fn old() {}\n").unwrap();
        git(root.path(), &["add", "."]);
        git(
            root.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "base",
            ],
        );
        git(root.path(), &["branch", "base-point"]);
        std::fs::write(root.path().join("source.rs"), "fn new() {}\n").unwrap();
        git(root.path(), &["add", "."]);
        git(
            root.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "change",
            ],
        );
        let sha = git_output(root.path(), &["rev-parse", "HEAD"]);
        let service = GitChangeService::new(root.path());

        let base = service
            .review_input(ReviewTarget::Base("base-point".into()))
            .await
            .unwrap();
        assert!(base.diff.contains("fn new"));
        let commit = service
            .review_input(ReviewTarget::Commit(sha))
            .await
            .unwrap();
        assert!(commit.diff.contains("fn new"));
        let custom = service
            .review_input(ReviewTarget::Custom("focus on correctness".into()))
            .await
            .unwrap();
        assert_eq!(
            custom.custom_instructions.as_deref(),
            Some("focus on correctness")
        );

        assert!(service
            .review_input(ReviewTarget::Base("--help".into()))
            .await
            .is_err());
        assert!(service
            .review_input(ReviewTarget::Base("main..evil".into()))
            .await
            .is_err());
        assert!(service
            .review_input(ReviewTarget::Commit("not-a-sha".into()))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn review_cap_omits_whole_files_instead_of_splitting_a_diff() {
        let root = TempDir::new().unwrap();
        git(root.path(), &["init", "-q"]);
        for name in ["one.txt", "two.txt"] {
            std::fs::write(root.path().join(name), "base\n").unwrap();
        }
        git(root.path(), &["add", "."]);
        git(
            root.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "base",
            ],
        );
        let large = format!("{}\n", "x".repeat(700_000));
        std::fs::write(root.path().join("one.txt"), &large).unwrap();
        std::fs::write(root.path().join("two.txt"), &large).unwrap();

        let input = GitChangeService::new(root.path())
            .review_input(ReviewTarget::Working)
            .await
            .unwrap();
        assert!(input.diff.len() <= MAX_REVIEW_DIFF_BYTES);
        assert_eq!(input.included_files.len(), 1);
        assert_eq!(input.omitted_files.len(), 1);
        assert!(input
            .diff
            .contains(&format!("--- FILE: {} ---", input.included_files[0])));
        assert!(!input
            .diff
            .contains(&format!("--- FILE: {} ---", input.omitted_files[0])));
    }

    #[test]
    fn sanitizer_removes_ansi_and_control_sequences() {
        assert_eq!(
            sanitize_terminal_text("\u{1b}[31mred\u{1b}[0m\u{7}\nnext"),
            "red\nnext"
        );
    }

    #[test]
    fn commit_subject_is_trimmed_bounded_and_single_line() {
        assert_eq!(
            validate_commit_subject("  focused change  ").unwrap(),
            "focused change"
        );
        assert!(validate_commit_subject("   ").is_err());
        assert!(validate_commit_subject("line one\nline two").is_err());
        assert!(validate_commit_subject(&"x".repeat(MAX_COMMIT_SUBJECT_CHARS + 1)).is_err());
        assert_eq!(
            validate_commit_subject("literal $(touch should-not-run); value").unwrap(),
            "literal $(touch should-not-run); value"
        );
    }

    #[tokio::test]
    async fn stage_and_unstage_are_exact_whole_file_mutations() {
        let root = dirty_repository();
        let service = GitChangeService::new(root.path());
        let before = service
            .load_for_review(ChangeReviewPurpose::Commit, 5)
            .await
            .unwrap();
        let modified = find_change(&before.changes, "modified.txt");
        let loaded = service.stage_file(&modified, 5).await.unwrap();
        let modified = find_change(&loaded.changes, "modified.txt");
        assert!(modified.staged);
        assert!(!modified.unstaged);
        assert_eq!(
            git_output(root.path(), &["show", ":modified.txt"]),
            "base\nstaged\nunstaged"
        );
        let added_before = git_output(root.path(), &["show", ":added.txt"]);

        let loaded = service.unstage_file(&modified, 5).await.unwrap();
        let modified = find_change(&loaded.changes, "modified.txt");
        assert!(!modified.staged);
        assert!(modified.unstaged);
        assert_eq!(
            std::fs::read_to_string(root.path().join("modified.txt")).unwrap(),
            "base\nstaged\nunstaged\n"
        );
        assert_eq!(
            git_output(root.path(), &["show", ":added.txt"]),
            added_before
        );
    }

    #[tokio::test]
    async fn literal_path_staging_does_not_expand_other_paths() {
        let root = TempDir::new().unwrap();
        git(root.path(), &["init", "-q"]);
        configure_identity(root.path());
        std::fs::write(root.path().join("base.txt"), "base\n").unwrap();
        git(root.path(), &["add", "base.txt"]);
        git(root.path(), &["commit", "-qm", "base"]);
        for name in ["-leading.txt", "[glob].txt", "space name.txt", "新規.txt"] {
            std::fs::write(root.path().join(name), format!("{name}\n")).unwrap();
        }
        let service = GitChangeService::new(root.path());
        let loaded = service
            .load_for_review(ChangeReviewPurpose::Commit, 5)
            .await
            .unwrap();
        let selected = find_change(&loaded.changes, "[glob].txt");
        let loaded = service.stage_file(&selected, 5).await.unwrap();
        assert!(find_change(&loaded.changes, "[glob].txt").staged);
        let selected = find_change(&loaded.changes, "[glob].txt");
        service.unstage_file(&selected, 5).await.unwrap();
        for selected_name in ["-leading.txt", "space name.txt", "新規.txt"] {
            let loaded = service
                .load_for_review(ChangeReviewPurpose::Commit, 5)
                .await
                .unwrap();
            let selected = find_change(&loaded.changes, selected_name);
            let loaded = service.stage_file(&selected, 5).await.unwrap();
            assert!(find_change(&loaded.changes, selected_name).staged);
            for other_name in ["-leading.txt", "[glob].txt", "space name.txt", "新規.txt"] {
                if other_name != selected_name {
                    assert!(
                        !find_change(&loaded.changes, other_name).staged,
                        "unexpectedly staged {other_name} while selecting {selected_name}"
                    );
                }
            }
            let selected = find_change(&loaded.changes, selected_name);
            let loaded = service.unstage_file(&selected, 5).await.unwrap();
            assert!(!find_change(&loaded.changes, selected_name).staged);
        }
    }

    #[tokio::test]
    async fn stage_and_unstage_cover_deleted_added_and_renamed_paths() {
        let root = dirty_repository();
        let service = GitChangeService::new(root.path());

        let loaded = service
            .load_for_review(ChangeReviewPurpose::Commit, 5)
            .await
            .unwrap();
        let deleted = find_change(&loaded.changes, "deleted.txt");
        let loaded = service.stage_file(&deleted, 5).await.unwrap();
        let deleted = find_change(&loaded.changes, "deleted.txt");
        assert!(deleted.staged && !deleted.unstaged);
        let loaded = service.unstage_file(&deleted, 5).await.unwrap();
        let deleted = find_change(&loaded.changes, "deleted.txt");
        assert!(!deleted.staged && deleted.unstaged);
        assert!(!root.path().join("deleted.txt").exists());

        let added = find_change(&loaded.changes, "added.txt");
        let loaded = service.unstage_file(&added, 5).await.unwrap();
        let added = find_change(&loaded.changes, "added.txt");
        assert_eq!(added.kind, ChangeKind::Untracked);
        let loaded = service.stage_file(&added, 5).await.unwrap();
        let added = find_change(&loaded.changes, "added.txt");
        assert!(added.staged && !added.unstaged);
        let loaded = service.unstage_file(&added, 5).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(root.path().join("added.txt")).unwrap(),
            "added\n"
        );

        let renamed = find_change(&loaded.changes, "renamed.txt");
        assert_eq!(
            renamed.old_path.as_deref(),
            Some(Path::new("rename-me.txt"))
        );
        assert!(renamed.staged && renamed.unstaged);
        let loaded = service.stage_file(&renamed, 5).await.unwrap();
        let renamed = find_change(&loaded.changes, "renamed.txt");
        assert!(renamed.staged && !renamed.unstaged);
        let loaded = service.unstage_file(&renamed, 5).await.unwrap();
        assert!(find_change(&loaded.changes, "renamed.txt").unstaged);
        assert_eq!(
            std::fs::read_to_string(root.path().join("renamed.txt")).unwrap(),
            "rename me\nupdated\n"
        );
    }

    #[tokio::test]
    async fn staging_refuses_a_selected_path_whose_bytes_changed() {
        let root = TempDir::new().unwrap();
        git(root.path(), &["init", "-q"]);
        configure_identity(root.path());
        std::fs::write(root.path().join("selected.txt"), "first\n").unwrap();
        let service = GitChangeService::new(root.path());
        let loaded = service
            .load_for_review(ChangeReviewPurpose::Commit, 5)
            .await
            .unwrap();
        let selected = find_change(&loaded.changes, "selected.txt");
        std::fs::write(root.path().join("selected.txt"), "second\n").unwrap();

        let error = service.stage_file(&selected, 5).await.unwrap_err();
        assert!(error.contains("changed since it was displayed"));
        assert!(git_output(root.path(), &["diff", "--cached", "--name-only"]).is_empty());
    }

    #[tokio::test]
    async fn commit_review_resolves_root_before_using_literal_paths() {
        let root = TempDir::new().unwrap();
        git(root.path(), &["init", "-q"]);
        configure_identity(root.path());
        std::fs::create_dir(root.path().join("nested")).unwrap();
        std::fs::write(root.path().join("nested/file.txt"), "base\n").unwrap();
        git(root.path(), &["add", "nested/file.txt"]);
        git(root.path(), &["commit", "-qm", "base"]);
        std::fs::write(root.path().join("nested/file.txt"), "changed\n").unwrap();

        let service = GitChangeService::new(root.path().join("nested"));
        let loaded = service
            .load_for_review(ChangeReviewPurpose::Commit, 5)
            .await
            .unwrap();
        assert_eq!(
            loaded
                .repository
                .as_ref()
                .unwrap()
                .root
                .canonicalize()
                .unwrap(),
            root.path().canonicalize().unwrap()
        );
        let selected = find_change(&loaded.changes, "nested/file.txt");
        let loaded = service.stage_file(&selected, 5).await.unwrap();
        assert!(find_change(&loaded.changes, "nested/file.txt").staged);
    }

    #[tokio::test]
    async fn commit_uses_exact_confirmed_index_and_preserves_unstaged_bytes() {
        let root = TempDir::new().unwrap();
        git(root.path(), &["init", "-q"]);
        configure_identity(root.path());
        std::fs::write(root.path().join("tracked.txt"), "base\n").unwrap();
        git(root.path(), &["add", "tracked.txt"]);
        git(root.path(), &["commit", "-qm", "base"]);
        std::fs::write(root.path().join("tracked.txt"), "staged\n").unwrap();
        git(root.path(), &["add", "tracked.txt"]);
        std::fs::write(root.path().join("tracked.txt"), "unstaged after snapshot\n").unwrap();
        std::fs::write(root.path().join("untracked.txt"), "leave me\n").unwrap();

        let service = GitChangeService::new(root.path());
        let snapshot = service.prepare_commit(5).await.unwrap();
        assert_eq!(snapshot.files.len(), 1);
        let subject = "safe $(touch shell-marker); commit";
        let result = service.commit(&snapshot, subject, 5).await.unwrap();
        assert_eq!(result.subject, subject);
        assert!(!root.path().join("shell-marker").exists());
        assert_eq!(
            git_output(root.path(), &["log", "-1", "--pretty=%s"]),
            subject
        );
        assert_eq!(
            git_output(root.path(), &["show", "HEAD:tracked.txt"]),
            "staged"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("tracked.txt")).unwrap(),
            "unstaged after snapshot\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("untracked.txt")).unwrap(),
            "leave me\n"
        );
        let parent = git_output(root.path(), &["rev-parse", "HEAD^"]);
        assert_eq!(snapshot.head.as_deref(), Some(parent.as_str()));
    }

    #[tokio::test]
    async fn initial_commit_is_allowed_but_detached_and_special_states_refuse() {
        let root = TempDir::new().unwrap();
        git(root.path(), &["init", "-q"]);
        configure_identity(root.path());
        let service = GitChangeService::new(root.path());
        assert!(service
            .prepare_commit(5)
            .await
            .unwrap_err()
            .contains("No staged changes"));
        std::fs::write(root.path().join("initial.txt"), "initial\n").unwrap();
        git(root.path(), &["add", "initial.txt"]);
        let snapshot = service.prepare_commit(5).await.unwrap();
        assert!(snapshot.head.is_none());
        service.commit(&snapshot, "initial", 5).await.unwrap();

        git(root.path(), &["checkout", "--detach", "-q"]);
        std::fs::write(root.path().join("initial.txt"), "detached\n").unwrap();
        assert!(service
            .load_for_review(ChangeReviewPurpose::Commit, 5)
            .await
            .unwrap_err()
            .contains("detached"));

        git(root.path(), &["checkout", "-q", "-"]);
        let git_dir = PathBuf::from(git_output(
            root.path(),
            &["rev-parse", "--absolute-git-dir"],
        ));
        std::fs::write(git_dir.join("MERGE_HEAD"), "synthetic\n").unwrap();
        assert!(service
            .load_for_review(ChangeReviewPurpose::Commit, 5)
            .await
            .unwrap_err()
            .contains("merge"));
    }

    #[tokio::test]
    async fn conflicts_and_confirmed_snapshot_drift_refuse() {
        let conflicted = parse_porcelain(b"UU conflict.txt\0").unwrap();
        assert!(conflicted[0].conflicted);
        assert_eq!(conflicted[0].marker(), "U");

        let root = TempDir::new().unwrap();
        git(root.path(), &["init", "-q"]);
        configure_identity(root.path());
        std::fs::write(root.path().join("one.txt"), "base\n").unwrap();
        std::fs::write(root.path().join("two.txt"), "base\n").unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-qm", "base"]);
        std::fs::write(root.path().join("one.txt"), "one\n").unwrap();
        git(root.path(), &["add", "one.txt"]);
        let service = GitChangeService::new(root.path());
        let snapshot = service.prepare_commit(5).await.unwrap();
        std::fs::write(root.path().join("two.txt"), "two\n").unwrap();
        git(root.path(), &["add", "two.txt"]);
        let failure = service.commit(&snapshot, "stale", 5).await.unwrap_err();
        assert!(!failure.uncertain);
        assert!(failure.message.contains("changed after confirmation"));
        assert_eq!(
            git_output(root.path(), &["log", "-1", "--pretty=%s"]),
            "base"
        );
    }

    #[tokio::test]
    async fn actual_conflicts_refuse_the_commit_surface() {
        let root = TempDir::new().unwrap();
        git(root.path(), &["init", "-q"]);
        configure_identity(root.path());
        std::fs::write(root.path().join("conflict.txt"), "base\n").unwrap();
        git(root.path(), &["add", "conflict.txt"]);
        git(root.path(), &["commit", "-qm", "base"]);
        let primary_branch = git_output(root.path(), &["branch", "--show-current"]);
        git(root.path(), &["checkout", "-qb", "side"]);
        std::fs::write(root.path().join("conflict.txt"), "side\n").unwrap();
        git(root.path(), &["commit", "-qam", "side"]);
        git(root.path(), &["checkout", "-q", &primary_branch]);
        std::fs::write(root.path().join("conflict.txt"), "master\n").unwrap();
        git(root.path(), &["commit", "-qam", "master"]);
        let merge = StdCommand::new("git")
            .args(["merge", "side"])
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(!merge.status.success());

        let error = GitChangeService::new(root.path())
            .load_for_review(ChangeReviewPurpose::Commit, 5)
            .await
            .unwrap_err();
        assert!(error.contains("merge") || error.contains("conflict"));
    }

    #[test]
    fn every_special_operation_marker_is_detected() {
        for (marker, expected, directory) in [
            ("MERGE_HEAD", "merge", false),
            ("CHERRY_PICK_HEAD", "cherry-pick", false),
            ("REVERT_HEAD", "revert", false),
            ("REBASE_HEAD", "rebase", false),
            ("rebase-merge", "rebase", true),
            ("rebase-apply", "rebase", true),
            ("sequencer", "sequencer", true),
            ("BISECT_LOG", "bisect", false),
        ] {
            let git_dir = TempDir::new().unwrap();
            if directory {
                std::fs::create_dir(git_dir.path().join(marker)).unwrap();
            } else {
                std::fs::write(git_dir.path().join(marker), "active\n").unwrap();
            }
            assert_eq!(active_git_operation(git_dir.path()), Some(expected));
        }
    }

    #[tokio::test]
    async fn changed_branch_and_head_refuse_a_confirmed_snapshot() {
        let root = TempDir::new().unwrap();
        git(root.path(), &["init", "-q"]);
        configure_identity(root.path());
        std::fs::write(root.path().join("file.txt"), "base\n").unwrap();
        git(root.path(), &["add", "file.txt"]);
        git(root.path(), &["commit", "-qm", "base"]);
        let primary_branch = git_output(root.path(), &["branch", "--show-current"]);
        std::fs::write(root.path().join("file.txt"), "staged\n").unwrap();
        git(root.path(), &["add", "file.txt"]);
        let service = GitChangeService::new(root.path());
        let snapshot = service.prepare_commit(5).await.unwrap();

        git(root.path(), &["checkout", "-qb", "other"]);
        let branch_failure = service
            .commit(&snapshot, "stale branch", 5)
            .await
            .unwrap_err();
        assert!(!branch_failure.uncertain);
        assert!(branch_failure
            .message
            .contains("changed after confirmation"));
        git(root.path(), &["checkout", "-q", &primary_branch]);

        let moved_head = git_output(
            root.path(),
            &["commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", "external"],
        );
        git(root.path(), &["update-ref", "HEAD", &moved_head]);
        let head_failure = service
            .commit(&snapshot, "stale head", 5)
            .await
            .unwrap_err();
        assert!(!head_failure.uncertain);
        assert!(head_failure.message.contains("changed after confirmation"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failing_hook_preserves_head_and_timeout_kills_hook_group() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().unwrap();
        git(root.path(), &["init", "-q"]);
        configure_identity(root.path());
        std::fs::write(root.path().join("file.txt"), "base\n").unwrap();
        git(root.path(), &["add", "file.txt"]);
        git(root.path(), &["commit", "-qm", "base"]);
        let original_head = git_output(root.path(), &["rev-parse", "HEAD"]);
        std::fs::write(root.path().join("file.txt"), "changed\n").unwrap();
        git(root.path(), &["add", "file.txt"]);
        let service = GitChangeService::new(root.path());
        let snapshot = service.prepare_commit(5).await.unwrap();
        let hooks = PathBuf::from(git_output(
            root.path(),
            &["rev-parse", "--absolute-git-dir"],
        ))
        .join("hooks");
        let hook = hooks.join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\necho hook-refused >&2\nexit 1\n").unwrap();
        let mut permissions = std::fs::metadata(&hook).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&hook, permissions).unwrap();
        let failure = service.commit(&snapshot, "blocked", 5).await.unwrap_err();
        assert!(!failure.uncertain);
        assert!(failure.message.contains("hook-refused"));
        assert_eq!(
            git_output(root.path(), &["rev-parse", "HEAD"]),
            original_head
        );

        let marker = root.path().join("leaked-hook-child");
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\n(sleep 3; printf leaked > '{}') &\nwait\n",
                marker.display()
            ),
        )
        .unwrap();
        let failure = service.commit(&snapshot, "timeout", 1).await.unwrap_err();
        assert!(!failure.uncertain);
        assert!(failure.message.contains("timed out"));
        tokio::time::sleep(Duration::from_millis(2300)).await;
        assert!(
            !marker.exists(),
            "hook child survived process-group timeout"
        );
        assert_eq!(
            git_output(root.path(), &["rev-parse", "HEAD"]),
            original_head
        );

        std::fs::write(
            &hook,
            "#!/bin/sh\ntree=$(git rev-parse 'HEAD^{tree}')\nparent=$(git rev-parse HEAD)\nmoved=$(git commit-tree \"$tree\" -p \"$parent\" -m hook-moved)\ngit update-ref HEAD \"$moved\"\necho moved-head >&2\nexit 1\n",
        )
        .unwrap();
        let failure = service.commit(&snapshot, "uncertain", 5).await.unwrap_err();
        assert!(failure.uncertain);
        assert!(failure.message.contains("HEAD changed"));
        assert_ne!(
            git_output(root.path(), &["rev-parse", "HEAD"]),
            original_head
        );
    }
}
