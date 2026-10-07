use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use uuid::Uuid;

const MANIFEST_VERSION: u32 = 1;
const MAX_RECORDS: usize = 128;
const MAX_GIT_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_DIFF_BYTES: usize = 1024 * 1024;
const GIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorktreeRecord {
    pub id: Uuid,
    pub label: String,
    pub repository_root: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    pub base_commit: String,
    pub git_common_dir: PathBuf,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorktreeAttachment {
    pub id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct WorktreeManifest {
    version: u32,
    records: Vec<WorktreeRecord>,
}

impl Default for WorktreeManifest {
    fn default() -> Self {
        Self {
            version: MANIFEST_VERSION,
            records: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeHealth {
    Ready,
    Missing,
    Mismatched,
}

impl WorktreeHealth {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Missing => "missing",
            Self::Mismatched => "identity mismatch",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeStatus {
    pub record: WorktreeRecord,
    pub health: WorktreeHealth,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
    pub changed_entries: usize,
    pub ahead: Option<u64>,
    pub behind: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeDiff {
    pub record: WorktreeRecord,
    pub text: String,
    pub omitted_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeCleanupResult {
    pub record: WorktreeRecord,
    pub directory_removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeAction {
    List,
    Create { label: String, base: Option<String> },
    Status { id: String },
    Diff { id: String },
    Attach { id: String },
    PrepareCleanup { id: String },
    ConfirmCleanup { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeActionRequest {
    pub generation: u64,
    pub action: WorktreeAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeCleanupConfirmation {
    pub id: Uuid,
    pub label: String,
    pub branch: String,
    pub remove_selected: bool,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("worktree label must be 1-48 lowercase letters, digits, '-' or '_'")]
    InvalidLabel,
    #[error("unsafe Git base reference")]
    InvalidBase,
    #[error("worktree registry is unavailable: {0}")]
    Registry(String),
    #[error("worktree registry contains an unsupported version")]
    UnsupportedVersion,
    #[error("worktree registry exceeds {MAX_RECORDS} entries")]
    TooManyRecords,
    #[error("unknown worktree: {0}")]
    Unknown(String),
    #[error("worktree identity does not match its owned record")]
    IdentityMismatch,
    #[error("worktree has staged, unstaged, or untracked changes")]
    Dirty,
    #[error("Git command timed out")]
    GitTimeout,
    #[error("Git command failed: {0}")]
    Git(String),
    #[error("worktree was created at {path}, but registration failed: {reason}; its directory and branch were preserved")]
    CreatedButUnregistered { path: PathBuf, reason: String },
}

#[derive(Clone)]
pub struct WorktreeStore {
    root: PathBuf,
    writable: bool,
    mutation: Arc<tokio::sync::Mutex<()>>,
}

impl std::fmt::Debug for WorktreeStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorktreeStore")
            .field("root", &self.root)
            .field("writable", &self.writable)
            .finish_non_exhaustive()
    }
}

impl WorktreeStore {
    pub fn new(root: impl Into<PathBuf>, writable: bool) -> Self {
        Self {
            root: root.into(),
            writable,
            mutation: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn writable(&self) -> bool {
        self.writable
    }

    pub fn records(&self) -> Result<Vec<WorktreeRecord>, WorktreeError> {
        let mut records = self.load_manifest()?.records;
        records.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| left.label.cmp(&right.label))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(records)
    }

    pub fn record(&self, raw_id: &str) -> Result<WorktreeRecord, WorktreeError> {
        let records = self.records()?;
        resolve_record(&records, raw_id)
    }

    pub async fn create(
        &self,
        source: &Path,
        label: &str,
        base: Option<&str>,
    ) -> Result<WorktreeRecord, WorktreeError> {
        require_writable(self.writable)?;
        validate_label(label)?;
        let base = base.unwrap_or("HEAD");
        validate_base(base)?;
        let _guard = self.mutation.lock().await;
        let source = canonical_regular_directory(source)?;
        let repository_text =
            run_git(&source, &["rev-parse", "--show-toplevel"], 16 * 1024).await?;
        let repository_root = canonical_regular_directory(Path::new(repository_text.trim()))?;
        let base_spec = format!("{base}^{{commit}}");
        let base_commit = run_git(
            &repository_root,
            &["rev-parse", "--verify", "--end-of-options", &base_spec],
            16 * 1024,
        )
        .await?
        .trim()
        .to_string();
        if !is_full_oid(&base_commit) {
            return Err(WorktreeError::Git(
                "base did not resolve to a commit".into(),
            ));
        }
        let common_text = run_git(
            &repository_root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            16 * 1024,
        )
        .await?;
        let git_common_dir = canonical_regular_directory(Path::new(common_text.trim()))?;
        let mut manifest = self.load_manifest()?;
        if manifest.records.len() >= MAX_RECORDS {
            return Err(WorktreeError::TooManyRecords);
        }
        ensure_registry_root(&self.root)?;
        let id = Uuid::new_v4();
        let repo_key = repository_key(&git_common_dir);
        let repository_parent = self.root.join(&repo_key);
        ensure_owned_parent(&repository_parent)?;
        let parent = repository_parent.join(id.to_string());
        std::fs::create_dir(&parent).map_err(|error| WorktreeError::Registry(error.to_string()))?;
        ensure_regular_directory(&parent)?;
        let target = parent.join(label);
        ensure_generated_target(&self.root, &target)?;
        let branch = format!("ca/{label}-{}", &id.simple().to_string()[..8]);
        let target_text = target.to_string_lossy().into_owned();
        let add = run_git(
            &repository_root,
            &["worktree", "add", "-b", &branch, &target_text, &base_commit],
            64 * 1024,
        )
        .await;
        if let Err(error) = add {
            if target.exists() {
                return Err(WorktreeError::CreatedButUnregistered {
                    path: target,
                    reason: error.to_string(),
                });
            }
            let _ = std::fs::remove_dir(&parent);
            return Err(error);
        }
        let path = canonical_regular_directory(&target).map_err(|error| {
            WorktreeError::CreatedButUnregistered {
                path: target.clone(),
                reason: error.to_string(),
            }
        })?;
        let record = WorktreeRecord {
            id,
            label: label.to_string(),
            repository_root,
            path,
            branch,
            base_commit,
            git_common_dir,
            created_at: Utc::now(),
        };
        if let Err(error) = verify_record(&self.root, &record).await {
            return Err(WorktreeError::CreatedButUnregistered {
                path: record.path.clone(),
                reason: error.to_string(),
            });
        }
        manifest.records.push(record.clone());
        if let Err(error) = self.save_manifest(&manifest) {
            return Err(WorktreeError::CreatedButUnregistered {
                path: record.path.clone(),
                reason: error.to_string(),
            });
        }
        Ok(record)
    }

    pub async fn status(&self, raw_id: &str) -> Result<WorktreeStatus, WorktreeError> {
        let record = self.record(raw_id)?;
        status_record(&self.root, record).await
    }

    pub async fn statuses(&self) -> Result<Vec<WorktreeStatus>, WorktreeError> {
        use futures::StreamExt;
        let records = self.records()?;
        let root = self.root.clone();
        let mut indexed =
            futures::stream::iter(records.into_iter().enumerate().map(|(index, record)| {
                let root = root.clone();
                async move { (index, status_record(&root, record).await) }
            }))
            .buffer_unordered(8)
            .collect::<Vec<_>>()
            .await;
        indexed.sort_by_key(|(index, _)| *index);
        indexed.into_iter().map(|(_, result)| result).collect()
    }

    pub async fn diff(&self, raw_id: &str) -> Result<WorktreeDiff, WorktreeError> {
        let status = self.status(raw_id).await?;
        if status.health != WorktreeHealth::Ready {
            return Err(WorktreeError::IdentityMismatch);
        }
        let record = status.record;
        let committed = run_git(
            &record.path,
            &[
                "diff",
                "--no-color",
                "--no-ext-diff",
                &format!("{}..HEAD", record.base_commit),
                "--",
            ],
            MAX_GIT_OUTPUT_BYTES,
        )
        .await?;
        let staged = run_git(
            &record.path,
            &["diff", "--cached", "--no-color", "--no-ext-diff", "--"],
            MAX_GIT_OUTPUT_BYTES,
        )
        .await?;
        let unstaged = run_git(
            &record.path,
            &["diff", "--no-color", "--no-ext-diff", "--"],
            MAX_GIT_OUTPUT_BYTES,
        )
        .await?;
        let porcelain = run_git_bytes(
            &record.path,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            256 * 1024,
        )
        .await?;
        let untracked = parse_untracked_names(&porcelain);
        let mut sections = Vec::new();
        if !committed.is_empty() {
            sections.push(format!("Committed since base\n{committed}"));
        }
        if !staged.is_empty() {
            sections.push(format!("Staged changes\n{staged}"));
        }
        if !unstaged.is_empty() {
            sections.push(format!("Unstaged changes\n{unstaged}"));
        }
        if !untracked.is_empty() {
            sections.push(format!(
                "Untracked files (names only)\n{}",
                untracked.join("\n")
            ));
        }
        let text = if sections.is_empty() {
            "No changes from the recorded base".to_string()
        } else {
            sections.join("\n\n")
        };
        let text = crate::terminal_text::sanitize_terminal_text(&text);
        let (text, omitted_bytes) = bound_text(&text, MAX_DIFF_BYTES);
        Ok(WorktreeDiff {
            record,
            text,
            omitted_bytes,
        })
    }

    pub async fn attach_context(
        &self,
        raw_id: &str,
    ) -> Result<(WorktreeRecord, crate::workspace::WorkspaceContext), WorktreeError> {
        require_writable(self.writable)?;
        let status = self.status(raw_id).await?;
        if status.health != WorktreeHealth::Ready {
            return Err(WorktreeError::IdentityMismatch);
        }
        let record = status.record;
        let context = crate::workspace::WorkspaceContext::from_primary(
            record.path.clone(),
            record.path.clone(),
        );
        Ok((record, context))
    }

    pub async fn cleanup(
        &self,
        raw_id: &str,
        attached: Option<Uuid>,
    ) -> Result<WorktreeCleanupResult, WorktreeError> {
        require_writable(self.writable)?;
        let _guard = self.mutation.lock().await;
        let mut manifest = self.load_manifest()?;
        let record = resolve_record(&manifest.records, raw_id)?;
        if attached == Some(record.id) {
            return Err(WorktreeError::Git(
                "the attached worktree cannot be cleaned up".into(),
            ));
        }
        let directory_removed = if record.path.exists() {
            let status = status_record(&self.root, record.clone()).await?;
            if status.health != WorktreeHealth::Ready {
                return Err(WorktreeError::IdentityMismatch);
            }
            if status.dirty {
                return Err(WorktreeError::Dirty);
            }
            let target = record.path.to_string_lossy().into_owned();
            run_git(
                &record.repository_root,
                &["worktree", "remove", "--", &target],
                64 * 1024,
            )
            .await?;
            true
        } else {
            false
        };
        manifest
            .records
            .retain(|candidate| candidate.id != record.id);
        self.save_manifest(&manifest)?;
        Ok(WorktreeCleanupResult {
            record,
            directory_removed,
        })
    }

    fn manifest_path(&self) -> PathBuf {
        self.root.join("manifest.json")
    }

    fn load_manifest(&self) -> Result<WorktreeManifest, WorktreeError> {
        let path = self.manifest_path();
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(WorktreeManifest::default())
            }
            Err(error) => return Err(WorktreeError::Registry(error.to_string())),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(WorktreeError::Registry(
                "manifest must be a regular file".into(),
            ));
        }
        if metadata.len() > 1024 * 1024 {
            return Err(WorktreeError::Registry("manifest is too large".into()));
        }
        let bytes =
            std::fs::read(&path).map_err(|error| WorktreeError::Registry(error.to_string()))?;
        let manifest: WorktreeManifest = serde_json::from_slice(&bytes)
            .map_err(|error| WorktreeError::Registry(error.to_string()))?;
        if manifest.version != MANIFEST_VERSION {
            return Err(WorktreeError::UnsupportedVersion);
        }
        if manifest.records.len() > MAX_RECORDS {
            return Err(WorktreeError::TooManyRecords);
        }
        for record in &manifest.records {
            validate_record_lexical(&self.root, record)?;
        }
        let unique_ids = manifest
            .records
            .iter()
            .map(|record| record.id)
            .collect::<std::collections::HashSet<_>>();
        let unique_paths = manifest
            .records
            .iter()
            .map(|record| &record.path)
            .collect::<std::collections::HashSet<_>>();
        let unique_branches = manifest
            .records
            .iter()
            .map(|record| &record.branch)
            .collect::<std::collections::HashSet<_>>();
        if unique_ids.len() != manifest.records.len()
            || unique_paths.len() != manifest.records.len()
            || unique_branches.len() != manifest.records.len()
        {
            return Err(WorktreeError::Registry(
                "manifest contains duplicate worktree identities".into(),
            ));
        }
        Ok(manifest)
    }

    fn save_manifest(&self, manifest: &WorktreeManifest) -> Result<(), WorktreeError> {
        require_writable(self.writable)?;
        ensure_registry_root(&self.root)?;
        if manifest.records.len() > MAX_RECORDS {
            return Err(WorktreeError::TooManyRecords);
        }
        let bytes = serde_json::to_vec_pretty(manifest)
            .map_err(|error| WorktreeError::Registry(error.to_string()))?;
        let path = self.manifest_path();
        if std::fs::symlink_metadata(&path)
            .ok()
            .is_some_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(WorktreeError::Registry(
                "manifest must not be a symlink".into(),
            ));
        }
        let temporary = self.root.join(format!(".manifest-{}.tmp", Uuid::new_v4()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = options
            .open(&temporary)
            .map_err(|error| WorktreeError::Registry(error.to_string()))?;
        use std::io::Write;
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &path)?;
            Ok::<_, std::io::Error>(())
        })();
        if let Err(error) = result {
            let _ = std::fs::remove_file(&temporary);
            return Err(WorktreeError::Registry(error.to_string()));
        }
        Ok(())
    }
}

pub fn render_list(records: &[WorktreeStatus], attached: Option<Uuid>) -> String {
    if records.is_empty() {
        return "Worktrees\n\nNo ca-owned worktrees for this profile.\nUse /worktree create <label> [base].".into();
    }
    let mut lines = vec!["Worktrees".to_string(), String::new()];
    for status in records {
        let attached_marker = if attached == Some(status.record.id) {
            " · attached"
        } else {
            ""
        };
        let dirty = if status.dirty { " · dirty" } else { "" };
        lines.push(format!(
            "{}  {}  {}{}{}",
            short_id(status.record.id),
            status.record.label,
            status.health.label(),
            dirty,
            attached_marker
        ));
        lines.push(format!("    branch: {}", status.record.branch));
    }
    lines.push(String::new());
    lines.push("/worktree status <id> · diff <id> · attach <id> · cleanup <id>".into());
    lines.join("\n")
}

pub fn render_status(status: &WorktreeStatus, attached: bool) -> String {
    let safe_path =
        crate::terminal_text::sanitize_terminal_text(&status.record.path.to_string_lossy());
    let mut lines = vec![
        format!("Worktree {}", status.record.label),
        String::new(),
        format!("ID: {}", status.record.id),
        format!("State: {}", status.health.label()),
        format!("Attached: {}", if attached { "yes" } else { "no" }),
        format!(
            "Branch: {}",
            status.branch.as_deref().unwrap_or(&status.record.branch)
        ),
        format!("Path: {safe_path}"),
        format!("Base: {}", short_oid(&status.record.base_commit)),
        format!(
            "HEAD: {}",
            status
                .head
                .as_deref()
                .map(short_oid)
                .unwrap_or("unavailable")
        ),
        format!(
            "Working tree: {}",
            if status.dirty {
                format!("dirty ({} status entries)", status.changed_entries)
            } else {
                "clean".into()
            }
        ),
    ];
    if let (Some(ahead), Some(behind)) = (status.ahead, status.behind) {
        lines.push(format!("From base: {ahead} ahead · {behind} behind"));
    }
    lines.join("\n")
}

pub fn short_id(id: Uuid) -> String {
    id.simple().to_string()[..8].to_string()
}

fn short_oid(value: &str) -> &str {
    value.get(..12).unwrap_or(value)
}

async fn status_record(
    root: &Path,
    record: WorktreeRecord,
) -> Result<WorktreeStatus, WorktreeError> {
    validate_record_lexical(root, &record)?;
    if !record.path.exists() {
        return Ok(WorktreeStatus {
            record,
            health: WorktreeHealth::Missing,
            head: None,
            branch: None,
            dirty: false,
            changed_entries: 0,
            ahead: None,
            behind: None,
        });
    }
    if verify_record(root, &record).await.is_err() {
        return Ok(WorktreeStatus {
            record,
            health: WorktreeHealth::Mismatched,
            head: None,
            branch: None,
            dirty: false,
            changed_entries: 0,
            ahead: None,
            behind: None,
        });
    }
    let head = run_git(&record.path, &["rev-parse", "HEAD"], 16 * 1024)
        .await?
        .trim()
        .to_string();
    let branch = run_git(
        &record.path,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        16 * 1024,
    )
    .await
    .ok()
    .map(|value| value.trim().to_string());
    let porcelain = run_git_bytes(
        &record.path,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        256 * 1024,
    )
    .await?;
    let changed_entries = porcelain
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .count();
    let divergence = run_git(
        &record.path,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{}...HEAD", record.base_commit),
        ],
        16 * 1024,
    )
    .await
    .ok()
    .and_then(|value| {
        let mut values = value.split_whitespace();
        Some((values.next()?.parse().ok()?, values.next()?.parse().ok()?))
    });
    Ok(WorktreeStatus {
        record,
        health: WorktreeHealth::Ready,
        head: Some(head),
        branch,
        dirty: changed_entries > 0,
        changed_entries,
        ahead: divergence.map(|(_, ahead)| ahead),
        behind: divergence.map(|(behind, _)| behind),
    })
}

async fn verify_record(root: &Path, record: &WorktreeRecord) -> Result<(), WorktreeError> {
    validate_record_lexical(root, record)?;
    let path = canonical_regular_directory(&record.path)?;
    let owned_root =
        std::fs::canonicalize(root).map_err(|error| WorktreeError::Registry(error.to_string()))?;
    if !path.starts_with(&owned_root) {
        return Err(WorktreeError::IdentityMismatch);
    }
    let top = run_git(&path, &["rev-parse", "--show-toplevel"], 16 * 1024).await?;
    if canonical_regular_directory(Path::new(top.trim()))? != path {
        return Err(WorktreeError::IdentityMismatch);
    }
    let common = run_git(
        &path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        16 * 1024,
    )
    .await?;
    if canonical_regular_directory(Path::new(common.trim()))? != record.git_common_dir {
        return Err(WorktreeError::IdentityMismatch);
    }
    let branch = run_git(
        &path,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        16 * 1024,
    )
    .await
    .map_err(|_| WorktreeError::IdentityMismatch)?;
    if branch.trim() != record.branch {
        return Err(WorktreeError::IdentityMismatch);
    }
    Ok(())
}

fn resolve_record(
    records: &[WorktreeRecord],
    raw_id: &str,
) -> Result<WorktreeRecord, WorktreeError> {
    let query = raw_id.trim().to_ascii_lowercase();
    if query.is_empty() {
        return Err(WorktreeError::Unknown(raw_id.into()));
    }
    let matches = records
        .iter()
        .filter(|record| {
            record.id.to_string().starts_with(&query) || record.label.to_ascii_lowercase() == query
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [record] => Ok((*record).clone()),
        _ => Err(WorktreeError::Unknown(raw_id.into())),
    }
}

fn require_writable(writable: bool) -> Result<(), WorktreeError> {
    if writable {
        Ok(())
    } else {
        Err(WorktreeError::Registry(
            "worktree mutations are disabled in bare mode".into(),
        ))
    }
}

pub(crate) fn validate_label(value: &str) -> Result<(), WorktreeError> {
    let valid = !value.is_empty()
        && value.len() <= 48
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        });
    if valid {
        Ok(())
    } else {
        Err(WorktreeError::InvalidLabel)
    }
}

pub(crate) fn validate_base(value: &str) -> Result<(), WorktreeError> {
    let valid = !value.is_empty()
        && value.len() <= 256
        && !value.starts_with('-')
        && !value.contains("..")
        && !value.contains("@{")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(WorktreeError::InvalidBase)
    }
}

fn validate_record_lexical(root: &Path, record: &WorktreeRecord) -> Result<(), WorktreeError> {
    validate_label(&record.label)?;
    let expected_tail = Path::new(&record.id.to_string()).join(&record.label);
    let normalized_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if !record.path.starts_with(normalized_root) || !record.path.ends_with(expected_tail) {
        return Err(WorktreeError::IdentityMismatch);
    }
    if record.branch
        != format!(
            "ca/{}-{}",
            record.label,
            &record.id.simple().to_string()[..8]
        )
        || !record.repository_root.is_absolute()
        || !record.git_common_dir.is_absolute()
        || !is_full_oid(&record.base_commit)
    {
        return Err(WorktreeError::IdentityMismatch);
    }
    Ok(())
}

fn ensure_registry_root(root: &Path) -> Result<(), WorktreeError> {
    if let Ok(metadata) = std::fs::symlink_metadata(root) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(WorktreeError::Registry(
                "registry root must be a regular directory".into(),
            ));
        }
    } else {
        std::fs::create_dir_all(root)
            .map_err(|error| WorktreeError::Registry(error.to_string()))?;
    }
    Ok(())
}

fn ensure_generated_target(root: &Path, target: &Path) -> Result<(), WorktreeError> {
    if target.exists() || std::fs::symlink_metadata(target).is_ok() || !target.starts_with(root) {
        return Err(WorktreeError::Registry(
            "generated worktree target already exists or is outside the registry".into(),
        ));
    }
    Ok(())
}

fn ensure_regular_directory(path: &Path) -> Result<(), WorktreeError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| WorktreeError::Registry(error.to_string()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(WorktreeError::Registry(
            "owned worktree parent must be a regular directory".into(),
        ));
    }
    Ok(())
}

fn ensure_owned_parent(path: &Path) -> Result<(), WorktreeError> {
    match std::fs::create_dir(path) {
        Ok(()) => ensure_regular_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure_regular_directory(path)
        }
        Err(error) => Err(WorktreeError::Registry(error.to_string())),
    }
}

fn canonical_regular_directory(path: &Path) -> Result<PathBuf, WorktreeError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| WorktreeError::Registry(error.to_string()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(WorktreeError::Registry(
            "expected a regular directory".into(),
        ));
    }
    std::fs::canonicalize(path).map_err(|error| WorktreeError::Registry(error.to_string()))
}

fn repository_key(common: &Path) -> String {
    Sha256::digest(common.as_os_str().as_encoded_bytes())[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_full_oid(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_untracked_names(porcelain: &[u8]) -> Vec<String> {
    porcelain
        .split(|byte| *byte == 0)
        .filter_map(|entry| entry.strip_prefix(b"?? "))
        .take(1000)
        .map(|name| crate::terminal_text::sanitize_terminal_text(&String::from_utf8_lossy(name)))
        .collect()
}

fn bound_text(value: &str, limit: usize) -> (String, usize) {
    if value.len() <= limit {
        return (value.to_string(), 0);
    }
    let marker_reserve = 80usize;
    let budget = limit.saturating_sub(marker_reserve);
    let head_budget = budget / 2;
    let tail_budget = budget.saturating_sub(head_budget);
    let mut head_end = head_budget.min(value.len());
    while !value.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = value.len().saturating_sub(tail_budget);
    while tail_start < value.len() && !value.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let omitted = tail_start.saturating_sub(head_end);
    (
        format!(
            "{}\n[worktree diff: {omitted} bytes omitted]\n{}",
            &value[..head_end],
            &value[tail_start..]
        ),
        omitted,
    )
}

async fn run_git(cwd: &Path, args: &[&str], limit: usize) -> Result<String, WorktreeError> {
    let bytes = run_git_bytes(cwd, args, limit).await?;
    String::from_utf8(bytes).map_err(|_| WorktreeError::Git("Git returned non-UTF-8 text".into()))
}

async fn run_git_bytes(cwd: &Path, args: &[&str], limit: usize) -> Result<Vec<u8>, WorktreeError> {
    let output_limit = limit.min(MAX_GIT_OUTPUT_BYTES);
    let native = ca_automation::runtime::current_run_store().is_some();
    let mut command = if native {
        Command::from(
            ca_automation::git_metadata::query(cwd, args)
                .map_err(|error| WorktreeError::Git(error.to_string()))?,
        )
    } else {
        let mut command = Command::new("git");
        command
            .args(args)
            .current_dir(cwd)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_PAGER", "cat")
            .env("PAGER", "cat");
        crate::run_environment::apply_tokio(&mut command);
        command
    };
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| WorktreeError::Git(error.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| WorktreeError::Git("Git stdout was unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| WorktreeError::Git("Git stderr was unavailable".into()))?;
    let completed = tokio::time::timeout(GIT_TIMEOUT, async {
        let (stdout, stderr, status) = tokio::join!(
            read_bounded(stdout, output_limit),
            read_bounded(stderr, 4096),
            child.wait(),
        );
        Ok::<_, WorktreeError>((
            stdout?,
            stderr?,
            status.map_err(|error| WorktreeError::Git(error.to_string()))?,
        ))
    })
    .await
    .map_err(|_| WorktreeError::GitTimeout)??;
    let (stdout, stderr, status) = completed;
    if !status.success() {
        let safe =
            crate::terminal_text::sanitize_terminal_text(&String::from_utf8_lossy(&stderr.bytes));
        return Err(WorktreeError::Git(if safe.trim().is_empty() {
            format!("exit status {status}")
        } else {
            safe.trim().to_string()
        }));
    }
    if stdout.omitted_bytes > 0 {
        return Err(WorktreeError::Git(
            "Git output exceeded its safe limit".into(),
        ));
    }
    Ok(stdout.bytes)
}

struct BoundedRead {
    bytes: Vec<u8>,
    omitted_bytes: usize,
}

async fn read_bounded(
    mut reader: impl AsyncRead + Unpin,
    limit: usize,
) -> Result<BoundedRead, WorktreeError> {
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    let mut omitted_bytes = 0usize;
    let mut chunk = [0_u8; 8192];
    loop {
        let count = reader
            .read(&mut chunk)
            .await
            .map_err(|error| WorktreeError::Git(error.to_string()))?;
        if count == 0 {
            break;
        }
        let retained = limit.saturating_sub(bytes.len()).min(count);
        bytes.extend_from_slice(&chunk[..retained]);
        omitted_bytes = omitted_bytes.saturating_add(count.saturating_sub(retained));
    }
    Ok(BoundedRead {
        bytes,
        omitted_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    fn git(cwd: &Path, args: &[&str]) {
        let output = StdCommand::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_AUTHOR_NAME", "CA Test")
            .env("GIT_AUTHOR_EMAIL", "ca@example.invalid")
            .env("GIT_COMMITTER_NAME", "CA Test")
            .env("GIT_COMMITTER_EMAIL", "ca@example.invalid")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repository() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q"]);
        std::fs::write(root.path().join("tracked.txt"), "base\n").unwrap();
        git(root.path(), &["add", "tracked.txt"]);
        git(root.path(), &["commit", "-q", "-m", "base"]);
        root
    }

    #[test]
    fn validates_labels_bases_and_bounds_text_at_utf8_boundaries() {
        assert!(validate_label("task_1").is_ok());
        assert!(validate_label("Task").is_err());
        assert!(validate_label("../escape").is_err());
        assert!(validate_base("HEAD~2").is_err());
        assert!(validate_base("feature/demo").is_ok());
        let value = "é".repeat(100);
        let (bounded, omitted) = bound_text(&value, 100);
        assert!(omitted > 0);
        assert!(bounded.is_char_boundary(bounded.len()));
    }

    #[tokio::test]
    async fn creates_statuses_diffs_and_cleans_without_deleting_branch() {
        let repository = repository();
        let data = tempfile::tempdir().unwrap();
        let store = WorktreeStore::new(data.path().join("worktrees"), true);
        let record = store
            .create(repository.path(), "review", None)
            .await
            .unwrap();
        assert!(record.path.is_dir());
        assert_eq!(
            store.records().unwrap().as_slice(),
            std::slice::from_ref(&record)
        );
        let clean = store.status(&short_id(record.id)).await.unwrap();
        assert_eq!(clean.health, WorktreeHealth::Ready);
        assert!(!clean.dirty);

        std::fs::write(
            record.path.join("tracked.txt"),
            "changed\n\x1b[31mred\x1b[0m\n",
        )
        .unwrap();
        std::fs::write(record.path.join("secret-untracked.txt"), "DO_NOT_READ").unwrap();
        let dirty = store.status(&record.label).await.unwrap();
        assert!(dirty.dirty);
        let diff = store.diff(&record.label).await.unwrap();
        assert!(diff.text.contains("changed"));
        assert!(!diff.text.contains('\x1b'));
        assert!(diff.text.contains("secret-untracked.txt"));
        assert!(!diff.text.contains("DO_NOT_READ"));
        assert!(matches!(
            store.cleanup(&record.label, None).await,
            Err(WorktreeError::Dirty)
        ));

        std::fs::write(record.path.join("tracked.txt"), "base\n").unwrap();
        std::fs::remove_file(record.path.join("secret-untracked.txt")).unwrap();
        std::fs::write(record.path.join("committed.txt"), "preserve this commit\n").unwrap();
        git(&record.path, &["add", "committed.txt"]);
        git(&record.path, &["commit", "-q", "-m", "worktree commit"]);
        let branch_head = run_git(&record.path, &["rev-parse", "HEAD"], 16 * 1024)
            .await
            .unwrap();
        let cleaned = store.cleanup(&record.label, None).await.unwrap();
        assert!(cleaned.directory_removed);
        assert!(!record.path.exists());
        assert!(store.records().unwrap().is_empty());
        let output = StdCommand::new("git")
            .args(["branch", "--list", &record.branch])
            .current_dir(repository.path())
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).contains(&record.branch));
        let preserved = run_git(repository.path(), &["rev-parse", &record.branch], 16 * 1024)
            .await
            .unwrap();
        assert_eq!(preserved.trim(), branch_head.trim());
        assert_ne!(
            run_git(repository.path(), &["rev-parse", "HEAD"], 16 * 1024)
                .await
                .unwrap()
                .trim(),
            branch_head.trim()
        );
    }

    #[tokio::test]
    async fn missing_tree_is_visible_and_can_be_forgotten_but_attached_is_protected() {
        let repository = repository();
        let data = tempfile::tempdir().unwrap();
        let store = WorktreeStore::new(data.path().join("worktrees"), true);
        let record = store
            .create(repository.path(), "missing", None)
            .await
            .unwrap();
        git(
            repository.path(),
            &["worktree", "remove", record.path.to_str().unwrap()],
        );
        let status = store.status(&record.label).await.unwrap();
        assert_eq!(status.health, WorktreeHealth::Missing);
        assert!(store.cleanup(&record.label, Some(record.id)).await.is_err());
        let result = store.cleanup(&record.label, None).await.unwrap();
        assert!(!result.directory_removed);
        assert!(store.records().unwrap().is_empty());
    }

    #[tokio::test]
    async fn profile_roots_and_bare_mutation_are_isolated() {
        let repository = repository();
        let data = tempfile::tempdir().unwrap();
        let first = WorktreeStore::new(data.path().join("first"), true);
        let second = WorktreeStore::new(data.path().join("second"), true);
        first.create(repository.path(), "one", None).await.unwrap();
        assert_eq!(first.records().unwrap().len(), 1);
        assert!(second.records().unwrap().is_empty());
        let bare = WorktreeStore::new(data.path().join("first"), false);
        assert_eq!(bare.records().unwrap().len(), 1);
        assert!(bare.create(repository.path(), "two", None).await.is_err());
        assert!(bare.attach_context("one").await.is_err());
    }

    #[tokio::test]
    async fn symlinked_manifest_and_mismatched_directory_fail_closed() {
        let repository = repository();
        let data = tempfile::tempdir().unwrap();
        let root = data.path().join("worktrees");
        std::fs::create_dir_all(&root).unwrap();
        let outside = data.path().join("outside.json");
        std::fs::write(&outside, "{}").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.join("manifest.json")).unwrap();
            let store = WorktreeStore::new(&root, true);
            assert!(store.records().is_err());
            std::fs::remove_file(root.join("manifest.json")).unwrap();
        }
        let store = WorktreeStore::new(&root, true);
        let record = store
            .create(repository.path(), "identity", None)
            .await
            .unwrap();
        git(
            repository.path(),
            &["worktree", "remove", record.path.to_str().unwrap()],
        );
        std::fs::create_dir_all(&record.path).unwrap();
        let status = store.status(&record.label).await.unwrap();
        assert_eq!(status.health, WorktreeHealth::Mismatched);
    }

    #[tokio::test]
    async fn attachment_returns_exact_owned_workspace() {
        let repository = repository();
        let data = tempfile::tempdir().unwrap();
        let store = WorktreeStore::new(data.path().join("worktrees"), true);
        let record = store
            .create(repository.path(), "attach", None)
            .await
            .unwrap();
        let (attached, workspace) = store.attach_context(&short_id(record.id)).await.unwrap();
        assert_eq!(attached, record);
        assert_eq!(workspace.primary().unwrap().path, record.path);
        assert_eq!(workspace.cwd(), record.path);
    }

    #[tokio::test]
    async fn invalid_inputs_and_symlinked_owned_parent_fail_before_git_mutation() {
        let repository = repository();
        let data = tempfile::tempdir().unwrap();
        let root = data.path().join("worktrees");
        let store = WorktreeStore::new(&root, true);
        assert!(store
            .create(repository.path(), "../escape", None)
            .await
            .is_err());
        assert!(store
            .create(repository.path(), "valid", Some("--upload-pack=bad"))
            .await
            .is_err());
        assert!(store.create(data.path(), "not-git", None).await.is_err());
        assert!(store.records().unwrap().is_empty());

        let common_text = run_git(
            repository.path(),
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            16 * 1024,
        )
        .await
        .unwrap();
        let common = canonical_regular_directory(Path::new(common_text.trim())).unwrap();
        let repo_key = repository_key(&common);
        std::fs::create_dir_all(&root).unwrap();
        let outside = data.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join(repo_key)).unwrap();

        assert!(store
            .create(repository.path(), "blocked", None)
            .await
            .is_err());
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
        let branches = StdCommand::new("git")
            .args(["branch", "--list", "ca/blocked-*"])
            .current_dir(repository.path())
            .output()
            .unwrap();
        assert!(branches.stdout.is_empty());
    }

    #[tokio::test]
    async fn bounded_reader_drains_without_retaining_the_omitted_bytes() {
        let (mut writer, reader) = tokio::io::duplex(32);
        let write = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            writer.write_all(&vec![b'x'; 4096]).await.unwrap();
        });
        let bounded = read_bounded(reader, 64).await.unwrap();
        write.await.unwrap();
        assert_eq!(bounded.bytes.len(), 64);
        assert_eq!(bounded.omitted_bytes, 4032);
    }
}
