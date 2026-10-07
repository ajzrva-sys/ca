//! Repository lock helpers for mutating orchestrator CLI commands.
//!
//! Owns acquisition/release of `.orchestrator/codex-team.lock` (JSON metadata with
//! PID + owner token). Dead-PID locks are recoverable; live and corrupt locks fail
//! closed. Release is token-safe.
//!
//! Commands that currently acquire the lock (from `src/app.rs` entrypoints):
//! `init`, `step`, `run`, `status`, `mission`, `models`, `disposition`,
//! `route-bench`, `observe`, `learn`, `commands`, `watch`, and `watch-loop`.
//! Advisory/read-oriented commands such as `cost` and `doctor` do not lock.
//!
//! Read when changing concurrency policy or lock file shape. Do not read for
//! queue parsing, verification recipes, or role prompts.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::paths::Paths;

#[derive(Clone, Debug)]
pub(crate) struct RepoLockTarget {
    root: PathBuf,
    orch: PathBuf,
    lock: PathBuf,
    command: String,
}

impl RepoLockTarget {
    pub(crate) fn from_paths(paths: &Paths, command: impl Into<String>) -> Self {
        Self {
            root: paths.root.clone(),
            orch: paths.orch.clone(),
            lock: paths.lock.clone(),
            command: command.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct LockMetadata {
    pid: u32,
    owner_token: String,
    repo: String,
    command: String,
    started_at: String,
    updated_at: String,
    version: String,
}

#[derive(Debug)]
pub(crate) struct RepoLockGuard {
    lock_path: PathBuf,
    owner_token: String,
    released: bool,
    _resource: Option<crate::runtime::ResourceLease>,
}

impl RepoLockGuard {
    pub(crate) fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    fn release_if_owned(&mut self) -> Result<()> {
        if self.released {
            return Ok(());
        }
        self.released = true;
        let Ok(raw) = fs::read_to_string(&self.lock_path) else {
            return Ok(());
        };
        let Ok(metadata) = serde_json::from_str::<LockMetadata>(&raw) else {
            return Ok(());
        };
        if metadata.owner_token == self.owner_token {
            fs::remove_file(&self.lock_path)
                .with_context(|| format!("remove repo lock {}", self.lock_path.display()))?;
        }
        Ok(())
    }
}

impl Drop for RepoLockGuard {
    fn drop(&mut self) {
        let _ = self.release_if_owned();
    }
}

pub(crate) fn acquire_repo_lock(paths: &Paths, command: &str) -> Result<RepoLockGuard> {
    acquire_target(RepoLockTarget::from_paths(paths, command), true)
}

/// Native publication already owns the machine-wide repository/ref leases but
/// also participates in the V1 marker protocol while older clients coexist.
pub(crate) fn acquire_legacy_publication_lock(repository: &Path) -> Result<RepoLockGuard> {
    ensure_no_legacy_repository_owner(repository)?;
    acquire_target(
        RepoLockTarget::from_paths(&Paths::new(repository)?, "ca-automation-publication"),
        false,
    )
}

/// Pre-migration binaries use only the V1 PID marker. Observe it without
/// deleting it or signalling the owner before native publication proceeds.
pub(crate) fn ensure_no_legacy_repository_owner(repository: &Path) -> Result<()> {
    let directory = repository.join(".orchestrator");
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => bail!("legacy repository ownership directory is not a regular directory"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    let path = directory.join("codex-team.lock");
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let file = match options.open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("legacy repository ownership is unavailable"),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        bail!("legacy repository ownership is not a bounded regular marker");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.nlink() != 1 {
            bail!("legacy repository ownership marker changed owner or link identity");
        }
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 {
        bail!("legacy repository ownership marker exceeded its bound");
    }
    let owner: LockMetadata =
        serde_json::from_slice(&bytes).context("legacy repository ownership marker is corrupt")?;
    if owner.pid == 0 || owner.pid > i32::MAX as u32 || owner.owner_token.is_empty() {
        bail!("legacy repository ownership identity is invalid");
    }
    #[cfg(unix)]
    {
        let active = unsafe { libc::kill(owner.pid as i32, 0) } == 0;
        let error = std::io::Error::last_os_error();
        if active || error.raw_os_error() != Some(libc::ESRCH) {
            bail!(
                "legacy repository ownership is held or cannot be disproved for pid {} ({})",
                owner.pid,
                owner.command
            );
        }
    }
    #[cfg(not(unix))]
    bail!("legacy repository ownership cannot be disproved on this platform");
    Ok(())
}

pub(crate) fn acquire_repo_locks(targets: Vec<RepoLockTarget>) -> Result<Vec<RepoLockGuard>> {
    let mut deduped = BTreeMap::new();
    for target in targets {
        deduped.entry(target.root.clone()).or_insert(target);
    }

    let mut guards = Vec::new();
    for (_, target) in deduped {
        guards.push(acquire_target(target, true)?);
    }
    Ok(guards)
}

fn acquire_target(target: RepoLockTarget, machine_resource: bool) -> Result<RepoLockGuard> {
    let resource = if machine_resource
        && crate::runtime::current_run_store().is_none()
        && !crate::runtime::worker_contained()
    {
        Some(
            crate::runtime::ResourceLease::acquire(
                &crate::runtime::machine_lease_root()?,
                &format!("repository:{}", target.root.canonicalize()?.display()),
            )
            .context("repo lock shared automation resource is busy")?,
        )
    } else {
        None
    };
    fs::create_dir_all(&target.orch)
        .with_context(|| format!("create lock dir {}", target.orch.display()))?;

    for _ in 0..2 {
        let metadata = LockMetadata::new(&target);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target.lock)
        {
            Ok(mut file) => {
                let text = serde_json::to_string_pretty(&metadata)?;
                if let Err(err) = write_lock_file(&mut file, &text) {
                    let _ = fs::remove_file(&target.lock);
                    return Err(err)
                        .with_context(|| format!("write repo lock {}", target.lock.display()));
                }
                return Ok(RepoLockGuard {
                    lock_path: target.lock,
                    owner_token: metadata.owner_token,
                    released: false,
                    _resource: resource,
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                if recover_dead_lock(&target.lock)? {
                    continue;
                }
                unreachable!("recover_dead_lock only returns Ok(true) or an error");
            }
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("create repo lock {}", target.lock.display()));
            }
        }
    }

    bail!(
        "repo lock {} could not be acquired after stale-lock recovery",
        target.lock.display()
    )
}

fn write_lock_file(file: &mut fs::File, text: &str) -> Result<()> {
    file.write_all(text.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn recover_dead_lock(lock_path: &Path) -> Result<bool> {
    let raw = match fs::read_to_string(lock_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(err) => {
            return Err(err).with_context(|| format!("read repo lock {}", lock_path.display()));
        }
    };
    let metadata: LockMetadata = serde_json::from_str(&raw).with_context(|| {
        format!(
            "repo lock {} is corrupt; remove it manually after confirming no codex-team process is active",
            lock_path.display()
        )
    })?;

    if process_is_live(metadata.pid) {
        bail!(
            "repo lock {} is held by live pid {} for `{}` in {}",
            lock_path.display(),
            metadata.pid,
            metadata.command,
            metadata.repo
        );
    }

    fs::remove_file(lock_path)
        .with_context(|| format!("remove stale repo lock {}", lock_path.display()))?;
    Ok(true)
}

fn process_is_live(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }

    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        false
    }
}

impl LockMetadata {
    fn new(target: &RepoLockTarget) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            pid: std::process::id(),
            owner_token: owner_token(),
            repo: target.root.display().to_string(),
            command: target.command.clone(),
            started_at: now.clone(),
            updated_at: now,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

fn owner_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-team-lock-{name}-{nonce}"))
    }

    fn write_lock(paths: &Paths, pid: u32, owner_token: &str) -> Result<()> {
        fs::create_dir_all(&paths.orch)?;
        let now = Utc::now().to_rfc3339();
        let metadata = LockMetadata {
            pid,
            owner_token: owner_token.to_string(),
            repo: paths.root.display().to_string(),
            command: "test".to_string(),
            started_at: now.clone(),
            updated_at: now,
            version: env!("CARGO_PKG_VERSION").to_string(),
        };
        fs::write(&paths.lock, serde_json::to_string_pretty(&metadata)?)?;
        Ok(())
    }

    #[test]
    fn repo_lock_acquires_and_releases() -> Result<()> {
        let root = temp_root("acquire-release");
        fs::create_dir_all(&root)?;
        let paths = Paths::new(&root)?;

        {
            let guard = acquire_repo_lock(&paths, "test")?;
            assert!(guard.lock_path().exists());
        }

        assert!(!paths.lock.exists());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn repo_lock_refuses_live_pid() -> Result<()> {
        let root = temp_root("live-pid");
        fs::create_dir_all(&root)?;
        let paths = Paths::new(&root)?;
        write_lock(&paths, std::process::id(), "other-owner")?;

        let err = acquire_repo_lock(&paths, "test").expect_err("live lock should be refused");

        assert!(err.to_string().contains("repo lock"));
        assert!(paths.lock.exists());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn repo_lock_recovers_dead_pid() -> Result<()> {
        let root = temp_root("dead-pid");
        fs::create_dir_all(&root)?;
        let paths = Paths::new(&root)?;
        write_lock(&paths, 999_999_999, "dead-owner")?;

        let guard = acquire_repo_lock(&paths, "test")?;

        assert!(guard.lock_path().exists());
        drop(guard);
        assert!(!paths.lock.exists());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn repo_lock_refuses_corrupt_lock() -> Result<()> {
        let root = temp_root("corrupt");
        fs::create_dir_all(root.join(".orchestrator"))?;
        let paths = Paths::new(&root)?;
        fs::write(&paths.lock, "not json")?;

        let err = acquire_repo_lock(&paths, "test").expect_err("corrupt lock should be refused");

        assert!(err.to_string().contains("corrupt"));
        assert!(paths.lock.exists());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn native_publication_observes_live_corrupt_and_dead_legacy_owners_without_mutation(
    ) -> Result<()> {
        let root = temp_root("native-observer");
        fs::create_dir_all(&root)?;
        let paths = Paths::new(&root)?;
        assert!(ensure_no_legacy_repository_owner(&root).is_ok());
        write_lock(&paths, std::process::id(), "older-binary")?;
        let live = fs::read(&paths.lock)?;
        assert!(ensure_no_legacy_repository_owner(&root).is_err());
        assert_eq!(fs::read(&paths.lock)?, live);
        fs::write(&paths.lock, "interrupted marker")?;
        assert!(ensure_no_legacy_repository_owner(&root).is_err());
        assert_eq!(fs::read_to_string(&paths.lock)?, "interrupted marker");
        write_lock(&paths, 999_999_999, "dead-older-binary")?;
        let dead = fs::read(&paths.lock)?;
        assert!(ensure_no_legacy_repository_owner(&root).is_ok());
        assert_eq!(fs::read(&paths.lock)?, dead);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn repo_lock_release_is_token_safe() -> Result<()> {
        let root = temp_root("token-safe");
        fs::create_dir_all(&root)?;
        let paths = Paths::new(&root)?;
        let guard = acquire_repo_lock(&paths, "test")?;
        write_lock(&paths, std::process::id(), "replacement-owner")?;

        drop(guard);

        assert!(paths.lock.exists());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn repo_locks_deduplicate_and_acquire_in_sorted_order() -> Result<()> {
        let root = temp_root("multi");
        let a = root.join("a");
        let b = root.join("b");
        fs::create_dir_all(&a)?;
        fs::create_dir_all(&b)?;
        let paths_a = Paths::new(&a)?;
        let paths_b = Paths::new(&b)?;

        let guards = acquire_repo_locks(vec![
            RepoLockTarget::from_paths(&paths_b, "watch-loop"),
            RepoLockTarget::from_paths(&paths_a, "watch-loop"),
            RepoLockTarget::from_paths(&paths_b, "watch-loop-duplicate"),
        ])?;

        assert_eq!(guards.len(), 2);
        assert_eq!(guards[0].lock_path(), paths_a.lock.as_path());
        assert_eq!(guards[1].lock_path(), paths_b.lock.as_path());
        drop(guards);
        assert!(!paths_a.lock.exists());
        assert!(!paths_b.lock.exists());
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
