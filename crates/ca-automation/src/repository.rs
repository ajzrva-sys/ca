//! One occurrence of repository engineering. The host owns admission and agents.
//!
//! A publication receipt binds the frozen target, candidate tree, host checks and
//! independent review. Recovery reads refs before deciding whether publication
//! happened; it never replays an ambiguous Git mutation.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

const OUTPUT_LIMIT: usize = 1024 * 1024;
const STATE_LIMIT: u64 = 4 * 1024 * 1024;
tokio::task_local! {
    /// Set by an isolated ca worker whose parent owns group cleanup.
    pub static CONTAINED_COMMANDS: bool;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RepositoryPricing {
    Metered { price_card_sha256: String },
    LocalZero,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RepositoryAgentRoute {
    pub provider: String,
    pub model: String,
    pub pricing: RepositoryPricing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RepositoryCheck {
    pub executable: PathBuf,
    pub executable_sha256: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RepositoryTask {
    pub repository: PathBuf,
    pub target_branch: String,
    /// Exact relative paths, directories ending in `/`, or explicit glob patterns.
    pub allowed_paths: Vec<String>,
    pub prompt: String,
    pub worker_route: RepositoryAgentRoute,
    pub reviewer_route: RepositoryAgentRoute,
    pub checks: Vec<RepositoryCheck>,
    pub integrate: bool,
}

impl RepositoryTask {
    pub fn validate(&self) -> Result<()> {
        if !self.repository.is_absolute()
            || self.prompt.trim().is_empty()
            || self.prompt.len() > 64 * 1024
            || self.target_branch.is_empty()
            || self.target_branch.len() > 256
            || self.target_branch.starts_with('-')
            || self.target_branch.starts_with("refs/")
            || self.target_branch.chars().any(char::is_control)
            || self.allowed_paths.is_empty()
            || self.allowed_paths.len() > 128
            || self.checks.is_empty()
            || self.checks.len() > 16
        {
            bail!("repository task requires a repository, local branch, bounded scope, prompt and host checks");
        }
        for route in [&self.worker_route, &self.reviewer_route] {
            if route.provider.is_empty()
                || route.model.is_empty()
                || route.provider.len() > 256
                || route.model.len() > 256
                || route.provider.chars().any(char::is_control)
                || route.model.chars().any(char::is_control)
            {
                bail!("repository task requires exact worker and reviewer routes");
            }
            if let RepositoryPricing::Metered { price_card_sha256 } = &route.pricing {
                require_digest(price_card_sha256)?;
            }
        }
        self.scope()?;
        for check in &self.checks {
            if !check.executable.is_absolute()
                || !(1..=86_400).contains(&check.timeout_secs)
                || check.args.len() > 128
                || check
                    .args
                    .iter()
                    .any(|arg| arg.len() > 4096 || arg.contains('\0'))
            {
                bail!("repository check requires an absolute executable and bounded arguments/runtime");
            }
            require_digest(&check.executable_sha256)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String> {
        Ok(digest(&serde_json::to_vec(self)?))
    }

    fn scope(&self) -> Result<globset::GlobSet> {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in &self.allowed_paths {
            let relative = pattern.trim_end_matches('/');
            if relative.is_empty()
                || relative.len() > 1024
                || relative.contains('\0')
                || Path::new(relative).is_absolute()
                || Path::new(relative)
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                bail!("repository allowed paths must stay relative to the repository");
            }
            let pattern = if pattern.ends_with('/') {
                format!("{pattern}**")
            } else {
                pattern.clone()
            };
            builder.add(globset::Glob::new(&pattern)?);
        }
        Ok(builder.build()?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositorySnapshot {
    pub repository: PathBuf,
    pub common_dir: PathBuf,
    pub target_ref: String,
    pub base_commit: String,
    pub task_sha256: String,
}

impl RepositorySnapshot {
    pub fn resource(&self) -> String {
        format!(
            "repository:{}:{}",
            self.common_dir.display(),
            self.target_ref
        )
    }
}

pub async fn inspect(task: &RepositoryTask) -> Result<RepositorySnapshot> {
    task.validate()?;
    git(
        &task.repository,
        &["check-ref-format", "--branch", &task.target_branch],
    )
    .await?;
    let root = git(&task.repository, &["rev-parse", "--show-toplevel"])
        .await
        .context("repository binding is unavailable; a Git repository is required")?;
    let repository = std::fs::canonicalize(root.trim())?;
    if repository != std::fs::canonicalize(&task.repository)? {
        bail!("repository task must bind the repository root");
    }
    let common_dir = git(
        &repository,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .await?;
    let target_ref = format!("refs/heads/{}", task.target_branch);
    let base_commit = git(
        &repository,
        &["rev-parse", "--verify", &format!("{target_ref}^{{commit}}")],
    )
    .await?
    .trim()
    .to_string();
    require_oid(&base_commit)?;
    Ok(RepositorySnapshot {
        repository,
        common_dir: std::fs::canonicalize(common_dir.trim())?,
        target_ref,
        base_commit,
        task_sha256: task.digest()?,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryCandidate {
    pub worktree: PathBuf,
    pub commit: String,
    pub tree: String,
    pub changed_paths: Vec<String>,
}

pub async fn capture_candidate(
    task: &RepositoryTask,
    frozen: &RepositorySnapshot,
    worktree: &Path,
    run_id: &str,
) -> Result<RepositoryCandidate> {
    require_run_id(run_id)?;
    if task.digest()? != frozen.task_sha256 {
        bail!("repository task changed after the target was frozen");
    }
    let common = git(
        worktree,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .await?;
    if std::fs::canonicalize(common.trim())? != frozen.common_dir {
        bail!("candidate worktree belongs to another repository");
    }
    let mut changed = path_output(
        &git(
            worktree,
            &[
                "diff",
                "--name-only",
                "-z",
                "--no-renames",
                &frozen.base_commit,
            ],
        )
        .await?,
    )?;
    changed.extend(path_output(
        &git(
            worktree,
            &["ls-files", "--others", "--exclude-standard", "-z"],
        )
        .await?,
    )?);
    let scope = task.scope()?;
    if let Some(path) = changed.iter().find(|path| !scope.is_match(path)) {
        bail!("candidate changes a path outside its granted scope: {path}");
    }
    git(worktree, &["add", "--all", "--", "."]).await?;
    let tree = git(worktree, &["write-tree"]).await?.trim().to_string();
    require_oid(&tree)?;
    // The candidate is a single commit based on the frozen target, independent
    // of any commits the worker made while producing the resulting tree.
    let message = format!("ca automation {run_id}");
    let commit = git(
        worktree,
        &[
            "-c",
            "user.name=ca automation",
            "-c",
            "user.email=automation@localhost",
            "commit-tree",
            &tree,
            "-p",
            &frozen.base_commit,
            "-m",
            &message,
        ],
    )
    .await?
    .trim()
    .to_string();
    require_oid(&commit)?;
    let prior_head = git(worktree, &["rev-parse", "HEAD"])
        .await?
        .trim()
        .to_string();
    git(worktree, &["update-ref", "HEAD", &commit, &prior_head]).await?;
    let candidate = RepositoryCandidate {
        worktree: std::fs::canonicalize(worktree)?,
        commit,
        tree,
        changed_paths: changed.into_iter().collect(),
    };
    verify_candidate(task, frozen, &candidate).await?;
    Ok(candidate)
}

async fn verify_candidate(
    task: &RepositoryTask,
    frozen: &RepositorySnapshot,
    candidate: &RepositoryCandidate,
) -> Result<()> {
    if task.digest()? != frozen.task_sha256 {
        bail!("repository task binding changed");
    }
    require_oid(&candidate.commit)?;
    require_oid(&candidate.tree)?;
    let tree = git(
        &candidate.worktree,
        &["rev-parse", &format!("{}^{{tree}}", candidate.commit)],
    )
    .await?;
    let head = git(&candidate.worktree, &["rev-parse", "HEAD"]).await?;
    if tree.trim() != candidate.tree || head.trim() != candidate.commit {
        bail!("candidate tree changed after capture");
    }
    let common = git(
        &candidate.worktree,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .await?;
    if std::fs::canonicalize(common.trim())? != frozen.common_dir {
        bail!("candidate repository binding changed after capture");
    }
    let index = git(
        &candidate.worktree,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    )
    .await?;
    if let Some(reason) = tracked_content_conflict(
        &candidate.worktree,
        Path::new(index.trim()),
        &candidate.tree,
    )
    .await?
    {
        bail!("candidate physical tree changed after capture: {reason}");
    }
    if !git(
        &candidate.worktree,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )
    .await?
    .is_empty()
    {
        bail!("candidate worktree changed after capture");
    }
    let parents = git(
        &candidate.worktree,
        &["rev-list", "--parents", "-n", "1", &candidate.commit],
    )
    .await?;
    if parents.split_whitespace().collect::<Vec<_>>()
        != [candidate.commit.as_str(), frozen.base_commit.as_str()]
    {
        bail!("candidate is not based on the frozen target");
    }
    let actual = path_output(
        &git(
            &candidate.worktree,
            &[
                "diff",
                "--name-only",
                "-z",
                "--no-renames",
                &frozen.base_commit,
                &candidate.commit,
            ],
        )
        .await?,
    )?;
    if actual
        != candidate
            .changed_paths
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
        || actual
            .iter()
            .any(|path| !task.scope().is_ok_and(|scope| scope.is_match(path)))
    {
        bail!("candidate scope evidence is stale");
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryCheckReceipt {
    pub check_sha256: String,
    pub candidate_tree: String,
    pub passed: bool,
    pub stdout: String,
    pub stderr: String,
}

/// The host supplies physical command isolation without exposing its provider,
/// profile or session implementation to the execution engine.
pub trait RepositoryCheckPreparer: Send + Sync {
    fn prepare(
        &self,
        executable: &Path,
        arguments: &[String],
        candidate_worktree: &Path,
    ) -> Result<PreparedRepositoryCheck>;
}

pub struct PreparedRepositoryCheck {
    pub program: PathBuf,
    pub arguments: Vec<String>,
    pub environment: Vec<(std::ffi::OsString, std::ffi::OsString)>,
}

pub async fn run_checks(
    task: &RepositoryTask,
    frozen: &RepositorySnapshot,
    candidate: &RepositoryCandidate,
) -> Result<Vec<RepositoryCheckReceipt>> {
    run_checks_bounded(task, frozen, candidate, OUTPUT_LIMIT as u64).await
}

pub async fn run_checks_bounded(
    task: &RepositoryTask,
    frozen: &RepositorySnapshot,
    candidate: &RepositoryCandidate,
    output_bytes: u64,
) -> Result<Vec<RepositoryCheckReceipt>> {
    run_checks_impl(task, frozen, candidate, output_bytes, None).await
}

pub async fn run_checks_with_preparer(
    task: &RepositoryTask,
    frozen: &RepositorySnapshot,
    candidate: &RepositoryCandidate,
    output_bytes: u64,
    preparer: &dyn RepositoryCheckPreparer,
) -> Result<Vec<RepositoryCheckReceipt>> {
    run_checks_impl(task, frozen, candidate, output_bytes, Some(preparer)).await
}

async fn run_checks_impl(
    task: &RepositoryTask,
    frozen: &RepositorySnapshot,
    candidate: &RepositoryCandidate,
    output_bytes: u64,
    preparer: Option<&dyn RepositoryCheckPreparer>,
) -> Result<Vec<RepositoryCheckReceipt>> {
    verify_candidate(task, frozen, candidate).await?;
    let mut receipts = Vec::new();
    let mut remaining = output_bytes.min(OUTPUT_LIMIT as u64) as usize;
    for (index, check) in task.checks.iter().enumerate() {
        let check_sha256 = digest(&serde_json::to_vec(check)?);
        let action_id = format!("repository-check-{index}");
        let run_store = crate::runtime::current_run_store();
        if let Some(store) = &run_store {
            store.prepare_action(&action_id)?;
            if !store.begin_action(&action_id)? {
                let saved = store
                    .status()?
                    .actions
                    .into_iter()
                    .find(|action| action.id == action_id)
                    .and_then(|action| action.result)
                    .context("confirmed repository check has no receipt")?;
                let receipt: RepositoryCheckReceipt = serde_json::from_value(saved)?;
                if receipt.check_sha256 != check_sha256 || receipt.candidate_tree != candidate.tree
                {
                    bail!("confirmed repository check belongs to a different candidate");
                }
                let bytes = receipt.stdout.len().saturating_add(receipt.stderr.len());
                if bytes > remaining {
                    bail!("repository check receipts exceed their output budget");
                }
                remaining -= bytes;
                let passed = receipt.passed;
                receipts.push(receipt);
                if !passed {
                    break;
                }
                continue;
            }
        }
        let executable = std::fs::canonicalize(&check.executable)?;
        if executable_digest(&executable).await? != check.executable_sha256 {
            bail!("repository check executable changed since approval");
        }
        let mut command = if let Some(preparer) = preparer {
            let prepared = preparer.prepare(&executable, &check.args, &candidate.worktree)?;
            let mut command = Command::new(prepared.program);
            command
                .args(prepared.arguments)
                .env_clear()
                .envs(prepared.environment);
            command
        } else {
            let mut command = Command::new(&executable);
            command.args(&check.args);
            command
        };
        command.current_dir(&candidate.worktree);
        let output = process(command, Duration::from_secs(check.timeout_secs), remaining).await?;
        if executable_digest(&executable).await? != check.executable_sha256 {
            bail!("repository check executable changed while verification was running");
        }
        remaining =
            remaining.saturating_sub(output.stdout.len().saturating_add(output.stderr.len()));
        let receipt = RepositoryCheckReceipt {
            check_sha256,
            candidate_tree: candidate.tree.clone(),
            passed: output.passed,
            stdout: output.stdout,
            stderr: output.stderr,
        };
        verify_candidate(task, frozen, candidate).await?;
        if let Some(store) = run_store {
            store.confirm_action(&action_id, serde_json::to_value(&receipt)?)?;
        }
        receipts.push(receipt);
        if !receipts.last().is_some_and(|receipt| receipt.passed) {
            break;
        }
    }
    Ok(receipts)
}

async fn executable_digest(path: &Path) -> Result<String> {
    const LIMIT: u64 = 256 * 1024 * 1024;
    let file = tokio::fs::File::open(path).await?;
    let metadata = file.metadata().await?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        bail!("repository check executable is not a bounded regular file");
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes).await?;
    if bytes.len() as u64 > LIMIT {
        bail!("repository check executable grew beyond its bound");
    }
    Ok(digest(&bytes))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryReviewReceipt {
    pub route: RepositoryAgentRoute,
    pub candidate_tree: String,
    pub passed: bool,
    /// Full structured independent-review output, kept in the private journal.
    pub report: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryAcceptance {
    pub candidate: RepositoryCandidate,
    pub checks: Vec<RepositoryCheckReceipt>,
    pub review: RepositoryReviewReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryPublicationState {
    Prepared,
    Publishing,
    Integrated,
    NeedsAttention,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryPublication {
    pub version: u32,
    pub run_id: String,
    pub snapshot: RepositorySnapshot,
    pub acceptance: RepositoryAcceptance,
    pub state: RepositoryPublicationState,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkout: Option<RepositoryCheckoutPublication>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryCheckoutPhase {
    IndexPrepared,
    RefPrepared,
    CheckoutApplying,
    IndexApplied,
    RefCommitted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryCheckoutPublication {
    pub phase: RepositoryCheckoutPhase,
    pub index_path: PathBuf,
    pub head_lock_path: PathBuf,
    pub alternate_index_path: PathBuf,
    pub owner_pid: u32,
    pub owner_token: String,
    pub index_guard_device: u64,
    pub index_guard_inode: u64,
}

pub struct RepositoryPublicationStore {
    root: PathBuf,
}

impl RepositoryPublicationStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn load(&self, run_id: &str) -> Result<Option<RepositoryPublication>> {
        require_run_id(run_id)?;
        let path = self.root.join(format!("{run_id}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > STATE_LIMIT
        {
            bail!("repository publication journal is not a bounded regular file");
        }
        let publication: RepositoryPublication = crate::runtime::read(&path, STATE_LIMIT)?;
        if publication.version != 1 || publication.run_id != run_id {
            bail!("repository publication journal identity is invalid");
        }
        Ok(Some(publication))
    }

    pub fn save(&self, publication: &RepositoryPublication) -> Result<()> {
        require_run_id(&publication.run_id)?;
        let bytes = serde_json::to_vec(publication)?;
        if bytes.len() as u64 > STATE_LIMIT {
            bail!("repository publication exceeds its journal bound");
        }
        crate::runtime::atomic_write(
            &self.root.join(format!("{}.json", publication.run_id)),
            publication,
        )
    }
}

pub async fn integrate(
    task: &RepositoryTask,
    frozen: &RepositorySnapshot,
    acceptance: RepositoryAcceptance,
    run_id: &str,
    journal: &RepositoryPublicationStore,
    lease_root: &Path,
) -> Result<RepositoryPublication> {
    require_run_id(run_id)?;
    let _repository =
        if crate::runtime::worker_contained() || crate::runtime::current_run_store().is_some() {
            None
        } else {
            Some(crate::runtime::ResourceLease::acquire(
                lease_root,
                &format!("repository:{}", frozen.repository.display()),
            )?)
        };
    let _lease = crate::runtime::ResourceLease::acquire(lease_root, &frozen.resource())
        .map_err(|error| anyhow!("repository integration ownership unavailable: {error}"))?;
    if let Some(existing) = journal.load(run_id)? {
        if existing.snapshot != *frozen || existing.acceptance != acceptance {
            bail!("repository run identity conflicts with its retained candidate");
        }
        return reconcile(&existing, journal).await;
    }
    verify_acceptance(task, frozen, &acceptance).await?;
    let mut publication = RepositoryPublication {
        version: 1,
        run_id: run_id.to_string(),
        snapshot: frozen.clone(),
        acceptance,
        state: RepositoryPublicationState::Prepared,
        summary: "Verified candidate retained for local integration".into(),
        checkout: None,
    };
    journal.save(&publication)?;
    let legacy_owner = match legacy_publication_owner(&publication).await {
        Ok(owner) => owner,
        Err(error) => {
            publication.state = RepositoryPublicationState::NeedsAttention;
            publication.summary = format!(
                "Repository ownership requires review: {error}; candidate retained at {}",
                publication.acceptance.candidate.worktree.display()
            );
            journal.save(&publication)?;
            return Ok(publication);
        }
    };
    // Recovery refs retain both trees even if the worktree is later removed.
    git(
        &frozen.repository,
        &[
            "update-ref",
            &format!("refs/ca/automation/{run_id}/previous"),
            &frozen.base_commit,
        ],
    )
    .await?;
    git(
        &frozen.repository,
        &[
            "update-ref",
            &format!("refs/ca/automation/{run_id}/candidate"),
            &publication.acceptance.candidate.commit,
        ],
    )
    .await?;
    journal.save(&publication)?;
    if !task.integrate {
        publication.state = RepositoryPublicationState::NeedsAttention;
        publication.summary = "Verified candidate requires local integration review".into();
        journal.save(&publication)?;
        return Ok(publication);
    }
    if let Some(reason) = target_conflict(frozen, legacy_owner.lock_path()).await? {
        publication.state = RepositoryPublicationState::NeedsAttention;
        publication.summary = format!(
            "{reason}; candidate retained at {}",
            publication.acceptance.candidate.worktree.display()
        );
        journal.save(&publication)?;
        return Ok(publication);
    }
    publication.state = RepositoryPublicationState::Publishing;
    journal.save(&publication)?;
    let checked_out = git(&frozen.repository, &["symbolic-ref", "--quiet", "HEAD"])
        .await
        .ok();
    let mutation = if checked_out.as_deref().map(str::trim) == Some(frozen.target_ref.as_str()) {
        publish_checked_out(&mut publication, journal, legacy_owner.lock_path(), &_lease).await
    } else {
        git(
            &frozen.repository,
            &[
                "update-ref",
                &frozen.target_ref,
                &publication.acceptance.candidate.commit,
                &frozen.base_commit,
            ],
        )
        .await
        .map(|_| ())
    };
    if let Err(error) = mutation {
        publication.summary = format!("Local integration outcome requires reconciliation: {error}");
    }
    reconcile_owned(&publication, journal).await
}

async fn legacy_publication_owner(
    publication: &RepositoryPublication,
) -> Result<crate::lock::RepoLockGuard> {
    let repository = &publication.snapshot.repository;
    let marker = ".orchestrator/codex-team.lock";
    if !git(repository, &["ls-files", "--", marker])
        .await?
        .is_empty()
        || !git(
            repository,
            &[
                "ls-tree",
                "--name-only",
                &publication.acceptance.candidate.commit,
                "--",
                marker,
            ],
        )
        .await?
        .is_empty()
    {
        bail!("Legacy repository lock is tracked source; ownership requires review");
    }
    crate::lock::acquire_legacy_publication_lock(repository)
}

/// Publish a candidate whose immutable evaluation/review authority was checked
/// by the host. This adapter does not rerun providers or acceptance checks.
/// It requires the frozen local branch to remain the checked-out HEAD, and
/// shares the same ownership, checkout transaction and recovery journal as
/// repository automations. A compatibility caller supplies its held repository
/// lease; a native worker must have adopted the exact repository resource.
pub async fn publish_authorized_candidate(
    mut publication: RepositoryPublication,
    journal: &RepositoryPublicationStore,
    lease_root: &Path,
    repository_lease: Option<&crate::runtime::ResourceLease>,
) -> Result<RepositoryPublication> {
    require_run_id(&publication.run_id)?;
    let repository_resource = format!("repository:{}", publication.snapshot.repository.display());
    let _repository = if let Some(lease) = repository_lease {
        lease.validate_binding(lease_root, &repository_resource)?;
        None
    } else if let Some(store) = crate::runtime::current_run_store() {
        if std::fs::canonicalize(lease_root)? != std::fs::canonicalize(&store.context().lease_root)?
        {
            bail!("authorized publication resource namespace changed");
        }
        store.require_adopted_resource(&repository_resource)?;
        None
    } else {
        Some(crate::runtime::ResourceLease::acquire(
            lease_root,
            &repository_resource,
        )?)
    };
    let lease =
        crate::runtime::ResourceLease::acquire(lease_root, &publication.snapshot.resource())
            .context("authorized publication ref ownership unavailable")?;
    if let Some(existing) = journal.load(&publication.run_id)? {
        if existing.snapshot != publication.snapshot
            || existing.acceptance != publication.acceptance
        {
            bail!("authorized publication identity conflicts with its retained candidate");
        }
        if existing.state == RepositoryPublicationState::Prepared {
            let mut retained = existing;
            retained.state = RepositoryPublicationState::NeedsAttention;
            retained.summary =
                "Publication preparation was interrupted; retained candidate requires review"
                    .into();
            journal.save(&retained)?;
            return Ok(retained);
        }
        return reconcile(&existing, journal).await;
    }
    validate_authorized_candidate(&publication).await?;
    journal.save(&publication)?;
    let legacy_owner = match legacy_publication_owner(&publication).await {
        Ok(owner) => owner,
        Err(error) => {
            publication.state = RepositoryPublicationState::NeedsAttention;
            publication.summary = format!(
                "Repository ownership requires review: {error}; authorized candidate retained"
            );
            journal.save(&publication)?;
            return Ok(publication);
        }
    };
    for (name, commit) in [
        ("previous", publication.snapshot.base_commit.as_str()),
        (
            "candidate",
            publication.acceptance.candidate.commit.as_str(),
        ),
    ] {
        git(
            &publication.snapshot.repository,
            &[
                "update-ref",
                &format!("refs/ca/automation/{}/{name}", publication.run_id),
                commit,
            ],
        )
        .await?;
    }
    if !git(
        &publication.snapshot.repository,
        &["symbolic-ref", "--quiet", "HEAD"],
    )
    .await
    .is_ok_and(|head| head.trim() == publication.snapshot.target_ref)
    {
        publication.state = RepositoryPublicationState::NeedsAttention;
        publication.summary = "Checked-out HEAD changed; authorized candidate retained".into();
        journal.save(&publication)?;
        return Ok(publication);
    }
    if let Some(reason) = target_conflict(&publication.snapshot, legacy_owner.lock_path()).await? {
        publication.state = RepositoryPublicationState::NeedsAttention;
        publication.summary = format!("{reason}; authorized candidate retained");
        journal.save(&publication)?;
        return Ok(publication);
    }
    publication.state = RepositoryPublicationState::Publishing;
    journal.save(&publication)?;
    if let Err(error) =
        publish_checked_out(&mut publication, journal, legacy_owner.lock_path(), &lease).await
    {
        publication.summary = format!("Authorized publication requires reconciliation: {error}");
    }
    reconcile_owned(&publication, journal).await
}

async fn validate_authorized_candidate(publication: &RepositoryPublication) -> Result<()> {
    let snapshot = &publication.snapshot;
    let acceptance = &publication.acceptance;
    let candidate = &acceptance.candidate;
    require_digest(&snapshot.task_sha256)?;
    require_oid(&snapshot.base_commit)?;
    require_oid(&candidate.commit)?;
    require_oid(&candidate.tree)?;
    if publication.version != 1
        || publication.state != RepositoryPublicationState::Prepared
        || publication.checkout.is_some()
        || serde_json::to_vec(publication)?.len() as u64 > STATE_LIMIT
        || !snapshot.repository.is_absolute()
        || !snapshot.common_dir.is_absolute()
        || !candidate.worktree.is_absolute()
        || !candidate.worktree.is_dir()
        || std::fs::canonicalize(&snapshot.repository)? != snapshot.repository
        || std::fs::canonicalize(&snapshot.common_dir)? != snapshot.common_dir
        || std::fs::canonicalize(&candidate.worktree)? != candidate.worktree
        || !snapshot.target_ref.starts_with("refs/heads/")
        || snapshot.target_ref.len() > 272
        || candidate.changed_paths.len() > 4096
        || candidate.changed_paths.iter().any(|path| {
            path.is_empty()
                || path.len() > 1024
                || path.contains('\0')
                || Path::new(path)
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
        })
        || !acceptance.review.passed
        || acceptance.review.report.is_null()
        || acceptance.review.candidate_tree != candidate.tree
        || acceptance.checks.len() > 128
        || acceptance.checks.iter().any(|check| {
            !check.passed
                || check.candidate_tree != candidate.tree
                || require_digest(&check.check_sha256).is_err()
        })
    {
        bail!("authorized publication has invalid or incomplete immutable bindings");
    }
    git(
        &snapshot.repository,
        &["check-ref-format", &snapshot.target_ref],
    )
    .await?;
    let root = git(&snapshot.repository, &["rev-parse", "--show-toplevel"]).await?;
    let common = git(
        &snapshot.repository,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .await?;
    if std::fs::canonicalize(root.trim())? != snapshot.repository
        || std::fs::canonicalize(common.trim())? != snapshot.common_dir
    {
        bail!("authorized publication repository binding changed");
    }
    if git(
        &snapshot.repository,
        &[
            "rev-parse",
            "--verify",
            &format!("{}^{{tree}}", candidate.commit),
        ],
    )
    .await?
    .trim()
        != candidate.tree
        || git(
            &snapshot.repository,
            &["rev-list", "--parents", "-n", "1", &candidate.commit],
        )
        .await?
        .split_whitespace()
        .collect::<Vec<_>>()
            != [candidate.commit.as_str(), snapshot.base_commit.as_str()]
        || path_output(
            &git(
                &snapshot.repository,
                &[
                    "diff",
                    "--name-only",
                    "-z",
                    "--no-renames",
                    &snapshot.base_commit,
                    &candidate.commit,
                ],
            )
            .await?,
        )? != candidate
            .changed_paths
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
    {
        bail!("authorized publication candidate tree or frozen-parent evidence changed");
    }
    Ok(())
}

fn index_marker(checkout: &RepositoryCheckoutPublication) -> Vec<u8> {
    format!(
        "ca.repository-index.v1 {} {}\n",
        checkout.owner_pid, checkout.owner_token
    )
    .into_bytes()
}

struct IndexGuard {
    path: PathBuf,
    file: std::fs::File,
    marker: Vec<u8>,
}

impl IndexGuard {
    fn create(checkout: &mut RepositoryCheckoutPublication) -> Result<Self> {
        let path = checkout.index_path.with_extension("lock");
        // Git appends `.lock` to `index`; do not change arbitrary extensions.
        let path = if path.file_name() == Some(std::ffi::OsStr::new("index.lock")) {
            path
        } else {
            PathBuf::from(format!("{}.lock", checkout.index_path.display()))
        };
        let mut options = std::fs::OpenOptions::new();
        options.write(true).read(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options
            .open(&path)
            .context("target index is already owned")?;
        let marker = index_marker(checkout);
        file.write_all(&marker)?;
        file.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = file.metadata()?;
            checkout.index_guard_device = metadata.dev();
            checkout.index_guard_inode = metadata.ino();
        }
        Ok(Self { path, file, marker })
    }
}

impl Drop for IndexGuard {
    fn drop(&mut self) {
        let Ok(owned) = self.file.metadata() else {
            return;
        };
        let Ok(current) = std::fs::symlink_metadata(&self.path) else {
            return;
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if current.dev() != owned.dev() || current.ino() != owned.ino() {
                return;
            }
        }
        if current.is_file()
            && !current.file_type().is_symlink()
            && current.len() == self.marker.len() as u64
            && std::fs::read(&self.path).is_ok_and(|bytes| bytes == self.marker)
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn validate_checkout_paths(
    publication: &RepositoryPublication,
    checkout: &RepositoryCheckoutPublication,
) -> Result<()> {
    require_digest(&checkout.owner_token)?;
    let git_dir = checkout
        .index_path
        .parent()
        .context("checkout index has no parent")?;
    if !git_dir.is_absolute()
        || checkout.index_path.file_name() != Some(std::ffi::OsStr::new("index"))
        || checkout.head_lock_path != git_dir.join("HEAD.lock")
        || checkout.alternate_index_path
            != git_dir.join(format!("ca-automation-{}.index", checkout.owner_token))
        || checkout.owner_pid == 0
        || !git_dir.starts_with(&publication.snapshot.common_dir)
    {
        bail!("checkout ownership paths are not bound to the frozen repository");
    }
    Ok(())
}

fn recover_index_guard(checkout: &RepositoryCheckoutPublication) -> Result<()> {
    let path = PathBuf::from(format!("{}.lock", checkout.index_path.display()));
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let expected = index_marker(checkout);
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() != expected.len() as u64
        || std::fs::read(&path)? != expected
    {
        bail!("index lock ownership cannot be proven; retained for review");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.dev() != checkout.index_guard_device
            || metadata.ino() != checkout.index_guard_inode
            || checkout.index_guard_inode == 0
        {
            bail!("index lock identity changed; retained for review");
        }
        let pid = i32::try_from(checkout.owner_pid).context("index owner PID is invalid")?;
        if unsafe { libc::kill(pid, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
        {
            bail!("index owner may still be alive; retained for review");
        }
        std::fs::remove_file(path)?;
        Ok(())
    }
    #[cfg(not(unix))]
    bail!("index owner death cannot be proven on this platform")
}

struct PreparedRefTransaction {
    child: tokio::process::Child,
    input: Option<tokio::process::ChildStdin>,
    output: tokio::process::ChildStdout,
    diagnostics: tokio::task::JoinHandle<Result<Vec<u8>>>,
    _group: ProcessGroup,
}

impl PreparedRefTransaction {
    async fn prepare(
        publication: &RepositoryPublication,
        publication_lease: &crate::runtime::ResourceLease,
    ) -> Result<Self> {
        let command = crate::git_metadata::query(
            &publication.snapshot.repository,
            &["update-ref", "--stdin"],
        )?;
        let mut command = Command::from(command);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let contained = CONTAINED_COMMANDS.try_with(|value| *value).unwrap_or(false);
        #[cfg(unix)]
        if !contained {
            command.process_group(0);
        }
        #[cfg(unix)]
        {
            let descriptor = publication_lease.raw_fd();
            // This fixed trusted Git transaction retains only its ref claim.
            // The parent's descriptor and ordinary command authority stay private.
            unsafe {
                command.pre_exec(move || {
                    let flags = libc::fcntl(descriptor, libc::F_GETFD);
                    if flags < 0
                        || libc::fcntl(descriptor, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        #[cfg(not(unix))]
        let _ = publication_lease;
        let mut child = command.spawn()?;
        let group = ProcessGroup(
            (!contained)
                .then(|| child.id().and_then(|pid| i32::try_from(pid).ok()))
                .flatten(),
        );
        let input = child
            .stdin
            .take()
            .context("Git ref transaction input unavailable")?;
        let output = child
            .stdout
            .take()
            .context("Git ref transaction output unavailable")?;
        let errors = child
            .stderr
            .take()
            .context("Git ref transaction diagnostics unavailable")?;
        let diagnostics = tokio::spawn(read_bounded(
            errors,
            64 * 1024,
            std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        ));
        let mut transaction = Self {
            child,
            input: Some(input),
            output,
            diagnostics,
            _group: group,
        };
        let request = format!(
            "start\nupdate {} {} {}\nprepare\n",
            publication.snapshot.target_ref,
            publication.acceptance.candidate.commit,
            publication.snapshot.base_commit
        );
        let prepared = tokio::time::timeout(Duration::from_secs(30), async {
            transaction
                .input
                .as_mut()
                .context("Git ref transaction input is closed")?
                .write_all(request.as_bytes())
                .await?;
            transaction.response("start: ok").await?;
            transaction.response("prepare: ok").await
        })
        .await
        .context("Git ref preparation timed out")?;
        if let Err(error) = prepared {
            transaction.abort().await;
            return Err(error);
        }
        Ok(transaction)
    }

    async fn response(&mut self, expected: &str) -> Result<()> {
        let mut line = Vec::new();
        for _ in 0..128 {
            let byte = self
                .output
                .read_u8()
                .await
                .context("Git ref transaction ended before acknowledgement")?;
            if byte == b'\n' {
                if line == expected.as_bytes() {
                    return Ok(());
                }
                bail!("Git ref transaction returned an unexpected acknowledgement");
            }
            line.push(byte);
        }
        bail!("Git ref transaction acknowledgement exceeded its bound")
    }

    async fn commit(&mut self) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(30), async {
            self.input
                .as_mut()
                .context("Git ref transaction input is closed")?
                .write_all(b"commit\n")
                .await?;
            self.response("commit: ok").await?;
            // Pipe shutdown does not close an owned ChildStdin descriptor.
            // Git waits for EOF after acknowledging the transaction.
            drop(self.input.take());
            if !self.child.wait().await?.success() {
                bail!("Git ref transaction commit failed");
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("Git ref commit timed out")?
    }

    async fn abort(&mut self) {
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            self.input
                .as_mut()
                .context("Git ref transaction input is closed")?
                .write_all(b"abort\n")
                .await?;
            self.response("abort: ok").await?;
            drop(self.input.take());
            self.child.wait().await?;
            Ok::<_, anyhow::Error>(())
        })
        .await;
        drop(self.input.take());
        let _ = self.child.start_kill();
    }
}

impl Drop for PreparedRefTransaction {
    fn drop(&mut self) {
        drop(self.input.take());
        let _ = self.child.start_kill();
        self.diagnostics.abort();
    }
}

async fn publish_checked_out(
    publication: &mut RepositoryPublication,
    journal: &RepositoryPublicationStore,
    legacy_marker: &Path,
    publication_lease: &crate::runtime::ResourceLease,
) -> Result<()> {
    let repository = publication.snapshot.repository.clone();
    let index = git(
        &repository,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    )
    .await?;
    let index_path = PathBuf::from(index.trim());
    let metadata = std::fs::symlink_metadata(&index_path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 * 1024 * 1024
    {
        bail!("target index is not a bounded regular file");
    }
    if git(&repository, &["ls-files", "-v", "-z"])
        .await?
        .split('\0')
        .filter(|line| !line.is_empty())
        .any(|line| !line.starts_with("H "))
    {
        bail!("target sparse or assume-unchanged index needs integration review");
    }
    let mut entropy = [0_u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut entropy)?;
    let owner_token = digest(&entropy);
    let git_dir = index_path.parent().context("target index has no parent")?;
    let mut checkout = RepositoryCheckoutPublication {
        phase: RepositoryCheckoutPhase::IndexPrepared,
        index_path: index_path.clone(),
        head_lock_path: git_dir.join("HEAD.lock"),
        alternate_index_path: git_dir.join(format!("ca-automation-{owner_token}.index")),
        owner_pid: std::process::id(),
        owner_token,
        index_guard_device: 0,
        index_guard_inode: 0,
    };
    validate_checkout_paths(publication, &checkout)?;
    publication.checkout = Some(checkout.clone());
    journal.save(publication)?;
    let _index_guard = IndexGuard::create(&mut checkout)?;
    publication.checkout = Some(checkout.clone());
    journal.save(publication)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut alternate = options.open(&checkout.alternate_index_path)?;
    let mut original = std::fs::File::open(&index_path)?.take(64 * 1024 * 1024 + 1);
    if std::io::copy(&mut original, &mut alternate)? > 64 * 1024 * 1024 {
        bail!("target index grew beyond its bound");
    }
    alternate.sync_all()?;
    if git(&repository, &["symbolic-ref", "--quiet", "HEAD"])
        .await?
        .trim()
        != publication.snapshot.target_ref
    {
        bail!("HEAD switched before publication preparation");
    }
    #[cfg(test)]
    pause_publication(RepositoryCheckoutPhase::IndexPrepared).await;
    let mut transaction = PreparedRefTransaction::prepare(publication, publication_lease).await?;
    let result = async {
        if git(&repository, &["symbolic-ref", "--quiet", "HEAD"])
            .await?
            .trim()
            != publication.snapshot.target_ref
            || !std::fs::symlink_metadata(&checkout.head_lock_path)
                .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        {
            bail!("Git did not pin the exact checked-out HEAD for publication");
        }
        if let Some(reason) = target_conflict(&publication.snapshot, legacy_marker).await? {
            bail!("{reason}");
        }
        checkout.phase = RepositoryCheckoutPhase::RefPrepared;
        publication.checkout = Some(checkout.clone());
        journal.save(publication)?;
        #[cfg(test)]
        pause_publication(RepositoryCheckoutPhase::RefPrepared).await;
        prevent_checkout_collisions(publication).await?;
        checkout.phase = RepositoryCheckoutPhase::CheckoutApplying;
        publication.checkout = Some(checkout.clone());
        journal.save(publication)?;
        let command = crate::git_metadata::query(
            &repository,
            &[
                "read-tree",
                "-u",
                "-m",
                "--no-sparse-checkout",
                &publication.snapshot.base_commit,
                &publication.acceptance.candidate.commit,
            ],
        )?;
        let mut command = Command::from(command);
        command.env("GIT_INDEX_FILE", &checkout.alternate_index_path);
        let output = process(command, Duration::from_secs(30), OUTPUT_LIMIT).await?;
        if !output.passed {
            bail!(
                "safe checkout refused publication: {}",
                output.stderr.trim()
            );
        }
        let mut command = Command::from(crate::git_metadata::query(&repository, &["write-tree"])?);
        command.env("GIT_INDEX_FILE", &checkout.alternate_index_path);
        let tree = process(command, Duration::from_secs(30), OUTPUT_LIMIT).await?;
        if !tree.passed || tree.stdout.trim() != publication.acceptance.candidate.tree {
            bail!("checkout index differs from the accepted candidate tree");
        }
        std::fs::File::open(&checkout.alternate_index_path)?.sync_all()?;
        std::fs::rename(&checkout.alternate_index_path, &checkout.index_path)?;
        std::fs::File::open(git_dir)?.sync_all()?;
        checkout.phase = RepositoryCheckoutPhase::IndexApplied;
        publication.checkout = Some(checkout.clone());
        journal.save(publication)?;
        if let Some(reason) = checkout_conflict(publication).await? {
            bail!("{reason}");
        }
        #[cfg(test)]
        pause_publication(RepositoryCheckoutPhase::IndexApplied).await;
        transaction.commit().await?;
        checkout.phase = RepositoryCheckoutPhase::RefCommitted;
        publication.checkout = Some(checkout);
        journal.save(publication)?;
        #[cfg(test)]
        pause_publication(RepositoryCheckoutPhase::RefCommitted).await;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if result.is_err() {
        transaction.abort().await;
    }
    result
}

async fn prevent_checkout_collisions(publication: &RepositoryPublication) -> Result<()> {
    let repository = &publication.snapshot.repository;
    let changed = &publication.acceptance.candidate.changed_paths;
    if changed.len() > 4096 || changed.iter().map(String::len).sum::<usize>() > 128 * 1024 {
        bail!("checkout collision validation exceeds its touched-path bound");
    }
    for path in changed {
        let relative = Path::new(path);
        let mut ancestor = relative.parent();
        while let Some(parent) = ancestor.filter(|parent| !parent.as_os_str().is_empty()) {
            match std::fs::symlink_metadata(repository.join(parent)) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    bail!("checkout path has a symlink ancestor; integration needs review");
                }
                Ok(metadata) if !metadata.is_dir() => {
                    let parent = parent.to_str().context("checkout ancestor is not UTF-8")?;
                    if git(repository, &["ls-files", "--stage", "--", parent])
                        .await?
                        .is_empty()
                    {
                        bail!("checkout would replace an untracked file ancestor");
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            ancestor = parent.parent();
        }
    }
    if changed.is_empty() {
        return Ok(());
    }
    // Omitting --exclude-standard includes ignored files. Git's read-tree
    // otherwise permits overwriting them, even in a two-tree merge.
    let mut arguments = vec!["ls-files", "--others", "-z", "--"];
    arguments.extend(changed.iter().map(String::as_str));
    let untracked = path_output(&git(repository, &arguments).await?)?;
    if untracked.iter().any(|path| {
        changed.iter().any(|changed| {
            Path::new(path).starts_with(changed) || Path::new(changed).starts_with(path)
        })
    }) {
        bail!("checkout would overwrite an untracked or ignored path; contents retained");
    }
    Ok(())
}

async fn checkout_conflict(publication: &RepositoryPublication) -> Result<Option<String>> {
    let repository = &publication.snapshot.repository;
    if !git(repository, &["symbolic-ref", "--quiet", "HEAD"])
        .await
        .is_ok_and(|head| head.trim() == publication.snapshot.target_ref)
    {
        return Ok(Some(
            "checked-out HEAD no longer names the frozen target".into(),
        ));
    }
    match checkout_index_conflict(publication).await {
        Ok(Some(reason)) => return Ok(Some(reason)),
        Err(error) => return Ok(Some(format!("target index proof requires review: {error}"))),
        Ok(None) => {}
    }
    let candidate = &publication.acceptance.candidate.commit;
    if !git(
        repository,
        &["diff", "--cached", "--name-only", "-z", candidate],
    )
    .await?
    .is_empty()
        || !git(repository, &["diff", "--name-only", "-z", candidate])
            .await?
            .is_empty()
    {
        return Ok(Some(
            "target index or files do not match the accepted candidate".into(),
        ));
    }
    let untracked = git(
        repository,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )
    .await?;
    if untracked
        .split('\0')
        .any(|path| !path.is_empty() && path != ".orchestrator/codex-team.lock")
    {
        return Ok(Some("target checkout contains uncommitted files".into()));
    }
    Ok(None)
}

struct PrivateIndexProof {
    path: PathBuf,
    identity: std::fs::Metadata,
}

impl Drop for PrivateIndexProof {
    fn drop(&mut self) {
        if std::fs::symlink_metadata(&self.path).is_ok_and(|metadata| {
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && same_file_identity(&metadata, &self.identity)
        }) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn same_file_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        left.dev() == right.dev() && left.ino() == right.ino()
    }
    #[cfg(not(unix))]
    {
        left.len() == right.len() && left.modified().ok() == right.modified().ok()
    }
}

fn read_bounded_index(path: &Path) -> Result<(Vec<u8>, std::fs::Metadata)> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
        bail!("target index is not a bounded regular file");
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 * 1024 {
        bail!("target index grew beyond its bound");
    }
    Ok((bytes, metadata))
}

async fn checkout_index_conflict(publication: &RepositoryPublication) -> Result<Option<String>> {
    let checkout = publication
        .checkout
        .as_ref()
        .context("checkout proof has no owned index binding")?;
    tracked_content_conflict(
        &publication.snapshot.repository,
        &checkout.index_path,
        &publication.acceptance.candidate.tree,
    )
    .await
}

async fn tracked_content_conflict(
    repository: &Path,
    index_path: &Path,
    accepted_tree: &str,
) -> Result<Option<String>> {
    let (bytes, identity) = read_bounded_index(index_path)?;
    let mut entropy = [0_u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut entropy)?;
    let path = index_path
        .parent()
        .context("checkout index has no parent")?
        .join(format!("ca-automation-{}-proof.index", digest(&entropy)));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&path)?;
    let mut proof = PrivateIndexProof {
        path,
        identity: file.metadata()?,
    };
    file.write_all(&bytes)?;
    drop(file);
    // write-tree needs its own index lock. The real index.lock remains held
    // throughout publication; this private copy supplies only read-back proof.
    let mut command = Command::from(crate::git_metadata::query(
        repository,
        &["ls-files", "-v", "-z"],
    )?);
    command.env("GIT_INDEX_FILE", &proof.path);
    let flags = process(command, Duration::from_secs(30), OUTPUT_LIMIT).await?;
    if !flags.passed {
        bail!("checkout index flags could not be verified");
    }
    if flags
        .stdout
        .split('\0')
        .filter(|line| !line.is_empty())
        .any(|line| !line.starts_with("H "))
    {
        return Ok(Some(
            "target index flags can hide checkout changes; integration review is required".into(),
        ));
    }
    let mut command = Command::from(crate::git_metadata::query(
        repository,
        &["add", "--renormalize", "--all", "--", "."],
    )?);
    command.env("GIT_INDEX_FILE", &proof.path);
    let refreshed = process(command, Duration::from_secs(30), OUTPUT_LIMIT).await?;
    if !refreshed.passed {
        bail!("tracked checkout content could not be verified");
    }
    proof.identity = std::fs::symlink_metadata(&proof.path)?;
    let mut command = Command::from(crate::git_metadata::query(repository, &["write-tree"])?);
    command.env("GIT_INDEX_FILE", &proof.path);
    let tree = process(command, Duration::from_secs(30), OUTPUT_LIMIT).await?;
    if !tree.passed {
        bail!("checkout index tree could not be verified");
    }
    proof.identity = std::fs::symlink_metadata(&proof.path)?;
    let (current, current_identity) = read_bounded_index(index_path)?;
    if !same_file_identity(&identity, &current_identity) || current != bytes {
        return Ok(Some("target index changed during checkout proof".into()));
    }
    if tree.stdout.trim() != accepted_tree {
        return Ok(Some(
            "target index tree does not match the accepted candidate".into(),
        ));
    }
    Ok(None)
}

#[cfg(test)]
struct PublicationBarrier {
    phase: RepositoryCheckoutPhase,
    ready: tokio::sync::Notify,
    resume: tokio::sync::Notify,
}
#[cfg(test)]
tokio::task_local! { static PUBLICATION_BARRIER: std::sync::Arc<PublicationBarrier>; }
#[cfg(test)]
async fn pause_publication(phase: RepositoryCheckoutPhase) {
    let barrier = PUBLICATION_BARRIER.try_with(Clone::clone).ok();
    if let Some(barrier) = barrier.filter(|barrier| barrier.phase == phase) {
        barrier.ready.notify_one();
        barrier.resume.notified().await;
    }
}

async fn verify_acceptance(
    task: &RepositoryTask,
    frozen: &RepositorySnapshot,
    acceptance: &RepositoryAcceptance,
) -> Result<()> {
    verify_candidate(task, frozen, &acceptance.candidate).await?;
    if acceptance.checks.len() != task.checks.len()
        || acceptance
            .checks
            .iter()
            .zip(&task.checks)
            .any(|(receipt, check)| {
                !receipt.passed
                    || receipt.candidate_tree != acceptance.candidate.tree
                    || serde_json::to_vec(check)
                        .map(|bytes| digest(&bytes))
                        .ok()
                        .as_deref()
                        != Some(receipt.check_sha256.as_str())
            })
        || !acceptance.review.passed
        || acceptance.review.route != task.reviewer_route
        || acceptance.review.candidate_tree != acceptance.candidate.tree
        || acceptance.review.report.is_null()
    {
        bail!("integration requires complete host checks and independent review of the exact candidate tree");
    }
    Ok(())
}

async fn target_conflict(
    frozen: &RepositorySnapshot,
    owned_legacy_marker: &Path,
) -> Result<Option<String>> {
    let current = git(
        &frozen.repository,
        &["rev-parse", "--verify", &frozen.target_ref],
    )
    .await?;
    if current.trim() != frozen.base_commit {
        return Ok(Some(
            "Target branch advanced after this occurrence was frozen".into(),
        ));
    }
    // Every checkout of the target must be clean. A branch checked out in a
    // different worktree is retained for review rather than updated behind it.
    let worktrees = git(&frozen.repository, &["worktree", "list", "--porcelain"]).await?;
    let mut path = None;
    for line in worktrees.lines() {
        if let Some(value) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(value));
        } else if line.strip_prefix("branch ") == Some(&frozen.target_ref) {
            let target = path
                .as_deref()
                .ok_or_else(|| anyhow!("target checkout identity is unavailable"))?;
            if std::fs::canonicalize(target)? != frozen.repository {
                return Ok(Some("Target branch is owned by another worktree".into()));
            }
            let status = git(
                target,
                &["status", "--porcelain=v1", "--untracked-files=all"],
            )
            .await?;
            let relative_marker = owned_legacy_marker
                .strip_prefix(target)
                .context("legacy marker is outside the owned target checkout")?;
            let own_untracked_marker = format!("?? {}", relative_marker.display());
            if status.lines().any(|line| line != own_untracked_marker) {
                return Ok(Some("Target checkout has uncommitted changes".into()));
            }
        }
    }
    Ok(None)
}

pub async fn reconcile(
    publication: &RepositoryPublication,
    journal: &RepositoryPublicationStore,
) -> Result<RepositoryPublication> {
    if !matches!(
        publication.state,
        RepositoryPublicationState::Publishing | RepositoryPublicationState::Integrated
    ) {
        return Ok(publication.clone());
    }
    let _legacy_owner = match legacy_publication_owner(publication).await {
        Ok(owner) => owner,
        Err(error) => {
            let mut retained = publication.clone();
            retained.state = RepositoryPublicationState::NeedsAttention;
            retained.summary = format!(
                "Repository ownership requires review during publication reconciliation: {error}; candidate retained at {}",
                retained.acceptance.candidate.worktree.display()
            );
            journal.save(&retained)?;
            return Ok(retained);
        }
    };
    reconcile_owned(publication, journal).await
}

async fn reconcile_owned(
    publication: &RepositoryPublication,
    journal: &RepositoryPublicationStore,
) -> Result<RepositoryPublication> {
    let mut publication = publication.clone();
    if !matches!(
        publication.state,
        RepositoryPublicationState::Publishing | RepositoryPublicationState::Integrated
    ) {
        return Ok(publication);
    }
    let current = git(
        &publication.snapshot.repository,
        &["rev-parse", "--verify", &publication.snapshot.target_ref],
    )
    .await?;
    let checkout_valid = if let Some(checkout) = &publication.checkout {
        validate_checkout_paths(&publication, checkout).and_then(|_| recover_index_guard(checkout))
    } else {
        Ok(())
    };
    let checkout_conflict = if checkout_valid.is_ok() && publication.checkout.is_some() {
        checkout_conflict(&publication).await?
    } else {
        checkout_valid.err().map(|error| error.to_string())
    };
    if current.trim() == publication.acceptance.candidate.commit && checkout_conflict.is_none() {
        publication.state = RepositoryPublicationState::Integrated;
        publication.summary = format!(
            "Verified candidate {} integrated by local fast-forward",
            publication.acceptance.candidate.commit
        );
    } else {
        publication.state = RepositoryPublicationState::NeedsAttention;
        publication.summary = if let Some(reason) = checkout_conflict {
            format!("Checkout publication requires attention: {reason}; candidate retained")
        } else if current.trim() == publication.snapshot.base_commit {
            "Publication did not advance the target; retained candidate needs integration review"
                .into()
        } else {
            "Target differs from both journaled refs; publication requires attention".into()
        };
    }
    journal.save(&publication)?;
    Ok(publication)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn require_digest(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid SHA-256 binding");
    }
    Ok(())
}

fn require_oid(value: &str) -> Result<()> {
    if !matches!(value.len(), 40 | 64) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("Git did not return a complete object identity");
    }
    Ok(())
}

fn require_run_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("repository run identity is unsafe");
    }
    Ok(())
}

fn path_output(output: &str) -> Result<BTreeSet<String>> {
    output
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(|path| {
            if Path::new(path)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            {
                bail!("Git changed path is unsafe");
            }
            Ok(path.to_string())
        })
        .collect()
}

#[derive(Debug)]
struct ProcessOutput {
    passed: bool,
    stdout: String,
    stderr: String,
}

struct ProcessGroup(Option<i32>);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            // The process group was created for this one command below.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
}

async fn process(
    mut command: Command,
    timeout: Duration,
    output_bytes: usize,
) -> Result<ProcessOutput> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let contained = CONTAINED_COMMANDS
        .try_with(|contained| *contained)
        .unwrap_or(false);
    #[cfg(unix)]
    if !contained {
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let _group = ProcessGroup(
        (!contained)
            .then(|| child.id().and_then(|pid| i32::try_from(pid).ok()))
            .flatten(),
    );
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("stdout pipe unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("stderr pipe unavailable"))?;
    let bounded = async {
        let retained = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (status, out, err) = tokio::try_join!(
            async { Ok::<_, anyhow::Error>(child.wait().await?) },
            read_bounded(stdout, output_bytes, retained.clone()),
            read_bounded(stderr, output_bytes, retained)
        )?;
        Ok::<_, anyhow::Error>(ProcessOutput {
            passed: status.success(),
            stdout: String::from_utf8(out)?,
            stderr: String::from_utf8(err)?,
        })
    };
    tokio::time::timeout(timeout, bounded)
        .await
        .map_err(|_| anyhow!("repository command timed out"))?
}

async fn read_bounded(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    limit: usize,
    retained: std::sync::Arc<std::sync::atomic::AtomicUsize>,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let count = reader.read(&mut chunk).await?;
        if count == 0 {
            return Ok(bytes);
        }
        let previous = retained.fetch_add(count, std::sync::atomic::Ordering::Relaxed);
        if previous.saturating_add(count) > limit {
            bail!("repository command output exceeds its bound");
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

async fn git(root: &Path, args: &[&str]) -> Result<String> {
    let command = Command::from(crate::git_metadata::query(root, args)?);
    let output = process(command, Duration::from_secs(30), OUTPUT_LIMIT).await?;
    if !output.passed {
        bail!("Git command failed: {}", output.stderr.trim());
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _temporary: tempfile::TempDir,
        root: PathBuf,
        task: RepositoryTask,
        frozen: RepositorySnapshot,
        candidate: RepositoryCandidate,
    }

    async fn fixture() -> Result<Fixture> {
        let temporary = tempfile::tempdir()?;
        let root = std::fs::canonicalize(temporary.path())?;
        let repo = root.join("repo");
        std::fs::create_dir(&repo)?;
        git(&repo, &["init", "-b", "main"]).await?;
        git(&repo, &["config", "user.name", "Fixture"]).await?;
        git(&repo, &["config", "user.email", "fixture@localhost"]).await?;
        std::fs::write(repo.join("allowed.txt"), "old\n")?;
        std::fs::write(repo.join("outside.txt"), "untouched\n")?;
        git(&repo, &["add", "."]).await?;
        git(&repo, &["commit", "-m", "base"]).await?;
        let executable = std::fs::canonicalize("/usr/bin/true")?;
        let route = RepositoryAgentRoute {
            provider: "fixture".into(),
            model: "fixture-model".into(),
            pricing: RepositoryPricing::LocalZero,
        };
        let task = RepositoryTask {
            repository: repo.clone(),
            target_branch: "main".into(),
            allowed_paths: vec!["allowed.txt".into()],
            prompt: "Update allowed.txt".into(),
            worker_route: route.clone(),
            reviewer_route: route,
            checks: vec![RepositoryCheck {
                executable_sha256: digest(&std::fs::read(&executable)?),
                executable,
                args: Vec::new(),
                timeout_secs: 5,
            }],
            integrate: true,
        };
        let frozen = inspect(&task).await?;
        let worktree = root.join("candidate");
        let path = worktree.to_string_lossy();
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "ca/fixture",
                &path,
                &frozen.base_commit,
            ],
        )
        .await?;
        std::fs::write(worktree.join("allowed.txt"), "new\n")?;
        let candidate = capture_candidate(&task, &frozen, &worktree, "fixture-run").await?;
        Ok(Fixture {
            _temporary: temporary,
            root,
            task,
            frozen,
            candidate,
        })
    }

    async fn acceptance(fixture: &Fixture) -> Result<RepositoryAcceptance> {
        Ok(RepositoryAcceptance {
            candidate: fixture.candidate.clone(),
            checks: run_checks(&fixture.task, &fixture.frozen, &fixture.candidate).await?,
            review: RepositoryReviewReceipt {
                route: fixture.task.reviewer_route.clone(),
                candidate_tree: fixture.candidate.tree.clone(),
                passed: true,
                report: serde_json::json!({"summary":"No findings", "findings":[]}),
            },
        })
    }

    async fn paused_integration(
        fixture: &Fixture,
        phase: RepositoryCheckoutPhase,
    ) -> Result<(
        std::sync::Arc<PublicationBarrier>,
        tokio::task::JoinHandle<Result<RepositoryPublication>>,
    )> {
        let barrier = std::sync::Arc::new(PublicationBarrier {
            phase,
            ready: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
        });
        let ready = barrier.clone();
        let task = fixture.task.clone();
        let frozen = fixture.frozen.clone();
        let evidence = acceptance(fixture).await?;
        let root = fixture.root.clone();
        let handle = tokio::spawn(PUBLICATION_BARRIER.scope(barrier.clone(), async move {
            integrate(
                &task,
                &frozen,
                evidence,
                "fixture-run",
                &RepositoryPublicationStore::new(root.join("journal")),
                &root.join("leases"),
            )
            .await
        }));
        tokio::time::timeout(Duration::from_secs(10), ready.ready.notified())
            .await
            .context("publication fixture did not reach its transaction boundary")?;
        Ok((barrier, handle))
    }

    #[tokio::test]
    async fn prepared_publication_blocks_external_head_switch_ref_rewind_and_index_reset(
    ) -> Result<()> {
        let mut fixture = fixture().await?;
        let parent = fixture.frozen.base_commit.clone();
        std::fs::write(fixture.task.repository.join("outside.txt"), "next base\n")?;
        git(&fixture.task.repository, &["add", "outside.txt"]).await?;
        git(&fixture.task.repository, &["commit", "-m", "next base"]).await?;
        fixture.frozen = inspect(&fixture.task).await?;
        let worktree = fixture.candidate.worktree.clone();
        git(&worktree, &["reset", "--hard", &fixture.frozen.base_commit]).await?;
        std::fs::write(worktree.join("allowed.txt"), "new\n")?;
        fixture.candidate =
            capture_candidate(&fixture.task, &fixture.frozen, &worktree, "fixture-run").await?;
        let (barrier, handle) =
            paused_integration(&fixture, RepositoryCheckoutPhase::RefPrepared).await?;
        for args in [
            vec!["symbolic-ref", "HEAD", "refs/heads/other"],
            vec!["update-ref", "refs/heads/main", parent.as_str()],
            vec!["reset", "--hard", parent.as_str()],
        ] {
            let command =
                Command::from(crate::git_metadata::query(&fixture.task.repository, &args)?);
            assert!(
                !process(command, Duration::from_secs(5), OUTPUT_LIMIT)
                    .await?
                    .passed,
                "external Git mutation bypassed prepared HEAD/ref/index ownership"
            );
        }
        barrier.resume.notify_one();
        let integrated = tokio::time::timeout(Duration::from_secs(10), handle).await???;
        assert_eq!(integrated.state, RepositoryPublicationState::Integrated);
        assert_eq!(
            git(&fixture.task.repository, &["symbolic-ref", "HEAD"])
                .await?
                .trim(),
            "refs/heads/main"
        );
        assert_eq!(
            git(&fixture.task.repository, &["rev-parse", "HEAD"])
                .await?
                .trim(),
            fixture.candidate.commit
        );
        assert_eq!(
            std::fs::read_to_string(fixture.task.repository.join("allowed.txt"))?,
            "new\n"
        );
        Ok(())
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn prepared_git_transaction_retains_ref_lease_after_coordinator_closes_its_copy(
    ) -> Result<()> {
        let fixture = fixture().await?;
        let publication = RepositoryPublication {
            version: 1,
            run_id: "fixture-run".into(),
            snapshot: fixture.frozen.clone(),
            acceptance: acceptance(&fixture).await?,
            state: RepositoryPublicationState::Publishing,
            summary: "prepared transaction fixture".into(),
            checkout: None,
        };
        let root = fixture.root.join("leases");
        let resource = fixture.frozen.resource();
        let lease = crate::runtime::ResourceLease::acquire(&root, &resource)?;
        let flags = unsafe { libc::fcntl(lease.raw_fd(), libc::F_GETFD) };
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
        let mut transaction = PreparedRefTransaction::prepare(&publication, &lease).await?;
        assert_eq!(unsafe { libc::fcntl(lease.raw_fd(), libc::F_GETFD) }, flags);
        drop(lease);
        assert!(crate::runtime::ResourceLease::acquire(&root, &resource).is_err());
        transaction.abort().await;
        assert!(crate::runtime::ResourceLease::acquire(&root, &resource).is_ok());
        assert_eq!(
            git(&fixture.task.repository, &["rev-parse", "main"])
                .await?
                .trim(),
            fixture.frozen.base_commit
        );
        let lease = crate::runtime::ResourceLease::acquire(&root, &resource)?;
        let mut committing = PreparedRefTransaction::prepare(&publication, &lease).await?;
        tokio::time::timeout(Duration::from_secs(2), committing.commit())
            .await
            .context("acknowledged Git commit did not exit after its input closed")??;
        assert_eq!(
            git(&fixture.task.repository, &["rev-parse", "main"])
                .await?
                .trim(),
            fixture.candidate.commit
        );
        Ok(())
    }

    #[tokio::test]
    async fn head_switch_before_transaction_retains_candidate_and_both_original_refs() -> Result<()>
    {
        let fixture = fixture().await?;
        git(&fixture.task.repository, &["branch", "other"]).await?;
        let (barrier, handle) =
            paused_integration(&fixture, RepositoryCheckoutPhase::IndexPrepared).await?;
        git(
            &fixture.task.repository,
            &["symbolic-ref", "HEAD", "refs/heads/other"],
        )
        .await?;
        barrier.resume.notify_one();
        let retained = tokio::time::timeout(Duration::from_secs(10), handle).await???;
        assert_eq!(retained.state, RepositoryPublicationState::NeedsAttention);
        assert!(retained.summary.contains("HEAD"));
        for name in ["main", "other"] {
            assert_eq!(
                git(&fixture.task.repository, &["rev-parse", name])
                    .await?
                    .trim(),
                fixture.frozen.base_commit
            );
        }
        assert_eq!(
            std::fs::read_to_string(fixture.task.repository.join("allowed.txt"))?,
            "old\n"
        );
        assert!(fixture.candidate.worktree.exists());
        Ok(())
    }

    #[tokio::test]
    async fn ignored_checkout_collision_retains_private_bytes_and_verified_candidate() -> Result<()>
    {
        let mut fixture = fixture().await?;
        std::fs::write(
            fixture.task.repository.join(".gitignore"),
            "collision.txt\n",
        )?;
        git(&fixture.task.repository, &["add", ".gitignore"]).await?;
        git(
            &fixture.task.repository,
            &["commit", "-m", "ignore private local file"],
        )
        .await?;
        fixture.task.allowed_paths.push("collision.txt".into());
        fixture.frozen = inspect(&fixture.task).await?;
        let worktree = fixture.candidate.worktree.clone();
        git(&worktree, &["reset", "--hard", &fixture.frozen.base_commit]).await?;
        std::fs::write(worktree.join("allowed.txt"), "new\n")?;
        std::fs::write(worktree.join("collision.txt"), "accepted candidate\n")?;
        git(&worktree, &["add", "--force", "collision.txt"]).await?;
        fixture.candidate =
            capture_candidate(&fixture.task, &fixture.frozen, &worktree, "fixture-run").await?;
        std::fs::write(
            fixture.task.repository.join("collision.txt"),
            "private existing bytes\n",
        )?;
        let retained = integrate(
            &fixture.task,
            &fixture.frozen,
            acceptance(&fixture).await?,
            "fixture-run",
            &RepositoryPublicationStore::new(fixture.root.join("journal")),
            &fixture.root.join("leases"),
        )
        .await?;
        assert_eq!(retained.state, RepositoryPublicationState::NeedsAttention);
        assert_eq!(
            std::fs::read_to_string(fixture.task.repository.join("collision.txt"))?,
            "private existing bytes\n"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.task.repository.join("allowed.txt"))?,
            "old\n"
        );
        assert_eq!(
            git(&fixture.task.repository, &["rev-parse", "main"])
                .await?
                .trim(),
            fixture.frozen.base_commit
        );
        assert_eq!(retained.acceptance.candidate, fixture.candidate);
        Ok(())
    }

    #[tokio::test]
    async fn interrupted_index_publication_requires_review_without_ref_or_checkout_replay(
    ) -> Result<()> {
        let fixture = fixture().await?;
        let (_barrier, handle) =
            paused_integration(&fixture, RepositoryCheckoutPhase::IndexApplied).await?;
        handle.abort();
        assert!(handle.await.unwrap_err().is_cancelled());
        let journal = RepositoryPublicationStore::new(fixture.root.join("journal"));
        let saved = journal
            .load("fixture-run")?
            .context("retained publication missing")?;
        assert_eq!(
            saved.checkout.as_ref().unwrap().phase,
            RepositoryCheckoutPhase::IndexApplied
        );
        let retained = reconcile(&saved, &journal).await?;
        assert_eq!(retained.state, RepositoryPublicationState::NeedsAttention);
        assert_eq!(
            git(&fixture.task.repository, &["rev-parse", "main"])
                .await?
                .trim(),
            fixture.frozen.base_commit
        );
        assert_eq!(
            std::fs::read_to_string(fixture.task.repository.join("allowed.txt"))?,
            "new\n"
        );
        assert_eq!(retained.acceptance.candidate, fixture.candidate);
        let repeated = reconcile(&retained, &journal).await?;
        assert_eq!(repeated, retained);
        Ok(())
    }

    #[tokio::test]
    async fn completed_checkout_transaction_recovers_from_lost_parent_receipt_once() -> Result<()> {
        let fixture = fixture().await?;
        let (_barrier, handle) =
            paused_integration(&fixture, RepositoryCheckoutPhase::RefCommitted).await?;
        handle.abort();
        assert!(handle.await.unwrap_err().is_cancelled());
        let journal = RepositoryPublicationStore::new(fixture.root.join("journal"));
        let saved = journal
            .load("fixture-run")?
            .context("retained publication missing")?;
        let recovered = reconcile(&saved, &journal).await?;
        assert_eq!(recovered.state, RepositoryPublicationState::Integrated);
        assert_eq!(reconcile(&recovered, &journal).await?, recovered);
        assert_eq!(
            git(&fixture.task.repository, &["rev-list", "--count", "main"])
                .await?
                .trim(),
            "2"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.task.repository.join("allowed.txt"))?,
            "new\n"
        );
        Ok(())
    }

    #[tokio::test]
    async fn recovery_retains_hidden_checkout_changes_for_review_without_replay() -> Result<()> {
        for flag in ["--assume-unchanged", "--skip-worktree"] {
            let fixture = fixture().await?;
            let (_barrier, handle) =
                paused_integration(&fixture, RepositoryCheckoutPhase::RefCommitted).await?;
            handle.abort();
            assert!(handle.await.unwrap_err().is_cancelled());
            let journal = RepositoryPublicationStore::new(fixture.root.join("journal"));
            let saved = journal
                .load("fixture-run")?
                .context("retained publication missing")?;
            git(
                &fixture.task.repository,
                &["update-index", flag, "allowed.txt"],
            )
            .await?;
            std::fs::write(
                fixture.task.repository.join("allowed.txt"),
                "private later edit\n",
            )?;
            // Git's ordinary diff cannot prove bytes are unchanged with these
            // flags, even though the accepted branch commit remains exact.
            assert!(git(
                &fixture.task.repository,
                &["diff", "--name-only", &fixture.candidate.commit],
            )
            .await?
            .is_empty());
            let retained = reconcile(&saved, &journal).await?;
            assert_eq!(retained.state, RepositoryPublicationState::NeedsAttention);
            assert!(retained.summary.contains("index flags"));
            assert_eq!(retained.acceptance.candidate, fixture.candidate);
            assert_eq!(
                std::fs::read_to_string(fixture.task.repository.join("allowed.txt"))?,
                "private later edit\n"
            );
            assert_eq!(
                git(&fixture.task.repository, &["rev-parse", "main"])
                    .await?
                    .trim(),
                fixture.candidate.commit
            );
            assert_eq!(reconcile(&retained, &journal).await?, retained);
        }
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn recovery_hashes_tracked_content_instead_of_trusting_configured_stat_cache(
    ) -> Result<()> {
        use std::os::unix::ffi::OsStrExt;

        let fixture = fixture().await?;
        git(
            &fixture.task.repository,
            &["config", "core.trustctime", "false"],
        )
        .await?;
        git(
            &fixture.task.repository,
            &["config", "core.checkstat", "minimal"],
        )
        .await?;
        let (_barrier, handle) =
            paused_integration(&fixture, RepositoryCheckoutPhase::RefCommitted).await?;
        handle.abort();
        assert!(handle.await.unwrap_err().is_cancelled());
        let path = fixture.task.repository.join("allowed.txt");
        let path_c = std::ffi::CString::new(path.as_os_str().as_bytes())?;
        let times = [libc::timespec {
            tv_sec: 1_000_000_000,
            tv_nsec: 0,
        }; 2];
        // A stable old timestamp avoids Git's protection for racily fresh files.
        assert_eq!(
            unsafe { libc::utimensat(libc::AT_FDCWD, path_c.as_ptr(), times.as_ptr(), 0) },
            0
        );
        git(&fixture.task.repository, &["update-index", "--refresh"]).await?;
        std::fs::write(&path, "bad\n")?;
        assert_eq!(
            unsafe { libc::utimensat(libc::AT_FDCWD, path_c.as_ptr(), times.as_ptr(), 0) },
            0
        );
        assert_eq!(
            git(
                &fixture.task.repository,
                &["ls-files", "-v", "--", "allowed.txt"]
            )
            .await?
            .trim(),
            "H allowed.txt"
        );
        assert!(git(
            &fixture.task.repository,
            &["diff", "--name-only", &fixture.candidate.commit],
        )
        .await?
        .is_empty());
        let journal = RepositoryPublicationStore::new(fixture.root.join("journal"));
        let saved = journal
            .load("fixture-run")?
            .context("retained publication missing")?;
        let retained = reconcile(&saved, &journal).await?;
        assert_eq!(retained.state, RepositoryPublicationState::NeedsAttention);
        assert!(retained.summary.contains("accepted candidate"));
        assert_eq!(std::fs::read_to_string(&path)?, "bad\n");
        assert_eq!(retained.acceptance.candidate, fixture.candidate);
        assert_eq!(reconcile(&retained, &journal).await?, retained);
        Ok(())
    }

    #[tokio::test]
    async fn verified_candidate_integrates_and_duplicate_dispatch_only_reads_back() -> Result<()> {
        let fixture = fixture().await?;
        let evidence = acceptance(&fixture).await?;
        let journal = RepositoryPublicationStore::new(fixture.root.join("journal"));
        let first = integrate(
            &fixture.task,
            &fixture.frozen,
            evidence.clone(),
            "fixture-run",
            &journal,
            &fixture.root.join("leases"),
        )
        .await?;
        assert_eq!(first.state, RepositoryPublicationState::Integrated);
        assert_eq!(
            std::fs::read_to_string(fixture.task.repository.join("allowed.txt"))?,
            "new\n"
        );
        let head = git(&fixture.task.repository, &["rev-parse", "main"]).await?;
        assert_eq!(head.trim(), fixture.candidate.commit);
        let second = integrate(
            &fixture.task,
            &fixture.frozen,
            evidence,
            "fixture-run",
            &journal,
            &fixture.root.join("leases"),
        )
        .await?;
        assert_eq!(second.state, RepositoryPublicationState::Integrated);
        assert_eq!(
            git(&fixture.task.repository, &["rev-list", "--count", "main"])
                .await?
                .trim(),
            "2"
        );
        Ok(())
    }

    #[tokio::test]
    async fn authorized_candidate_uses_held_repository_ownership_and_guarded_recovery() -> Result<()>
    {
        let fixture = fixture().await?;
        let lease_root = fixture.root.join("leases");
        let resource = format!("repository:{}", fixture.frozen.repository.display());
        let owned = crate::runtime::ResourceLease::acquire(&lease_root, &resource)?;
        let publication = RepositoryPublication {
            version: 1,
            run_id: "authorized-run".into(),
            snapshot: fixture.frozen.clone(),
            acceptance: acceptance(&fixture).await?,
            state: RepositoryPublicationState::Prepared,
            summary: "Host authorized exact candidate".into(),
            checkout: None,
        };
        let journal = RepositoryPublicationStore::new(fixture.root.join("authorized-journal"));
        let integrated =
            publish_authorized_candidate(publication.clone(), &journal, &lease_root, Some(&owned))
                .await?;
        assert_eq!(integrated.state, RepositoryPublicationState::Integrated);
        assert!(crate::runtime::ResourceLease::acquire(&lease_root, &resource).is_err());
        assert_eq!(
            publish_authorized_candidate(publication, &journal, &lease_root, Some(&owned)).await?,
            integrated
        );
        assert_eq!(
            git(&fixture.task.repository, &["rev-list", "--count", "main"])
                .await?
                .trim(),
            "2"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.task.repository.join("allowed.txt"))?,
            "new\n"
        );
        Ok(())
    }

    #[tokio::test]
    async fn authorized_candidate_rejects_wrong_repository_claim_and_changed_immutable_evidence(
    ) -> Result<()> {
        let fixture = fixture().await?;
        let lease_root = fixture.root.join("leases");
        let wrong = crate::runtime::ResourceLease::acquire(&lease_root, "another-repository")?;
        let mut publication = RepositoryPublication {
            version: 1,
            run_id: "authorized-run".into(),
            snapshot: fixture.frozen.clone(),
            acceptance: acceptance(&fixture).await?,
            state: RepositoryPublicationState::Prepared,
            summary: "Host authorized exact candidate".into(),
            checkout: None,
        };
        let journal = RepositoryPublicationStore::new(fixture.root.join("authorized-journal"));
        assert!(publish_authorized_candidate(
            publication.clone(),
            &journal,
            &lease_root,
            Some(&wrong),
        )
        .await
        .is_err());
        assert!(journal.load("authorized-run")?.is_none());
        publication.acceptance.candidate.tree = fixture.frozen.base_commit.clone();
        assert!(
            publish_authorized_candidate(publication, &journal, &lease_root, None)
                .await
                .is_err()
        );
        assert!(journal.load("authorized-run")?.is_none());
        assert_eq!(
            git(&fixture.task.repository, &["rev-parse", "main"])
                .await?
                .trim(),
            fixture.frozen.base_commit
        );
        Ok(())
    }

    #[tokio::test]
    async fn stale_target_retains_candidate_without_rebase_or_ref_write() -> Result<()> {
        let fixture = fixture().await?;
        let evidence = acceptance(&fixture).await?;
        std::fs::write(fixture.task.repository.join("outside.txt"), "advanced\n")?;
        git(&fixture.task.repository, &["add", "."]).await?;
        git(
            &fixture.task.repository,
            &["commit", "-m", "advance independently"],
        )
        .await?;
        let advanced = git(&fixture.task.repository, &["rev-parse", "main"]).await?;
        let result = integrate(
            &fixture.task,
            &fixture.frozen,
            evidence,
            "fixture-run",
            &RepositoryPublicationStore::new(fixture.root.join("journal")),
            &fixture.root.join("leases"),
        )
        .await?;
        assert_eq!(result.state, RepositoryPublicationState::NeedsAttention);
        assert!(result.summary.contains("advanced"));
        assert_eq!(
            git(&fixture.task.repository, &["rev-parse", "main"]).await?,
            advanced
        );
        assert!(fixture.candidate.worktree.exists());
        Ok(())
    }

    #[tokio::test]
    async fn dirty_target_preserves_user_changes_and_candidate() -> Result<()> {
        let fixture = fixture().await?;
        let evidence = acceptance(&fixture).await?;
        std::fs::write(fixture.task.repository.join("allowed.txt"), "user edit\n")?;
        let result = integrate(
            &fixture.task,
            &fixture.frozen,
            evidence,
            "fixture-run",
            &RepositoryPublicationStore::new(fixture.root.join("journal")),
            &fixture.root.join("leases"),
        )
        .await?;
        assert_eq!(result.state, RepositoryPublicationState::NeedsAttention);
        assert!(result.summary.contains("uncommitted"));
        assert_eq!(
            std::fs::read_to_string(fixture.task.repository.join("allowed.txt"))?,
            "user edit\n"
        );
        assert_eq!(
            git(&fixture.task.repository, &["rev-parse", "main"])
                .await?
                .trim(),
            fixture.frozen.base_commit
        );
        Ok(())
    }

    #[tokio::test]
    async fn publication_after_parent_crash_reconciles_exact_new_ref() -> Result<()> {
        let fixture = fixture().await?;
        let journal = RepositoryPublicationStore::new(fixture.root.join("journal"));
        let publication = RepositoryPublication {
            version: 1,
            run_id: "fixture-run".into(),
            snapshot: fixture.frozen.clone(),
            acceptance: acceptance(&fixture).await?,
            state: RepositoryPublicationState::Publishing,
            summary: "publishing".into(),
            checkout: None,
        };
        journal.save(&publication)?;
        git(
            &fixture.task.repository,
            &["merge", "--ff-only", &fixture.candidate.commit],
        )
        .await?;
        let recovered = reconcile(
            &journal.load("fixture-run")?.expect("retained publication"),
            &journal,
        )
        .await?;
        assert_eq!(recovered.state, RepositoryPublicationState::Integrated);
        assert_eq!(
            git(&fixture.task.repository, &["rev-list", "--count", "main"])
                .await?
                .trim(),
            "2"
        );
        Ok(())
    }

    #[tokio::test]
    async fn publication_reconciliation_retains_live_legacy_ownership() -> Result<()> {
        let fixture = fixture().await?;
        let journal = RepositoryPublicationStore::new(fixture.root.join("journal"));
        let publication = RepositoryPublication {
            version: 1,
            run_id: "fixture-run".into(),
            snapshot: fixture.frozen.clone(),
            acceptance: acceptance(&fixture).await?,
            state: RepositoryPublicationState::Publishing,
            summary: "publishing".into(),
            checkout: None,
        };
        journal.save(&publication)?;
        git(
            &fixture.task.repository,
            &["merge", "--ff-only", &fixture.candidate.commit],
        )
        .await?;
        let owner = crate::lock::acquire_legacy_publication_lock(&fixture.task.repository)?;
        let marker = std::fs::read(owner.lock_path())?;
        let recovered = reconcile(&publication, &journal).await?;
        assert_eq!(recovered.state, RepositoryPublicationState::NeedsAttention);
        assert!(recovered.summary.contains("ownership requires review"));
        assert_eq!(std::fs::read(owner.lock_path())?, marker);
        assert_eq!(
            git(&fixture.task.repository, &["rev-parse", "main"])
                .await?
                .trim(),
            fixture.candidate.commit
        );
        assert_eq!(recovered.acceptance.candidate, fixture.candidate);
        Ok(())
    }

    #[tokio::test]
    async fn unpublished_old_ref_requires_review_instead_of_replaying() -> Result<()> {
        let fixture = fixture().await?;
        let journal = RepositoryPublicationStore::new(fixture.root.join("journal"));
        let publication = RepositoryPublication {
            version: 1,
            run_id: "fixture-run".into(),
            snapshot: fixture.frozen.clone(),
            acceptance: acceptance(&fixture).await?,
            state: RepositoryPublicationState::Publishing,
            summary: "publishing".into(),
            checkout: None,
        };
        journal.save(&publication)?;
        let recovered = reconcile(&publication, &journal).await?;
        assert_eq!(recovered.state, RepositoryPublicationState::NeedsAttention);
        assert_eq!(
            git(&fixture.task.repository, &["rev-parse", "main"])
                .await?
                .trim(),
            fixture.frozen.base_commit
        );
        Ok(())
    }

    #[tokio::test]
    async fn stale_tree_or_review_receipt_cannot_integrate() -> Result<()> {
        let fixture = fixture().await?;
        let mut evidence = acceptance(&fixture).await?;
        evidence.review.candidate_tree = fixture.frozen.base_commit.clone();
        let result = integrate(
            &fixture.task,
            &fixture.frozen,
            evidence,
            "fixture-run",
            &RepositoryPublicationStore::new(fixture.root.join("journal")),
            &fixture.root.join("leases"),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(
            git(&fixture.task.repository, &["rev-parse", "main"])
                .await?
                .trim(),
            fixture.frozen.base_commit
        );
        std::fs::write(
            fixture.candidate.worktree.join("allowed.txt"),
            "late mutation\n",
        )?;
        assert!(
            run_checks(&fixture.task, &fixture.frozen, &fixture.candidate)
                .await
                .is_err()
        );
        Ok(())
    }

    #[tokio::test]
    async fn scope_escape_and_changed_check_executable_fail_closed() -> Result<()> {
        let fixture = fixture().await?;
        std::fs::write(fixture.candidate.worktree.join("outside.txt"), "escape\n")?;
        assert!(capture_candidate(
            &fixture.task,
            &fixture.frozen,
            &fixture.candidate.worktree,
            "escape"
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("outside its granted scope"));
        git(&fixture.candidate.worktree, &["restore", "outside.txt"]).await?;
        let mut task = fixture.task.clone();
        task.checks[0].executable_sha256 = "0".repeat(64);
        let mut frozen = fixture.frozen.clone();
        frozen.task_sha256 = task.digest()?;
        assert!(run_checks(&task, &frozen, &fixture.candidate)
            .await
            .unwrap_err()
            .to_string()
            .contains("changed since approval"));
        Ok(())
    }

    #[tokio::test]
    async fn integration_lease_blocks_conflicting_profiles() -> Result<()> {
        let fixture = fixture().await?;
        let lease_root = fixture.root.join("leases");
        let _other_profile =
            crate::runtime::ResourceLease::acquire(&lease_root, &fixture.frozen.resource())?;
        let result = integrate(
            &fixture.task,
            &fixture.frozen,
            acceptance(&fixture).await?,
            "fixture-run",
            &RepositoryPublicationStore::new(fixture.root.join("journal")),
            &lease_root,
        )
        .await;
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("ownership unavailable"));
        assert_eq!(
            git(&fixture.task.repository, &["rev-parse", "main"])
                .await?
                .trim(),
            fixture.frozen.base_commit
        );
        Ok(())
    }

    #[tokio::test]
    async fn standalone_check_timeout_cleans_descendants_and_output_is_bounded() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let root = std::fs::canonicalize(temporary.path())?;
        let marker = root.join("descendant-completed");
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "(sleep 0.2; printf escaped > \"$1\") & wait",
                "fixture",
            ])
            .arg(&marker);
        assert!(process(command, Duration::from_millis(30), 4096)
            .await
            .is_err());
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            !marker.exists(),
            "timed-out host check descendant escaped cleanup"
        );
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf 123456789"]);
        assert!(process(command, Duration::from_secs(5), 8)
            .await
            .unwrap_err()
            .to_string()
            .contains("output exceeds"));
        Ok(())
    }

    #[tokio::test]
    async fn host_check_checkpoint_reads_confirmation_and_never_repeats_unknown_effect(
    ) -> Result<()> {
        let mut fixture = fixture().await?;
        let marker = fixture.root.join("host-check-effects");
        let executable = std::fs::canonicalize("/bin/sh")?;
        fixture.task.checks = vec![RepositoryCheck {
            executable_sha256: digest(&std::fs::read(&executable)?),
            executable,
            args: vec![
                "-c".into(),
                "printf 'effect\\n' >> \"$1\"".into(),
                "fixture".into(),
                marker.display().to_string(),
            ],
            timeout_secs: 5,
        }];
        fixture.frozen.task_sha256 = fixture.task.digest()?;
        let context = crate::runtime::ExecutionContext {
            schema: crate::runtime::EXECUTION_SCHEMA.into(),
            run_id: "check-confirmed".into(),
            profile: "fixture".into(),
            state_root: fixture.root.join("state"),
            lease_root: fixture.root.join("leases"),
            definition_digest: fixture.task.digest()?,
            resource_binding_digest: "b".repeat(64),
            policy_digest: "c".repeat(64),
            resources: Vec::new(),
            runtime_seconds: 30,
            output_bytes: 4096,
        };
        let store = crate::runtime::RunStore::open(&context)?;
        store.start()?;
        let first = crate::runtime::EXECUTION_STORE
            .scope(
                store.clone(),
                run_checks(&fixture.task, &fixture.frozen, &fixture.candidate),
            )
            .await?;
        let repeated = crate::runtime::EXECUTION_STORE
            .scope(
                store.clone(),
                run_checks(&fixture.task, &fixture.frozen, &fixture.candidate),
            )
            .await?;
        assert_eq!(first, repeated);
        assert_eq!(std::fs::read_to_string(&marker)?, "effect\n");
        assert_eq!(
            store.status()?.actions[0].state,
            crate::runtime::ActionState::Confirmed
        );
        let context = crate::runtime::ExecutionContext {
            run_id: "check-unknown".into(),
            ..context
        };
        let store = crate::runtime::RunStore::open(&context)?;
        store.start()?;
        store.prepare_action("repository-check-0")?;
        assert!(store.begin_action("repository-check-0")?);
        assert!(crate::runtime::EXECUTION_STORE
            .scope(
                store.clone(),
                run_checks(&fixture.task, &fixture.frozen, &fixture.candidate)
            )
            .await
            .is_err());
        assert_eq!(std::fs::read_to_string(&marker)?, "effect\n");
        assert_eq!(
            store.reconcile()?.state,
            crate::runtime::ExecutionState::NeedsAttention
        );
        Ok(())
    }

    #[tokio::test]
    async fn host_preparer_receives_exact_command_and_candidate_without_ambient_environment(
    ) -> Result<()> {
        struct Preparer {
            worktree: PathBuf,
        }
        impl RepositoryCheckPreparer for Preparer {
            fn prepare(
                &self,
                executable: &Path,
                arguments: &[String],
                worktree: &Path,
            ) -> Result<PreparedRepositoryCheck> {
                assert_eq!(worktree, self.worktree);
                Ok(PreparedRepositoryCheck {
                    program: executable.to_owned(),
                    arguments: arguments.to_owned(),
                    environment: vec![("PREPARED_CANDIDATE".into(), "yes".into())],
                })
            }
        }
        let mut fixture = fixture().await?;
        let executable = std::fs::canonicalize("/bin/sh")?;
        fixture.task.checks = vec![RepositoryCheck {
            executable_sha256: digest(&std::fs::read(&executable)?),
            executable,
            args: vec![
                "-c".into(),
                "test \"$PREPARED_CANDIDATE\" = yes && test -z \"$HOME\" && test -r allowed.txt"
                    .into(),
            ],
            timeout_secs: 5,
        }];
        fixture.frozen.task_sha256 = fixture.task.digest()?;
        let checks = run_checks_with_preparer(
            &fixture.task,
            &fixture.frozen,
            &fixture.candidate,
            4096,
            &Preparer {
                worktree: fixture.candidate.worktree.clone(),
            },
        )
        .await?;
        assert_eq!(checks.len(), 1);
        assert!(checks[0].passed);
        assert_eq!(checks[0].candidate_tree, fixture.candidate.tree);
        Ok(())
    }

    #[tokio::test]
    async fn live_legacy_owner_retains_exact_marker_and_candidate_without_integration() -> Result<()>
    {
        let fixture = fixture().await?;
        let owner = crate::lock::acquire_legacy_publication_lock(&fixture.frozen.repository)?;
        let bytes = std::fs::read(owner.lock_path())?;
        let journal = RepositoryPublicationStore::new(fixture.root.join("publications"));
        let result = integrate(
            &fixture.task,
            &fixture.frozen,
            acceptance(&fixture).await?,
            "legacy-live",
            &journal,
            &fixture.root.join("leases"),
        )
        .await?;
        assert_eq!(result.state, RepositoryPublicationState::NeedsAttention);
        assert!(result.summary.contains("ownership requires review"));
        assert_eq!(std::fs::read(owner.lock_path())?, bytes);
        assert_eq!(
            git(&fixture.frozen.repository, &["rev-parse", "HEAD"])
                .await?
                .trim(),
            fixture.frozen.base_commit
        );
        assert_eq!(
            git(&fixture.candidate.worktree, &["rev-parse", "HEAD"])
                .await?
                .trim(),
            fixture.candidate.commit
        );
        assert_eq!(
            journal.load("legacy-live")?.unwrap().acceptance.candidate,
            fixture.candidate
        );
        Ok(())
    }

    #[tokio::test]
    async fn corrupt_legacy_owner_is_not_removed_or_hidden_from_conflict_review() -> Result<()> {
        let fixture = fixture().await?;
        let marker = fixture
            .frozen
            .repository
            .join(".orchestrator/codex-team.lock");
        std::fs::create_dir_all(marker.parent().unwrap())?;
        let bytes = b"opaque legacy ownership; never delete";
        std::fs::write(&marker, bytes)?;
        let journal = RepositoryPublicationStore::new(fixture.root.join("publications"));
        let result = integrate(
            &fixture.task,
            &fixture.frozen,
            acceptance(&fixture).await?,
            "legacy-corrupt",
            &journal,
            &fixture.root.join("leases"),
        )
        .await?;
        assert_eq!(result.state, RepositoryPublicationState::NeedsAttention);
        assert!(result.summary.contains("ownership requires review"));
        assert_eq!(std::fs::read(marker)?, bytes);
        assert_eq!(
            git(&fixture.frozen.repository, &["rev-parse", "HEAD"])
                .await?
                .trim(),
            fixture.frozen.base_commit
        );
        Ok(())
    }

    #[tokio::test]
    async fn owned_legacy_marker_does_not_hide_other_orchestrator_changes() -> Result<()> {
        let fixture = fixture().await?;
        let notes = fixture
            .frozen
            .repository
            .join(".orchestrator/user-notes.txt");
        std::fs::create_dir_all(notes.parent().unwrap())?;
        std::fs::write(&notes, b"preserve user notes")?;
        let journal = RepositoryPublicationStore::new(fixture.root.join("publications"));
        let result = integrate(
            &fixture.task,
            &fixture.frozen,
            acceptance(&fixture).await?,
            "legacy-dirty",
            &journal,
            &fixture.root.join("leases"),
        )
        .await?;
        assert_eq!(result.state, RepositoryPublicationState::NeedsAttention);
        assert!(result.summary.contains("uncommitted changes"));
        assert_eq!(std::fs::read(notes)?, b"preserve user notes");
        assert!(!fixture
            .frozen
            .repository
            .join(".orchestrator/codex-team.lock")
            .exists());
        assert_eq!(
            git(&fixture.frozen.repository, &["rev-parse", "HEAD"])
                .await?
                .trim(),
            fixture.frozen.base_commit
        );
        Ok(())
    }
}
