//! Private, owner-scoped immutable command logs and inference receipts.
use crate::jobs::JobOwner;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use uuid::Uuid;

pub const MAX_JOB_LOG_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SESSION_LOG_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_SLICE_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    Stdout,
    Stderr,
}
impl LogStream {
    fn name(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactSlice {
    pub artifact_id: String,
    pub stream: LogStream,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobLogArtifact {
    pub artifact_id: String,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
    pub total_bytes: usize,
    pub complete: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArtifactStore {
    root: PathBuf,
    session_root: PathBuf,
    owner: JobOwner,
    coordinator: bool,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    run_id: Uuid,
    session_id: Option<Uuid>,
    artifact: JobLogArtifact,
    stdout_sha256: String,
    stderr_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    outcome: Option<StoredJobOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
enum StoredJobOutcome {
    Completed { exit_code: i32 },
    Failed { exit_code: Option<i32> },
    Cancelled,
    TimedOut,
}

impl StoredJobOutcome {
    fn from_terminal_status(status: crate::jobs::JobStatus) -> Option<Self> {
        use crate::jobs::JobStatus;
        match status {
            JobStatus::Completed { exit_code } => Some(Self::Completed { exit_code }),
            JobStatus::Failed { exit_code } => Some(Self::Failed { exit_code }),
            JobStatus::Cancelled => Some(Self::Cancelled),
            JobStatus::TimedOut => Some(Self::TimedOut),
            JobStatus::Running | JobStatus::Cancelling => None,
        }
    }
}

impl ArtifactStore {
    pub fn for_owner(owner: JobOwner) -> Result<Self, String> {
        #[cfg(not(test))]
        let base = crate::runtime::RuntimePaths::discover()
            .map_err(|error| error.to_string())?
            .data_dir
            .join("inference-artifacts");
        #[cfg(test)]
        let base = test_root();
        Ok(Self::at(base, owner))
    }

    /// Explicit host grant for the coordinator of a resumed conversation.
    /// Child callers keep `for_owner`, even if they carry a session identity.
    pub fn for_coordinator(owner: JobOwner) -> Result<Self, String> {
        let mut store = Self::for_owner(owner)?;
        store.coordinator = owner.session_id.is_some();
        Ok(store)
    }

    pub fn at(base: PathBuf, owner: JobOwner) -> Self {
        let session_root = base.join(owner.session_id.unwrap_or(owner.run_id).to_string());
        Self {
            root: session_root.join(owner.run_id.to_string()),
            session_root,
            owner,
            coordinator: false,
        }
    }

    pub(crate) async fn begin_capture(&self) -> Result<JobLogCapture, String> {
        self.begin_capture_limit(MAX_JOB_LOG_BYTES).await
    }

    pub(crate) async fn begin_capture_limit(&self, limit: usize) -> Result<JobLogCapture, String> {
        let capacity = limit.min(MAX_JOB_LOG_BYTES);
        let store = self.clone();
        let (directory, reservation, stdout, stderr) = tokio::task::spawn_blocking(move || {
            private_dir(&store.session_root)?;
            private_dir(&store.root)?;
            let lock = private_file(&store.session_root.join("quota.lock"), false)?;
            #[cfg(unix)]
            {
                use std::os::fd::AsRawFd;
                if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
                    return Err("Command log quota cannot be locked".to_string());
                }
            }
            let mut used = 0usize;
            for entry in std::fs::read_dir(&store.session_root).map_err(io_error)? {
                let entry = entry.map_err(io_error)?;
                if entry.file_type().map_err(io_error)?.is_file()
                    && entry
                        .path()
                        .extension()
                        .is_some_and(|value| value == "reserve")
                {
                    let mut text = String::new();
                    private_read(&entry.path())?
                        .take(32)
                        .read_to_string(&mut text)
                        .map_err(io_error)?;
                    used = used.saturating_add(
                        text.parse::<usize>()
                            .map_err(|_| "Command log quota is invalid".to_string())?,
                    );
                }
            }
            if used.saturating_add(capacity) > MAX_SESSION_LOG_BYTES {
                return Err("Session command log quota exhausted".into());
            }
            let id = Uuid::new_v4();
            let reservation = store.session_root.join(format!("{id}.reserve"));
            let directory = store.root.join(format!("capture-{id}"));
            let prepared = (|| {
                let mut file = private_file(&reservation, true)?;
                file.write_all(capacity.to_string().as_bytes())
                    .map_err(io_error)?;
                file.sync_all().map_err(io_error)?;
                private_dir(&directory)?;
                let stdout = private_file(&directory.join("stdout"), true)?;
                let stderr = private_file(&directory.join("stderr"), true)?;
                Ok((stdout, stderr))
            })();
            match prepared {
                Ok((stdout, stderr)) => Ok((directory, reservation, stdout, stderr)),
                Err(error) => {
                    let _ = std::fs::remove_dir_all(&directory);
                    let _ = std::fs::remove_file(&reservation);
                    Err(error)
                }
            }
        })
        .await
        .map_err(|_| "Command log storage worker stopped".to_string())??;
        Ok(JobLogCapture {
            store: self.clone(),
            state: Arc::new(Mutex::new(CaptureState {
                directory,
                reservation,
                stdout: tokio::fs::File::from_std(stdout),
                stderr: tokio::fs::File::from_std(stderr),
                stdout_bytes: 0,
                stderr_bytes: 0,
                total_bytes: 0,
                capacity,
                stdout_digest: Sha256::new(),
                stderr_digest: Sha256::new(),
                error: None,
                receipt: None,
            })),
        })
    }

    pub async fn read_slice(&self, reference: &ArtifactSlice) -> Result<String, String> {
        let store = self.clone();
        let reference = reference.clone();
        tokio::task::spawn_blocking(move || {
            let (manifest, bytes) =
                store.read_stream_sync(&reference.artifact_id, reference.stream)?;
            if !manifest.artifact.complete {
                return Err("Command log is incomplete".into());
            }
            if reference.start >= reference.end
                || reference.end > bytes.len()
                || reference.end - reference.start > MAX_SLICE_BYTES
            {
                return Err("Invalid or oversized command log slice".into());
            }
            String::from_utf8(bytes[reference.start..reference.end].to_vec())
                .map_err(|_| "Command log slice is not complete UTF-8 text".into())
        })
        .await
        .map_err(|_| "Command log read worker stopped".to_string())?
    }

    pub async fn read_stream(
        &self,
        artifact_id: &str,
        stream: LogStream,
    ) -> Result<Vec<u8>, String> {
        let store = self.clone();
        let id = artifact_id.to_owned();
        tokio::task::spawn_blocking(move || {
            store.read_stream_sync(&id, stream).map(|(_, bytes)| bytes)
        })
        .await
        .map_err(|_| "Command log read worker stopped".to_string())?
    }

    fn read_stream_sync(&self, id: &str, stream: LogStream) -> Result<(Manifest, Vec<u8>), String> {
        validate_id(id)?;
        let (directory, run_id) = self.resolve_directory(id)?;
        ensure_directory(&directory)?;
        let mut raw_manifest = Vec::new();
        private_read(&directory.join("manifest.json"))?
            .take(16 * 1024)
            .read_to_end(&mut raw_manifest)
            .map_err(io_error)?;
        let manifest: Manifest = serde_json::from_slice(&raw_manifest)
            .map_err(|_| "Command log manifest is invalid".to_string())?;
        if manifest.artifact.artifact_id != id
            || manifest.run_id != run_id
            || manifest.session_id != self.owner.session_id
        {
            return Err("Command log identity changed".into());
        }
        let mut bytes = Vec::new();
        private_read(&directory.join(stream.name()))?
            .take(MAX_JOB_LOG_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        let (expected, length) = match stream {
            LogStream::Stdout => (&manifest.stdout_sha256, manifest.artifact.stdout_bytes),
            LogStream::Stderr => (&manifest.stderr_sha256, manifest.artifact.stderr_bytes),
        };
        if bytes.len() != length || digest(&bytes) != *expected || bytes.len() > MAX_JOB_LOG_BYTES {
            return Err("Command log content changed".into());
        }
        let identity = artifact_identity(
            &manifest.stdout_sha256,
            &manifest.stderr_sha256,
            manifest.artifact.stdout_bytes,
            manifest.artifact.stderr_bytes,
        );
        if id.rsplit('-').next() != Some(identity.as_str()) {
            return Err("Command log digest changed".into());
        }
        Ok((manifest, bytes))
    }

    fn resolve_directory(&self, id: &str) -> Result<(PathBuf, Uuid), String> {
        let local = self.root.join(id);
        match std::fs::symlink_metadata(&local) {
            Ok(_) => return Ok((local, self.owner.run_id)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
        if !self.coordinator || self.owner.session_id.is_none() {
            return Err("Command log is not owned by this run".into());
        }
        ensure_directory(&self.session_root)?;
        let mut found = None;
        for entry in std::fs::read_dir(&self.session_root).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            if !entry.file_type().map_err(io_error)?.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(run_id) = Uuid::parse_str(&name) else {
                continue;
            };
            if name != run_id.to_string() {
                continue;
            }
            let candidate = entry.path().join(id);
            match std::fs::symlink_metadata(&candidate) {
                Ok(_) => {
                    if found.is_some() {
                        return Err("Command log identity is ambiguous".into());
                    }
                    found = Some((candidate, run_id));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(error)),
            }
        }
        found.ok_or_else(|| "Command log is not owned by this session".into())
    }

    pub async fn save_trace(
        &self,
        trace_id: Uuid,
        value: serde_json::Value,
    ) -> Result<String, String> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            private_dir(&store.root)?;
            let bytes = serde_json::to_vec(&value)
                .map_err(|_| "Inference trace cannot be encoded".to_string())?;
            if bytes.len() > 256 * 1024 {
                return Err("Inference trace exceeds its storage limit".into());
            }
            let name = format!("trace-{trace_id}-{}.json", digest(&bytes));
            let destination = store.root.join(&name);
            let temporary = store.root.join(format!("trace-{trace_id}.pending"));
            if destination.exists() {
                let mut previous = Vec::new();
                private_read(&destination)?
                    .take(256 * 1024 + 1)
                    .read_to_end(&mut previous)
                    .map_err(io_error)?;
                return if previous == bytes {
                    Ok(name)
                } else {
                    Err("Inference trace identity changed".into())
                };
            }
            if temporary.exists() {
                let mut previous = Vec::new();
                private_read(&temporary)?
                    .take(256 * 1024 + 1)
                    .read_to_end(&mut previous)
                    .map_err(io_error)?;
                if previous != bytes {
                    return Err("Pending inference trace requires reconciliation".into());
                }
            } else {
                let mut file = private_file(&temporary, true)?;
                file.write_all(&bytes).map_err(io_error)?;
                file.sync_all().map_err(io_error)?;
            }
            std::fs::rename(&temporary, &destination).map_err(io_error)?;
            std::fs::File::open(&store.root)
                .and_then(|file| file.sync_all())
                .map_err(io_error)?;
            Ok(name)
        })
        .await
        .map_err(|_| "Inference trace storage worker stopped".to_string())?
    }
}

#[derive(Debug, Clone)]
pub(crate) struct JobLogCapture {
    store: ArtifactStore,
    state: Arc<Mutex<CaptureState>>,
}
#[derive(Debug)]
struct CaptureState {
    directory: PathBuf,
    reservation: PathBuf,
    stdout: tokio::fs::File,
    stderr: tokio::fs::File,
    stdout_bytes: usize,
    stderr_bytes: usize,
    total_bytes: usize,
    capacity: usize,
    stdout_digest: Sha256,
    stderr_digest: Sha256,
    error: Option<String>,
    receipt: Option<JobLogArtifact>,
}
impl JobLogCapture {
    pub(crate) async fn discard_unstarted(self) {
        let state = self.state.lock().await;
        let directory = state.directory.clone();
        let reservation = state.reservation.clone();
        let session_root = self.store.session_root.clone();
        let _ = tokio::task::spawn_blocking(move || -> Result<(), String> {
            let lock = private_file(&session_root.join("quota.lock"), false)?;
            #[cfg(unix)]
            {
                use std::os::fd::AsRawFd;
                if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
                    return Err("Command log quota cannot be locked".into());
                }
            }
            std::fs::remove_dir_all(directory).map_err(io_error)?;
            std::fs::remove_file(reservation).map_err(io_error)
        })
        .await;
    }
    pub(crate) async fn append(&self, stream: LogStream, bytes: &[u8]) {
        let mut state = self.state.lock().await;
        state.total_bytes = state.total_bytes.saturating_add(bytes.len());
        let available = state
            .capacity
            .saturating_sub(state.stdout_bytes + state.stderr_bytes);
        let bytes = &bytes[..bytes.len().min(available)];
        if state.error.is_some() || state.receipt.is_some() {
            return;
        }
        let result = match stream {
            LogStream::Stdout => state.stdout.write_all(bytes).await,
            LogStream::Stderr => state.stderr.write_all(bytes).await,
        };
        if result.is_err() {
            state.error = Some("Command log capture failed".into());
            return;
        }
        match stream {
            LogStream::Stdout => {
                state.stdout_bytes += bytes.len();
                state.stdout_digest.update(bytes);
            }
            LogStream::Stderr => {
                state.stderr_bytes += bytes.len();
                state.stderr_digest.update(bytes);
            }
        }
    }

    pub(crate) async fn mark_read_error(&self) {
        self.state.lock().await.error = Some("Command output stream ended unexpectedly".into());
    }

    #[cfg(test)]
    pub(crate) async fn finish(&self) {
        self.finish_with_outcome(None).await;
    }

    pub(crate) async fn finish_terminal(&self, status: crate::jobs::JobStatus) {
        self.finish_with_outcome(StoredJobOutcome::from_terminal_status(status))
            .await;
    }

    async fn finish_with_outcome(&self, outcome: Option<StoredJobOutcome>) {
        let mut state = self.state.lock().await;
        if state.receipt.is_some() {
            return;
        }
        // Tokio writes can still be buffered when sync_all is called. Drain
        // both streams first so a delayed write error cannot certify capture.
        let stdout_saved =
            state.stdout.flush().await.is_ok() && state.stdout.sync_all().await.is_ok();
        let stderr_saved =
            state.stderr.flush().await.is_ok() && state.stderr.sync_all().await.is_ok();
        if !stdout_saved || !stderr_saved {
            state.error = Some("Command log could not be committed".into());
        }
        let stdout_sha256 = format!("{:x}", state.stdout_digest.clone().finalize());
        let stderr_sha256 = format!("{:x}", state.stderr_digest.clone().finalize());
        let artifact_id = format!(
            "{}-{}",
            Uuid::new_v4(),
            artifact_identity(
                &stdout_sha256,
                &stderr_sha256,
                state.stdout_bytes,
                state.stderr_bytes
            )
        );
        let mut artifact = JobLogArtifact {
            artifact_id,
            stdout_bytes: state.stdout_bytes,
            stderr_bytes: state.stderr_bytes,
            total_bytes: state.total_bytes,
            complete: state.error.is_none()
                && state.total_bytes == state.stdout_bytes + state.stderr_bytes,
            error: state.error.clone(),
        };
        if !artifact.complete && artifact.error.is_none() {
            artifact.error = Some("Command log exceeded its capture limit".into());
        }
        let manifest = Manifest {
            run_id: self.store.owner.run_id,
            session_id: self.store.owner.session_id,
            artifact: artifact.clone(),
            stdout_sha256,
            stderr_sha256,
            outcome,
        };
        let directory = state.directory.clone();
        let reservation = state.reservation.clone();
        let capacity = state.capacity;
        let destination = self.store.root.join(&artifact.artifact_id);
        let saved = tokio::task::spawn_blocking(move || {
            let bytes = serde_json::to_vec(&manifest)
                .map_err(|_| "Command log manifest cannot be encoded".to_string())?;
            let mut file = private_file(&directory.join("manifest.json"), true)?;
            file.write_all(&bytes).map_err(io_error)?;
            file.sync_all().map_err(io_error)?;
            std::fs::rename(&directory, &destination).map_err(io_error)?;
            let session_root = reservation
                .parent()
                .ok_or("Command log quota location is invalid")?;
            let lock = private_file(&session_root.join("quota.lock"), false)?;
            #[cfg(unix)]
            {
                use std::os::fd::AsRawFd;
                if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
                    return Err("Command log quota cannot be locked".to_string());
                }
            }
            let mut quota = private_file(&reservation, false)?;
            quota.set_len(0).map_err(io_error)?;
            quota
                .write_all(
                    (if manifest.artifact.error.is_some() {
                        capacity
                    } else {
                        manifest.artifact.stdout_bytes + manifest.artifact.stderr_bytes
                    })
                    .to_string()
                    .as_bytes(),
                )
                .map_err(io_error)?;
            quota.sync_all().map_err(io_error)?;
            std::fs::File::open(destination.parent().unwrap())
                .and_then(|file| file.sync_all())
                .map_err(io_error)
        })
        .await;
        if !matches!(saved, Ok(Ok(()))) {
            artifact.complete = false;
            artifact.error =
                Some("Command log finalization failed; command must not be replayed".into());
        }
        state.receipt = Some(artifact);
    }

    pub(crate) async fn receipt(&self) -> Option<JobLogArtifact> {
        self.state.lock().await.receipt.clone()
    }
}

fn artifact_identity(
    stdout: &str,
    stderr: &str,
    stdout_bytes: usize,
    stderr_bytes: usize,
) -> String {
    digest(format!("{stdout}:{stderr}:{stdout_bytes}:{stderr_bytes}").as_bytes())
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn validate_id(id: &str) -> Result<(), String> {
    if id.len() != 101
        || !id.is_ascii()
        || Uuid::parse_str(&id[..36]).is_err()
        || id.as_bytes()[36] != b'-'
        || !id[37..].bytes().all(|value| value.is_ascii_hexdigit())
    {
        return Err("Invalid command log identity".into());
    }
    Ok(())
}
/// Called only by the owning session deletion path after its normal access check.
pub fn purge_session(session_id: Uuid) -> Result<(), String> {
    let store = ArtifactStore::for_owner(JobOwner {
        run_id: Uuid::nil(),
        session_id: Some(session_id),
    })?;
    match std::fs::symlink_metadata(&store.session_root) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            std::fs::remove_dir_all(&store.session_root).map_err(io_error)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err("Inference artifacts cannot be purged through a redirected path".into()),
    }
}

fn io_error(_: std::io::Error) -> String {
    "Private inference artifact storage unavailable".into()
}
fn ensure_directory(path: &Path) -> Result<(), String> {
    if !std::fs::symlink_metadata(path)
        .map_err(io_error)?
        .file_type()
        .is_dir()
    {
        return Err("Inference artifact path is not a private directory".into());
    }
    Ok(())
}
fn private_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(io_error)?;
    ensure_directory(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(io_error)?;
    }
    Ok(())
}
fn private_file(path: &Path, create_new: bool) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options
        .write(true)
        .create(!create_new)
        .create_new(create_new);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).map_err(io_error)
}
fn private_read(path: &Path) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err("Inference artifact is not a regular file".into());
    }
    Ok(file)
}
#[cfg(test)]
fn test_root() -> PathBuf {
    static ROOT: std::sync::LazyLock<PathBuf> = std::sync::LazyLock::new(|| {
        std::env::temp_dir().join(format!("ca-inference-tests-{}", Uuid::new_v4()))
    });
    ROOT.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn one_shot_local_producer(root: &Path) -> (std::process::Output, PathBuf) {
        let counter = root.join("producer-count");
        let output = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                "printf x >> \"$1\"; printf 'original stdout\\n'; printf 'original failure stderr\\n' >&2; exit 101",
                "local-producer",
            ])
            .arg(&counter)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(101));
        assert_eq!(std::fs::read(&counter).unwrap(), b"x");
        (output, counter)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn capture_write_failure_before_commit_is_explicit_without_replaying_producer() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::at(temporary.path().join("artifacts"), JobOwner::new(None));
        let capture = store.begin_capture().await.unwrap();
        let (output, counter) = one_shot_local_producer(temporary.path());
        {
            let mut state = capture.state.lock().await;
            state.stdout = tokio::fs::File::from_std(
                std::fs::File::open(state.directory.join("stdout")).unwrap(),
            );
        }
        capture.append(LogStream::Stdout, &output.stdout).await;
        capture.append(LogStream::Stderr, &output.stderr).await;
        capture
            .finish_terminal(crate::jobs::JobStatus::Failed {
                exit_code: output.status.code(),
            })
            .await;
        let receipt = capture.receipt().await.unwrap();
        assert!(!receipt.complete);
        assert!(receipt.error.is_some());
        assert!(
            std::fs::read(store.root.join(&receipt.artifact_id).join("stdout"))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            receipt.total_bytes,
            output.stdout.len() + output.stderr.len()
        );
        assert!(store
            .read_slice(&ArtifactSlice {
                artifact_id: receipt.artifact_id.clone(),
                stream: LogStream::Stdout,
                start: 0,
                end: 1,
            })
            .await
            .is_err());
        capture
            .finish_terminal(crate::jobs::JobStatus::Failed {
                exit_code: output.status.code(),
            })
            .await;
        assert_eq!(capture.receipt().await.unwrap(), receipt);
        assert_eq!(std::fs::read(counter).unwrap(), b"x");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn capture_post_commit_quota_failure_preserves_one_artifact_and_original_outcome() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::at(temporary.path().join("artifacts"), JobOwner::new(None));
        let capture = store.begin_capture().await.unwrap();
        let (output, counter) = one_shot_local_producer(temporary.path());
        capture.append(LogStream::Stdout, &output.stdout).await;
        capture.append(LogStream::Stderr, &output.stderr).await;
        let (staging, reservation) = {
            let state = capture.state.lock().await;
            (state.directory.clone(), state.reservation.clone())
        };
        std::fs::remove_file(&reservation).unwrap();
        std::fs::create_dir(&reservation).unwrap();
        capture
            .finish_terminal(crate::jobs::JobStatus::Failed {
                exit_code: output.status.code(),
            })
            .await;
        let receipt = capture.receipt().await.unwrap();
        assert!(!receipt.complete);
        assert!(receipt
            .error
            .as_deref()
            .unwrap()
            .contains("must not be replayed"));
        assert!(
            !staging.exists(),
            "artifact rename must precede the injected quota failure"
        );
        let committed = store.root.join(&receipt.artifact_id);
        assert!(committed.is_dir());
        let (manifest, stdout) = store
            .read_stream_sync(&receipt.artifact_id, LogStream::Stdout)
            .unwrap();
        let (_, stderr) = store
            .read_stream_sync(&receipt.artifact_id, LogStream::Stderr)
            .unwrap();
        assert_eq!(
            manifest.outcome,
            Some(StoredJobOutcome::Failed {
                exit_code: Some(101)
            })
        );
        assert_eq!(stdout, output.stdout);
        assert_eq!(stderr, output.stderr);
        assert_eq!(manifest.stdout_sha256, digest(&stdout));
        assert_eq!(manifest.stderr_sha256, digest(&stderr));
        assert_eq!(manifest.artifact.stdout_bytes, stdout.len());
        assert_eq!(manifest.artifact.stderr_bytes, stderr.len());
        let before = std::fs::read(committed.join("manifest.json")).unwrap();
        capture
            .finish_terminal(crate::jobs::JobStatus::Failed {
                exit_code: output.status.code(),
            })
            .await;
        assert_eq!(capture.receipt().await.unwrap(), receipt);
        assert_eq!(
            std::fs::read(committed.join("manifest.json")).unwrap(),
            before
        );
        assert_eq!(std::fs::read(committed.join("stdout")).unwrap(), stdout);
        assert_eq!(std::fs::read(committed.join("stderr")).unwrap(), stderr);
        assert_eq!(std::fs::read_dir(&store.root).unwrap().count(), 1);
        assert_eq!(std::fs::read(counter).unwrap(), b"x");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn trace_post_commit_lost_ack_recovers_identical_bytes_without_replaying_producer() {
        async fn save_then_lose_ack(
            store: &ArtifactStore,
            id: Uuid,
            value: serde_json::Value,
        ) -> Result<String, String> {
            store.save_trace(id, value).await?;
            Err("injected local acknowledgement loss after committed trace".into())
        }

        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::at(temporary.path().join("artifacts"), JobOwner::new(None));
        let (output, counter) = one_shot_local_producer(temporary.path());
        let id = Uuid::new_v4();
        let value = serde_json::json!({
            "status": "complete",
            "value": String::from_utf8(output.stdout).unwrap(),
            "original_exit_code": output.status.code(),
        });
        let expected_bytes = serde_json::to_vec(&value).unwrap();
        let failure = save_then_lose_ack(&store, id, value.clone()).await;
        assert!(failure.unwrap_err().contains("after committed trace"));
        let files = std::fs::read_dir(&store.root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 1);
        assert_eq!(std::fs::read(&files[0]).unwrap(), expected_bytes);
        let recovered = store.save_trace(id, value).await.unwrap();
        assert_eq!(store.root.join(recovered), files[0]);
        assert_eq!(std::fs::read(&files[0]).unwrap(), expected_bytes);
        assert_eq!(std::fs::read_dir(&store.root).unwrap().count(), 1);
        assert_eq!(std::fs::read(counter).unwrap(), b"x");
    }

    #[tokio::test]
    async fn committed_log_reopens_with_exact_terminal_outcome_and_stream_identity() {
        let temporary = tempfile::tempdir().unwrap();
        let owner = JobOwner::new(Some(Uuid::new_v4()));
        let store = ArtifactStore::at(temporary.path().to_path_buf(), owner);
        let capture = store.begin_capture().await.unwrap();
        let stdout = b"original command stdout\n";
        let stderr = b"error[E0425]: original command failed\n";
        capture.append(LogStream::Stdout, stdout).await;
        capture.append(LogStream::Stderr, stderr).await;
        capture
            .finish_terminal(crate::jobs::JobStatus::Failed {
                exit_code: Some(101),
            })
            .await;
        let receipt = capture.receipt().await.unwrap();
        assert!(receipt.complete);
        drop(capture);
        drop(store);

        let reopened = ArtifactStore::at(temporary.path().to_path_buf(), owner);
        let (manifest, saved_stdout) = reopened
            .read_stream_sync(&receipt.artifact_id, LogStream::Stdout)
            .unwrap();
        assert_eq!(
            manifest.outcome,
            Some(StoredJobOutcome::Failed {
                exit_code: Some(101)
            })
        );
        assert_eq!(saved_stdout, stdout);
        assert_eq!(manifest.stdout_sha256, digest(stdout));
        assert_eq!(manifest.stderr_sha256, digest(stderr));
        assert_eq!(manifest.artifact.stdout_bytes, stdout.len());
        assert_eq!(manifest.artifact.stderr_bytes, stderr.len());
        let (_, saved_stderr) = reopened
            .read_stream_sync(&receipt.artifact_id, LogStream::Stderr)
            .unwrap();
        assert_eq!(saved_stderr, stderr);

        let mut legacy = serde_json::to_value(&manifest).unwrap();
        legacy.as_object_mut().unwrap().remove("outcome");
        let legacy: Manifest = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.outcome, None);
    }

    #[tokio::test]
    async fn original_middle_of_large_log_is_durable_and_owner_scoped() {
        let temporary = tempfile::tempdir().unwrap();
        let owner = JobOwner::new(Some(Uuid::new_v4()));
        let store = ArtifactStore::at(temporary.path().to_path_buf(), owner);
        let capture = store.begin_capture().await.unwrap();
        let mut bytes = vec![b'x'; 150 * 1024];
        let diagnostic = b"error[E0308]: hidden middle diagnostic\n";
        let start = 75 * 1024;
        bytes[start..start + diagnostic.len()].copy_from_slice(diagnostic);
        capture.append(LogStream::Stderr, &bytes).await;
        capture.finish().await;
        let receipt = capture.receipt().await.unwrap();
        assert!(receipt.complete);
        assert_eq!(receipt.total_bytes, bytes.len());
        let reference = ArtifactSlice {
            artifact_id: receipt.artifact_id.clone(),
            stream: LogStream::Stderr,
            start,
            end: start + diagnostic.len(),
        };
        assert_eq!(
            store.read_slice(&reference).await.unwrap().as_bytes(),
            diagnostic
        );
        let other = ArtifactStore::at(
            temporary.path().to_path_buf(),
            JobOwner::new(owner.session_id),
        );
        assert!(other.read_slice(&reference).await.is_err());
        let path = store.root.join(&receipt.artifact_id).join("stderr");
        std::fs::write(path, b"tampered").unwrap();
        assert!(store.read_slice(&reference).await.is_err());
    }

    #[tokio::test]
    async fn resumed_coordinator_can_read_prior_run_without_granting_child_or_other_session() {
        let session_id = Uuid::new_v4();
        let owner = JobOwner::new(Some(session_id));
        let store = ArtifactStore::for_owner(owner).unwrap();
        let capture = store.begin_capture().await.unwrap();
        capture
            .append(LogStream::Stderr, b"original session evidence")
            .await;
        capture.finish().await;
        let artifact = capture.receipt().await.unwrap();
        let reference = ArtifactSlice {
            artifact_id: artifact.artifact_id,
            stream: LogStream::Stderr,
            start: 0,
            end: artifact.stderr_bytes,
        };
        let resumed_owner = JobOwner::new(Some(session_id));
        assert!(ArtifactStore::for_owner(resumed_owner)
            .unwrap()
            .read_slice(&reference)
            .await
            .is_err());
        assert_eq!(
            ArtifactStore::for_coordinator(resumed_owner)
                .unwrap()
                .read_slice(&reference)
                .await
                .unwrap(),
            "original session evidence"
        );
        let child = JobOwner::new(Some(session_id));
        assert!(ArtifactStore::for_owner(child)
            .unwrap()
            .read_slice(&reference)
            .await
            .is_err());
        let other_session =
            ArtifactStore::for_coordinator(JobOwner::new(Some(Uuid::new_v4()))).unwrap();
        assert!(other_session.read_slice(&reference).await.is_err());
        purge_session(session_id).unwrap();
        assert!(store.read_slice(&reference).await.is_err());
    }

    #[tokio::test]
    async fn capture_limit_is_explicit_and_cannot_supply_complete_evidence() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::at(temporary.path().to_path_buf(), JobOwner::new(None));
        let capture = store.begin_capture().await.unwrap();
        capture
            .append(LogStream::Stdout, &vec![b'a'; MAX_JOB_LOG_BYTES + 1])
            .await;
        capture.finish().await;
        let receipt = capture.receipt().await.unwrap();
        assert!(!receipt.complete);
        assert_eq!(receipt.stdout_bytes, MAX_JOB_LOG_BYTES);
        assert_eq!(receipt.total_bytes, MAX_JOB_LOG_BYTES + 1);
        assert!(store
            .read_slice(&ArtifactSlice {
                artifact_id: receipt.artifact_id,
                stream: LogStream::Stdout,
                start: 0,
                end: 1
            })
            .await
            .is_err());
        let bounded = store.begin_capture_limit(4).await.unwrap();
        bounded.append(LogStream::Stdout, b"12345678").await;
        bounded.finish().await;
        let bounded = bounded.receipt().await.unwrap();
        assert_eq!(bounded.stdout_bytes, 4);
        assert_eq!(bounded.total_bytes, 8);
        assert!(!bounded.complete);
    }

    #[tokio::test]
    async fn local_commit_failure_remains_incomplete_and_is_not_reexecuted() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::at(temporary.path().to_path_buf(), JobOwner::new(None));
        let capture = store.begin_capture().await.unwrap();
        capture
            .append(LogStream::Stdout, b"command executed once")
            .await;
        let directory = capture.state.lock().await.directory.clone();
        std::fs::write(directory.join("manifest.json"), b"injected collision").unwrap();
        capture.finish().await;
        let receipt = capture.receipt().await.unwrap();
        assert!(!receipt.complete);
        assert!(receipt
            .error
            .as_deref()
            .unwrap()
            .contains("must not be replayed"));
        capture.finish().await;
        assert_eq!(capture.receipt().await.unwrap(), receipt);
        assert_eq!(
            std::fs::read(directory.join("stdout")).unwrap(),
            b"command executed once"
        );
    }

    #[tokio::test]
    async fn trace_save_reconciles_identical_commits_and_preserves_conflicting_pending() {
        let temporary = tempfile::tempdir().unwrap();
        let store = ArtifactStore::at(temporary.path().to_path_buf(), JobOwner::new(None));
        let id = Uuid::new_v4();
        let value = serde_json::json!({"value":"already computed"});
        let reference = store.save_trace(id, value.clone()).await.unwrap();
        assert_eq!(store.save_trace(id, value).await.unwrap(), reference);
        let pending_id = Uuid::new_v4();
        std::fs::write(
            store.root.join(format!("trace-{pending_id}.pending")),
            b"interrupted different value",
        )
        .unwrap();
        assert!(store
            .save_trace(pending_id, serde_json::json!({"different":true}))
            .await
            .is_err());
        assert_eq!(
            std::fs::read(store.root.join(format!("trace-{pending_id}.pending"))).unwrap(),
            b"interrupted different value"
        );
    }
}
