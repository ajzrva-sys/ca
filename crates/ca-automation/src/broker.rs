//! Repository-wide mechanistic broker shared by local codex-team stdio facades.
//!
//! The broker is a private, local-only executor coordinator. It never exposes
//! a network listener and it does not add MCP tools or resources.

use anyhow::{bail, Context, Result};
use ca_automation_protocol::{
    DelegationReadinessV1, MechanisticActionResultV1, MechanisticBrokerDispositionV1,
    MechanisticBrokerReceiptV1, MechanisticConfidenceV1, MechanisticEvidenceCapsuleV1,
    MechanisticEvidenceFragmentV1, MechanisticReceiptStatusV1, MechanisticReceiptV1,
    MechanisticRequestPurposeV1, MechanisticRequestV1, MAX_MECHANISTIC_OUTPUT_BYTES,
    MECHANISTIC_RECEIPT_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::FileTypeExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{watch, Semaphore};

use crate::cancellation::CancellationToken;
use crate::mcp::jobs::{
    validate_job_id, JobKind, JobProgress, JobRecord, JobState, JobStore, JOB_SCHEMA,
};
use crate::mechanistic::{
    execute_query_action, request_digest, validate_request, MechanisticExecutor,
};
use crate::paths::Paths;
use crate::revision::repository_revision;

const BROKER_WIRE_SCHEMA: &str = "codex-team.broker_wire.v1";
const BROKER_OWNER_SCHEMA: &str = "codex-team.broker_owner.v1";
const BROKER_CACHE_SCHEMA: &str = "codex-team.broker_cache.v1";
const MAX_WIRE_BYTES: u64 = 1024 * 1024;
const MAX_CACHE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CACHE_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const CLIENT_RECONNECT_GRACE: Duration = Duration::from_secs(30);
const MAX_UNIQUE_EVIDENCE_JOBS: usize = 8;
const MAX_PARALLEL_QUERIES: usize = 4;
// Leave deterministic headroom under the public 64 KiB receipt ceiling for
// capsule metadata and the CA facade envelope. Action caches may retain the
// larger primitive result; batch receipts are bounded here before publication.
const MAX_QUERY_RECEIPT_ENTRY_BYTES: usize = 36 * 1024;
const MAX_CAPSULE_ENTRY_BYTES: usize = 8 * 1024;
const MAX_QUERY_BASE_RECEIPT_BYTES: usize = 48 * 1024;
const MAX_PUBLIC_BROKER_RECEIPT_BYTES: usize = 60 * 1024;
const MAX_ATOMIC_TEMP_ATTEMPTS: usize = 16;
static NEXT_BROKER_JOB_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_BROKER_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerRecord {
    schema: String,
    nonce: String,
    process_id: u32,
    start_token: Option<String>,
    repository_hash: String,
    binary_digest: String,
    started_at_epoch_seconds: u64,
}

#[derive(Debug, Clone)]
struct BrokerPaths {
    root: PathBuf,
    socket: PathBuf,
    owner: PathBuf,
    token: PathBuf,
    cache: PathBuf,
    repository_hash: String,
    runtime_key: String,
}

impl BrokerPaths {
    fn new(repo: &Path) -> Result<Self> {
        let repo = fs::canonicalize(repo).context("canonicalize broker repository")?;
        #[cfg(debug_assertions)]
        let home = std::env::var_os("CODEX_TEAM_TEST_BROKER_ROOT")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .context("HOME is unavailable for broker runtime")?;
        #[cfg(not(debug_assertions))]
        let home = PathBuf::from(
            std::env::var_os("HOME").context("HOME is unavailable for broker runtime")?,
        );
        let metadata = fs::symlink_metadata(&home).context("inspect HOME for broker runtime")?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("HOME must be a real directory for broker runtime");
        }
        let home = fs::canonicalize(home)?;
        let repository_hash = format!("{:x}", Sha256::digest(repo.as_os_str().as_encoded_bytes()));
        // macOS limits Unix-domain socket paths to roughly one hundred bytes.
        // The full digest still crosses the wire and is checked in owner records;
        // this collision-resistant prefix is only the private directory name.
        let runtime_key = repository_hash[..32].to_string();
        let root = home
            .join(".codex-team")
            .join("broker")
            .join("v1")
            .join(&runtime_key);
        Ok(Self {
            socket: root.join("s"),
            owner: root.join("owner.json"),
            token: root.join("token"),
            cache: root.join("cache"),
            root,
            repository_hash,
            runtime_key,
        })
    }

    fn ensure_private(&self) -> Result<()> {
        let home = self
            .root
            .ancestors()
            .nth(4)
            .context("broker root has no HOME anchor")?;
        let mut current = home.to_path_buf();
        for component in [".codex-team", "broker", "v1", &self.runtime_key] {
            current.push(component);
            match fs::symlink_metadata(&current) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() || !metadata.is_dir() {
                        bail!("broker runtime contains a symlink or non-directory");
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    match fs::create_dir(&current) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                            let metadata = fs::symlink_metadata(&current)?;
                            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                                bail!("broker runtime election created an unsafe directory");
                            }
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                Err(error) => return Err(error.into()),
            }
            set_private_directory_permissions(&current)?;
        }
        match fs::symlink_metadata(&self.cache) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    bail!("broker cache must be a real directory");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::create_dir(&self.cache) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
            }
            Err(error) => return Err(error.into()),
        }
        set_private_directory_permissions(&self.cache)?;
        Ok(())
    }

    fn token(&self) -> Result<String> {
        self.ensure_private()?;
        match fs::symlink_metadata(&self.token) {
            Ok(metadata) => {
                require_private_regular_file(&metadata, "broker token")?;
                let token = fs::read_to_string(&self.token)?;
                validate_digest(token.trim())?;
                Ok(token.trim().to_string())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let token = random_digest()?;
                match write_private_new(&self.token, format!("{token}\n").as_bytes()) {
                    Ok(()) => Ok(token),
                    Err(error)
                        if error
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|io| io.kind() == std::io::ErrorKind::AlreadyExists) =>
                    {
                        let metadata = fs::symlink_metadata(&self.token)?;
                        require_private_regular_file(&metadata, "broker token")?;
                        let token = fs::read_to_string(&self.token)?;
                        validate_digest(token.trim())?;
                        Ok(token.trim().to_string())
                    }
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum BrokerOperation {
    Status,
    Stop,
    Query { request: MechanisticRequestV1 },
    Start { request: MechanisticRequestV1 },
    JobStatus { job_id: String, wait_seconds: u8 },
    Cancel { job_id: String },
    Feedback { arguments: Value },
    Detach,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BrokerRequest {
    schema: String,
    token: String,
    client_id: String,
    repository_hash: String,
    binary_digest: String,
    environment_fingerprint: String,
    operation: BrokerOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BrokerWireError {
    code: String,
    message: String,
    retryable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BrokerFeedbackArgs {
    job_id: String,
    expected_revision: String,
    outcome: String,
    category: String,
    #[serde(default)]
    human_confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BrokerResponse {
    schema: String,
    ok: bool,
    value: Option<Value>,
    error: Option<BrokerWireError>,
}

impl BrokerResponse {
    fn success(value: Value) -> Self {
        Self {
            schema: BROKER_WIRE_SCHEMA.to_string(),
            ok: true,
            value: Some(value),
            error: None,
        }
    }

    fn error(code: &str, error: &anyhow::Error, retryable: bool) -> Self {
        Self {
            schema: BROKER_WIRE_SCHEMA.to_string(),
            ok: false,
            value: None,
            error: Some(BrokerWireError {
                code: code.to_string(),
                message: bounded(&format!("{error:#}"), 2_048),
                retryable,
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BrokerCallError {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) retryable: bool,
    pub(crate) safe_local_fallback: bool,
}

impl std::fmt::Display for BrokerCallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for BrokerCallError {}

#[derive(Debug, Clone)]
pub(crate) struct BrokerClient {
    repo: PathBuf,
    paths: BrokerPaths,
    client_id: String,
    binary_digest: String,
    environment_fingerprint: String,
}

impl BrokerClient {
    pub(crate) fn new(repo: &Path) -> Result<Self> {
        let repo = fs::canonicalize(repo)?;
        Ok(Self {
            paths: BrokerPaths::new(&repo)?,
            repo,
            client_id: format!("client-{}", random_digest()?),
            binary_digest: file_digest(&std::env::current_exe()?)?,
            environment_fingerprint: environment_fingerprint(),
        })
    }

    pub(crate) async fn call(&self, operation: BrokerOperation) -> Result<Value, BrokerCallError> {
        self.ensure_running().await?;
        self.call_existing(operation).await
    }

    pub(crate) async fn detach(&self) {
        let _ = self.call_existing(BrokerOperation::Detach).await;
    }

    pub(crate) async fn status_if_running(&self) -> Value {
        match inspect_owner(&self.paths) {
            Ok(OwnerInspection::Live) => match self.call_existing(BrokerOperation::Status).await {
                Ok(value) => value,
                Err(error) => json!({
                    "schema": "codex-team.broker_status.v1",
                    "running": false,
                    "state": "unreachable",
                    "error_code": error.code,
                }),
            },
            Ok(OwnerInspection::Stale) => json!({
                "schema": "codex-team.broker_status.v1",
                "running": false,
                "state": "stale_owner",
            }),
            Ok(OwnerInspection::Absent) => json!({
                "schema": "codex-team.broker_status.v1",
                "running": false,
                "state": "not_started",
            }),
            Err(_) => json!({
                "schema": "codex-team.broker_status.v1",
                "running": false,
                "state": "unsafe_runtime",
            }),
        }
    }

    async fn call_existing(&self, operation: BrokerOperation) -> Result<Value, BrokerCallError> {
        let token = self.paths.token().map_err(|error| BrokerCallError {
            code: "BROKER_RUNTIME_UNSAFE".into(),
            message: bounded(&format!("{error:#}"), 2_048),
            retryable: false,
            safe_local_fallback: false,
        })?;
        let request = BrokerRequest {
            schema: BROKER_WIRE_SCHEMA.to_string(),
            token,
            client_id: self.client_id.clone(),
            repository_hash: self.paths.repository_hash.clone(),
            binary_digest: self.binary_digest.clone(),
            environment_fingerprint: self.environment_fingerprint.clone(),
            operation,
        };
        let bytes = serde_json::to_vec(&request).map_err(internal_call_error)?;
        if bytes.len() as u64 > MAX_WIRE_BYTES {
            return Err(BrokerCallError {
                code: "BROKER_REQUEST_TOO_LARGE".into(),
                message: "broker request exceeds 1 MiB".into(),
                retryable: false,
                safe_local_fallback: false,
            });
        }
        let mut stream = UnixStream::connect(&self.paths.socket)
            .await
            .map_err(|error| connection_call_error(error, false))?;
        stream
            .write_all(&bytes)
            .await
            .map_err(|error| connection_call_error(error, false))?;
        stream
            .shutdown()
            .await
            .map_err(|error| connection_call_error(error, false))?;
        let mut response_bytes = Vec::new();
        stream
            .take(MAX_WIRE_BYTES + 1)
            .read_to_end(&mut response_bytes)
            .await
            .map_err(|error| connection_call_error(error, false))?;
        if response_bytes.len() as u64 > MAX_WIRE_BYTES {
            return Err(BrokerCallError {
                code: "BROKER_RESPONSE_TOO_LARGE".into(),
                message: "broker response exceeds 1 MiB".into(),
                retryable: false,
                safe_local_fallback: false,
            });
        }
        let response: BrokerResponse =
            serde_json::from_slice(&response_bytes).map_err(internal_call_error)?;
        if response.schema != BROKER_WIRE_SCHEMA {
            return Err(BrokerCallError {
                code: "BROKER_PROTOCOL".into(),
                message: "broker returned an unsupported protocol".into(),
                retryable: false,
                safe_local_fallback: false,
            });
        }
        if response.ok {
            response.value.ok_or_else(|| BrokerCallError {
                code: "BROKER_PROTOCOL".into(),
                message: "broker omitted its response value".into(),
                retryable: false,
                safe_local_fallback: false,
            })
        } else {
            let error = response.error.unwrap_or(BrokerWireError {
                code: "BROKER_FAILED".into(),
                message: "broker failed without details".into(),
                retryable: false,
            });
            let safe_local_fallback = matches!(
                error.code.as_str(),
                "BROKER_OBSERVATION_LOCAL" | "BROKER_ENVIRONMENT_MISMATCH"
            );
            Err(BrokerCallError {
                code: error.code,
                message: error.message,
                retryable: error.retryable,
                safe_local_fallback,
            })
        }
    }

    async fn ensure_running(&self) -> Result<(), BrokerCallError> {
        #[cfg(test)]
        if std::env::current_exe()
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
            })
            .is_some_and(|name| {
                name.starts_with("codex_team-") || name.starts_with("ca_automation-")
            })
        {
            return Err(BrokerCallError {
                code: "BROKER_TEST_HARNESS".into(),
                message: "unit-test harness cannot host the standalone broker".into(),
                retryable: false,
                safe_local_fallback: true,
            });
        }
        if self.paths.socket.exists()
            && self
                .call_existing_without_ensure(BrokerOperation::Status)
                .await
                .is_ok()
        {
            return Ok(());
        }
        let ownership = inspect_owner(&self.paths).map_err(|error| BrokerCallError {
            code: "BROKER_RUNTIME_UNSAFE".into(),
            message: bounded(&format!("{error:#}"), 2_048),
            retryable: false,
            safe_local_fallback: false,
        })?;
        match ownership {
            OwnerInspection::Live => {
                return wait_for_broker(self, false).await;
            }
            OwnerInspection::Stale => {
                remove_stale_runtime(&self.paths).map_err(|error| BrokerCallError {
                    code: "BROKER_STALE_RECOVERY_FAILED".into(),
                    message: bounded(&format!("{error:#}"), 2_048),
                    retryable: false,
                    safe_local_fallback: false,
                })?;
            }
            OwnerInspection::Absent => {}
        }
        self.paths
            .ensure_private()
            .map_err(|error| BrokerCallError {
                code: "BROKER_RUNTIME_UNSAFE".into(),
                message: bounded(&format!("{error:#}"), 2_048),
                retryable: false,
                safe_local_fallback: false,
            })?;
        let nonce = random_digest().map_err(internal_call_error)?;
        let executable = std::env::current_exe().map_err(internal_call_error)?;
        let owner = OwnerRecord {
            schema: BROKER_OWNER_SCHEMA.to_string(),
            nonce: nonce.clone(),
            process_id: 0,
            start_token: None,
            repository_hash: self.paths.repository_hash.clone(),
            binary_digest: file_digest(&executable).map_err(internal_call_error)?,
            started_at_epoch_seconds: epoch_seconds(),
        };
        match write_private_new(
            &self.paths.owner,
            &serde_json::to_vec(&owner).unwrap_or_default(),
        ) {
            Ok(()) => {}
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::AlreadyExists) =>
            {
                return wait_for_broker(self, false).await
            }
            Err(error) => {
                return Err(BrokerCallError {
                    code: "BROKER_ELECTION_FAILED".into(),
                    message: bounded(&format!("{error:#}"), 2_048),
                    retryable: false,
                    safe_local_fallback: true,
                })
            }
        }
        let mut command = Command::new(executable);
        command
            .arg("broker")
            .arg("internal-serve")
            .arg("--repo")
            .arg(&self.repo)
            .arg("--owner-nonce")
            .arg(&nonce)
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        #[cfg(debug_assertions)]
        if std::env::var_os("CODEX_TEAM_TEST_BROKER_ROOT").is_some() {
            let log_path = self.paths.root.join("broker-test.stderr");
            let log = open_private_append(&log_path).map_err(internal_call_error)?;
            command.stderr(log);
        } else {
            command.stderr(Stdio::null());
        }
        #[cfg(not(debug_assertions))]
        command.stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        if let Err(error) = command.spawn() {
            let _ = remove_owner_if_nonce(&self.paths, &nonce);
            return Err(BrokerCallError {
                code: "BROKER_START_FAILED".into(),
                message: bounded(&error.to_string(), 2_048),
                retryable: true,
                safe_local_fallback: true,
            });
        }
        wait_for_broker(self, true).await
    }

    async fn call_existing_without_ensure(
        &self,
        operation: BrokerOperation,
    ) -> Result<Value, BrokerCallError> {
        self.call_existing(operation).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnerInspection {
    Absent,
    Live,
    Stale,
}

async fn wait_for_broker(client: &BrokerClient, starter: bool) -> Result<(), BrokerCallError> {
    // Debug and release binaries can be large enough that concurrent clients
    // spend several seconds authenticating their executable digest before the
    // elected child reaches its socket bind. Keep startup bounded, but do not
    // mistake that deterministic authentication work for a dead owner.
    const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let mut last_error = None;
    loop {
        if client.paths.socket.exists() {
            match client
                .call_existing_without_ensure(BrokerOperation::Status)
                .await
            {
                Ok(_) => return Ok(()),
                Err(error) => last_error = Some(error),
            }
        }
        if Instant::now() >= deadline {
            let message = if let Some(error) = last_error {
                format!(
                    "broker did not become ready within ten seconds; last handshake error {}: {}",
                    error.code, error.message
                )
            } else {
                "broker did not become ready within ten seconds".to_string()
            };
            #[cfg(debug_assertions)]
            let message = {
                let mut message = message;
                if std::env::var_os("CODEX_TEAM_TEST_BROKER_ROOT").is_some() {
                    if let Ok(log) =
                        fs::read_to_string(client.paths.root.join("broker-test.stderr"))
                    {
                        let log = bounded(log.trim(), 1_024);
                        if !log.is_empty() {
                            message.push_str("; daemon: ");
                            message.push_str(&log);
                        }
                    }
                }
                message
            };
            let safe_local_fallback = starter
                && !client.paths.socket.exists()
                && matches!(
                    inspect_owner(&client.paths),
                    Ok(OwnerInspection::Absent | OwnerInspection::Stale)
                );
            return Err(BrokerCallError {
                code: "BROKER_START_TIMEOUT".into(),
                message,
                retryable: true,
                safe_local_fallback,
            });
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheRecord {
    schema: String,
    kind: String,
    key: String,
    revision: String,
    created_at_epoch_seconds: u64,
    last_used_epoch_seconds: u64,
    receipt: Value,
    record_digest: String,
    source_job_id: Option<String>,
    source_receipt_digest: String,
    duration_millis: u64,
    #[serde(default)]
    constituent_keys: Vec<String>,
}

#[derive(Debug, Clone)]
struct SharedQuery {
    receiver: watch::Receiver<Option<Result<MechanisticActionResultV1, String>>>,
}

#[derive(Debug, Clone)]
struct SharedEvidence {
    job_id: String,
    consumers: BTreeSet<String>,
    cancellation: CancellationToken,
    started: Instant,
}

#[derive(Debug)]
struct BrokerCoordinator {
    repo: PathBuf,
    paths: BrokerPaths,
    broker_id: String,
    binary_digest: String,
    broker_environment_fingerprint: String,
    query_inflight: tokio::sync::Mutex<HashMap<String, SharedQuery>>,
    evidence: tokio::sync::Mutex<HashMap<String, SharedEvidence>>,
    jobs_by_id: tokio::sync::Mutex<HashMap<String, String>>,
    query_limit: Arc<Semaphore>,
    evidence_limit: Arc<Semaphore>,
    readiness: tokio::sync::OnceCell<DelegationReadinessV1>,
    clients: tokio::sync::Mutex<HashMap<String, Instant>>,
    stop: std::sync::atomic::AtomicBool,
    served_request: std::sync::atomic::AtomicBool,
    last_activity: tokio::sync::Mutex<Instant>,
}

impl BrokerCoordinator {
    fn new(repo: PathBuf, paths: BrokerPaths, broker_id: String) -> Self {
        Self {
            repo,
            paths,
            broker_id,
            binary_digest: std::env::current_exe()
                .ok()
                .and_then(|path| file_digest(&path).ok())
                .unwrap_or_default(),
            broker_environment_fingerprint: environment_fingerprint(),
            query_inflight: tokio::sync::Mutex::new(HashMap::new()),
            evidence: tokio::sync::Mutex::new(HashMap::new()),
            jobs_by_id: tokio::sync::Mutex::new(HashMap::new()),
            query_limit: Arc::new(Semaphore::new(MAX_PARALLEL_QUERIES)),
            evidence_limit: Arc::new(Semaphore::new(1)),
            readiness: tokio::sync::OnceCell::new(),
            clients: tokio::sync::Mutex::new(HashMap::new()),
            stop: std::sync::atomic::AtomicBool::new(false),
            served_request: std::sync::atomic::AtomicBool::new(false),
            last_activity: tokio::sync::Mutex::new(Instant::now()),
        }
    }

    async fn readiness(&self) -> Result<DelegationReadinessV1> {
        let repo = self.repo.clone();
        self.readiness
            .get_or_try_init(|| async move {
                tokio::task::spawn_blocking(move || {
                    MechanisticExecutor::new(&repo).map(|executor| executor.readiness())
                })
                .await
                .context("broker readiness probe panicked")?
            })
            .await
            .cloned()
    }

    async fn dispatch(
        self: &Arc<Self>,
        request: BrokerRequest,
        expected_token: &str,
    ) -> BrokerResponse {
        if request.schema != BROKER_WIRE_SCHEMA
            || request.token != expected_token
            || request.repository_hash != self.paths.repository_hash
            || request.binary_digest != self.binary_digest
        {
            return BrokerResponse::error(
                "BROKER_AUTH_FAILED",
                &anyhow::anyhow!("broker authentication failed"),
                false,
            );
        }
        if validate_client_id(&request.client_id).is_err()
            || validate_digest(&request.environment_fingerprint).is_err()
        {
            return BrokerResponse::error(
                "BROKER_INVALID_ARGUMENT",
                &anyhow::anyhow!("broker client identity is invalid"),
                false,
            );
        }
        self.served_request
            .store(true, std::sync::atomic::Ordering::Release);
        *self.last_activity.lock().await = Instant::now();
        self.clients
            .lock()
            .await
            .insert(request.client_id.clone(), Instant::now());
        let result = match request.operation {
            BrokerOperation::Status => self.status().await,
            BrokerOperation::Stop => self.stop().await,
            BrokerOperation::Query { request: query } => self.execute_queries(query).await,
            BrokerOperation::Start {
                request: delegation,
            } => {
                self.start_delegation(
                    &request.client_id,
                    &request.environment_fingerprint,
                    delegation,
                )
                .await
            }
            BrokerOperation::JobStatus {
                job_id,
                wait_seconds,
            } => {
                self.job_status(&request.client_id, &job_id, wait_seconds)
                    .await
            }
            BrokerOperation::Cancel { job_id } => self.cancel(&request.client_id, &job_id).await,
            BrokerOperation::Feedback { arguments } => self.feedback(arguments).await,
            BrokerOperation::Detach => self.detach(&request.client_id).await,
        };
        match result {
            Ok(value) => BrokerResponse::success(value),
            Err(error) => BrokerResponse::error(classify_broker_error(&error), &error, false),
        }
    }

    async fn status(&self) -> Result<Value> {
        let clients = self.clients.lock().await.len();
        let evidence = self.evidence.lock().await.len();
        let cache_bytes = cache_size(&self.paths.cache)?;
        let idle_remaining = idle_timeout()
            .saturating_sub(self.last_activity.lock().await.elapsed())
            .as_secs();
        Ok(json!({
            "schema": "codex-team.broker_status.v1",
            "running": true,
            "broker_id": self.broker_id,
            "binary_digest": self.binary_digest,
            "clients": clients,
            "active_jobs": evidence,
            "cache_bytes": cache_bytes,
            "cache_limit_bytes": MAX_CACHE_BYTES,
            "retention_seconds": MAX_CACHE_AGE.as_secs(),
            "idle_timeout_seconds": idle_timeout().as_secs(),
            "idle_remaining_seconds": idle_remaining,
            "idle_deadline_epoch_seconds": epoch_seconds().saturating_add(idle_remaining),
        }))
    }

    async fn stop(&self) -> Result<Value> {
        if !self.evidence.lock().await.is_empty() {
            bail!("broker stop refused while jobs are active");
        }
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        Ok(json!({
            "schema": "codex-team.broker_stop.v1",
            "status": "stopping"
        }))
    }

    async fn should_stop(&self) -> bool {
        if self.stop.load(std::sync::atomic::Ordering::Acquire) {
            return true;
        }
        if !self
            .served_request
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return false;
        }
        self.expire_clients().await;
        self.evidence.lock().await.is_empty()
            && self.clients.lock().await.is_empty()
            && self.last_activity.lock().await.elapsed() >= idle_timeout()
    }

    async fn expire_clients(&self) {
        let now = Instant::now();
        let expired = {
            let mut clients = self.clients.lock().await;
            let expired = clients
                .iter()
                .filter(|(_, seen)| now.duration_since(**seen) >= CLIENT_RECONNECT_GRACE)
                .map(|(client, _)| client.clone())
                .collect::<Vec<_>>();
            for client in &expired {
                clients.remove(client);
            }
            expired
        };
        let had_expired = !expired.is_empty();
        for client in expired {
            let _ = self.detach(&client).await;
        }
        if had_expired {
            // Reconnect grace is part of the client's lifetime. Start the idle
            // deadline when an abruptly disconnected client actually expires,
            // rather than measuring it from that client's last request.
            *self.last_activity.lock().await = Instant::now();
        }
    }

    async fn execute_queries(self: &Arc<Self>, request: MechanisticRequestV1) -> Result<Value> {
        validate_request(&request)?;
        if request.actions.iter().any(|action| !action.is_query()) {
            bail!("repo_query accepts repository query actions only");
        }
        let paths = Paths::new(&self.repo)?;
        if repository_revision(&paths)? != request.expected_revision {
            bail!("repository revision is stale; refresh status before delegation");
        }
        let started = Instant::now();
        let started_at = chrono::Utc::now().to_rfc3339();
        let mut tasks = tokio::task::JoinSet::new();
        for (index, action) in request.actions.iter().cloned().enumerate() {
            let coordinator = Arc::clone(self);
            let revision = request.expected_revision.clone();
            tasks.spawn(async move {
                (
                    index,
                    coordinator.query_action(revision, index, action).await,
                )
            });
        }
        let mut raw_results = Vec::new();
        while let Some(result) = tasks.join_next().await {
            raw_results.push(result.context("shared query task panicked")?);
        }
        raw_results.sort_by_key(|(index, _)| *index);
        let mut output_bytes = 0_usize;
        let mut results = Vec::new();
        let mut dispositions = Vec::new();
        let mut avoided = 0_u64;
        for (index, result) in raw_results.drain(..) {
            let (mut result, disposition, avoided_millis) = result?;
            result.action_index = u8::try_from(index).unwrap_or(u8::MAX);
            let remaining = MAX_QUERY_RECEIPT_ENTRY_BYTES.saturating_sub(output_bytes);
            bound_action_result(&mut result, remaining);
            output_bytes = output_bytes.saturating_add(
                result
                    .entries
                    .iter()
                    .map(|entry| encoded_entry_size(entry))
                    .sum::<usize>(),
            );
            dispositions.push(disposition);
            avoided = avoided.saturating_add(avoided_millis);
            results.push(result);
            if output_bytes >= MAX_QUERY_RECEIPT_ENTRY_BYTES {
                break;
            }
        }
        if repository_revision(&paths)? != request.expected_revision {
            bail!("repository revision changed while query work was running");
        }
        let disposition = if dispositions
            .iter()
            .all(|value| *value == MechanisticBrokerDispositionV1::CacheHit)
        {
            MechanisticBrokerDispositionV1::CacheHit
        } else if dispositions.contains(&MechanisticBrokerDispositionV1::Attached) {
            MechanisticBrokerDispositionV1::Attached
        } else {
            MechanisticBrokerDispositionV1::Executed
        };
        let work_digest = query_batch_digest(&request)?;
        let rust_api_baseline_ids = if matches!(
            request.purpose,
            MechanisticRequestPurposeV1::AutomaticPreflight
                | MechanisticRequestPurposeV1::PredictiveWarmup
        ) {
            let selected_paths = results
                .iter()
                .filter_map(|result| result.decision.as_ref())
                .flat_map(|decision| decision.selected_paths.iter().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .take(64)
                .collect::<Vec<_>>();
            crate::mechanistic::prime_rust_api(&self.repo, &selected_paths).unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut receipt = MechanisticReceiptV1 {
            schema: MECHANISTIC_RECEIPT_SCHEMA.to_string(),
            request_digest: request_digest(&request)?,
            repository_revision: request.expected_revision.clone(),
            status: MechanisticReceiptStatusV1::Completed,
            readiness: self.readiness().await?,
            action_results: results,
            output_bytes: output_bytes as u64,
            started_at,
            finished_at: chrono::Utc::now().to_rfc3339(),
            elapsed_millis: started.elapsed().as_millis() as u64,
            fallback_reason: None,
            broker: Some(MechanisticBrokerReceiptV1 {
                disposition,
                work_digest: work_digest.clone(),
                source_job_id: None,
                source_receipt_digest: None,
                consumer_count: 1,
                avoided_execution_millis: avoided,
                rust_api_baseline_ids,
            }),
            capsule: None,
        };
        compact_query_base_receipt(&mut receipt, MAX_QUERY_BASE_RECEIPT_BYTES)?;
        let source_digest = canonical_query_receipt_digest(&request, &receipt.action_results)?;
        if let Some(broker) = &mut receipt.broker {
            broker.source_receipt_digest = Some(source_digest.clone());
        }
        let job_id = format!("query-{}", &work_digest[..32]);
        if let Some(broker) = &mut receipt.broker {
            broker.source_job_id = Some(job_id.clone());
        }
        receipt.capsule = Some(query_capsule(
            &request,
            &receipt.action_results,
            &source_digest,
            receipt.broker.as_ref(),
        )?);
        compact_query_capsule(&mut receipt, MAX_PUBLIC_BROKER_RECEIPT_BYTES)?;
        let value = serde_json::to_value(&receipt)?;
        write_cache_record(
            &self.paths,
            &CacheRecord {
                schema: BROKER_CACHE_SCHEMA.to_string(),
                kind: "query_receipt".into(),
                key: work_digest,
                revision: request.expected_revision.clone(),
                created_at_epoch_seconds: epoch_seconds(),
                last_used_epoch_seconds: epoch_seconds(),
                receipt: value.clone(),
                record_digest: String::new(),
                source_job_id: Some(job_id),
                source_receipt_digest: source_digest,
                duration_millis: receipt.elapsed_millis,
                constituent_keys: request
                    .actions
                    .iter()
                    .filter_map(|action| {
                        query_action_digest(&request.expected_revision, action).ok()
                    })
                    .collect(),
            },
        )?;
        Ok(value)
    }

    async fn query_action(
        &self,
        revision: String,
        index: usize,
        action: ca_automation_protocol::MechanisticActionV1,
    ) -> Result<(
        MechanisticActionResultV1,
        MechanisticBrokerDispositionV1,
        u64,
    )> {
        let key = query_action_digest(&revision, &action)?;
        if let Some(record) = read_cache_record(&self.paths, "query", &key)? {
            let mut result: MechanisticActionResultV1 = serde_json::from_value(record.receipt)?;
            result.action_index = u8::try_from(index).unwrap_or(u8::MAX);
            return Ok((
                result,
                MechanisticBrokerDispositionV1::CacheHit,
                record.duration_millis,
            ));
        }
        let receiver = {
            let mut inflight = self.query_inflight.lock().await;
            if let Some(shared) = inflight.get(&key) {
                Some((shared.receiver.clone(), true))
            } else {
                let (sender, receiver) = watch::channel(None);
                inflight.insert(
                    key.clone(),
                    SharedQuery {
                        receiver: receiver.clone(),
                    },
                );
                let repo = self.repo.clone();
                let cache_paths = self.paths.clone();
                let key_for_task = key.clone();
                let revision_for_task = revision.clone();
                let limit = self.query_limit.clone();
                tokio::spawn(async move {
                    let permit = limit.acquire_owned().await;
                    let started = Instant::now();
                    let outcome = match permit {
                        Ok(_permit) => tokio::task::spawn_blocking(move || {
                            execute_query_action(
                                &repo,
                                index,
                                &action,
                                MAX_MECHANISTIC_OUTPUT_BYTES,
                            )
                        })
                        .await
                        .map_err(|error| error.to_string())
                        .and_then(|result| result.map_err(|error| format!("{error:#}"))),
                        Err(error) => Err(error.to_string()),
                    };
                    if let Ok(result) = &outcome {
                        let _ = write_cache_record(
                            &cache_paths,
                            &CacheRecord {
                                schema: BROKER_CACHE_SCHEMA.to_string(),
                                kind: "query".into(),
                                key: key_for_task,
                                revision: revision_for_task,
                                created_at_epoch_seconds: epoch_seconds(),
                                last_used_epoch_seconds: epoch_seconds(),
                                receipt: serde_json::to_value(result).unwrap_or(Value::Null),
                                record_digest: String::new(),
                                source_job_id: None,
                                source_receipt_digest: receipt_digest(result).unwrap_or_default(),
                                duration_millis: started.elapsed().as_millis() as u64,
                                constituent_keys: Vec::new(),
                            },
                        );
                    }
                    let _ = sender.send(Some(outcome));
                });
                Some((receiver, false))
            }
        }
        .context("create broker query waiter")?;
        let (mut receiver, attached) = receiver;
        loop {
            let outcome = receiver.borrow().clone();
            if let Some(outcome) = outcome {
                self.query_inflight.lock().await.remove(&key);
                return outcome
                    .map(|result| {
                        (
                            result,
                            if attached {
                                MechanisticBrokerDispositionV1::Attached
                            } else {
                                MechanisticBrokerDispositionV1::Executed
                            },
                            0,
                        )
                    })
                    .map_err(anyhow::Error::msg);
            }
            receiver
                .changed()
                .await
                .context("shared query task ended")?;
        }
    }

    async fn start_delegation(
        self: &Arc<Self>,
        client_id: &str,
        environment_fingerprint: &str,
        request: MechanisticRequestV1,
    ) -> Result<Value> {
        validate_request(&request)?;
        if request.actions.len() == 1 && request.actions[0].is_observation() {
            bail!("BROKER_OBSERVATION_LOCAL: local observations remain facade-owned")
        }
        if request.actions.iter().any(|action| !action.is_evidence()) {
            bail!("delegation_start accepts an evidence chain or one local observation")
        }
        if environment_fingerprint != self.broker_environment_fingerprint {
            bail!("BROKER_ENVIRONMENT_MISMATCH: evidence environment differs from broker owner")
        }
        let paths = Paths::new(&self.repo)?;
        let current_revision = repository_revision(&paths)?;
        if current_revision != request.expected_revision {
            bail!("repository revision is stale; inspect status before delegation")
        }
        let readiness = self.readiness().await?;
        if !readiness.evidence_available && !request.allow_unisolated_evidence {
            return Ok(json!({
                "schema": "codex-team.mcp_delegation_start.v1",
                "status": "unavailable",
                "expected_revision": request.expected_revision,
                "readiness": readiness,
                "poll_after_ms": Value::Null,
                "broker": {
                    "disposition": "executed",
                    "work_digest": evidence_work_digest(
                        &request,
                        environment_fingerprint,
                        readiness.evidence_backend.as_deref().unwrap_or("unavailable"),
                    )?,
                    "consumer_count": 1,
                    "avoided_execution_millis": 0
                }
            }));
        }
        let isolation_backend = readiness
            .evidence_backend
            .clone()
            .unwrap_or_else(|| "host_unisolated".to_string());
        let work_digest =
            evidence_work_digest(&request, environment_fingerprint, &isolation_backend)?;

        if let Some(record) = read_cache_record(&self.paths, "evidence", &work_digest)? {
            let mut receipt: MechanisticReceiptV1 = serde_json::from_value(record.receipt)?;
            receipt.request_digest = request_digest(&request)?;
            let now = chrono::Utc::now().to_rfc3339();
            receipt.started_at = now.clone();
            receipt.finished_at = now;
            receipt.elapsed_millis = 0;
            receipt.broker = Some(MechanisticBrokerReceiptV1 {
                disposition: MechanisticBrokerDispositionV1::CacheHit,
                work_digest: work_digest.clone(),
                source_job_id: record.source_job_id.clone(),
                source_receipt_digest: Some(record.source_receipt_digest.clone()),
                consumer_count: 1,
                avoided_execution_millis: record.duration_millis,
                rust_api_baseline_ids: Vec::new(),
            });
            let job_id = new_broker_job_id();
            save_terminal_evidence_job(&self.repo, &job_id, &request, &receipt)?;
            return Ok(delegation_start_receipt(
                &job_id,
                &request,
                &readiness,
                DelegationStartBroker {
                    disposition: MechanisticBrokerDispositionV1::CacheHit,
                    work_digest: &work_digest,
                    source_job_id: record.source_job_id.as_deref(),
                    avoided_execution_millis: record.duration_millis,
                    consumer_count: 1,
                },
            ));
        }

        {
            let mut evidence = self.evidence.lock().await;
            if let Some(shared) = evidence.get_mut(&work_digest) {
                shared.consumers.insert(client_id.to_string());
                self.jobs_by_id
                    .lock()
                    .await
                    .insert(shared.job_id.clone(), work_digest.clone());
                return Ok(delegation_start_receipt(
                    &shared.job_id,
                    &request,
                    &readiness,
                    DelegationStartBroker {
                        disposition: MechanisticBrokerDispositionV1::Attached,
                        work_digest: &work_digest,
                        source_job_id: Some(&shared.job_id),
                        avoided_execution_millis: shared.started.elapsed().as_millis() as u64,
                        consumer_count: shared.consumers.len(),
                    },
                ));
            }
            if evidence.len() >= MAX_UNIQUE_EVIDENCE_JOBS {
                bail!("broker evidence queue is full")
            }
            let job_id = new_broker_job_id();
            let cancellation = CancellationToken::default();
            let mut consumers = BTreeSet::new();
            consumers.insert(client_id.to_string());
            evidence.insert(
                work_digest.clone(),
                SharedEvidence {
                    job_id: job_id.clone(),
                    consumers,
                    cancellation: cancellation.clone(),
                    started: Instant::now(),
                },
            );
            self.jobs_by_id
                .lock()
                .await
                .insert(job_id.clone(), work_digest.clone());
            save_running_evidence_job(&self.repo, &job_id, &request, "queued")?;

            let coordinator = Arc::clone(self);
            let background_job_id = job_id.clone();
            let background_request = request.clone();
            let background_digest = work_digest.clone();
            let background_readiness = readiness.clone();
            tokio::spawn(async move {
                coordinator
                    .run_evidence(
                        background_job_id,
                        background_digest,
                        background_request,
                        background_readiness,
                        cancellation,
                    )
                    .await;
            });
            Ok(delegation_start_receipt(
                &job_id,
                &request,
                &readiness,
                DelegationStartBroker {
                    disposition: MechanisticBrokerDispositionV1::Executed,
                    work_digest: &work_digest,
                    source_job_id: None,
                    avoided_execution_millis: 0,
                    consumer_count: 1,
                },
            ))
        }
    }

    async fn run_evidence(
        self: &Arc<Self>,
        job_id: String,
        work_digest: String,
        request: MechanisticRequestV1,
        _readiness: ca_automation_protocol::DelegationReadinessV1,
        cancellation: CancellationToken,
    ) {
        let store = JobStore::new(&self.repo);
        let permit = self.evidence_limit.clone().acquire_owned().await;
        if permit.is_err() {
            finish_broker_job_error(
                &store,
                &job_id,
                JobState::Interrupted,
                "broker queue closed",
            );
            return;
        }
        if let Ok(mut record) = store.load_sync(&job_id) {
            record.updated_at = chrono::Utc::now().to_rfc3339();
            record.progress = Some(JobProgress {
                phase: "verifying".to_string(),
                summary: "shared typed evidence chain is running".to_string(),
            });
            let _ = store.save_atomic(&record);
        }
        let repo = self.repo.clone();
        let request_for_run = request.clone();
        let cancellation_for_run = cancellation.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            MechanisticExecutor::new(&repo)?
                .execute_evidence_with_cancellation(&request_for_run, &cancellation_for_run)
        })
        .await;
        let consumer_count = self
            .evidence
            .lock()
            .await
            .get(&work_digest)
            .map(|shared| shared.consumers.len())
            .unwrap_or(0);
        let mut record = match store.load_sync(&job_id) {
            Ok(record) => record,
            Err(_) => return,
        };
        record.updated_at = chrono::Utc::now().to_rfc3339();
        record.progress = Some(JobProgress {
            phase: "finished".to_string(),
            summary: "shared typed evidence delegation finished".to_string(),
        });
        match outcome {
            Ok(Ok(mut receipt)) => {
                let source_digest = receipt_digest(&receipt).unwrap_or_default();
                receipt.broker = Some(MechanisticBrokerReceiptV1 {
                    disposition: MechanisticBrokerDispositionV1::Executed,
                    work_digest: work_digest.clone(),
                    source_job_id: Some(job_id.clone()),
                    source_receipt_digest: Some(source_digest.clone()),
                    consumer_count: u16::try_from(consumer_count).unwrap_or(u16::MAX),
                    avoided_execution_millis: 0,
                    rust_api_baseline_ids: request
                        .actions
                        .iter()
                        .find_map(|action| match action {
                            ca_automation_protocol::MechanisticActionV1::VerifyChangedFiles {
                                rust_api_baseline_ids,
                                ..
                            } => Some(rust_api_baseline_ids.clone()),
                            _ => None,
                        })
                        .unwrap_or_default(),
                });
                record.status = if receipt.status == MechanisticReceiptStatusV1::Completed {
                    JobState::Completed
                } else {
                    JobState::Failed
                };
                record.mechanistic_receipt = serde_json::to_value(&receipt).ok();
                if record.status == JobState::Completed
                    && receipt.readiness.evidence_available
                    && receipt
                        .action_results
                        .iter()
                        .all(|result| result.status == MechanisticReceiptStatusV1::Completed)
                {
                    let _ = write_cache_record(
                        &self.paths,
                        &CacheRecord {
                            schema: BROKER_CACHE_SCHEMA.to_string(),
                            kind: "evidence".to_string(),
                            key: work_digest.clone(),
                            revision: request.expected_revision.clone(),
                            created_at_epoch_seconds: epoch_seconds(),
                            last_used_epoch_seconds: epoch_seconds(),
                            receipt: serde_json::to_value(&receipt).unwrap_or(Value::Null),
                            record_digest: String::new(),
                            source_job_id: Some(job_id.clone()),
                            source_receipt_digest: source_digest,
                            duration_millis: receipt.elapsed_millis,
                            constituent_keys: Vec::new(),
                        },
                    );
                }
            }
            Ok(Err(error)) => {
                record.status = if cancellation.is_cancelled() {
                    JobState::Cancelled
                } else {
                    JobState::Failed
                };
                record.error = Some(bounded(&format!("{error:#}"), 16 * 1024));
            }
            Err(error) => {
                record.status = JobState::Interrupted;
                record.error = Some(bounded(&error.to_string(), 16 * 1024));
            }
        }
        record.repository_may_be_partial = false;
        let _ = store.save_atomic(&record);
        self.evidence.lock().await.remove(&work_digest);
        self.jobs_by_id.lock().await.remove(&job_id);
    }

    async fn job_status(&self, client_id: &str, job_id: &str, wait: u8) -> Result<Value> {
        validate_job_id(job_id)?;
        let key = self.jobs_by_id.lock().await.get(job_id).cloned();
        let broker = if let Some(key) = key {
            let mut evidence = self.evidence.lock().await;
            evidence.get_mut(&key).map(|shared| {
                shared.consumers.insert(client_id.to_string());
                json!({
                    "disposition": "attached",
                    "work_digest": key,
                    "source_job_id": shared.job_id,
                    "consumer_count": shared.consumers.len(),
                    "avoided_execution_millis": shared.started.elapsed().as_millis() as u64,
                })
            })
        } else {
            None
        };
        let record = JobStore::new(&self.repo).wait(job_id, wait).await?;
        if record.kind == JobKind::Step {
            bail!("step jobs must be read through step_status")
        }
        let mut value = serde_json::to_value(record)?;
        if let (Some(object), Some(broker)) = (value.as_object_mut(), broker) {
            object.insert("broker".to_string(), broker);
        }
        Ok(value)
    }

    async fn cancel(&self, client_id: &str, job_id: &str) -> Result<Value> {
        validate_job_id(job_id)?;
        let store = JobStore::new(&self.repo);
        let record = store.load(job_id).await?;
        if record.status.is_terminal() {
            return Ok(json!({
                "schema": "codex-team.mcp_delegation_cancel.v1",
                "job_id": job_id,
                "status": record.status,
                "already_terminal": true,
                "observed_pid_signalled": false,
            }));
        }
        let key = self
            .jobs_by_id
            .lock()
            .await
            .get(job_id)
            .cloned()
            .context("running delegation is not controlled by this broker")?;
        let mut evidence = self.evidence.lock().await;
        let shared = evidence
            .get_mut(&key)
            .context("shared delegation is no longer active")?;
        shared.consumers.remove(client_id);
        let remaining = shared.consumers.len();
        if remaining == 0 {
            shared.cancellation.cancel();
        }
        Ok(json!({
            "schema": "codex-team.mcp_delegation_cancel.v1",
            "job_id": job_id,
            "status": if remaining == 0 { "cancellation_requested" } else { "detached" },
            "already_terminal": false,
            "observed_pid_signalled": false,
            "broker": {
                "disposition": "attached",
                "work_digest": key,
                "source_job_id": job_id,
                "consumer_count": remaining,
                "avoided_execution_millis": 0,
            }
        }))
    }

    async fn feedback(&self, arguments: Value) -> Result<Value> {
        let args: BrokerFeedbackArgs = serde_json::from_value(arguments)?;
        validate_job_id(&args.job_id)?;
        validate_digest(&args.expected_revision)?;
        if !matches!(args.outcome.as_str(), "success" | "failure" | "correction") {
            bail!("feedback outcome is unsupported")
        }
        if args.outcome == "correction" && !args.human_confirmed {
            bail!("human correction feedback requires exact human confirmation")
        }
        if args.category.is_empty()
            || args.category.len() > 128
            || !args.category.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
            })
        {
            bail!("feedback category must be 1-128 safe ASCII bytes")
        }
        let paths = Paths::new(&self.repo)?;
        if repository_revision(&paths)? != args.expected_revision {
            bail!("repository revision is stale; refresh status before feedback")
        }
        let invalidated = if args.outcome == "success" {
            0
        } else {
            invalidate_cache_for_job(&self.paths, &args.job_id)?
        };
        let store = JobStore::new(&self.repo);
        if let Ok(mut record) = store.load_sync(&args.job_id) {
            if record.kind == JobKind::Step || !record.status.is_terminal() {
                bail!("delegation feedback requires one terminal delegation job")
            }
            if let Some(existing) = &record.delegation_feedback {
                let exact = existing.get("outcome").and_then(Value::as_str)
                    == Some(args.outcome.as_str())
                    && existing.get("category").and_then(Value::as_str)
                        == Some(args.category.as_str())
                    && existing.get("expected_revision").and_then(Value::as_str)
                        == Some(args.expected_revision.as_str());
                if exact {
                    return Ok(existing.clone());
                }
                if args.outcome != "correction" || !args.human_confirmed {
                    bail!("feedback already exists; only an exact human correction may override it")
                }
            }
            let mut feedback = json!({
                "schema": "codex-team.delegation_feedback.v1",
                "job_id": args.job_id,
                "expected_revision": args.expected_revision,
                "outcome": args.outcome,
                "category": args.category,
                "source": if args.human_confirmed { "human" } else { "agent" },
                "recorded_at": chrono::Utc::now().to_rfc3339(),
                "broker_cache_entries_invalidated": invalidated,
            });
            let digest = crate::json_identity::digest_value(&feedback)?;
            feedback
                .as_object_mut()
                .expect("feedback is an object")
                .insert("digest".to_string(), Value::String(digest));
            if args.outcome != "success" {
                if let Some(request_digest) = record
                    .mechanistic_receipt
                    .as_ref()
                    .and_then(|receipt| receipt.get("request_digest"))
                    .and_then(Value::as_str)
                {
                    crate::mechanistic::invalidate_evidence_cache_for_request(request_digest);
                }
            }
            if let Some(manifest_digest) = &record.delegation_manifest_digest {
                if let Some(vault) = crate::self_improvement::LearningVault::existing_machine()? {
                    match args.outcome.as_str() {
                        "correction" => {
                            crate::self_improvement::record_delegation_human_correction(
                                &vault,
                                manifest_digest,
                            )?;
                        }
                        "failure" => {
                            crate::self_improvement::record_delegation_failure(
                                &vault,
                                manifest_digest,
                                &args.expected_revision,
                            )?;
                        }
                        _ => {}
                    }
                }
            }
            record.delegation_feedback = Some(feedback.clone());
            record.updated_at = chrono::Utc::now().to_rfc3339();
            store.save_atomic(&record)?;
            return Ok(feedback);
        }
        Ok(json!({
            "schema": "codex-team.delegation_feedback.v1",
            "job_id": args.job_id,
            "expected_revision": args.expected_revision,
            "outcome": args.outcome,
            "category": args.category,
            "source": if args.human_confirmed { "human" } else { "agent" },
            "broker_cache_entries_invalidated": invalidated,
            "recorded_at": chrono::Utc::now().to_rfc3339(),
        }))
    }

    async fn detach(&self, client_id: &str) -> Result<Value> {
        self.clients.lock().await.remove(client_id);
        let keys = self
            .evidence
            .lock()
            .await
            .iter()
            .filter(|(_, shared)| shared.consumers.contains(client_id))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in keys {
            let mut evidence = self.evidence.lock().await;
            if let Some(shared) = evidence.get_mut(&key) {
                shared.consumers.remove(client_id);
                if shared.consumers.is_empty() {
                    shared.cancellation.cancel();
                }
            }
        }
        Ok(json!({
            "schema": "codex-team.broker_detach.v1",
            "status": "detached"
        }))
    }
}

pub fn serve(repo: PathBuf, owner_nonce: String) -> Result<()> {
    let repo = fs::canonicalize(repo)?;
    let paths = BrokerPaths::new(&repo)?;
    paths.ensure_private()?;
    validate_digest(&owner_nonce)?;
    let mut owner = read_owner(&paths)?.context("broker owner record is missing")?;
    if owner.nonce != owner_nonce || owner.repository_hash != paths.repository_hash {
        bail!("broker owner nonce does not match the elected runtime");
    }
    owner.process_id = std::process::id();
    owner.start_token = broker_process_start_token(owner.process_id).ok();
    owner.binary_digest = file_digest(&std::env::current_exe()?)?;
    write_private_atomic(&paths.owner, &serde_json::to_vec(&owner)?)?;
    recover_interrupted_broker_jobs(&repo)?;
    if let Ok(metadata) = fs::symlink_metadata(&paths.socket) {
        if !metadata.file_type().is_socket() {
            bail!("broker socket path is not a Unix socket");
        }
        fs::remove_file(&paths.socket)?;
    }
    let token = paths.token()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async {
        let listener = UnixListener::bind(&paths.socket)?;
        set_private_file_permissions(&paths.socket)?;
        let coordinator = Arc::new(BrokerCoordinator::new(
            repo,
            paths.clone(),
            owner_nonce.clone(),
        ));
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (stream, _) = accepted?;
                    let coordinator = coordinator.clone();
                    let token = token.clone();
                    tokio::spawn(async move {
                        let _ = handle_connection(stream, coordinator, token).await;
                    });
                }
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    if coordinator.should_stop().await {
                        break;
                    }
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    });
    let _ = fs::remove_file(&paths.socket);
    let _ = remove_owner_if_nonce(&paths, &owner_nonce);
    result
}

fn recover_interrupted_broker_jobs(repo: &Path) -> Result<()> {
    let store = JobStore::new(repo);
    for mut record in store.records()? {
        if record.status == JobState::Running && record.job_id.starts_with("broker-") {
            record.status = JobState::Interrupted;
            record.updated_at = chrono::Utc::now().to_rfc3339();
            record.error =
                Some("the owning mechanistic broker stopped while work was active".into());
            record.repository_may_be_partial = false;
            store.save_atomic(&record)?;
        }
    }
    Ok(())
}

pub fn status(repo: &Path) -> Result<Value> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let client = BrokerClient::new(repo)?;
    let status = runtime.block_on(client.status_if_running());
    runtime.block_on(client.detach());
    Ok(status)
}

pub fn stop(repo: &Path) -> Result<Value> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let client = BrokerClient::new(repo)?;
    match inspect_owner(&client.paths)? {
        OwnerInspection::Absent | OwnerInspection::Stale => Ok(json!({
            "schema": "codex-team.broker_stop.v1",
            "status": "already_stopped"
        })),
        OwnerInspection::Live => runtime
            .block_on(client.call_existing(BrokerOperation::Stop))
            .map_err(anyhow::Error::new),
    }
}

async fn handle_connection(
    mut stream: UnixStream,
    coordinator: Arc<BrokerCoordinator>,
    token: String,
) -> Result<()> {
    let peer = stream
        .peer_cred()
        .context("inspect broker peer credentials")?;
    if peer.uid() != unsafe { libc::geteuid() } {
        bail!("broker peer is not owned by the current user")
    }
    let mut bytes = Vec::new();
    (&mut stream)
        .take(MAX_WIRE_BYTES + 1)
        .read_to_end(&mut bytes)
        .await?;
    let response = if bytes.len() as u64 > MAX_WIRE_BYTES {
        BrokerResponse::error(
            "BROKER_REQUEST_TOO_LARGE",
            &anyhow::anyhow!("broker request exceeds 1 MiB"),
            false,
        )
    } else {
        match serde_json::from_slice::<BrokerRequest>(&bytes) {
            Ok(request) => coordinator.dispatch(request, &token).await,
            Err(error) => {
                BrokerResponse::error("BROKER_PROTOCOL", &anyhow::Error::new(error), false)
            }
        }
    };
    let bytes = serde_json::to_vec(&response)?;
    stream.write_all(&bytes).await?;
    stream.shutdown().await?;
    Ok(())
}

fn evidence_work_digest(
    request: &MechanisticRequestV1,
    environment_fingerprint: &str,
    isolation_backend: &str,
) -> Result<String> {
    let binary_digest = file_digest(&std::env::current_exe()?)?;
    crate::json_identity::digest_value(&json!({
        "schema": "codex-team.broker_evidence_key.v1",
        "repository_revision": request.expected_revision,
        "actions": request.actions,
        "allow_unisolated_evidence": request.allow_unisolated_evidence,
        "environment_fingerprint": environment_fingerprint,
        "isolation_backend": isolation_backend,
        "binary_digest": binary_digest,
        "protocol": BROKER_WIRE_SCHEMA,
    }))
}

fn new_broker_job_id() -> String {
    let sequence = NEXT_BROKER_JOB_ID.fetch_add(1, Ordering::Relaxed);
    format!("broker-{:x}-{sequence:x}", epoch_nanos())
}

fn save_running_evidence_job(
    repo: &Path,
    job_id: &str,
    request: &MechanisticRequestV1,
    phase: &str,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    JobStore::new(repo).save_atomic(&JobRecord {
        schema: JOB_SCHEMA.to_string(),
        job_id: job_id.to_string(),
        kind: JobKind::DelegationEvidence,
        status: JobState::Running,
        expected_revision: request.expected_revision.clone(),
        started_at: now.clone(),
        updated_at: now,
        outer_process_group: None,
        launch_nonce: None,
        process_provenance: None,
        active_child_process_group: None,
        progress: Some(JobProgress {
            phase: phase.to_string(),
            summary: "shared typed evidence delegation is queued".to_string(),
        }),
        cycle_receipt: None,
        mechanistic_receipt: None,
        delegation_feedback: None,
        delegation_manifest_digest: crate::self_improvement::active_manifest_digest_for_actions(
            &request.actions,
        )
        .ok()
        .flatten(),
        error: None,
        repository_may_be_partial: false,
    })
}

fn save_terminal_evidence_job(
    repo: &Path,
    job_id: &str,
    request: &MechanisticRequestV1,
    receipt: &MechanisticReceiptV1,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    JobStore::new(repo).save_atomic(&JobRecord {
        schema: JOB_SCHEMA.to_string(),
        job_id: job_id.to_string(),
        kind: JobKind::DelegationEvidence,
        status: if receipt.status == MechanisticReceiptStatusV1::Completed {
            JobState::Completed
        } else {
            JobState::Failed
        },
        expected_revision: request.expected_revision.clone(),
        started_at: now.clone(),
        updated_at: now,
        outer_process_group: None,
        launch_nonce: None,
        process_provenance: None,
        active_child_process_group: None,
        progress: Some(JobProgress {
            phase: "finished".to_string(),
            summary: "authoritative shared evidence was reused".to_string(),
        }),
        cycle_receipt: None,
        mechanistic_receipt: Some(serde_json::to_value(receipt)?),
        delegation_feedback: None,
        delegation_manifest_digest: crate::self_improvement::active_manifest_digest_for_actions(
            &request.actions,
        )
        .ok()
        .flatten(),
        error: None,
        repository_may_be_partial: false,
    })
}

fn finish_broker_job_error(store: &JobStore, job_id: &str, status: JobState, message: &str) {
    if let Ok(mut record) = store.load_sync(job_id) {
        record.status = status;
        record.updated_at = chrono::Utc::now().to_rfc3339();
        record.error = Some(bounded(message, 16 * 1024));
        let _ = store.save_atomic(&record);
    }
}

struct DelegationStartBroker<'a> {
    disposition: MechanisticBrokerDispositionV1,
    work_digest: &'a str,
    source_job_id: Option<&'a str>,
    avoided_execution_millis: u64,
    consumer_count: usize,
}

fn delegation_start_receipt(
    job_id: &str,
    request: &MechanisticRequestV1,
    readiness: &ca_automation_protocol::DelegationReadinessV1,
    broker: DelegationStartBroker<'_>,
) -> Value {
    json!({
        "schema": "codex-team.mcp_delegation_start.v1",
        "job_id": job_id,
        "status": "running",
        "kind": "evidence",
        "request_digest": request_digest(request).unwrap_or_default(),
        "expected_revision": request.expected_revision,
        "readiness": readiness,
        "poll_after_ms": 250,
        "broker": {
            "disposition": broker.disposition,
            "work_digest": broker.work_digest,
            "source_job_id": broker.source_job_id,
            "consumer_count": broker.consumer_count,
            "avoided_execution_millis": broker.avoided_execution_millis,
        }
    })
}

fn query_action_digest(
    revision: &str,
    action: &ca_automation_protocol::MechanisticActionV1,
) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"codex-team-broker-query-action-v1\0");
    hasher.update(revision.as_bytes());
    hasher.update(b"\0");
    hasher.update(serde_json::to_vec(action)?);
    Ok(format!("{:x}", hasher.finalize()))
}

fn query_batch_digest(request: &MechanisticRequestV1) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"codex-team-broker-query-batch-v1\0");
    hasher.update(crate::json_identity::value_bytes(&json!({
        "repository_revision": request.expected_revision,
        "actions": request.actions,
    }))?);
    Ok(format!("{:x}", hasher.finalize()))
}

fn canonical_query_receipt_digest(
    request: &MechanisticRequestV1,
    results: &[MechanisticActionResultV1],
) -> Result<String> {
    crate::json_identity::digest_value(&json!({
        "schema": "codex-team.broker_query_source.v1",
        "repository_revision": request.expected_revision,
        "actions": request.actions,
        "results": results,
    }))
}

fn query_capsule(
    request: &MechanisticRequestV1,
    results: &[MechanisticActionResultV1],
    source_receipt_digest: &str,
    broker: Option<&MechanisticBrokerReceiptV1>,
) -> Result<MechanisticEvidenceCapsuleV1> {
    let mut used = 0_usize;
    let mut truncated = false;
    let fragments = results
        .iter()
        .enumerate()
        .map(|(index, result)| {
            let action_kind = request
                .actions
                .get(index)
                .and_then(|action| serde_json::to_value(action).ok())
                .and_then(|value| {
                    value
                        .get("kind")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "query".to_string());
            let mut entries = Vec::new();
            for entry in &result.entries {
                let size = encoded_entry_size(entry);
                if used.saturating_add(size) > MAX_CAPSULE_ENTRY_BYTES {
                    truncated = true;
                    break;
                }
                used = used.saturating_add(size);
                entries.push(entry.clone());
            }
            truncated |= result.truncated || entries.len() != result.entries.len();
            let fragment_truncated = result.truncated || entries.len() != result.entries.len();
            MechanisticEvidenceFragmentV1 {
                action_index: result.action_index,
                action_kind,
                summary: result.summary.clone(),
                entries,
                truncated: fragment_truncated,
            }
        })
        .collect::<Vec<_>>();
    let decisions = results
        .iter()
        .filter_map(|result| result.decision.as_ref())
        .collect::<Vec<_>>();
    let selected_paths = decisions
        .iter()
        .flat_map(|decision| decision.selected_paths.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(64)
        .collect();
    let selected_recipes = decisions
        .iter()
        .flat_map(|decision| decision.selected_recipes.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(8)
        .collect();
    let reason_codes = decisions
        .iter()
        .flat_map(|decision| decision.reason_codes.iter().cloned())
        .chain(std::iter::once("transparent_mechanistic_query".to_string()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(16)
        .collect();
    let confidence = decisions
        .first()
        .map_or(MechanisticConfidenceV1::High, |decision| {
            decision.confidence
        });
    let dependency_impact = decisions
        .iter()
        .find_map(|decision| decision.dependency_selection.clone());
    let uncertainties = if truncated {
        vec!["bounded_capsule_truncation".to_string()]
    } else {
        Vec::new()
    };
    let mut capsule = MechanisticEvidenceCapsuleV1 {
        revision: request.expected_revision.clone(),
        action_digest: query_batch_digest(request)?,
        source_receipt_digest: source_receipt_digest.to_string(),
        confidence,
        selected_paths,
        selected_recipes,
        dependency_impact,
        reason_codes,
        uncertainties,
        fragments,
        truncated,
        broker: broker.cloned(),
        digest: String::new(),
    };
    capsule.digest = crate::json_identity::digest_value(&serde_json::to_value(&capsule)?)?;
    Ok(capsule)
}

pub(crate) fn attach_query_capsule(
    request: &MechanisticRequestV1,
    receipt: &mut MechanisticReceiptV1,
) -> Result<()> {
    compact_query_base_receipt(receipt, MAX_QUERY_BASE_RECEIPT_BYTES)?;
    let source_digest = canonical_query_receipt_digest(request, &receipt.action_results)?;
    if let Some(broker) = receipt.broker.as_mut() {
        broker.source_receipt_digest = Some(source_digest.clone());
    }
    receipt.capsule = Some(query_capsule(
        request,
        &receipt.action_results,
        &source_digest,
        receipt.broker.as_ref(),
    )?);
    compact_query_capsule(receipt, MAX_PUBLIC_BROKER_RECEIPT_BYTES)
}

fn bound_action_result(result: &mut MechanisticActionResultV1, remaining: usize) {
    let mut used = 0_usize;
    result.entries.retain(|entry| {
        let size = encoded_entry_size(entry);
        if used.saturating_add(size) > remaining {
            result.truncated = true;
            false
        } else {
            used = used.saturating_add(size);
            true
        }
    });
}

fn encoded_entry_size(entry: &str) -> usize {
    serde_json::to_vec(entry).map_or(usize::MAX, |encoded| encoded.len().saturating_add(1))
}

fn compact_query_base_receipt(receipt: &mut MechanisticReceiptV1, limit: usize) -> Result<()> {
    while serde_json::to_vec(receipt)?.len() > limit {
        if let Some(result) = receipt
            .action_results
            .iter_mut()
            .rev()
            .find(|result| !result.entries.is_empty())
        {
            result.entries.pop();
            result.truncated = true;
            continue;
        }
        if let Some(decision) = receipt
            .action_results
            .iter_mut()
            .rev()
            .filter_map(|result| result.decision.as_mut())
            .find(|decision| {
                !decision.selected_paths.is_empty()
                    || !decision.selected_recipes.is_empty()
                    || !decision.reason_codes.is_empty()
                    || decision.dependency_selection.is_some()
            })
        {
            if decision.selected_paths.pop().is_some()
                || decision.selected_recipes.pop().is_some()
                || decision.reason_codes.pop().is_some()
            {
                continue;
            }
            decision.dependency_selection = None;
            continue;
        }
        bail!("bounded query receipt metadata exceeds the public receipt ceiling");
    }
    receipt.output_bytes = receipt
        .action_results
        .iter()
        .flat_map(|result| result.entries.iter())
        .map(|entry| encoded_entry_size(entry) as u64)
        .sum();
    Ok(())
}

fn compact_query_capsule(receipt: &mut MechanisticReceiptV1, limit: usize) -> Result<()> {
    while serde_json::to_vec(receipt)?.len() > limit {
        let capsule = receipt
            .capsule
            .as_mut()
            .context("query receipt capsule missing during compaction")?;
        let removed_entry = capsule
            .fragments
            .iter_mut()
            .rev()
            .find_map(|fragment| fragment.entries.pop())
            .is_some();
        if removed_entry {
            capsule.truncated = true;
            continue;
        }
        if capsule.selected_paths.pop().is_some()
            || capsule.selected_recipes.pop().is_some()
            || capsule.reason_codes.pop().is_some()
            || capsule.uncertainties.pop().is_some()
            || capsule.fragments.pop().is_some()
        {
            capsule.truncated = true;
            continue;
        }
        if capsule.dependency_impact.take().is_some() {
            capsule.truncated = true;
            continue;
        }
        bail!("bounded query capsule metadata exceeds the public receipt ceiling");
    }
    if let Some(capsule) = receipt.capsule.as_mut() {
        capsule.digest.clear();
        capsule.digest = receipt_digest(capsule)?;
    }
    Ok(())
}

fn write_cache_record(paths: &BrokerPaths, record: &CacheRecord) -> Result<()> {
    paths.ensure_private().context("secure broker cache root")?;
    let _mutation = CacheMutation::acquire(&paths.cache)?;
    prune_cache_locked(paths, MAX_CACHE_BYTES, MAX_CACHE_AGE)
        .context("prune broker cache before publication")?;
    validate_digest(&record.key)?;
    validate_digest(&record.revision)?;
    validate_digest(&record.source_receipt_digest)?;
    if !matches!(record.kind.as_str(), "query" | "query_receipt" | "evidence") {
        bail!("broker cache record kind is unsupported");
    }
    let directory = paths.cache.join(&record.kind);
    ensure_private_subdirectory(&directory, &paths.cache)
        .context("secure broker cache kind directory")?;
    let path = directory.join(format!("{}.json", record.key));
    let mut record = record.clone();
    record.record_digest = receipt_digest(&record.receipt)?;
    let bytes = serde_json::to_vec(&record)?;
    if bytes.len() as u64 > MAX_WIRE_BYTES {
        bail!("broker cache record exceeds 1 MiB");
    }
    write_private_atomic(&path, &bytes).context("publish broker cache record")?;
    prune_cache_locked(paths, MAX_CACHE_BYTES, MAX_CACHE_AGE)
        .context("prune broker cache after publication")
}

fn read_cache_record(paths: &BrokerPaths, kind: &str, key: &str) -> Result<Option<CacheRecord>> {
    validate_digest(key)?;
    if !paths.cache.exists() {
        return Ok(None);
    }
    let _mutation = CacheMutation::acquire(&paths.cache)?;
    let path = paths.cache.join(kind).join(format!("{key}.json"));
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    require_private_regular_file(&metadata, "broker cache record")?;
    if metadata.len() > MAX_WIRE_BYTES {
        return Ok(None);
    }
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut record: CacheRecord = match serde_json::from_slice(&bytes) {
        Ok(record) => record,
        Err(_) => return Ok(None),
    };
    if record.schema != BROKER_CACHE_SCHEMA
        || record.kind != kind
        || record.key != key
        || validate_digest(&record.revision).is_err()
        || validate_digest(&record.record_digest).is_err()
        || validate_digest(&record.source_receipt_digest).is_err()
        || receipt_digest(&record.receipt).ok().as_deref() != Some(record.record_digest.as_str())
        || record
            .constituent_keys
            .iter()
            .any(|digest| validate_digest(digest).is_err())
        || epoch_seconds().saturating_sub(record.created_at_epoch_seconds) > MAX_CACHE_AGE.as_secs()
    {
        return Ok(None);
    }
    record.last_used_epoch_seconds = epoch_seconds();
    write_private_atomic(&path, &serde_json::to_vec(&record)?)?;
    Ok(Some(record))
}

#[cfg(test)]
fn prune_cache_with_limits(paths: &BrokerPaths, max_bytes: u64, max_age: Duration) -> Result<()> {
    if !paths.cache.exists() {
        return Ok(());
    }
    let _mutation = CacheMutation::acquire(&paths.cache)?;
    prune_cache_locked(paths, max_bytes, max_age)
}

fn prune_cache_locked(paths: &BrokerPaths, max_bytes: u64, max_age: Duration) -> Result<()> {
    let mut records = Vec::new();
    collect_cache_files(&paths.cache, &mut records, 10_000)?;
    let now = epoch_seconds();
    for (path, metadata) in &records {
        if now.saturating_sub(
            metadata
                .modified()?
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        ) > max_age.as_secs()
        {
            let _ = fs::remove_file(path);
        }
    }
    records.retain(|(path, _)| path.exists());
    let mut total = records
        .iter()
        .map(|(_, metadata)| metadata.len())
        .sum::<u64>();
    if total <= max_bytes {
        return Ok(());
    }
    records.sort_by_key(|(_, metadata)| metadata.modified().ok());
    for (path, metadata) in records {
        if total <= max_bytes {
            break;
        }
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        total = total.saturating_sub(metadata.len());
    }
    Ok(())
}

fn collect_cache_files(
    root: &Path,
    output: &mut Vec<(PathBuf, fs::Metadata)>,
    remaining: usize,
) -> Result<()> {
    if !root.exists() || output.len() >= remaining {
        return Ok(());
    }
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("broker cache contains an unsafe directory");
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries.take(remaining.saturating_sub(output.len())) {
        let entry = entry?;
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() {
            bail!("broker cache contains a symlink");
        }
        // Atomic publishers own their in-progress files. They must not be
        // counted or evicted while another client is collecting the cache.
        let name = entry.file_name();
        let temporary = name.to_str().is_some_and(|name| {
            let parts = name.split('.').collect::<Vec<_>>();
            parts.len() == 5
                && parts[0].is_empty()
                && parts[4] == "tmp"
                && parts[1..4]
                    .iter()
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        });
        if metadata.is_file() && temporary {
            continue;
        }
        if metadata.is_dir() {
            collect_cache_files(&path, output, remaining)?;
        } else if metadata.is_file() {
            require_single_link(&metadata, "broker cache record")?;
            output.push((path, metadata));
        }
    }
    Ok(())
}

fn invalidate_cache_for_job(paths: &BrokerPaths, job_id: &str) -> Result<usize> {
    if !paths.cache.exists() {
        return Ok(0);
    }
    let _mutation = CacheMutation::acquire(&paths.cache)?;
    let mut records = Vec::new();
    let mut removed = 0_usize;
    let mut constituent_keys = BTreeSet::new();
    collect_cache_files(&paths.cache, &mut records, 10_000)?;
    for (path, metadata) in records {
        if metadata.len() > MAX_WIRE_BYTES {
            continue;
        }
        let Ok(record) = serde_json::from_slice::<CacheRecord>(&fs::read(&path)?) else {
            continue;
        };
        if record.source_job_id.as_deref() == Some(job_id) {
            constituent_keys.extend(record.constituent_keys);
            fs::remove_file(path)?;
            removed += 1;
        }
    }
    for key in constituent_keys {
        validate_digest(&key)?;
        let path = paths.cache.join("query").join(format!("{key}.json"));
        if path.exists() {
            let metadata = fs::symlink_metadata(&path)?;
            require_private_regular_file(&metadata, "broker query cache record")?;
            fs::remove_file(path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn cache_size(root: &Path) -> Result<u64> {
    if !root.exists() {
        return Ok(0);
    }
    let _mutation = CacheMutation::acquire(root)?;
    let mut records = Vec::new();
    collect_cache_files(root, &mut records, 10_000)?;
    Ok(records.iter().map(|(_, metadata)| metadata.len()).sum())
}

/// Publication, inspection and eviction share this short filesystem section.
/// Atomic replacement may briefly report two links on supported filesystems;
/// observers must inspect only after our publisher has completed the rename.
/// The strict single-link check still rejects links made outside this section.
struct CacheMutation {
    file: File,
}

impl CacheMutation {
    fn acquire(cache: &Path) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
            use std::os::unix::io::AsRawFd;

            let path = cache
                .parent()
                .context("broker cache has no parent")?
                .join("cache-mutation.lock");
            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&path)?;
            let metadata = file.metadata()?;
            require_private_regular_file(&metadata, "broker cache mutation lock")?;
            anyhow::ensure!(
                metadata.uid() == unsafe { libc::geteuid() },
                "broker cache mutation lock has a different owner"
            );
            loop {
                if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
                    break;
                }
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::Interrupted {
                    return Err(error.into());
                }
            }
            let actual = fs::symlink_metadata(path)?;
            require_private_regular_file(&actual, "broker cache mutation lock")?;
            anyhow::ensure!(
                metadata.dev() == actual.dev() && metadata.ino() == actual.ino(),
                "broker cache mutation lock changed while claiming it"
            );
            Ok(Self { file })
        }
        #[cfg(not(unix))]
        {
            let _ = cache;
            bail!("broker cache ownership is unavailable on this platform")
        }
    }
}

impl Drop for CacheMutation {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            // No child uses this transient filesystem claim. An unrelated
            // thread's fork must not extend it beyond the completed section.
            unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

fn inspect_owner(paths: &BrokerPaths) -> Result<OwnerInspection> {
    let Some(owner) = read_owner(paths)? else {
        return Ok(OwnerInspection::Absent);
    };
    if owner.schema != BROKER_OWNER_SCHEMA
        || owner.repository_hash != paths.repository_hash
        || validate_digest(&owner.nonce).is_err()
        || validate_digest(&owner.binary_digest).is_err()
    {
        bail!("broker owner record is malformed");
    }
    if owner.process_id == 0 {
        return if epoch_seconds().saturating_sub(owner.started_at_epoch_seconds) <= 5 {
            Ok(OwnerInspection::Live)
        } else {
            Ok(OwnerInspection::Stale)
        };
    }
    if !process_exists(owner.process_id) {
        return Ok(OwnerInspection::Stale);
    }
    if let Some(expected) = owner.start_token {
        if broker_process_start_token(owner.process_id).ok().as_deref() != Some(expected.as_str()) {
            return Ok(OwnerInspection::Stale);
        }
    }
    Ok(OwnerInspection::Live)
}

fn read_owner(paths: &BrokerPaths) -> Result<Option<OwnerRecord>> {
    let metadata = match fs::symlink_metadata(&paths.owner) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    require_private_regular_file(&metadata, "broker owner record")?;
    if metadata.len() > 64 * 1024 {
        bail!("broker owner record exceeds 64 KiB");
    }
    Ok(Some(serde_json::from_slice(&fs::read(&paths.owner)?)?))
}

fn remove_stale_runtime(paths: &BrokerPaths) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(&paths.socket) {
        if !metadata.file_type().is_socket() {
            bail!("stale broker socket path is unsafe");
        }
        fs::remove_file(&paths.socket)?;
    }
    if paths.owner.exists() {
        let metadata = fs::symlink_metadata(&paths.owner)?;
        require_private_regular_file(&metadata, "stale broker owner record")?;
        fs::remove_file(&paths.owner)?;
    }
    Ok(())
}

fn remove_owner_if_nonce(paths: &BrokerPaths, nonce: &str) -> Result<()> {
    if read_owner(paths)?.is_some_and(|owner| owner.nonce == nonce) {
        fs::remove_file(&paths.owner)?;
    }
    Ok(())
}

fn random_digest() -> Result<String> {
    let mut bytes = [0_u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn environment_fingerprint() -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"codex-team-broker-environment-v1\0");
    hasher.update(std::env::consts::OS.as_bytes());
    hasher.update(b"\0");
    hasher.update(std::env::consts::ARCH.as_bytes());
    for name in [
        "PATH",
        "RUSTUP_TOOLCHAIN",
        "RUSTC_WRAPPER",
        "CARGO_HOME",
        "NODE_OPTIONS",
        "VIRTUAL_ENV",
        "SWIFT_EXEC",
    ] {
        hasher.update(b"\0");
        hasher.update(name.as_bytes());
        hasher.update(b"=");
        if let Some(value) = std::env::var_os(name) {
            hasher.update(value.as_encoded_bytes());
        }
    }
    format!("{:x}", hasher.finalize())
}

fn file_digest(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("broker executable must be a regular non-symlink file");
    }
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn write_private_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = create_private_new(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn create_private_new(path: &Path) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new().create_new(true).write(true).open(path)
    }
}

#[cfg(debug_assertions)]
fn open_private_append(path: &Path) -> Result<File> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        require_private_regular_file(&metadata, "broker debug log")?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        Ok(OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?)
    }
    #[cfg(not(unix))]
    {
        Ok(OpenOptions::new().create(true).append(true).open(path)?)
    }
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    write_private_atomic_with_timestamp(path, bytes, epoch_nanos(), &NEXT_BROKER_TEMP_ID)
}

fn write_private_atomic_with_timestamp(
    path: &Path,
    bytes: &[u8],
    timestamp: u128,
    sequence: &AtomicU64,
) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        require_private_regular_file(&metadata, "broker runtime record")?;
    }
    let parent = path.parent().context("broker record has no parent")?;
    let (temp, file) = reserve_private_atomic_temp(parent, timestamp, sequence)?;
    publish_private_atomic(path, parent, &temp, file, bytes)
}

fn reserve_private_atomic_temp(
    parent: &Path,
    timestamp: u128,
    sequence: &AtomicU64,
) -> Result<(PathBuf, File)> {
    for _ in 0..MAX_ATOMIC_TEMP_ATTEMPTS {
        let candidate =
            atomic_temp_path(parent, timestamp, sequence.fetch_add(1, Ordering::Relaxed));
        match create_private_new(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    bail!("could not reserve a temporary broker record")
}

fn publish_private_atomic(
    path: &Path,
    parent: &Path,
    temp: &Path,
    mut file: File,
    bytes: &[u8],
) -> Result<()> {
    let write_result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    drop(file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(temp);
        return Err(error);
    }
    let result = (|| -> Result<()> {
        fs::rename(temp, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

fn atomic_temp_path(parent: &Path, timestamp: u128, sequence: u64) -> PathBuf {
    parent.join(format!(
        ".{}.{}.{sequence}.tmp",
        std::process::id(),
        timestamp
    ))
}

fn ensure_private_subdirectory(path: &Path, parent: &Path) -> Result<()> {
    if path.parent() != Some(parent) {
        bail!("broker cache subdirectory escaped its parent");
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                bail!("broker cache subdirectory is unsafe");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(path)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    bail!("broker cache subdirectory race produced an unsafe entry");
                }
            }
            Err(error) => return Err(error.into()),
        },
        Err(error) => return Err(error.into()),
    }
    set_private_directory_permissions(path)
}

fn validate_client_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("broker client/job identity is invalid");
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("expected one lowercase SHA-256 digest");
    }
    Ok(())
}

fn require_private_regular_file(metadata: &fs::Metadata, label: &str) -> Result<()> {
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("{label} must be a regular non-symlink file");
    }
    require_single_link(metadata, label)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!("{label} permissions are not private");
        }
    }
    Ok(())
}

fn require_single_link(metadata: &fs::Metadata, label: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            bail!("{label} must not be hardlinked");
        }
    }
    Ok(())
}

fn set_private_directory_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn set_private_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn process_exists(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(target_os = "macos")]
fn broker_process_start_token(process_id: u32) -> Result<String> {
    let process_id = i32::try_from(process_id).context("broker process ID exceeds i32")?;
    if process_id <= 0 {
        bail!("broker process ID is invalid");
    }
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let read = unsafe {
        libc::proc_pidinfo(
            process_id,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            i32::try_from(size).expect("proc_bsdinfo fits in i32"),
        )
    };
    if read != i32::try_from(size).expect("proc_bsdinfo fits in i32") {
        return Err(std::io::Error::last_os_error()).context("inspect broker process identity");
    }
    let info = unsafe { info.assume_init() };
    if info.pbi_pid != u32::try_from(process_id).unwrap_or_default()
        || info.pbi_uid != unsafe { libc::geteuid() }
    {
        bail!("broker process is not owned by the current user");
    }
    Ok(format!(
        "macos:{}:{}",
        info.pbi_start_tvsec, info.pbi_start_tvusec
    ))
}

#[cfg(target_os = "linux")]
fn broker_process_start_token(process_id: u32) -> Result<String> {
    let stat = fs::read_to_string(format!("/proc/{process_id}/stat"))?;
    let fields = stat
        .rsplit_once(") ")
        .context("broker /proc stat is malformed")?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let start = fields
        .get(19)
        .context("broker /proc stat lacks start time")?;
    Ok(format!("linux:{start}"))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn broker_process_start_token(_process_id: u32) -> Result<String> {
    bail!("broker process identity is unsupported on this platform")
}

fn epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn epoch_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn idle_timeout() -> Duration {
    #[cfg(debug_assertions)]
    if let Ok(seconds) = std::env::var("CODEX_TEAM_TEST_BROKER_IDLE_SECONDS") {
        if let Ok(seconds) = seconds.parse::<u64>() {
            return Duration::from_secs(seconds.clamp(1, 3_600));
        }
    }
    DEFAULT_IDLE_TIMEOUT
}

fn receipt_digest(value: &impl Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

fn bounded(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

fn classify_broker_error(error: &anyhow::Error) -> &'static str {
    let text = error.to_string().to_ascii_lowercase();
    if text.contains("revision") && (text.contains("stale") || text.contains("changed")) {
        "STALE_REVISION"
    } else if text.contains("invalid") || text.contains("accepts") || text.contains("requires") {
        "INVALID_ARGUMENT"
    } else {
        "BROKER_EXECUTION_FAILED"
    }
}

fn internal_call_error(error: impl std::fmt::Display) -> BrokerCallError {
    BrokerCallError {
        code: "BROKER_INTERNAL".into(),
        message: bounded(&error.to_string(), 2_048),
        retryable: false,
        safe_local_fallback: false,
    }
}

fn connection_call_error(error: std::io::Error, safe_local_fallback: bool) -> BrokerCallError {
    BrokerCallError {
        code: "BROKER_CONNECTION_FAILED".into(),
        message: bounded(&error.to_string(), 2_048),
        retryable: true,
        safe_local_fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_fixture() -> (PathBuf, BrokerPaths) {
        let home = PathBuf::from("/tmp").join(format!(
            "codex-team-broker-unit-{}-{}",
            std::process::id(),
            epoch_nanos()
        ));
        fs::create_dir(&home).unwrap();
        let repository_hash = "a".repeat(64);
        let runtime_key = repository_hash[..32].to_string();
        let root = home
            .join(".codex-team")
            .join("broker")
            .join("v1")
            .join(&runtime_key);
        let paths = BrokerPaths {
            socket: root.join("s"),
            owner: root.join("owner.json"),
            token: root.join("token"),
            cache: root.join("cache"),
            root,
            repository_hash,
            runtime_key,
        };
        (home, paths)
    }

    fn cache_record() -> CacheRecord {
        let receipt = json!({"status":"completed","entries":["redacted"]});
        CacheRecord {
            schema: BROKER_CACHE_SCHEMA.to_string(),
            kind: "query".to_string(),
            key: "b".repeat(64),
            revision: "c".repeat(64),
            created_at_epoch_seconds: epoch_seconds(),
            last_used_epoch_seconds: epoch_seconds(),
            record_digest: String::new(),
            source_receipt_digest: receipt_digest(&receipt).unwrap(),
            source_job_id: None,
            receipt,
            duration_millis: 7,
            constituent_keys: Vec::new(),
        }
    }

    #[test]
    fn environment_fingerprint_is_stable_and_private() {
        let first = environment_fingerprint();
        let second = environment_fingerprint();
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert!(!first.contains('/'));
    }

    #[test]
    fn broker_wire_is_closed() {
        let request = json!({
            "schema": BROKER_WIRE_SCHEMA,
            "token": "a".repeat(64),
            "client_id": "client-one",
            "repository_hash": "b".repeat(64),
            "binary_digest": "c".repeat(64),
            "environment_fingerprint": "d".repeat(64),
            "operation": "status",
            "extra": true
        });
        assert!(serde_json::from_value::<BrokerRequest>(request).is_err());
    }

    #[tokio::test]
    async fn broker_rejects_incompatible_binary_before_dispatch() {
        let (home, paths) = cache_fixture();
        paths.ensure_private().unwrap();
        let coordinator = Arc::new(BrokerCoordinator::new(
            home.clone(),
            paths.clone(),
            "d".repeat(64),
        ));
        let response = coordinator
            .dispatch(
                BrokerRequest {
                    schema: BROKER_WIRE_SCHEMA.to_string(),
                    token: "a".repeat(64),
                    client_id: "client-one".to_string(),
                    repository_hash: paths.repository_hash.clone(),
                    binary_digest: "f".repeat(64),
                    environment_fingerprint: environment_fingerprint(),
                    operation: BrokerOperation::Status,
                },
                &"a".repeat(64),
            )
            .await;
        assert!(!response.ok);
        assert_eq!(response.error.unwrap().code, "BROKER_AUTH_FAILED");
        fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cache_is_private_and_tampered_records_are_not_reused() {
        use std::os::unix::fs::PermissionsExt;

        let (home, paths) = cache_fixture();
        let record = cache_record();
        write_cache_record(&paths, &record).unwrap();
        assert_eq!(
            fs::metadata(&paths.root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let path = paths
            .cache
            .join("query")
            .join(format!("{}.json", record.key));
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(read_cache_record(&paths, "query", &record.key)
            .unwrap()
            .is_some());

        let mut tampered: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        tampered["receipt"]["entries"] = json!(["poisoned"]);
        fs::write(&path, serde_json::to_vec(&tampered).unwrap()).unwrap();
        assert!(read_cache_record(&paths, "query", &record.key)
            .unwrap()
            .is_none());
        fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn hardlinked_cache_records_are_rejected() {
        let (home, paths) = cache_fixture();
        let record = cache_record();
        write_cache_record(&paths, &record).unwrap();
        let path = paths
            .cache
            .join("query")
            .join(format!("{}.json", record.key));
        fs::hard_link(&path, paths.root.join("cache-link")).unwrap();
        assert!(read_cache_record(&paths, "query", &record.key).is_err());
        assert!(prune_cache_with_limits(&paths, 0, MAX_CACHE_AGE).is_err());
        assert!(path.exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cache_observers_wait_until_the_owned_publication_link_is_removed() {
        let (home, paths) = cache_fixture();
        let record = cache_record();
        write_cache_record(&paths, &record).unwrap();
        let paths = Arc::new(paths);
        let mutation = CacheMutation::acquire(&paths.cache).unwrap();
        let path = paths
            .cache
            .join("query")
            .join(format!("{}.json", record.key));
        let temporary = atomic_temp_path(path.parent().unwrap(), epoch_nanos(), 9000);
        fs::hard_link(&path, &temporary).unwrap();
        let (started, receiver) = std::sync::mpsc::channel();
        let reader_paths = paths.clone();
        let reader = std::thread::spawn(move || {
            started.send(()).unwrap();
            read_cache_record(&reader_paths, "query", &record.key)
        });
        receiver.recv().unwrap();
        let prune_paths = paths.clone();
        let pruner = std::thread::spawn(move || {
            prune_cache_with_limits(&prune_paths, MAX_CACHE_BYTES, MAX_CACHE_AGE)
        });
        // Neither observer may diagnose or evict the publisher's intermediate
        // inode. Their strict checks run only after the held section ends.
        assert!(!reader.is_finished());
        assert!(!pruner.is_finished());
        fs::remove_file(temporary).unwrap();
        drop(mutation);
        assert!(reader.join().unwrap().unwrap().is_some());
        pruner.join().unwrap().unwrap();
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn cache_pruning_enforces_a_strict_size_ceiling() {
        let (home, paths) = cache_fixture();
        let record = cache_record();
        write_cache_record(&paths, &record).unwrap();
        prune_cache_with_limits(&paths, 1, MAX_CACHE_AGE).unwrap();
        assert_eq!(cache_size(&paths.cache).unwrap(), 0);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn cache_pruning_retains_an_atomic_publishers_incomplete_file() {
        let (home, paths) = cache_fixture();
        paths.ensure_private().unwrap();
        let directory = paths.cache.join("query");
        ensure_private_subdirectory(&directory, &paths.cache).unwrap();
        let (temporary, mut file) =
            reserve_private_atomic_temp(&directory, epoch_nanos(), &AtomicU64::new(1)).unwrap();
        file.write_all(b"in-progress cache record").unwrap();
        file.sync_all().unwrap();
        prune_cache_with_limits(&paths, 0, MAX_CACHE_AGE).unwrap();
        assert_eq!(fs::read(&temporary).unwrap(), b"in-progress cache record");
        drop(file);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn cache_publication_and_pruning_can_run_concurrently_without_missing_file_errors() {
        let (home, paths) = cache_fixture();
        paths.ensure_private().unwrap();
        let paths = Arc::new(paths);
        let barrier = Arc::new(std::sync::Barrier::new(5));
        let writers = (1..=4)
            .map(|index| {
                let paths = paths.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || -> Result<()> {
                    let mut record = cache_record();
                    record.key = format!("{index:064x}");
                    barrier.wait();
                    for _ in 0..8 {
                        write_cache_record(&paths, &record)?;
                        // An evicted entry is a cache miss, never a failed query.
                        let _ = read_cache_record(&paths, "query", &record.key)?;
                    }
                    Ok(())
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        for _ in 0..64 {
            prune_cache_with_limits(&paths, 1, MAX_CACHE_AGE).unwrap();
        }
        for writer in writers {
            writer.join().unwrap().unwrap();
        }
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn atomic_publication_survives_concurrent_publishers_in_one_clock_tick() {
        let (home, paths) = cache_fixture();
        paths.ensure_private().unwrap();
        let target = paths.root.join("concurrent.json");
        let timestamp = 1_787_973_000_000_000_000_u128;
        let sequence = Arc::new(AtomicU64::new(1));
        let barrier = Arc::new(std::sync::Barrier::new(16));
        let payloads = (0..16)
            .map(|index| format!("complete-payload-{index}"))
            .collect::<Vec<_>>();
        let writers = payloads
            .iter()
            .cloned()
            .map(|payload| {
                let target = target.clone();
                let sequence = Arc::clone(&sequence);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    write_private_atomic_with_timestamp(
                        &target,
                        payload.as_bytes(),
                        timestamp,
                        &sequence,
                    )
                })
            })
            .collect::<Vec<_>>();
        for writer in writers {
            writer.join().unwrap().unwrap();
        }
        assert_eq!(sequence.load(Ordering::Relaxed), 17);
        let published = fs::read_to_string(&target).unwrap();
        assert!(payloads.contains(&published));
        let temp_prefix = format!(".{}.{timestamp}.", std::process::id());
        assert!(fs::read_dir(&paths.root).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(&temp_prefix)
        }));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn atomic_publication_retries_without_removing_a_foreign_collision() {
        let (home, paths) = cache_fixture();
        paths.ensure_private().unwrap();
        let target = paths.root.join("retry.json");
        let timestamp = 1_787_973_000_000_000_001_u128;
        let sequence = AtomicU64::new(7);
        let collision = atomic_temp_path(&paths.root, timestamp, 7);
        write_private_new(&collision, b"foreign").unwrap();

        write_private_atomic_with_timestamp(&target, b"published", timestamp, &sequence).unwrap();

        assert_eq!(fs::read(&collision).unwrap(), b"foreign");
        assert_eq!(fs::read(&target).unwrap(), b"published");
        assert!(!atomic_temp_path(&paths.root, timestamp, 8).exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn atomic_publication_exhaustion_is_fail_closed() {
        let (home, paths) = cache_fixture();
        paths.ensure_private().unwrap();
        let target = paths.root.join("exhausted.json");
        write_private_new(&target, b"original").unwrap();
        let timestamp = 1_787_973_000_000_000_002_u128;
        let sequence = AtomicU64::new(1);
        let collisions = (1..=MAX_ATOMIC_TEMP_ATTEMPTS as u64)
            .map(|value| atomic_temp_path(&paths.root, timestamp, value))
            .collect::<Vec<_>>();
        for collision in &collisions {
            write_private_new(collision, b"foreign").unwrap();
        }

        let error =
            write_private_atomic_with_timestamp(&target, b"replacement", timestamp, &sequence)
                .unwrap_err();

        assert!(error.to_string().contains("temporary broker record"));
        assert_eq!(fs::read(&target).unwrap(), b"original");
        assert!(collisions
            .iter()
            .all(|path| fs::read(path).unwrap() == b"foreign"));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn atomic_publication_failure_removes_only_the_owned_temporary_file() {
        let (home, paths) = cache_fixture();
        paths.ensure_private().unwrap();
        let target = paths.root.join("publish-failure");
        let timestamp = 1_787_973_000_000_000_003_u128;
        let sequence = AtomicU64::new(1);
        let (temp, file) = reserve_private_atomic_temp(&paths.root, timestamp, &sequence).unwrap();
        fs::create_dir(&target).unwrap();

        let error = publish_private_atomic(&target, &paths.root, &temp, file, b"complete-payload")
            .unwrap_err();

        assert!(!error.to_string().is_empty());
        assert!(target.is_dir());
        assert!(!temp.exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn query_receipt_and_capsule_share_the_public_size_ceiling() {
        let request = MechanisticRequestV1 {
            schema: ca_automation_protocol::MECHANISTIC_REQUEST_SCHEMA.into(),
            expected_revision: "a".repeat(64),
            delegation_policy: ca_automation_protocol::DelegationPolicy::Prefer,
            purpose: MechanisticRequestPurposeV1::TransparentTool,
            allow_unisolated_evidence: false,
            actions: vec![
                ca_automation_protocol::MechanisticActionV1::ListTrackedFiles {
                    prefix: None,
                    max_entries: 500,
                },
            ],
        };
        let mut receipt = MechanisticReceiptV1 {
            schema: MECHANISTIC_RECEIPT_SCHEMA.into(),
            request_digest: "b".repeat(64),
            repository_revision: request.expected_revision.clone(),
            status: MechanisticReceiptStatusV1::Completed,
            readiness: DelegationReadinessV1 {
                queries_available: true,
                evidence_available: false,
                observation_available: true,
                evidence_backend: None,
                reasons: Vec::new(),
            },
            action_results: vec![MechanisticActionResultV1 {
                action_index: 0,
                category: "query".into(),
                status: MechanisticReceiptStatusV1::Completed,
                summary: "bounded tracked files".into(),
                entries: vec!["\\\"\n".repeat(30_000)],
                truncated: false,
                duration_millis: 1,
                decision: None,
            }],
            output_bytes: 90_000,
            started_at: "2026-08-27T00:00:00Z".into(),
            finished_at: "2026-08-27T00:00:01Z".into(),
            elapsed_millis: 1,
            fallback_reason: None,
            broker: None,
            capsule: None,
        };
        compact_query_base_receipt(&mut receipt, MAX_QUERY_BASE_RECEIPT_BYTES).unwrap();
        assert!(serde_json::to_vec(&receipt).unwrap().len() <= MAX_QUERY_BASE_RECEIPT_BYTES);
        let source = canonical_query_receipt_digest(&request, &receipt.action_results).unwrap();
        receipt.capsule =
            Some(query_capsule(&request, &receipt.action_results, &source, None).unwrap());
        compact_query_capsule(&mut receipt, MAX_PUBLIC_BROKER_RECEIPT_BYTES).unwrap();
        assert!(serde_json::to_vec(&receipt).unwrap().len() <= MAX_PUBLIC_BROKER_RECEIPT_BYTES);
        assert_eq!(receipt.capsule.unwrap().digest.len(), 64);
    }
}
