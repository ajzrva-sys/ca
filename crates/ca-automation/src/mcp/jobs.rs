//! Durable MCP job observations.
//!
//! Records are written by the operate-mode job controller and read by the
//! observe-mode `step_status` tool. Reads are deliberately strict and bounded:
//! a corrupt, oversized, symlinked, or mismatched record fails closed.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::revision::MCP_PARTIAL_STATE_LATCH;

pub(crate) const JOB_SCHEMA: &str = "codex-team.mcp_job.v1";
pub(crate) const STEP_PROCESS_PROVENANCE_SCHEMA: &str = "codex-team.mcp_step_process_provenance.v1";
const MAX_JOB_BYTES: u64 = 1024 * 1024;
const PARTIAL_LATCH_SCHEMA: &str = "codex-team.mcp_partial_state.v1";
const MAX_PARTIAL_LATCH_BYTES: u64 = 64 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JobState {
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Interrupted,
}

impl JobState {
    pub(crate) fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct JobProgress {
    pub(crate) phase: String,
    pub(crate) summary: String,
}

/// Durable identity captured from the operating system immediately after the
/// MCP server launches a step. Restart recovery treats the job JSON as
/// untrusted: these values are necessary but never sufficient without a fresh
/// OS inspection of the live process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct StepProcessProvenance {
    pub(crate) schema: String,
    pub(crate) process_id: u32,
    pub(crate) start_token: String,
    pub(crate) spawn_nonce: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JobKind {
    #[default]
    Step,
    DelegationEvidence,
    DelegationObservation,
}

impl JobKind {
    fn is_step(&self) -> bool {
        *self == Self::Step
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct JobRecord {
    pub(crate) schema: String,
    pub(crate) job_id: String,
    #[serde(default, skip_serializing_if = "JobKind::is_step")]
    pub(crate) kind: JobKind,
    pub(crate) status: JobState,
    pub(crate) expected_revision: String,
    pub(crate) started_at: String,
    pub(crate) updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) outer_process_group: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) launch_nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) process_provenance: Option<StepProcessProvenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) active_child_process_group: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) progress: Option<JobProgress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) cycle_receipt: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mechanistic_receipt: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) delegation_feedback: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) delegation_manifest_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
    #[serde(default)]
    pub(crate) repository_may_be_partial: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PartialStateLatch {
    schema: String,
    job_id: String,
    job_status: JobState,
    expected_revision: String,
    recorded_at: String,
    repository_may_be_partial: bool,
}

impl JobRecord {
    pub(crate) fn validate(&self, expected_id: &str) -> Result<()> {
        if self.schema != JOB_SCHEMA {
            bail!("unsupported MCP job schema `{}`", self.schema);
        }
        if self.job_id != expected_id {
            bail!("MCP job record ID does not match requested job");
        }
        validate_job_id(&self.job_id)?;
        if self.expected_revision.len() != 64
            || self.expected_revision != self.expected_revision.to_ascii_lowercase()
            || !self
                .expected_revision
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            bail!("MCP job record has an invalid expected revision");
        }
        if self
            .error
            .as_ref()
            .is_some_and(|text| text.len() > 16 * 1024)
        {
            bail!("MCP job record error summary exceeds 16 KiB");
        }
        if let Some(provenance) = &self.process_provenance {
            if provenance.schema != STEP_PROCESS_PROVENANCE_SCHEMA {
                bail!("MCP job record has unsupported step process provenance");
            }
            if provenance.process_id == 0
                || self.outer_process_group.is_some_and(|process_group| {
                    u32::try_from(process_group).ok() != Some(provenance.process_id)
                })
            {
                bail!("MCP job record has inconsistent step process provenance");
            }
            if provenance.start_token.is_empty()
                || provenance.start_token.len() > 128
                || !provenance
                    .start_token
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_'))
            {
                bail!("MCP job record has an invalid step process start token");
            }
            if provenance.spawn_nonce.len() != 64
                || !provenance
                    .spawn_nonce
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                bail!("MCP job record has an invalid step process nonce");
            }
            if self.launch_nonce.as_deref() != Some(provenance.spawn_nonce.as_str()) {
                bail!("MCP job record process provenance does not match its launch intent");
            }
        }
        if let Some(launch_nonce) = &self.launch_nonce {
            if launch_nonce.len() != 64
                || !launch_nonce
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                bail!("MCP job record has an invalid step launch nonce");
            }
        }
        if self.status == JobState::Running
            && self.process_provenance.is_some()
            && self.outer_process_group.is_none()
        {
            bail!("running MCP job provenance is missing its outer process group");
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct JobStore {
    repo: PathBuf,
    jobs_dir: PathBuf,
}

impl JobStore {
    pub(crate) fn new(repo: &Path) -> Self {
        Self {
            repo: repo.to_path_buf(),
            jobs_dir: repo.join(".orchestrator/mcp/jobs"),
        }
    }

    pub(crate) fn path_for(&self, job_id: &str) -> Result<PathBuf> {
        validate_job_id(job_id)?;
        Ok(self.jobs_dir.join(format!("{job_id}.json")))
    }

    pub(crate) fn ensure_dir(&self) -> Result<()> {
        ensure_real_directory(&self.repo)?;
        let orchestrator = self.repo.join(".orchestrator");
        let mcp = orchestrator.join("mcp");
        for directory in [&orchestrator, &mcp, &self.jobs_dir] {
            match fs::create_dir(directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("create MCP jobs directory {}", directory.display())
                    })
                }
            }
            ensure_real_directory(directory)?;
        }
        let canonical_repo = fs::canonicalize(&self.repo)?;
        let canonical_jobs = fs::canonicalize(&self.jobs_dir)?;
        if canonical_jobs != canonical_repo.join(".orchestrator/mcp/jobs") {
            bail!("MCP jobs directory escapes the pinned repository");
        }
        Ok(())
    }

    pub(crate) fn save_atomic(&self, record: &JobRecord) -> Result<()> {
        record.validate(&record.job_id)?;
        self.ensure_dir()?;
        if record.repository_may_be_partial
            || (record.kind == JobKind::Step
                && matches!(
                    record.status,
                    JobState::Failed
                        | JobState::Cancelled
                        | JobState::TimedOut
                        | JobState::Interrupted
                ))
        {
            self.save_partial_state_latch(record)?;
        }
        let path = self.path_for(&record.job_id)?;
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                bail!("MCP job record target must be a regular, non-symlink file");
            }
        }
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = self.jobs_dir.join(format!(
            ".{}.{}.{}.tmp",
            record.job_id,
            std::process::id(),
            sequence
        ));
        let bytes = serde_json::to_vec_pretty(record)?;
        if bytes.len() as u64 > MAX_JOB_BYTES {
            bail!("MCP job record exceeds the 1 MiB limit");
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .with_context(|| format!("create temporary MCP job record {}", temp.display()))?;
        let write_result = (|| -> Result<()> {
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temp, &path)?;
            File::open(&self.jobs_dir)?.sync_all()?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        write_result.with_context(|| format!("atomically write MCP job `{}`", record.job_id))
    }

    fn save_partial_state_latch(&self, record: &JobRecord) -> Result<()> {
        let path = self.repo.join(MCP_PARTIAL_STATE_LATCH);
        if path.parent() != Some(self.jobs_dir.parent().expect("jobs directory has parent")) {
            bail!("MCP partial-state latch path escaped its control directory");
        }
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                bail!("MCP partial-state latch must be a regular, non-symlink file");
            }
        }
        let latch = PartialStateLatch {
            schema: PARTIAL_LATCH_SCHEMA.to_string(),
            job_id: record.job_id.clone(),
            job_status: record.status,
            expected_revision: record.expected_revision.clone(),
            recorded_at: record.updated_at.clone(),
            repository_may_be_partial: true,
        };
        let bytes = serde_json::to_vec_pretty(&latch)?;
        if bytes.len() as u64 > MAX_PARTIAL_LATCH_BYTES {
            bail!("MCP partial-state latch exceeds the 64 KiB limit");
        }
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = self
            .jobs_dir
            .parent()
            .expect("jobs directory has parent")
            .join(format!(
                ".approval-required.{}.{}.tmp",
                std::process::id(),
                sequence
            ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .with_context(|| {
                format!(
                    "create temporary MCP partial-state latch {}",
                    temp.display()
                )
            })?;
        let write_result = (|| -> Result<()> {
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temp, &path)?;
            File::open(self.jobs_dir.parent().expect("jobs directory has parent"))?.sync_all()?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        write_result.context("atomically write MCP partial-state approval latch")
    }

    pub(crate) fn load_sync(&self, job_id: &str) -> Result<JobRecord> {
        let path = self.path_for(job_id)?;
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("MCP job `{job_id}` was not found"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("MCP job record must be a regular, non-symlink file");
        }
        if metadata.len() > MAX_JOB_BYTES {
            bail!("MCP job record exceeds the 1 MiB limit");
        }
        let bytes = fs::read(&path)?;
        let record: JobRecord = serde_json::from_slice(&bytes)
            .with_context(|| format!("MCP job `{job_id}` is corrupt"))?;
        record.validate(job_id)?;
        Ok(record)
    }

    pub(crate) fn records(&self) -> Result<Vec<JobRecord>> {
        if !self.jobs_dir.exists() {
            return Ok(Vec::new());
        }
        let metadata = fs::symlink_metadata(&self.jobs_dir)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("MCP jobs path must be a regular directory");
        }
        let mut paths = fs::read_dir(&self.jobs_dir)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        if paths.len() > 1_000 {
            bail!("MCP jobs directory exceeds the 1,000-record inspection limit");
        }
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let id = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .context("MCP job filename was not safe UTF-8")?;
                self.load_sync(id)
            })
            .collect()
    }

    pub(crate) fn reject_active_job(&self) -> Result<()> {
        if let Some(active) = self
            .records()?
            .into_iter()
            .find(|record| !record.status.is_terminal())
        {
            bail!("MCP job `{}` is already active", active.job_id);
        }
        Ok(())
    }

    pub(crate) fn prune_terminal(&self) -> Result<()> {
        let mut terminal = self
            .records()?
            .into_iter()
            .filter(|record| record.status.is_terminal())
            .collect::<Vec<_>>();
        if terminal.len() <= 50 {
            return Ok(());
        }
        terminal.sort_by(|left, right| left.updated_at.cmp(&right.updated_at));
        let remove_count = terminal.len() - 50;
        for record in terminal.into_iter().take(remove_count) {
            fs::remove_file(self.path_for(&record.job_id)?)?;
        }
        Ok(())
    }

    pub(crate) async fn load(&self, job_id: &str) -> Result<JobRecord> {
        let path = self.path_for(job_id)?;
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .with_context(|| format!("MCP job `{job_id}` was not found"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("MCP job record must be a regular, non-symlink file");
        }
        if metadata.len() > MAX_JOB_BYTES {
            bail!("MCP job record exceeds the 1 MiB limit");
        }
        let bytes = tokio::fs::read(&path)
            .await
            .with_context(|| format!("failed to read MCP job `{job_id}`"))?;
        if bytes.len() as u64 > MAX_JOB_BYTES {
            bail!("MCP job record exceeds the 1 MiB limit");
        }
        let record: JobRecord = serde_json::from_slice(&bytes)
            .with_context(|| format!("MCP job `{job_id}` is corrupt"))?;
        record.validate(job_id)?;
        Ok(record)
    }

    pub(crate) async fn wait(&self, job_id: &str, wait_seconds: u8) -> Result<JobRecord> {
        if wait_seconds > 20 {
            bail!("wait_seconds must be between 0 and 20");
        }
        let first = self.load(job_id).await?;
        if wait_seconds == 0 || first.status.is_terminal() {
            return Ok(first);
        }
        let observed_update = first.updated_at.clone();
        let deadline = Instant::now() + Duration::from_secs(u64::from(wait_seconds));
        loop {
            let now = Instant::now();
            if now >= deadline {
                return self.load(job_id).await;
            }
            tokio::time::sleep((deadline - now).min(Duration::from_millis(250))).await;
            let current = self.load(job_id).await?;
            if current.updated_at != observed_update || current.status.is_terminal() {
                return Ok(current);
            }
        }
    }
}

fn ensure_real_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect MCP directory {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("MCP path {} must be a real directory", path.display());
    }
    Ok(())
}

pub(crate) fn validate_job_id(job_id: &str) -> Result<()> {
    if job_id.is_empty() || job_id.len() > 128 {
        bail!("job_id must contain 1 to 128 safe ASCII bytes");
    }
    if !job_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("job_id contains unsafe characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let repo = std::env::temp_dir().join(format!("codex-team-mcp-jobs-{name}-{nonce}"));
        fs::create_dir_all(&repo).unwrap();
        repo
    }

    fn record(job_id: &str) -> JobRecord {
        JobRecord {
            schema: JOB_SCHEMA.to_string(),
            job_id: job_id.to_string(),
            kind: crate::mcp::jobs::JobKind::Step,
            status: JobState::Running,
            expected_revision: "a".repeat(64),
            started_at: "2026-08-20T00:00:00Z".to_string(),
            updated_at: "2026-08-20T00:00:00Z".to_string(),
            outer_process_group: None,
            launch_nonce: None,
            process_provenance: None,
            active_child_process_group: None,
            progress: None,
            cycle_receipt: None,
            mechanistic_receipt: None,
            delegation_feedback: None,
            delegation_manifest_digest: None,
            error: None,
            repository_may_be_partial: false,
        }
    }

    #[test]
    fn job_ids_cannot_escape_the_store() {
        assert!(validate_job_id("job-123_ok").is_ok());
        assert!(validate_job_id("../state").is_err());
        assert!(validate_job_id("a/b").is_err());
        assert!(validate_job_id("").is_err());
    }

    #[test]
    fn job_records_require_lowercase_revisions() {
        let mut item = record("job-1");
        item.expected_revision = "A".repeat(64);
        assert!(item.validate("job-1").is_err());
    }

    #[tokio::test]
    async fn load_rejects_record_id_mismatch() -> Result<()> {
        let repo = temp_repo("mismatch");
        let store = JobStore::new(&repo);
        fs::create_dir_all(&store.jobs_dir)?;
        let path = store.path_for("requested")?;
        fs::write(&path, serde_json::to_vec(&record("different"))?)?;
        let result = store.load("requested").await;
        fs::remove_dir_all(repo)?;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn atomic_save_round_trips_a_record() -> Result<()> {
        let repo = temp_repo("roundtrip");
        let store = JobStore::new(&repo);
        let expected = record("job-1");
        store.save_atomic(&expected)?;
        assert_eq!(store.load("job-1").await?, expected);
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn partial_terminal_state_writes_bounded_approval_latch() -> Result<()> {
        let repo = temp_repo("partial-latch");
        let store = JobStore::new(&repo);
        let mut item = record("job-1");
        item.status = JobState::Cancelled;
        item.repository_may_be_partial = true;
        item.updated_at = "2026-08-20T00:00:01Z".to_string();
        store.save_atomic(&item)?;
        let path = repo.join(MCP_PARTIAL_STATE_LATCH);
        let metadata = fs::symlink_metadata(&path)?;
        assert!(metadata.is_file());
        assert!(metadata.len() <= MAX_PARTIAL_LATCH_BYTES);
        let latch: PartialStateLatch = serde_json::from_slice(&fs::read(path)?)?;
        assert_eq!(latch.job_id, "job-1");
        assert_eq!(latch.job_status, JobState::Cancelled);
        assert!(latch.repository_may_be_partial);
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn partial_state_latch_rejects_symlink_target() -> Result<()> {
        let repo = temp_repo("partial-latch-symlink");
        let store = JobStore::new(&repo);
        store.ensure_dir()?;
        let outside = repo.join("outside.json");
        fs::write(&outside, "unchanged")?;
        std::os::unix::fs::symlink(&outside, repo.join(MCP_PARTIAL_STATE_LATCH))?;
        let mut item = record("job-1");
        item.status = JobState::Failed;
        item.repository_may_be_partial = true;
        assert!(store.save_atomic(&item).is_err());
        assert_eq!(fs::read_to_string(&outside)?, "unchanged");
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn terminal_retention_is_bounded_to_fifty() -> Result<()> {
        let repo = temp_repo("retention");
        let store = JobStore::new(&repo);
        for index in 0..55 {
            let mut item = record(&format!("job-{index}"));
            item.status = JobState::Completed;
            item.updated_at = format!("2026-08-20T00:00:{index:02}Z");
            store.save_atomic(&item)?;
        }
        store.prune_terminal()?;
        assert_eq!(store.records()?.len(), 50);
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn load_rejects_symlink_records() -> Result<()> {
        let repo = temp_repo("symlink");
        let store = JobStore::new(&repo);
        fs::create_dir_all(&store.jobs_dir)?;
        let target = repo.join("record.json");
        fs::write(&target, serde_json::to_vec(&record("job"))?)?;
        std::os::unix::fs::symlink(&target, store.path_for("job")?)?;
        let result = store.load("job").await;
        fs::remove_dir_all(repo)?;
        assert!(result.is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn save_rejects_a_symlinked_jobs_directory() -> Result<()> {
        let repo = temp_repo("jobs-dir-symlink");
        let outside = temp_repo("jobs-dir-outside");
        fs::create_dir_all(repo.join(".orchestrator/mcp"))?;
        std::os::unix::fs::symlink(&outside, repo.join(".orchestrator/mcp/jobs"))?;
        let store = JobStore::new(&repo);
        assert!(store.save_atomic(&record("job-1")).is_err());
        assert!(fs::read_dir(&outside)?.next().is_none());
        fs::remove_dir_all(repo)?;
        fs::remove_dir_all(outside)?;
        Ok(())
    }
}
