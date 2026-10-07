use base64::Engine as _;
use clap::{Parser, Subcommand};
use futures::StreamExt as _;
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::IpAddr;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const STORE_VERSION: u32 = 1;
const STATE_MAX_BYTES: u64 = 8 * 1024 * 1024;
const KEY_MAX_BYTES: u64 = 4096;
const CATALOG_MAX_BYTES: u64 = 2 * 1024 * 1024;
const CATALOG_PAYLOAD_MAX_BYTES: usize = 1024 * 1024;
const FILE_MAX_BYTES: u64 = 512 * 1024 * 1024;
const RUNTIME_MAX_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_CATALOGS: usize = 32;
const MAX_ENTRIES: usize = 512;
const MAX_FILES: usize = 32;
const MAX_ENTRYPOINTS: usize = 16;
const ENVELOPE_SCHEMA: &str = "ca.external-runtime.catalog-envelope.v1";
const CATALOG_SCHEMA: &str = "ca.external-runtime.catalog.v1";
const RECEIPT_SCHEMA: &str = "ca.external-runtime.receipt.v1";
const CATALOG_FILE: &str = "catalog.json";
const RECEIPT_FILE: &str = ".ca-runtime.json";

#[derive(Parser)]
#[command(
    name = "ca runtime",
    bin_name = "ca runtime",
    about = "Manage explicitly selected signed external runtimes"
)]
struct RawRuntimeArgs {
    #[arg(long, value_name = "NAME")]
    profile: Option<String>,

    #[command(subcommand)]
    command: RuntimeCommand,
}

#[derive(Subcommand)]
enum RuntimeCommand {
    /// Pin one exact catalog source and local Ed25519 trust root
    Trust {
        id: String,
        source: String,
        public_key: PathBuf,
        #[arg(long, value_name = "ID")]
        confirm: String,
    },
    /// Remove catalog trust while retaining installed runtime files
    Untrust {
        id: String,
        #[arg(long, value_name = "ID")]
        confirm: String,
    },
    /// Explicitly fetch and verify one pinned catalog snapshot
    Sync { id: String },
    /// List safe catalog and installed-runtime metadata
    List { id: Option<String> },
    /// Preview exact bytes and destination for install/update or removal
    Preview {
        name: String,
        #[arg(long)]
        remove: bool,
    },
    /// Download, verify, and retain one catalog runtime
    Install {
        name: String,
        #[arg(long, value_name = "RUNTIME_ID")]
        confirm: String,
    },
    /// Download and activate one newer signed catalog runtime
    Update {
        name: String,
        #[arg(long, value_name = "RUNTIME_ID")]
        confirm: String,
    },
    /// Select one already retained and fully reverified version
    Rollback {
        runtime: String,
        #[arg(long, value_name = "VERSION")]
        to: String,
        #[arg(long, value_name = "RUNTIME_ID")]
        confirm: String,
    },
    /// Print one fully reverified managed entrypoint path
    Path { runtime: String, entrypoint: String },
    /// Permanently remove exactly one manager-owned runtime tree
    Remove {
        runtime: String,
        #[arg(long, value_name = "RUNTIME_ID")]
        confirm: String,
    },
}

pub struct RuntimeCliArgs {
    active_profile: crate::runtime::ActiveProfile,
    command: RuntimeCommand,
}

impl std::fmt::Debug for RuntimeCliArgs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeCliArgs")
            .field("active_profile", &self.active_profile)
            .field("command", &self.command.label())
            .finish()
    }
}

impl RuntimeCommand {
    fn label(&self) -> &'static str {
        match self {
            Self::Trust { .. } => "trust",
            Self::Untrust { .. } => "untrust",
            Self::Sync { .. } => "sync",
            Self::List { .. } => "list",
            Self::Preview { .. } => "preview",
            Self::Install { .. } => "install",
            Self::Update { .. } => "update",
            Self::Rollback { .. } => "rollback",
            Self::Path { .. } => "path",
            Self::Remove { .. } => "remove",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("runtime and catalog IDs must be 1-64 lowercase ASCII letters, digits, '-' or '_', starting with a letter")]
    InvalidId,
    #[error("runtime versions must be 1-64 ASCII letters, digits, '.', '-' or '_'")]
    InvalidVersion,
    #[error("runtime operation requires --confirm with the exact ID")]
    Confirmation,
    #[error("runtime catalogs require an HTTPS base URL, or literal-loopback HTTP, without credentials, query, or fragment")]
    InvalidSource,
    #[error("runtime trust key must be a bounded regular non-symlink file containing one base64 Ed25519 public key")]
    InvalidKey,
    #[error("runtime catalog is invalid or exceeds its bound")]
    InvalidCatalog,
    #[error("runtime catalog signature verification failed")]
    Signature,
    #[error("runtime catalog revision is stale or conflicts with the verified revision")]
    StaleRevision,
    #[error("runtime catalog is already trusted with different authority: {0}")]
    DuplicateCatalog(String),
    #[error("runtime catalog is not trusted: {0}")]
    MissingCatalog(String),
    #[error("runtime is not present for this platform in the verified catalog: {0}")]
    MissingRuntime(String),
    #[error("runtime is already installed: {0}")]
    AlreadyInstalled(String),
    #[error("runtime is not installed: {0}")]
    NotInstalled(String),
    #[error("runtime update cannot change its pinned catalog owner")]
    CatalogOwner,
    #[error("runtime version is already current or retained; use rollback for retained versions")]
    ExistingVersion,
    #[error("retained runtime version was not found")]
    MissingVersion,
    #[error("runtime manifest, artifact, or retained installation is invalid")]
    InvalidArtifact,
    #[error("runtime request failed with HTTP {0}")]
    HttpStatus(u16),
    #[error("runtime request timed out")]
    Timeout,
    #[error("runtime source could not be reached")]
    Connect,
    #[error("another runtime mutation is in progress")]
    Busy,
    #[error("runtime manager state is invalid")]
    InvalidState,
    #[error("runtime I/O failed: {0}")]
    Io(String),
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CatalogEnvelope {
    schema: String,
    payload: String,
    signature: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CatalogPayload {
    schema: String,
    catalog_id: String,
    revision: u64,
    entries: Vec<RuntimeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RuntimeEntry {
    runtime_id: String,
    version: String,
    target: String,
    license: String,
    files: Vec<RuntimeFile>,
    entrypoints: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RuntimeFile {
    path: String,
    source: String,
    sha256: String,
    bytes: u64,
    #[serde(default)]
    executable: bool,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct TrustedCatalog {
    source: String,
    public_key: String,
    verified: Option<VerifiedCatalog>,
}

impl std::fmt::Debug for TrustedCatalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrustedCatalog")
            .field("source", &"[PINNED SOURCE REDACTED]")
            .field("public_key", &"[TRUST ROOT REDACTED]")
            .field(
                "verified_revision",
                &self.verified.as_ref().map(|value| value.revision),
            )
            .field(
                "runtime_count",
                &self
                    .verified
                    .as_ref()
                    .map_or(0, |value| value.entries.len()),
            )
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct VerifiedCatalog {
    revision: u64,
    payload_sha256: String,
    entries: Vec<RuntimeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RetainedRuntime {
    version: String,
    digest: String,
    directory: String,
    entry: RuntimeEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct InstalledRuntime {
    catalog_id: String,
    current_digest: String,
    versions: Vec<RetainedRuntime>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ManagerState {
    version: u32,
    revision: u64,
    catalogs: BTreeMap<String, TrustedCatalog>,
    installed: BTreeMap<String, InstalledRuntime>,
}

impl Default for ManagerState {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            revision: 0,
            catalogs: BTreeMap::new(),
            installed: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RuntimeReceipt {
    schema: String,
    catalog_id: String,
    digest: String,
    entry: RuntimeEntry,
}

/// Safe metadata for one installed runtime. Paths, sources, trust roots, and file bytes are omitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedRuntime {
    pub id: String,
    pub version: String,
    pub target: String,
    pub entrypoints: Vec<String>,
    pub installed_bytes: u64,
    pub retained_versions: usize,
}

struct RuntimeStore {
    root: PathBuf,
    path: PathBuf,
}

impl RuntimeStore {
    fn new(root: PathBuf) -> Self {
        let root = normalize_missing_path(&root).unwrap_or(root);
        Self {
            path: root.join("state.json"),
            root,
        }
    }

    fn load(&self) -> Result<ManagerState, RuntimeError> {
        if !self.path.exists() {
            return Ok(ManagerState::default());
        }
        reject_symlink_components(&self.path)?;
        let metadata = std::fs::symlink_metadata(&self.path)
            .map_err(|error| RuntimeError::Io(error.to_string()))?;
        if !metadata.is_file() || metadata.len() > STATE_MAX_BYTES {
            return Err(RuntimeError::InvalidState);
        }
        check_owner_only(&metadata)?;
        let bytes = read_bounded(&self.path, STATE_MAX_BYTES).map_err(RuntimeError::Io)?;
        let state: ManagerState =
            serde_json::from_slice(&bytes).map_err(|_| RuntimeError::InvalidState)?;
        validate_state(&state)?;
        Ok(state)
    }

    fn save(&self, state: &mut ManagerState) -> Result<(), RuntimeError> {
        ensure_private_dir(&self.root)?;
        state.revision = state.revision.wrapping_add(1);
        let bytes = serde_json::to_vec_pretty(state).map_err(|_| RuntimeError::InvalidState)?;
        if bytes.len() as u64 > STATE_MAX_BYTES {
            return Err(RuntimeError::InvalidState);
        }
        let temporary = self.root.join(format!(".state.{}.tmp", Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| {
            let mut file = options
                .open(&temporary)
                .map_err(|error| RuntimeError::Io(error.to_string()))?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|error| RuntimeError::Io(error.to_string()))?;
            std::fs::rename(&temporary, &self.path)
                .map_err(|error| RuntimeError::Io(error.to_string()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }

    fn lock(&self) -> Result<RuntimeLock, RuntimeError> {
        ensure_private_dir(&self.root)?;
        let lock = RuntimeLock::acquire(self.root.join("manager.lock"))?;
        cleanup_abandoned_staging(&self.root)?;
        Ok(lock)
    }

    fn installs_root(&self) -> PathBuf {
        self.root.join("installs")
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockOwner {
    pid: u32,
    token: Uuid,
}

struct RuntimeLock {
    path: PathBuf,
    token: Uuid,
}

struct StagingGuard {
    staging_root: PathBuf,
    staging: PathBuf,
    runtime_root: PathBuf,
    active: bool,
}

impl StagingGuard {
    fn new(staging_root: PathBuf, staging: PathBuf, runtime_root: PathBuf) -> Self {
        Self {
            staging_root,
            staging,
            runtime_root,
            active: true,
        }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if self.active {
            remove_owned_tree(&self.staging_root, &self.staging);
            let _ = std::fs::remove_dir(&self.runtime_root);
        }
    }
}

impl RuntimeLock {
    fn acquire(path: PathBuf) -> Result<Self, RuntimeError> {
        for attempt in 0..2 {
            let token = Uuid::new_v4();
            let owner = LockOwner {
                pid: std::process::id(),
                token,
            };
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(mut file) => {
                    let bytes =
                        serde_json::to_vec(&owner).map_err(|_| RuntimeError::InvalidState)?;
                    file.write_all(&bytes)
                        .and_then(|_| file.sync_all())
                        .map_err(|error| RuntimeError::Io(error.to_string()))?;
                    return Ok(Self { path, token });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempt == 0 => {
                    if stale_lock(&path)? {
                        std::fs::remove_file(&path)
                            .map_err(|error| RuntimeError::Io(error.to_string()))?;
                        continue;
                    }
                    return Err(RuntimeError::Busy);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Err(RuntimeError::Busy);
                }
                Err(error) => return Err(RuntimeError::Io(error.to_string())),
            }
        }
        Err(RuntimeError::Busy)
    }
}

impl Drop for RuntimeLock {
    fn drop(&mut self) {
        let owner = std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<LockOwner>(&bytes).ok());
        if owner.is_some_and(|owner| owner.pid == std::process::id() && owner.token == self.token) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub fn parse_args<I, T>(args: I) -> Result<RuntimeCliArgs, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let raw = RawRuntimeArgs::try_parse_from(args)?;
    let active_profile =
        crate::runtime::ActiveProfile::parse(raw.profile.as_deref()).map_err(|error| {
            <RawRuntimeArgs as clap::CommandFactory>::command()
                .error(clap::error::ErrorKind::InvalidValue, error.to_string())
        })?;
    if matches!(active_profile, crate::runtime::ActiveProfile::Bare) {
        return Err(<RawRuntimeArgs as clap::CommandFactory>::command().error(
            clap::error::ErrorKind::ArgumentConflict,
            "--profile bare is non-persistent; managed runtimes are unavailable",
        ));
    }
    Ok(RuntimeCliArgs {
        active_profile,
        command: raw.command,
    })
}

pub async fn run_cli(cli: RuntimeCliArgs) -> i32 {
    match run_cli_inner(cli).await {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ca runtime: {error}");
            match error {
                RuntimeError::Busy => 4,
                RuntimeError::Timeout | RuntimeError::Connect | RuntimeError::HttpStatus(_) => 6,
                RuntimeError::Io(_) | RuntimeError::InvalidState => 8,
                _ => 2,
            }
        }
    }
}

async fn run_cli_inner(cli: RuntimeCliArgs) -> Result<(), RuntimeError> {
    let paths = crate::runtime::RuntimePaths::discover()
        .map_err(|error| RuntimeError::Io(error.to_string()))?;
    let store = RuntimeStore::new(paths.external_runtimes_dir(&cli.active_profile));
    let active_profile = cli.active_profile.clone();
    match cli.command {
        RuntimeCommand::Trust {
            id,
            source,
            public_key,
            confirm,
        } => trust_catalog(&store, &id, &source, &public_key, &confirm),
        RuntimeCommand::Untrust { id, confirm } => untrust_catalog(&store, &id, &confirm),
        RuntimeCommand::Sync { id } => sync_catalog(&store, &id).await,
        RuntimeCommand::List { id } => {
            let installed = managed_runtimes(&paths, &active_profile)?;
            print_list(&store.load()?, &installed, id.as_deref())
        }
        RuntimeCommand::Preview { name, remove } => {
            let state = store.load()?;
            if remove {
                preview_remove(&store, &state, &name)
            } else {
                let (catalog_id, runtime_id) = split_name(&name)?;
                let entry = catalog_entry(&state, catalog_id, runtime_id)?;
                preview_entry(&store, &state, catalog_id, entry)
            }
        }
        RuntimeCommand::Install { name, confirm } => {
            install_or_update(&store, &name, &confirm, false).await
        }
        RuntimeCommand::Update { name, confirm } => {
            install_or_update(&store, &name, &confirm, true).await
        }
        RuntimeCommand::Rollback {
            runtime,
            to,
            confirm,
        } => rollback(&store, &runtime, &to, &confirm),
        RuntimeCommand::Path {
            runtime,
            entrypoint,
        } => {
            let path = resolve_entrypoint(&paths, &active_profile, &runtime, &entrypoint)?;
            println!("{}", path.display());
            Ok(())
        }
        RuntimeCommand::Remove { runtime, confirm } => remove_runtime(&store, &runtime, &confirm),
    }
}

fn trust_catalog(
    store: &RuntimeStore,
    id: &str,
    source: &str,
    public_key: &Path,
    confirm: &str,
) -> Result<(), RuntimeError> {
    validate_id(id)?;
    require_confirmation(id, confirm)?;
    let source = parse_source_base(source)?.to_string();
    let public_key = load_public_key(public_key)?;
    let _lock = store.lock()?;
    let mut state = store.load()?;
    if let Some(existing) = state.catalogs.get(id) {
        if existing.source == source && existing.public_key == public_key {
            println!("Runtime catalog {id} trust is already pinned");
            return Ok(());
        }
        return Err(RuntimeError::DuplicateCatalog(id.to_string()));
    }
    if state.catalogs.len() >= MAX_CATALOGS {
        return Err(RuntimeError::InvalidState);
    }
    state.catalogs.insert(
        id.to_string(),
        TrustedCatalog {
            source,
            public_key,
            verified: None,
        },
    );
    store.save(&mut state)?;
    println!("Pinned runtime catalog {id}; no catalog or artifacts were fetched");
    Ok(())
}

fn untrust_catalog(store: &RuntimeStore, id: &str, confirm: &str) -> Result<(), RuntimeError> {
    validate_id(id)?;
    require_confirmation(id, confirm)?;
    let _lock = store.lock()?;
    let mut state = store.load()?;
    if state.catalogs.remove(id).is_none() {
        return Err(RuntimeError::MissingCatalog(id.to_string()));
    }
    store.save(&mut state)?;
    println!("Removed runtime catalog {id} trust; installed runtimes were retained");
    Ok(())
}

async fn sync_catalog(store: &RuntimeStore, id: &str) -> Result<(), RuntimeError> {
    validate_id(id)?;
    let _lock = store.lock()?;
    let mut state = store.load()?;
    let trusted = state
        .catalogs
        .get(id)
        .cloned()
        .ok_or_else(|| RuntimeError::MissingCatalog(id.to_string()))?;
    let source = parse_source_base(&trusted.source)?;
    let catalog_url = join_source(&source, CATALOG_FILE)?;
    let client = http_client(Duration::from_secs(30))?;
    let bytes = fetch_bounded(&client, catalog_url, CATALOG_MAX_BYTES).await?;
    let (mut payload, payload_sha256) = verify_catalog(&trusted, id, &bytes)?;
    payload.entries.sort_by(|left, right| {
        left.runtime_id
            .cmp(&right.runtime_id)
            .then_with(|| left.version.cmp(&right.version))
            .then_with(|| left.target.cmp(&right.target))
    });
    let compatible = payload
        .entries
        .into_iter()
        .filter(|entry| entry.target == current_target())
        .collect::<Vec<_>>();
    if let Some(existing) = trusted.verified {
        if existing.revision == payload.revision && existing.payload_sha256 == payload_sha256 {
            println!(
                "Runtime catalog {id} revision {} is already verified",
                payload.revision
            );
            return Ok(());
        }
        if payload.revision <= existing.revision {
            return Err(RuntimeError::StaleRevision);
        }
    }
    state.catalogs.get_mut(id).unwrap().verified = Some(VerifiedCatalog {
        revision: payload.revision,
        payload_sha256,
        entries: compatible,
    });
    store.save(&mut state)?;
    let verified = state.catalogs[id].verified.as_ref().unwrap();
    println!(
        "Verified runtime catalog {id} revision {} ({} compatible runtimes); no artifacts were fetched",
        verified.revision,
        verified.entries.len()
    );
    Ok(())
}

fn verify_catalog(
    trusted: &TrustedCatalog,
    id: &str,
    bytes: &[u8],
) -> Result<(CatalogPayload, String), RuntimeError> {
    let envelope: CatalogEnvelope =
        serde_json::from_slice(bytes).map_err(|_| RuntimeError::InvalidCatalog)?;
    if envelope.schema != ENVELOPE_SCHEMA {
        return Err(RuntimeError::InvalidCatalog);
    }
    let payload = base64::engine::general_purpose::STANDARD
        .decode(&envelope.payload)
        .map_err(|_| RuntimeError::InvalidCatalog)?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(&envelope.signature)
        .map_err(|_| RuntimeError::InvalidCatalog)?;
    let public_key = base64::engine::general_purpose::STANDARD
        .decode(&trusted.public_key)
        .map_err(|_| RuntimeError::InvalidState)?;
    if payload.len() > CATALOG_PAYLOAD_MAX_BYTES || signature.len() != 64 || public_key.len() != 32
    {
        return Err(RuntimeError::InvalidCatalog);
    }
    UnparsedPublicKey::new(&ED25519, &public_key)
        .verify(&payload, &signature)
        .map_err(|_| RuntimeError::Signature)?;
    let payload_sha256 = sha256_hex(&payload);
    let payload: CatalogPayload =
        serde_json::from_slice(&payload).map_err(|_| RuntimeError::InvalidCatalog)?;
    validate_catalog_payload(&payload, id)?;
    Ok((payload, payload_sha256))
}

fn validate_catalog_payload(payload: &CatalogPayload, id: &str) -> Result<(), RuntimeError> {
    if payload.schema != CATALOG_SCHEMA
        || payload.catalog_id != id
        || payload.revision == 0
        || payload.entries.is_empty()
        || payload.entries.len() > MAX_ENTRIES
    {
        return Err(RuntimeError::InvalidCatalog);
    }
    let mut identities = BTreeSet::new();
    for entry in &payload.entries {
        validate_entry(entry)?;
        if !identities.insert((entry.runtime_id.clone(), entry.target.clone())) {
            return Err(RuntimeError::InvalidCatalog);
        }
    }
    Ok(())
}

fn validate_entry(entry: &RuntimeEntry) -> Result<(), RuntimeError> {
    validate_id(&entry.runtime_id)?;
    validate_version(&entry.version)?;
    validate_target(&entry.target)?;
    if entry.license.trim().is_empty()
        || entry.license.len() > 128
        || entry.license.chars().any(char::is_control)
        || entry.files.is_empty()
        || entry.files.len() > MAX_FILES
        || entry.entrypoints.is_empty()
        || entry.entrypoints.len() > MAX_ENTRYPOINTS
    {
        return Err(RuntimeError::InvalidCatalog);
    }
    let mut paths = BTreeSet::new();
    let mut sources = BTreeSet::new();
    let mut total = 0_u64;
    for file in &entry.files {
        validate_relative_path(&file.path)?;
        validate_relative_source(&file.source)?;
        validate_sha256(&file.sha256)?;
        if file.bytes == 0
            || file.bytes > FILE_MAX_BYTES
            || !paths.insert(file.path.as_str())
            || !sources.insert(file.source.as_str())
        {
            return Err(RuntimeError::InvalidCatalog);
        }
        total = total
            .checked_add(file.bytes)
            .ok_or(RuntimeError::InvalidCatalog)?;
        if total > RUNTIME_MAX_BYTES {
            return Err(RuntimeError::InvalidCatalog);
        }
    }
    for (name, path) in &entry.entrypoints {
        validate_id(name)?;
        validate_relative_path(path)?;
        if !entry
            .files
            .iter()
            .any(|file| file.path == *path && file.executable)
        {
            return Err(RuntimeError::InvalidCatalog);
        }
    }
    Ok(())
}

fn print_list(
    state: &ManagerState,
    installed: &[ManagedRuntime],
    selected: Option<&str>,
) -> Result<(), RuntimeError> {
    if let Some(id) = selected {
        validate_id(id)?;
        let mut found = false;
        if let Some(catalog) = state.catalogs.get(id) {
            print_catalog(id, catalog);
            found = true;
        }
        if let Some(runtime) = installed.iter().find(|runtime| runtime.id == id) {
            print_installed(runtime);
            found = true;
        }
        if !found {
            return Err(RuntimeError::MissingRuntime(id.to_string()));
        }
        return Ok(());
    }
    if state.catalogs.is_empty() && state.installed.is_empty() {
        println!("No runtime catalogs or managed runtimes configured");
        return Ok(());
    }
    for (id, catalog) in &state.catalogs {
        print_catalog(id, catalog);
    }
    for runtime in installed {
        print_installed(runtime);
    }
    Ok(())
}

fn print_catalog(id: &str, catalog: &TrustedCatalog) {
    if let Some(verified) = &catalog.verified {
        println!(
            "catalog {id}\tverified revision {}\t{} compatible runtimes",
            verified.revision,
            verified.entries.len()
        );
        for entry in &verified.entries {
            println!(
                "  {}/{}\t{}\t{}\t{} bytes\tlicense {}",
                id,
                entry.runtime_id,
                entry.version,
                entry.target,
                entry_total_bytes(entry),
                entry.license
            );
        }
    } else {
        println!("catalog {id}\tpinned\tnot synced");
    }
}

fn print_installed(runtime: &ManagedRuntime) {
    println!(
        "runtime {id}\t{}\t{}\t{} retained versions\t{} bytes",
        runtime.version,
        runtime.target,
        runtime.retained_versions,
        runtime.installed_bytes,
        id = runtime.id,
    );
}

fn preview_entry(
    store: &RuntimeStore,
    state: &ManagerState,
    catalog_id: &str,
    entry: &RuntimeEntry,
) -> Result<(), RuntimeError> {
    let action = match state.installed.get(&entry.runtime_id) {
        Some(installed) => {
            if installed.catalog_id != catalog_id {
                return Err(RuntimeError::CatalogOwner);
            }
            "update"
        }
        None => "install",
    };
    let digest = entry_digest(entry)?;
    let destination = store
        .installs_root()
        .join(&entry.runtime_id)
        .join(version_directory(&entry.version, &digest));
    println!("Runtime {action} preview:");
    println!("  runtime: {}/{}", catalog_id, entry.runtime_id);
    println!("  version: {}", entry.version);
    println!("  target: {}", entry.target);
    println!("  license: {}", entry.license);
    println!("  files: {}", entry.files.len());
    println!("  download/install bytes: {}", entry_total_bytes(entry));
    println!("  destination: {}", destination.display());
    println!(
        "  retained versions after success: {}",
        state
            .installed
            .get(&entry.runtime_id)
            .map_or(1, |runtime| runtime.versions.len() + 1)
    );
    Ok(())
}

fn preview_remove(
    store: &RuntimeStore,
    state: &ManagerState,
    runtime_id: &str,
) -> Result<(), RuntimeError> {
    validate_id(runtime_id)?;
    let installed = state
        .installed
        .get(runtime_id)
        .ok_or_else(|| RuntimeError::NotInstalled(runtime_id.to_string()))?;
    println!("Runtime removal preview:");
    println!("  runtime: {runtime_id}");
    println!("  retained versions: {}", installed.versions.len());
    println!("  bytes removed: {}", installed_total_bytes(installed));
    println!(
        "  exact tree: {}",
        store.installs_root().join(runtime_id).display()
    );
    println!("  catalogs and other runtimes: unchanged");
    Ok(())
}

async fn install_or_update(
    store: &RuntimeStore,
    name: &str,
    confirm: &str,
    update: bool,
) -> Result<(), RuntimeError> {
    let (catalog_id, runtime_id) = split_name(name)?;
    require_confirmation(runtime_id, confirm)?;
    let _lock = store.lock()?;
    let mut state = store.load()?;
    let entry = catalog_entry(&state, catalog_id, runtime_id)?.clone();
    let trusted = state
        .catalogs
        .get(catalog_id)
        .cloned()
        .ok_or_else(|| RuntimeError::MissingCatalog(catalog_id.to_string()))?;
    match (update, state.installed.get(runtime_id)) {
        (false, Some(_)) => return Err(RuntimeError::AlreadyInstalled(runtime_id.to_string())),
        (true, None) => return Err(RuntimeError::NotInstalled(runtime_id.to_string())),
        (true, Some(installed)) if installed.catalog_id != catalog_id => {
            return Err(RuntimeError::CatalogOwner)
        }
        _ => {}
    }
    let digest = entry_digest(&entry)?;
    if state.installed.get(runtime_id).is_some_and(|installed| {
        installed
            .versions
            .iter()
            .any(|version| version.version == entry.version || version.digest == digest)
    }) {
        return Err(RuntimeError::ExistingVersion);
    }
    preview_entry(store, &state, catalog_id, &entry)?;

    let source = parse_source_base(&trusted.source)?;
    let installs_root = store.installs_root();
    ensure_private_dir(&installs_root)?;
    let runtime_root = installs_root.join(runtime_id);
    ensure_private_dir(&runtime_root)?;
    let destination_name = version_directory(&entry.version, &digest);
    let destination = runtime_root.join(&destination_name);
    if destination.exists() {
        return Err(RuntimeError::InvalidArtifact);
    }
    let staging_root = store.root.join("staging");
    ensure_private_dir(&staging_root)?;
    let staging = staging_root.join(Uuid::new_v4().to_string());
    ensure_private_dir(&staging)?;
    let mut staging_guard =
        StagingGuard::new(staging_root.clone(), staging.clone(), runtime_root.clone());
    let client = http_client(Duration::from_secs(120))?;
    let result = async {
        for file in &entry.files {
            let url = join_source(&source, &file.source)?;
            let target = staging.join(&file.path);
            let parent = target.parent().ok_or(RuntimeError::InvalidArtifact)?;
            ensure_private_dir(parent)?;
            download_file(&client, url, &target, file).await?;
        }
        let receipt = RuntimeReceipt {
            schema: RECEIPT_SCHEMA.to_string(),
            catalog_id: catalog_id.to_string(),
            digest: digest.clone(),
            entry: entry.clone(),
        };
        let receipt =
            serde_json::to_vec_pretty(&receipt).map_err(|_| RuntimeError::InvalidArtifact)?;
        write_private_file(&staging.join(RECEIPT_FILE), &receipt, false)?;
        verify_runtime_directory(&staging, catalog_id, &entry, &digest)?;
        std::fs::rename(&staging, &destination)
            .map_err(|error| RuntimeError::Io(error.to_string()))?;
        staging_guard.disarm();
        Ok::<(), RuntimeError>(())
    }
    .await;
    if let Err(error) = result {
        remove_owned_tree(&staging_root, &staging);
        remove_owned_tree(&runtime_root, &destination);
        let _ = std::fs::remove_dir(&runtime_root);
        return Err(error);
    }

    let retained = RetainedRuntime {
        version: entry.version.clone(),
        digest: digest.clone(),
        directory: destination_name,
        entry: entry.clone(),
    };
    if update {
        let installed = state.installed.get_mut(runtime_id).unwrap();
        installed.current_digest = digest;
        installed.versions.push(retained);
        installed.versions.sort_by(|left, right| {
            left.version
                .cmp(&right.version)
                .then(left.digest.cmp(&right.digest))
        });
    } else {
        state.installed.insert(
            runtime_id.to_string(),
            InstalledRuntime {
                catalog_id: catalog_id.to_string(),
                current_digest: digest,
                versions: vec![retained],
            },
        );
    }
    if let Err(error) = store.save(&mut state) {
        remove_owned_tree(&runtime_root, &destination);
        let _ = std::fs::remove_dir(&runtime_root);
        return Err(error);
    }
    println!(
        "{} runtime {runtime_id} {} after exact hash verification; nothing was executed",
        if update { "Updated" } else { "Installed" },
        entry.version
    );
    Ok(())
}

fn rollback(
    store: &RuntimeStore,
    runtime_id: &str,
    version: &str,
    confirm: &str,
) -> Result<(), RuntimeError> {
    validate_id(runtime_id)?;
    validate_version(version)?;
    require_confirmation(runtime_id, confirm)?;
    let _lock = store.lock()?;
    let mut state = store.load()?;
    let installed = state
        .installed
        .get_mut(runtime_id)
        .ok_or_else(|| RuntimeError::NotInstalled(runtime_id.to_string()))?;
    let matches = installed
        .versions
        .iter()
        .filter(|retained| retained.version == version)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(RuntimeError::MissingVersion);
    }
    let retained = matches[0];
    let root = retained_root(store, runtime_id, retained)?;
    verify_runtime_directory(
        &root,
        &installed.catalog_id,
        &retained.entry,
        &retained.digest,
    )?;
    installed.current_digest = retained.digest.clone();
    store.save(&mut state)?;
    println!("Rolled back runtime {runtime_id} to {version}; nothing was executed");
    Ok(())
}

fn remove_runtime(
    store: &RuntimeStore,
    runtime_id: &str,
    confirm: &str,
) -> Result<(), RuntimeError> {
    validate_id(runtime_id)?;
    require_confirmation(runtime_id, confirm)?;
    let _lock = store.lock()?;
    let mut state = store.load()?;
    preview_remove(store, &state, runtime_id)?;
    let record = state
        .installed
        .remove(runtime_id)
        .ok_or_else(|| RuntimeError::NotInstalled(runtime_id.to_string()))?;
    for retained in &record.versions {
        let root = retained_root(store, runtime_id, retained)?;
        verify_runtime_directory(&root, &record.catalog_id, &retained.entry, &retained.digest)?;
    }
    let runtime_root = store.installs_root().join(runtime_id);
    reject_symlink_components(&runtime_root)?;
    let trash_root = store.root.join("removing");
    ensure_private_dir(&trash_root)?;
    let tombstone = trash_root.join(Uuid::new_v4().to_string());
    std::fs::rename(&runtime_root, &tombstone)
        .map_err(|error| RuntimeError::Io(error.to_string()))?;
    if let Err(error) = store.save(&mut state) {
        let _ = std::fs::rename(&tombstone, &runtime_root);
        return Err(error);
    }
    if let Err(error) = std::fs::remove_dir_all(&tombstone) {
        let mut rollback_state = store.load()?;
        rollback_state
            .installed
            .insert(runtime_id.to_string(), record);
        if store.save(&mut rollback_state).is_ok() {
            let _ = std::fs::rename(&tombstone, &runtime_root);
        }
        return Err(RuntimeError::Io(error.to_string()));
    }
    println!("Removed managed runtime {runtime_id}; catalogs and other runtimes were unchanged");
    Ok(())
}

/// Resolve one managed executable only after re-verifying its complete retained installation.
pub fn resolve_entrypoint(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    runtime_id: &str,
    entrypoint: &str,
) -> Result<PathBuf, RuntimeError> {
    let store = RuntimeStore::new(paths.external_runtimes_dir(profile));
    resolve_entrypoint_from_store(&store, runtime_id, entrypoint)
}

fn resolve_entrypoint_from_store(
    store: &RuntimeStore,
    runtime_id: &str,
    entrypoint: &str,
) -> Result<PathBuf, RuntimeError> {
    validate_id(runtime_id)?;
    validate_id(entrypoint)?;
    let state = store.load()?;
    let installed = state
        .installed
        .get(runtime_id)
        .ok_or_else(|| RuntimeError::NotInstalled(runtime_id.to_string()))?;
    let retained = current_retained(installed)?;
    let root = retained_root(store, runtime_id, retained)?;
    verify_runtime_directory(
        &root,
        &installed.catalog_id,
        &retained.entry,
        &retained.digest,
    )?;
    let relative = retained
        .entry
        .entrypoints
        .get(entrypoint)
        .ok_or(RuntimeError::InvalidArtifact)?;
    let path = root.join(relative);
    let path = path
        .canonicalize()
        .map_err(|_| RuntimeError::InvalidArtifact)?;
    if !path.starts_with(&root) {
        return Err(RuntimeError::InvalidArtifact);
    }
    Ok(path)
}

pub fn managed_runtimes(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
) -> Result<Vec<ManagedRuntime>, RuntimeError> {
    let store = RuntimeStore::new(paths.external_runtimes_dir(profile));
    let state = store.load()?;
    state
        .installed
        .iter()
        .map(|(id, installed)| {
            let current = current_retained(installed)?;
            Ok(ManagedRuntime {
                id: id.clone(),
                version: current.version.clone(),
                target: current.entry.target.clone(),
                entrypoints: current.entry.entrypoints.keys().cloned().collect(),
                installed_bytes: installed_total_bytes(installed),
                retained_versions: installed.versions.len(),
            })
        })
        .collect()
}

fn verify_runtime_directory(
    root: &Path,
    catalog_id: &str,
    entry: &RuntimeEntry,
    digest: &str,
) -> Result<(), RuntimeError> {
    reject_symlink_components(root)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|_| RuntimeError::InvalidArtifact)?;
    if !canonical_root.is_dir() {
        return Err(RuntimeError::InvalidArtifact);
    }
    let receipt_path = canonical_regular(&canonical_root.join(RECEIPT_FILE), STATE_MAX_BYTES)?;
    if !receipt_path.starts_with(&canonical_root) {
        return Err(RuntimeError::InvalidArtifact);
    }
    let receipt_bytes =
        read_bounded(&receipt_path, STATE_MAX_BYTES).map_err(|_| RuntimeError::InvalidArtifact)?;
    let receipt: RuntimeReceipt =
        serde_json::from_slice(&receipt_bytes).map_err(|_| RuntimeError::InvalidArtifact)?;
    if receipt.schema != RECEIPT_SCHEMA
        || receipt.catalog_id != catalog_id
        || receipt.digest != digest
        || receipt.entry != *entry
        || entry_digest(entry)? != digest
    {
        return Err(RuntimeError::InvalidArtifact);
    }
    let expected = entry
        .files
        .iter()
        .map(|file| PathBuf::from(&file.path))
        .chain(std::iter::once(PathBuf::from(RECEIPT_FILE)))
        .collect::<BTreeSet<_>>();
    if collect_files(&canonical_root)? != expected {
        return Err(RuntimeError::InvalidArtifact);
    }
    for expected in &entry.files {
        let path = canonical_regular(&canonical_root.join(&expected.path), expected.bytes)?;
        if !path.starts_with(&canonical_root) {
            return Err(RuntimeError::InvalidArtifact);
        }
        let metadata = std::fs::metadata(&path).map_err(|_| RuntimeError::InvalidArtifact)?;
        check_owner_only(&metadata).map_err(|_| RuntimeError::InvalidArtifact)?;
        if metadata.len() != expected.bytes
            || sha256_file(&path, expected.bytes)? != expected.sha256
        {
            return Err(RuntimeError::InvalidArtifact);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode() & 0o777;
            if (expected.executable && mode != 0o700) || (!expected.executable && mode != 0o600) {
                return Err(RuntimeError::InvalidArtifact);
            }
        }
    }
    Ok(())
}

fn validate_state(state: &ManagerState) -> Result<(), RuntimeError> {
    if state.version != STORE_VERSION
        || state.catalogs.len() > MAX_CATALOGS
        || state.installed.len() > MAX_ENTRIES
    {
        return Err(RuntimeError::InvalidState);
    }
    for (id, catalog) in &state.catalogs {
        validate_id(id).map_err(|_| RuntimeError::InvalidState)?;
        parse_source_base(&catalog.source).map_err(|_| RuntimeError::InvalidState)?;
        let key = base64::engine::general_purpose::STANDARD
            .decode(&catalog.public_key)
            .map_err(|_| RuntimeError::InvalidState)?;
        if key.len() != 32 {
            return Err(RuntimeError::InvalidState);
        }
        if let Some(verified) = &catalog.verified {
            if verified.revision == 0
                || verified.entries.len() > MAX_ENTRIES
                || validate_sha256(&verified.payload_sha256).is_err()
            {
                return Err(RuntimeError::InvalidState);
            }
            for entry in &verified.entries {
                validate_entry(entry).map_err(|_| RuntimeError::InvalidState)?;
                if entry.target != current_target() {
                    return Err(RuntimeError::InvalidState);
                }
            }
        }
    }
    for (id, installed) in &state.installed {
        validate_id(id).map_err(|_| RuntimeError::InvalidState)?;
        validate_id(&installed.catalog_id).map_err(|_| RuntimeError::InvalidState)?;
        validate_sha256(&installed.current_digest).map_err(|_| RuntimeError::InvalidState)?;
        if installed.versions.is_empty() || installed.versions.len() > MAX_ENTRIES {
            return Err(RuntimeError::InvalidState);
        }
        let mut digests = BTreeSet::new();
        for retained in &installed.versions {
            validate_version(&retained.version).map_err(|_| RuntimeError::InvalidState)?;
            validate_sha256(&retained.digest).map_err(|_| RuntimeError::InvalidState)?;
            validate_relative_path(&retained.directory).map_err(|_| RuntimeError::InvalidState)?;
            validate_entry(&retained.entry).map_err(|_| RuntimeError::InvalidState)?;
            if retained.entry.runtime_id != *id
                || retained.entry.version != retained.version
                || retained.entry.target != current_target()
                || retained.directory != version_directory(&retained.version, &retained.digest)
                || entry_digest(&retained.entry).map_err(|_| RuntimeError::InvalidState)?
                    != retained.digest
                || !digests.insert(retained.digest.as_str())
            {
                return Err(RuntimeError::InvalidState);
            }
        }
        if !digests.contains(installed.current_digest.as_str()) {
            return Err(RuntimeError::InvalidState);
        }
    }
    Ok(())
}

fn current_retained(installed: &InstalledRuntime) -> Result<&RetainedRuntime, RuntimeError> {
    installed
        .versions
        .iter()
        .find(|retained| retained.digest == installed.current_digest)
        .ok_or(RuntimeError::InvalidState)
}

fn retained_root(
    store: &RuntimeStore,
    runtime_id: &str,
    retained: &RetainedRuntime,
) -> Result<PathBuf, RuntimeError> {
    validate_id(runtime_id)?;
    if retained.directory != version_directory(&retained.version, &retained.digest) {
        return Err(RuntimeError::InvalidState);
    }
    let runtime_root = store.installs_root().join(runtime_id);
    let root = runtime_root.join(&retained.directory);
    reject_symlink_components(&root)?;
    let canonical = root
        .canonicalize()
        .map_err(|_| RuntimeError::InvalidArtifact)?;
    let canonical_runtime = runtime_root
        .canonicalize()
        .map_err(|_| RuntimeError::InvalidArtifact)?;
    if !canonical.starts_with(&canonical_runtime) {
        return Err(RuntimeError::InvalidArtifact);
    }
    Ok(canonical)
}

fn catalog_entry<'a>(
    state: &'a ManagerState,
    catalog_id: &str,
    runtime_id: &str,
) -> Result<&'a RuntimeEntry, RuntimeError> {
    validate_id(catalog_id)?;
    validate_id(runtime_id)?;
    state
        .catalogs
        .get(catalog_id)
        .and_then(|catalog| catalog.verified.as_ref())
        .ok_or_else(|| RuntimeError::MissingCatalog(catalog_id.to_string()))?
        .entries
        .iter()
        .find(|entry| entry.runtime_id == runtime_id)
        .ok_or_else(|| RuntimeError::MissingRuntime(runtime_id.to_string()))
}

fn split_name(name: &str) -> Result<(&str, &str), RuntimeError> {
    let (catalog, runtime) = name.split_once('/').ok_or(RuntimeError::InvalidId)?;
    if runtime.contains('/') {
        return Err(RuntimeError::InvalidId);
    }
    validate_id(catalog)?;
    validate_id(runtime)?;
    Ok((catalog, runtime))
}

fn entry_total_bytes(entry: &RuntimeEntry) -> u64 {
    entry.files.iter().map(|file| file.bytes).sum()
}

fn installed_total_bytes(installed: &InstalledRuntime) -> u64 {
    installed
        .versions
        .iter()
        .map(|retained| entry_total_bytes(&retained.entry))
        .sum()
}

fn entry_digest(entry: &RuntimeEntry) -> Result<String, RuntimeError> {
    serde_json::to_vec(entry)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(|_| RuntimeError::InvalidArtifact)
}

fn version_directory(version: &str, digest: &str) -> String {
    format!("{version}-{digest}")
}

fn current_target() -> String {
    format!(
        "{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH.replace('_', "-")
    )
}

fn parse_source_base(value: &str) -> Result<reqwest::Url, RuntimeError> {
    let url = reqwest::Url::parse(value).map_err(|_| RuntimeError::InvalidSource)?;
    if url.cannot_be_a_base()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().ends_with('/')
    {
        return Err(RuntimeError::InvalidSource);
    }
    match url.scheme() {
        "https" => {}
        "http" => {
            let ip = url
                .host_str()
                .map(|host| host.trim_matches(['[', ']']))
                .and_then(|host| host.parse::<IpAddr>().ok())
                .ok_or(RuntimeError::InvalidSource)?;
            if !ip.is_loopback() {
                return Err(RuntimeError::InvalidSource);
            }
        }
        _ => return Err(RuntimeError::InvalidSource),
    }
    Ok(url)
}

fn join_source(base: &reqwest::Url, relative: &str) -> Result<reqwest::Url, RuntimeError> {
    validate_relative_source(relative)?;
    let joined = base
        .join(relative)
        .map_err(|_| RuntimeError::InvalidSource)?;
    if joined.scheme() != base.scheme()
        || joined.host_str() != base.host_str()
        || joined.port_or_known_default() != base.port_or_known_default()
        || !joined.path().starts_with(base.path())
        || !joined.username().is_empty()
        || joined.password().is_some()
        || joined.query().is_some()
        || joined.fragment().is_some()
    {
        return Err(RuntimeError::InvalidSource);
    }
    Ok(joined)
}

fn http_client(timeout: Duration) -> Result<reqwest::Client, RuntimeError> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(timeout)
        .build()
        .map_err(|_| RuntimeError::Connect)
}

async fn fetch_bounded(
    client: &reqwest::Client,
    url: reqwest::Url,
    max: u64,
) -> Result<Vec<u8>, RuntimeError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(classify_request_error)?;
    if !response.status().is_success() {
        return Err(RuntimeError::HttpStatus(response.status().as_u16()));
    }
    if response.content_length().is_some_and(|length| length > max) {
        return Err(RuntimeError::InvalidCatalog);
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(classify_request_error)?;
        if (bytes.len() as u64).saturating_add(chunk.len() as u64) > max {
            return Err(RuntimeError::InvalidCatalog);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn download_file(
    client: &reqwest::Client,
    url: reqwest::Url,
    target: &Path,
    expected: &RuntimeFile,
) -> Result<(), RuntimeError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(classify_request_error)?;
    if !response.status().is_success() {
        return Err(RuntimeError::HttpStatus(response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > expected.bytes || length > FILE_MAX_BYTES)
    {
        return Err(RuntimeError::InvalidArtifact);
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(target)
        .map_err(|error| RuntimeError::Io(error.to_string()))?;
    let mut stream = response.bytes_stream();
    let mut total = 0_u64;
    let mut hasher = Sha256::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(classify_request_error)?;
        total = total
            .checked_add(chunk.len() as u64)
            .ok_or(RuntimeError::InvalidArtifact)?;
        if total > expected.bytes || total > FILE_MAX_BYTES {
            let _ = std::fs::remove_file(target);
            return Err(RuntimeError::InvalidArtifact);
        }
        file.write_all(&chunk)
            .map_err(|error| RuntimeError::Io(error.to_string()))?;
        hasher.update(&chunk);
    }
    file.sync_all()
        .map_err(|error| RuntimeError::Io(error.to_string()))?;
    let digest = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if total != expected.bytes || digest != expected.sha256 {
        drop(file);
        let _ = std::fs::remove_file(target);
        return Err(RuntimeError::InvalidArtifact);
    }
    drop(file);
    set_private_mode(target, expected.executable)?;
    Ok(())
}

fn classify_request_error(error: reqwest::Error) -> RuntimeError {
    if error.is_timeout() {
        RuntimeError::Timeout
    } else {
        RuntimeError::Connect
    }
}

fn load_public_key(path: &Path) -> Result<String, RuntimeError> {
    let path = canonical_regular(path, KEY_MAX_BYTES).map_err(|_| RuntimeError::InvalidKey)?;
    let bytes = read_bounded(&path, KEY_MAX_BYTES).map_err(|_| RuntimeError::InvalidKey)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| RuntimeError::InvalidKey)?;
    let key = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|_| RuntimeError::InvalidKey)?;
    if key.len() != 32 {
        return Err(RuntimeError::InvalidKey);
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(key))
}

fn collect_files(root: &Path) -> Result<BTreeSet<PathBuf>, RuntimeError> {
    fn visit(
        root: &Path,
        directory: &Path,
        files: &mut BTreeSet<PathBuf>,
    ) -> Result<(), RuntimeError> {
        for entry in std::fs::read_dir(directory).map_err(|_| RuntimeError::InvalidArtifact)? {
            let entry = entry.map_err(|_| RuntimeError::InvalidArtifact)?;
            let kind = entry
                .file_type()
                .map_err(|_| RuntimeError::InvalidArtifact)?;
            if kind.is_symlink() {
                return Err(RuntimeError::InvalidArtifact);
            }
            if kind.is_dir() {
                visit(root, &entry.path(), files)?;
            } else if kind.is_file() {
                files.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .map_err(|_| RuntimeError::InvalidArtifact)?
                        .to_path_buf(),
                );
            } else {
                return Err(RuntimeError::InvalidArtifact);
            }
            if files.len() > MAX_FILES + 1 {
                return Err(RuntimeError::InvalidArtifact);
            }
        }
        Ok(())
    }
    let mut files = BTreeSet::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

fn canonical_regular(path: &Path, max: u64) -> Result<PathBuf, RuntimeError> {
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(RuntimeError::InvalidArtifact);
    }
    let path = path
        .canonicalize()
        .map_err(|_| RuntimeError::InvalidArtifact)?;
    reject_symlink_components(&path)?;
    let metadata = std::fs::metadata(&path).map_err(|_| RuntimeError::InvalidArtifact)?;
    if !metadata.is_file() || metadata.len() > max {
        return Err(RuntimeError::InvalidArtifact);
    }
    Ok(path)
}

fn sha256_file(path: &Path, max: u64) -> Result<String, RuntimeError> {
    let mut file = File::open(path).map_err(|_| RuntimeError::InvalidArtifact)?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| RuntimeError::InvalidArtifact)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or(RuntimeError::InvalidArtifact)?;
        if total > max {
            return Err(RuntimeError::InvalidArtifact);
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(max + 1).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > max {
        return Err("input exceeds its bound".into());
    }
    Ok(bytes)
}

fn write_private_file(path: &Path, bytes: &[u8], executable: bool) -> Result<(), RuntimeError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if executable { 0o700 } else { 0o600 });
    }
    let mut file = options
        .open(path)
        .map_err(|error| RuntimeError::Io(error.to_string()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| RuntimeError::Io(error.to_string()))?;
    set_private_mode(path, executable)
}

fn set_private_mode(path: &Path, executable: bool) -> Result<(), RuntimeError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(if executable { 0o700 } else { 0o600 }),
        )
        .map_err(|error| RuntimeError::Io(error.to_string()))?;
    }
    let _ = executable;
    Ok(())
}

fn ensure_private_dir(path: &Path) -> Result<(), RuntimeError> {
    reject_symlink_components(path)?;
    std::fs::create_dir_all(path).map_err(|error| RuntimeError::Io(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| RuntimeError::Io(error.to_string()))?;
    }
    Ok(())
}

fn check_owner_only(metadata: &std::fs::Metadata) -> Result<(), RuntimeError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(RuntimeError::InvalidState);
        }
    }
    let _ = metadata;
    Ok(())
}

fn reject_symlink_components(path: &Path) -> Result<(), RuntimeError> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if std::fs::symlink_metadata(ancestor)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(RuntimeError::InvalidArtifact);
        }
    }
    Ok(())
}

fn stale_lock(path: &Path) -> Result<bool, RuntimeError> {
    reject_symlink_components(path)?;
    let bytes = read_bounded(path, 4096).map_err(RuntimeError::Io)?;
    let owner: LockOwner = serde_json::from_slice(&bytes).map_err(|_| RuntimeError::Busy)?;
    #[cfg(unix)]
    {
        let pid = i32::try_from(owner.pid).map_err(|_| RuntimeError::Busy)?;
        match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None) {
            Ok(()) | Err(nix::errno::Errno::EPERM) => Ok(false),
            Err(nix::errno::Errno::ESRCH) => Ok(true),
            Err(_) => Err(RuntimeError::Busy),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = owner;
        Err(RuntimeError::Busy)
    }
}

fn remove_owned_tree(parent: &Path, child: &Path) {
    if child.parent() == Some(parent) && child.exists() {
        let _ = std::fs::remove_dir_all(child);
    }
}

fn cleanup_abandoned_staging(root: &Path) -> Result<(), RuntimeError> {
    let staging = root.join("staging");
    if !staging.exists() {
        return Ok(());
    }
    reject_symlink_components(&staging)?;
    for entry in std::fs::read_dir(&staging).map_err(|error| RuntimeError::Io(error.to_string()))? {
        let entry = entry.map_err(|error| RuntimeError::Io(error.to_string()))?;
        let kind = entry
            .file_type()
            .map_err(|error| RuntimeError::Io(error.to_string()))?;
        let name = entry
            .file_name()
            .to_str()
            .and_then(|name| Uuid::parse_str(name).ok());
        if name.is_none() || !kind.is_dir() || kind.is_symlink() {
            return Err(RuntimeError::InvalidArtifact);
        }
        std::fs::remove_dir_all(entry.path())
            .map_err(|error| RuntimeError::Io(error.to_string()))?;
    }
    Ok(())
}

fn validate_relative_path(value: &str) -> Result<(), RuntimeError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > 1024
        || path.is_absolute()
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || path.components().any(|component| {
            !matches!(component, Component::Normal(_))
                || component
                    .as_os_str()
                    .to_str()
                    .is_none_or(|part| part.is_empty() || part.len() > 255)
        })
    {
        return Err(RuntimeError::InvalidCatalog);
    }
    Ok(())
}

fn validate_relative_source(value: &str) -> Result<(), RuntimeError> {
    if value.is_empty()
        || value.len() > 1024
        || value.starts_with('/')
        || value.contains(['\\', '?', '#', '%'])
        || value.chars().any(char::is_control)
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.len() > 255)
    {
        return Err(RuntimeError::InvalidSource);
    }
    Ok(())
}

fn validate_id(value: &str) -> Result<(), RuntimeError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        });
    if valid {
        Ok(())
    } else {
        Err(RuntimeError::InvalidId)
    }
}

fn validate_version(value: &str) -> Result<(), RuntimeError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(RuntimeError::InvalidVersion)
    }
}

fn validate_target(value: &str) -> Result<(), RuntimeError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        Err(RuntimeError::InvalidCatalog)
    } else {
        Ok(())
    }
}

fn validate_sha256(value: &str) -> Result<(), RuntimeError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(RuntimeError::InvalidCatalog)
    }
}

fn require_confirmation(id: &str, confirm: &str) -> Result<(), RuntimeError> {
    if id == confirm {
        Ok(())
    } else {
        Err(RuntimeError::Confirmation)
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn normalize_missing_path(path: &Path) -> Option<PathBuf> {
    let existing = path.ancestors().find(|ancestor| ancestor.exists())?;
    Some(
        existing
            .canonicalize()
            .ok()?
            .join(path.strip_prefix(existing).ok()?),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    #[derive(Clone)]
    struct TestResponse {
        status: &'static str,
        headers: Vec<(&'static str, String)>,
        body: Vec<u8>,
        delay_ms: u64,
    }

    fn ok(body: Vec<u8>) -> TestResponse {
        TestResponse {
            status: "200 OK",
            headers: Vec::new(),
            body,
            delay_ms: 0,
        }
    }

    fn key_pair() -> Ed25519KeyPair {
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap()
    }

    fn write_key(root: &Path, key: &Ed25519KeyPair) -> PathBuf {
        let path = root.join("runtime.pub");
        std::fs::write(
            &path,
            base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref()),
        )
        .unwrap();
        path
    }

    fn entry(version: &str, source: &str, body: &[u8]) -> RuntimeEntry {
        RuntimeEntry {
            runtime_id: "fake".into(),
            version: version.into(),
            target: current_target(),
            license: "MIT".into(),
            files: vec![RuntimeFile {
                path: "bin/fake".into(),
                source: source.into(),
                sha256: sha256_hex(body),
                bytes: body.len() as u64,
                executable: true,
            }],
            entrypoints: BTreeMap::from([("main".into(), "bin/fake".into())]),
        }
    }

    fn signed_catalog(key: &Ed25519KeyPair, revision: u64, entries: Vec<RuntimeEntry>) -> Vec<u8> {
        let payload = serde_json::to_vec(&CatalogPayload {
            schema: CATALOG_SCHEMA.into(),
            catalog_id: "official".into(),
            revision,
            entries,
        })
        .unwrap();
        serde_json::to_vec(&CatalogEnvelope {
            schema: ENVELOPE_SCHEMA.into(),
            payload: base64::engine::general_purpose::STANDARD.encode(&payload),
            signature: base64::engine::general_purpose::STANDARD
                .encode(key.sign(&payload).as_ref()),
        })
        .unwrap()
    }

    async fn response_server(
        responses: Vec<TestResponse>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 2048];
                loop {
                    let read = stream.read(&mut buffer).await.unwrap();
                    assert!(read > 0, "request ended before headers");
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                    assert!(request.len() < 64 * 1024);
                }
                requests.push(
                    String::from_utf8_lossy(&request)
                        .lines()
                        .next()
                        .unwrap()
                        .to_string(),
                );
                let mut headers = format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    response.status,
                    response.body.len()
                );
                for (name, value) in response.headers {
                    headers.push_str(&format!("{name}: {value}\r\n"));
                }
                headers.push_str("\r\n");
                if response.delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(response.delay_ms)).await;
                }
                if stream.write_all(headers.as_bytes()).await.is_err() {
                    continue;
                }
                let _ = stream.write_all(&response.body).await;
                let _ = stream.shutdown().await;
            }
            requests
        });
        (format!("http://{address}/runtimes/"), task)
    }

    #[test]
    fn bare_profile_is_rejected_before_runtime_state() {
        assert!(parse_args(["ca", "--profile", "bare", "list"]).is_err());
    }

    #[test]
    fn debug_never_contains_a_source_url() {
        let cli = parse_args([
            "ca",
            "trust",
            "official",
            "https://downloads.example.invalid/runtimes/",
            "/tmp/root.pub",
            "--confirm",
            "official",
        ])
        .unwrap();
        let debug = format!("{cli:?}");
        assert!(!debug.contains("downloads.example.invalid"));
        assert!(debug.contains("trust"));
    }

    #[test]
    fn sources_are_exact_https_or_literal_loopback_http_bases() {
        assert!(parse_source_base("https://example.invalid/runtimes/").is_ok());
        assert!(parse_source_base("http://127.0.0.1:8080/runtimes/").is_ok());
        assert!(parse_source_base("http://[::1]:8080/runtimes/").is_ok());
        for invalid in [
            "http://example.invalid/runtimes/",
            "http://localhost:8080/runtimes/",
            "https://user@example.invalid/runtimes/",
            "https://example.invalid/runtimes/?token=x",
            "https://example.invalid/runtimes/#x",
            "https://example.invalid/runtimes",
            "file:///tmp/runtimes/",
        ] {
            assert!(parse_source_base(invalid).is_err(), "accepted {invalid}");
        }
        let base = parse_source_base("https://example.invalid/runtimes/").unwrap();
        assert!(join_source(&base, "bin/tool").is_ok());
        for invalid in ["../tool", "/tool", "bin/%2e%2e/tool", "bin/tool?key=x"] {
            assert!(join_source(&base, invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn current_target_uses_the_closed_manifest_grammar() {
        validate_target(&current_target()).unwrap();
        assert!(!current_target().contains('_'));
    }

    #[test]
    fn manifests_reject_traversal_duplicates_and_non_executable_entrypoints() {
        let mut invalid = entry("1.0.0", "artifacts/fake", b"one");
        invalid.files[0].path = "../fake".into();
        assert!(validate_entry(&invalid).is_err());

        let mut invalid = entry("1.0.0", "artifacts/fake", b"one");
        invalid.files[0].executable = false;
        assert!(validate_entry(&invalid).is_err());

        let valid = entry("1.0.0", "artifacts/fake", b"one");
        let payload = CatalogPayload {
            schema: CATALOG_SCHEMA.into(),
            catalog_id: "official".into(),
            revision: 1,
            entries: vec![valid.clone(), valid],
        };
        assert!(validate_catalog_payload(&payload, "official").is_err());
    }

    #[tokio::test]
    async fn signed_lifecycle_is_explicit_private_reverified_and_profile_scoped() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path());
        let profile = crate::runtime::ActiveProfile::Named("work".into());
        let store = RuntimeStore::new(paths.external_runtimes_dir(&profile));
        let key = key_pair();
        let one = b"#!/bin/sh\nprintf one\n".to_vec();
        let two = b"#!/bin/sh\nprintf two\n".to_vec();
        let one_entry = entry("1.0.0", "artifacts/fake-v1", &one);
        let two_entry = entry("2.0.0", "artifacts/fake-v2", &two);
        let (source, requests) = response_server(vec![
            ok(signed_catalog(&key, 1, vec![one_entry.clone()])),
            ok(one.clone()),
            ok(signed_catalog(&key, 2, vec![two_entry.clone()])),
            ok(two.clone()),
        ])
        .await;

        trust_catalog(
            &store,
            "official",
            &source,
            &write_key(root.path(), &key),
            "official",
        )
        .unwrap();
        assert!(!requests.is_finished(), "trust must not fetch");
        sync_catalog(&store, "official").await.unwrap();
        let state = store.load().unwrap();
        preview_entry(
            &store,
            &state,
            "official",
            catalog_entry(&state, "official", "fake").unwrap(),
        )
        .unwrap();
        assert!(!requests.is_finished(), "preview must not fetch artifacts");
        install_or_update(&store, "official/fake", "fake", false)
            .await
            .unwrap();

        let path = resolve_entrypoint(&paths, &profile, "fake", "main").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), one);
        let safe = managed_runtimes(&paths, &profile).unwrap();
        assert_eq!(safe.len(), 1);
        assert_eq!(safe[0].version, "1.0.0");
        assert_eq!(safe[0].entrypoints, vec!["main"]);
        assert_eq!(safe[0].retained_versions, 1);
        assert!(!paths
            .external_runtimes_dir(&crate::runtime::ActiveProfile::Default)
            .exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let state_path = store.root.join("state.json");
            assert_eq!(std::fs::metadata(state_path).unwrap().mode() & 0o777, 0o600);
            assert_eq!(
                std::fs::metadata(&store.root).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }

        let current_root = path.parent().unwrap().parent().unwrap();
        std::fs::write(current_root.join("unexpected"), b"drift").unwrap();
        assert!(matches!(
            resolve_entrypoint(&paths, &profile, "fake", "main"),
            Err(RuntimeError::InvalidArtifact)
        ));
        std::fs::remove_file(current_root.join("unexpected")).unwrap();

        sync_catalog(&store, "official").await.unwrap();
        install_or_update(&store, "official/fake", "fake", true)
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(resolve_entrypoint(&paths, &profile, "fake", "main").unwrap()).unwrap(),
            two
        );
        rollback(&store, "fake", "1.0.0", "fake").unwrap();
        assert_eq!(
            std::fs::read(resolve_entrypoint(&paths, &profile, "fake", "main").unwrap()).unwrap(),
            one
        );
        let safe = managed_runtimes(&paths, &profile).unwrap();
        assert_eq!(safe[0].version, "1.0.0");
        assert_eq!(safe[0].retained_versions, 2);

        remove_runtime(&store, "fake", "fake").unwrap();
        assert!(managed_runtimes(&paths, &profile).unwrap().is_empty());
        assert!(!store.installs_root().join("fake").exists());
        assert!(store.load().unwrap().catalogs.contains_key("official"));

        assert_eq!(
            requests.await.unwrap(),
            vec![
                "GET /runtimes/catalog.json HTTP/1.1",
                "GET /runtimes/artifacts/fake-v1 HTTP/1.1",
                "GET /runtimes/catalog.json HTTP/1.1",
                "GET /runtimes/artifacts/fake-v2 HTTP/1.1",
            ]
        );
    }

    #[tokio::test]
    async fn failed_artifact_and_http_body_leave_no_registered_or_staged_runtime() {
        for response in [
            ok(b"wrong".to_vec()),
            TestResponse {
                status: "401 Unauthorized",
                headers: Vec::new(),
                body: b"DO-NOT-RETAIN-THIS-BODY".to_vec(),
                delay_ms: 0,
            },
        ] {
            let root = tempfile::tempdir().unwrap();
            let paths = crate::runtime::RuntimePaths::from_home(root.path());
            let store = RuntimeStore::new(
                paths.external_runtimes_dir(&crate::runtime::ActiveProfile::Default),
            );
            let key = key_pair();
            let expected = b"expected".to_vec();
            let signed = signed_catalog(&key, 1, vec![entry("1.0.0", "artifacts/fake", &expected)]);
            let (source, requests) = response_server(vec![ok(signed), response]).await;
            trust_catalog(
                &store,
                "official",
                &source,
                &write_key(root.path(), &key),
                "official",
            )
            .unwrap();
            sync_catalog(&store, "official").await.unwrap();
            let error = install_or_update(&store, "official/fake", "fake", false)
                .await
                .unwrap_err();
            assert!(!error.to_string().contains("DO-NOT-RETAIN"));
            assert!(store.load().unwrap().installed.is_empty());
            assert!(!store.installs_root().join("fake").exists());
            let staging = store.root.join("staging");
            assert!(
                !staging.exists() || std::fs::read_dir(staging).unwrap().next().is_none(),
                "failed download left staging content"
            );
            assert_eq!(requests.await.unwrap().len(), 2);
        }
    }

    #[tokio::test]
    async fn signature_redirect_and_stale_revision_fail_without_unsafe_catalog_mutation() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path());
        let store =
            RuntimeStore::new(paths.external_runtimes_dir(&crate::runtime::ActiveProfile::Default));
        let trusted_key = key_pair();
        let wrong_key = key_pair();
        let body = b"body".to_vec();
        let wrong = signed_catalog(&wrong_key, 1, vec![entry("1.0.0", "artifacts/fake", &body)]);
        let valid_two = signed_catalog(
            &trusted_key,
            2,
            vec![entry("2.0.0", "artifacts/fake-v2", &body)],
        );
        let stale_one = signed_catalog(
            &trusted_key,
            1,
            vec![entry("1.0.0", "artifacts/fake-v1", &body)],
        );
        let (source, requests) = response_server(vec![
            ok(wrong),
            TestResponse {
                status: "302 Found",
                headers: vec![("Location", "https://other.invalid/catalog.json".into())],
                body: Vec::new(),
                delay_ms: 0,
            },
            ok(valid_two),
            ok(stale_one),
        ])
        .await;
        trust_catalog(
            &store,
            "official",
            &source,
            &write_key(root.path(), &trusted_key),
            "official",
        )
        .unwrap();
        assert!(matches!(
            sync_catalog(&store, "official").await,
            Err(RuntimeError::Signature)
        ));
        assert!(store.load().unwrap().catalogs["official"]
            .verified
            .is_none());
        assert!(matches!(
            sync_catalog(&store, "official").await,
            Err(RuntimeError::HttpStatus(302))
        ));
        assert!(store.load().unwrap().catalogs["official"]
            .verified
            .is_none());
        sync_catalog(&store, "official").await.unwrap();
        assert!(matches!(
            sync_catalog(&store, "official").await,
            Err(RuntimeError::StaleRevision)
        ));
        assert_eq!(
            store.load().unwrap().catalogs["official"]
                .verified
                .as_ref()
                .unwrap()
                .revision,
            2
        );
        assert_eq!(requests.await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn response_caps_and_timeout_are_enforced_without_body_or_file_retention() {
        let oversized = ok(b"123456789".to_vec());
        let slow = TestResponse {
            status: "200 OK",
            headers: Vec::new(),
            body: b"expected".to_vec(),
            delay_ms: 250,
        };
        let (source, requests) = response_server(vec![oversized, slow]).await;
        let base = parse_source_base(&source).unwrap();
        let client = http_client(Duration::from_millis(40)).unwrap();
        assert!(matches!(
            fetch_bounded(&client, join_source(&base, "catalog.json").unwrap(), 8).await,
            Err(RuntimeError::InvalidCatalog)
        ));
        let output = tempfile::tempdir().unwrap();
        let mut test_entry = entry("1.0.0", "artifacts/fake", b"expected");
        let file = test_entry.files.remove(0);
        let target = output.path().join("fake");
        assert!(matches!(
            download_file(
                &client,
                join_source(&base, "artifacts/fake").unwrap(),
                &target,
                &file
            )
            .await,
            Err(RuntimeError::Timeout)
        ));
        assert!(!target.exists());
        assert_eq!(requests.await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn cancelled_download_drops_staging_and_never_registers_the_runtime() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path());
        let store =
            RuntimeStore::new(paths.external_runtimes_dir(&crate::runtime::ActiveProfile::Default));
        let key = key_pair();
        let body = b"eventual-artifact".to_vec();
        let signed = signed_catalog(&key, 1, vec![entry("1.0.0", "artifacts/fake", &body)]);
        let (source, requests) = response_server(vec![
            ok(signed),
            TestResponse {
                status: "200 OK",
                headers: Vec::new(),
                body,
                delay_ms: 500,
            },
        ])
        .await;
        trust_catalog(
            &store,
            "official",
            &source,
            &write_key(root.path(), &key),
            "official",
        )
        .unwrap();
        sync_catalog(&store, "official").await.unwrap();

        let store_root = store.root.clone();
        let task = tokio::spawn(async move {
            let store = RuntimeStore::new(store_root);
            install_or_update(&store, "official/fake", "fake", false).await
        });
        tokio::time::sleep(Duration::from_millis(60)).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::sleep(Duration::from_millis(20)).await;

        let store =
            RuntimeStore::new(paths.external_runtimes_dir(&crate::runtime::ActiveProfile::Default));
        assert!(store.load().unwrap().installed.is_empty());
        assert!(!store.installs_root().join("fake").exists());
        let staging = store.root.join("staging");
        assert!(
            !staging.exists() || std::fs::read_dir(staging).unwrap().next().is_none(),
            "cancelled download left staging content"
        );
        assert_eq!(requests.await.unwrap().len(), 2);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinked_install_root_is_rejected_before_artifact_traffic() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path());
        let store =
            RuntimeStore::new(paths.external_runtimes_dir(&crate::runtime::ActiveProfile::Default));
        let key = key_pair();
        let body = b"never-requested".to_vec();
        let signed = signed_catalog(&key, 1, vec![entry("1.0.0", "artifacts/fake", &body)]);
        let (source, requests) = response_server(vec![ok(signed)]).await;
        trust_catalog(
            &store,
            "official",
            &source,
            &write_key(root.path(), &key),
            "official",
        )
        .unwrap();
        sync_catalog(&store, "official").await.unwrap();
        assert_eq!(requests.await.unwrap().len(), 1);

        ensure_private_dir(&store.installs_root()).unwrap();
        symlink(outside.path(), store.installs_root().join("fake")).unwrap();
        assert!(matches!(
            install_or_update(&store, "official/fake", "fake", false).await,
            Err(RuntimeError::InvalidArtifact)
        ));
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
        assert!(store.load().unwrap().installed.is_empty());
    }
}
