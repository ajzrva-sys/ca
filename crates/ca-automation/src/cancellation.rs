//! Process-wide cooperative cancellation shared by role and verification
//! runners. A token is process-local authority only; it is never serialized.

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths::Paths;

static PROCESS_CANCELLATION: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static SIGNAL_HANDLERS: OnceLock<std::result::Result<(), String>> = OnceLock::new();
static MARKER_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub(crate) const MCP_JOB_ID_ENV: &str = "CODEX_TEAM_MCP_JOB_ID";
const ACTIVE_CHILD_SCHEMA: &str = "codex-team.mcp_active_child.v1";
const MAX_ACTIVE_CHILD_MARKER_BYTES: u64 = 16 * 1024;

fn process_cancellation() -> &'static Arc<AtomicBool> {
    PROCESS_CANCELLATION.get_or_init(|| Arc::new(AtomicBool::new(false)))
}

/// Install process-wide SIGINT/SIGTERM handling once. Role and verification
/// runners create their own local tokens, but every token also observes this
/// process flag so an asynchronous MCP step cancellation crosses all internal
/// process-group boundaries.
pub(crate) fn install_process_signal_handlers() -> Result<()> {
    let result = SIGNAL_HANDLERS.get_or_init(|| {
        let flag = process_cancellation().clone();
        signal_hook::flag::register(signal_hook::consts::SIGINT, flag.clone())
            .map_err(|error| format!("register SIGINT cancellation handler: {error}"))?;
        signal_hook::flag::register(signal_hook::consts::SIGTERM, flag)
            .map_err(|error| format!("register SIGTERM cancellation handler: {error}"))?;
        Ok(())
    });
    result.clone().map_err(|message| anyhow!(message))
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst) || process_cancellation().load(Ordering::SeqCst)
    }
}

/// A process-local lease on the MCP job's current nested process group.
///
/// MCP launches `codex-team step` in one process group, while role and
/// verification runners deliberately create separate groups. When a step was
/// launched by MCP, each runner publishes its actual group in a strict,
/// repository-contained marker. Dropping the lease clears only the marker that
/// still names this group, so a later runner cannot be erased accidentally.
#[derive(Debug)]
pub(crate) struct ActiveChildProcessGroup {
    marker: Option<PathBuf>,
    job_id: Option<String>,
    process_group: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ActiveChildMarker {
    schema: String,
    job_id: String,
    process_group: i32,
    owner_process_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActiveChildProcessEvidence {
    pub(crate) process_group: i32,
    pub(crate) owner_process_id: u32,
}

impl ActiveChildProcessGroup {
    /// Publish `process_group` when this process is an MCP-owned step. Ordinary
    /// CLI execution has no bridge environment and remains unchanged.
    pub(crate) fn register(paths: &Paths, process_group: u32) -> Result<Self> {
        let job_id = env::var_os(MCP_JOB_ID_ENV);
        Self::register_for_job(paths, process_group, job_id)
    }

    fn register_for_job(
        paths: &Paths,
        process_group: u32,
        job_id: Option<std::ffi::OsString>,
    ) -> Result<Self> {
        let process_group = i32::try_from(process_group)
            .ok()
            .filter(|value| *value > 0)
            .context("child process did not have a valid process-group ID")?;
        let Some(job_id) = job_id else {
            return Ok(Self {
                marker: None,
                job_id: None,
                process_group,
            });
        };
        let job_id = job_id
            .into_string()
            .map_err(|_| anyhow!("{MCP_JOB_ID_ENV} was not valid UTF-8"))?;
        validate_bridge_job_id(&job_id)?;
        let marker = active_child_marker_path(&paths.root, &job_id)?;
        let value = ActiveChildMarker {
            schema: ACTIVE_CHILD_SCHEMA.to_string(),
            job_id: job_id.clone(),
            process_group,
            owner_process_id: std::process::id(),
        };
        write_marker_atomic(&marker, &value)?;
        Ok(Self {
            marker: Some(marker),
            job_id: Some(job_id),
            process_group,
        })
    }

    pub(crate) fn clear(&mut self) -> Result<()> {
        let (Some(marker), Some(job_id)) = (&self.marker, &self.job_id) else {
            return Ok(());
        };
        match read_marker(marker, job_id)? {
            Some(current) if current.process_group == self.process_group => {
                fs::remove_file(marker)
                    .with_context(|| format!("clear active child marker {}", marker.display()))?;
                sync_parent(marker)?;
            }
            _ => {}
        }
        self.marker = None;
        self.job_id = None;
        Ok(())
    }
}

impl Drop for ActiveChildProcessGroup {
    fn drop(&mut self) {
        let _ = self.clear();
    }
}

/// Prepare a unique job marker path before spawning the MCP step and remove
/// any impossible stale collision. All path components are verified as real
/// directories; symlinked roots or marker targets fail closed.
pub(crate) fn prepare_active_child_bridge(repo: &Path, job_id: &str) -> Result<PathBuf> {
    validate_bridge_job_id(job_id)?;
    let directory = ensure_active_child_directory(repo, true)?;
    let marker = directory.join(format!("{job_id}.json"));
    match fs::symlink_metadata(&marker) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                bail!("active child marker target must be a regular, non-symlink file");
            }
            fs::remove_file(&marker).with_context(|| {
                format!("remove stale active child marker {}", marker.display())
            })?;
            File::open(&directory)?.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("inspect active child marker target"),
    }
    Ok(marker)
}

/// Read the current nested group for an MCP job. Missing markers mean no role
/// or deterministic verification process is currently active.
pub(crate) fn read_active_child_process_group(
    repo: &Path,
    job_id: &str,
) -> Result<Option<ActiveChildProcessEvidence>> {
    validate_bridge_job_id(job_id)?;
    let directory = match ensure_active_child_directory(repo, false) {
        Ok(directory) => directory,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None)
        }
        Err(error) => return Err(error),
    };
    let marker = directory.join(format!("{job_id}.json"));
    Ok(
        read_marker(&marker, job_id)?.map(|value| ActiveChildProcessEvidence {
            process_group: value.process_group,
            owner_process_id: value.owner_process_id,
        }),
    )
}

pub(crate) fn clear_active_child_bridge(repo: &Path, job_id: &str) -> Result<()> {
    validate_bridge_job_id(job_id)?;
    let directory = ensure_active_child_directory(repo, false)?;
    let marker = directory.join(format!("{job_id}.json"));
    match fs::symlink_metadata(&marker) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                bail!("active child marker target must be a regular, non-symlink file");
            }
            fs::remove_file(&marker)?;
            File::open(directory)?.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("inspect active child marker target"),
    }
    Ok(())
}

fn active_child_marker_path(repo: &Path, job_id: &str) -> Result<PathBuf> {
    validate_bridge_job_id(job_id)?;
    Ok(ensure_active_child_directory(repo, false)?.join(format!("{job_id}.json")))
}

fn ensure_active_child_directory(repo: &Path, create: bool) -> Result<PathBuf> {
    let canonical_repo = fs::canonicalize(repo)
        .with_context(|| format!("canonicalize repository {}", repo.display()))?;
    if canonical_repo != repo {
        bail!("active child bridge requires the canonical repository root");
    }
    ensure_real_directory(&canonical_repo)?;
    let orchestrator = canonical_repo.join(".orchestrator");
    let mcp = orchestrator.join("mcp");
    let active = mcp.join("active");
    for directory in [&orchestrator, &mcp, &active] {
        if create {
            match fs::create_dir(directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("create MCP bridge directory {}", directory.display())
                    })
                }
            }
        }
        ensure_real_directory(directory)?;
    }
    if fs::canonicalize(&active)? != canonical_repo.join(".orchestrator/mcp/active") {
        bail!("active child bridge directory escapes the pinned repository");
    }
    Ok(active)
}

fn ensure_real_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect MCP bridge directory {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!(
            "MCP bridge path {} must be a real directory",
            path.display()
        );
    }
    Ok(())
}

fn write_marker_atomic(path: &Path, marker: &ActiveChildMarker) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("active child marker target must be a regular, non-symlink file");
        }
    }
    let bytes = serde_json::to_vec(marker)?;
    if bytes.len() as u64 > MAX_ACTIVE_CHILD_MARKER_BYTES {
        bail!("active child marker exceeds the 16 KiB limit");
    }
    let parent = path.parent().context("active child marker has no parent")?;
    ensure_real_directory(parent)?;
    let sequence = MARKER_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(
        ".active-child.{}.{}.tmp",
        std::process::id(),
        sequence
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .with_context(|| {
            format!(
                "create active child marker temporary file {}",
                temp.display()
            )
        })?;
    let result = (|| -> Result<()> {
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.context("atomically publish active child process group")
}

fn read_marker(path: &Path, expected_job_id: &str) -> Result<Option<ActiveChildMarker>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("inspect active child marker"),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("active child marker must be a regular, non-symlink file");
    }
    if metadata.len() > MAX_ACTIVE_CHILD_MARKER_BYTES {
        bail!("active child marker exceeds the 16 KiB limit");
    }
    let bytes = fs::read(path)?;
    let marker: ActiveChildMarker =
        serde_json::from_slice(&bytes).context("parse active child marker")?;
    if marker.schema != ACTIVE_CHILD_SCHEMA
        || marker.job_id != expected_job_id
        || marker.process_group <= 0
        || marker.owner_process_id == 0
    {
        bail!("active child marker failed identity validation");
    }
    Ok(Some(marker))
}

fn sync_parent(path: &Path) -> Result<()> {
    File::open(path.parent().context("active child marker has no parent")?)?
        .sync_all()
        .context("sync active child marker directory")
}

fn validate_bridge_job_id(job_id: &str) -> Result<()> {
    if job_id.is_empty()
        || job_id.len() > 128
        || !job_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("MCP bridge job ID must contain 1 to 128 safe ASCII bytes");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn clones_share_cancellation_state() {
        let first = CancellationToken::default();
        let second = first.clone();
        assert!(!second.is_cancelled());
        first.cancel();
        assert!(second.is_cancelled());
    }

    #[test]
    fn active_child_bridge_is_contained_atomic_and_clearable() -> Result<()> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let repo = std::env::temp_dir().join(format!("codex-team-active-child-{nonce}"));
        fs::create_dir_all(repo.join(".orchestrator"))?;
        let repo = fs::canonicalize(repo)?;
        let paths = Paths::new(&repo)?;
        prepare_active_child_bridge(&repo, "job-1")?;
        let mut registration = ActiveChildProcessGroup::register_for_job(
            &paths,
            12_345,
            Some(std::ffi::OsString::from("job-1")),
        )?;
        assert_eq!(
            read_active_child_process_group(&repo, "job-1")?.map(|evidence| evidence.process_group),
            Some(12_345)
        );
        registration.clear()?;
        assert_eq!(read_active_child_process_group(&repo, "job-1")?, None);
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn active_child_bridge_rejects_symlinked_directory() -> Result<()> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("codex-team-active-symlink-{nonce}"));
        let repo = root.join("repo");
        let outside = root.join("outside");
        fs::create_dir_all(repo.join(".orchestrator/mcp"))?;
        fs::create_dir_all(&outside)?;
        std::os::unix::fs::symlink(&outside, repo.join(".orchestrator/mcp/active"))?;
        let repo = fs::canonicalize(repo)?;
        assert!(prepare_active_child_bridge(&repo, "job-1").is_err());
        assert!(fs::read_dir(outside)?.next().is_none());
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
