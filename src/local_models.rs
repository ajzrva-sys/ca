use clap::{Parser, Subcommand};
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::IpAddr;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt as _;
use tokio::sync::Mutex;
use uuid::Uuid;

const STORE_VERSION: u32 = 1;
const STATE_MAX_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MODELS: usize = 256;
const MAX_MODEL_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
const HEADER_BYTES: usize = 24;
const LOG_BYTES_PER_STREAM: usize = 64 * 1024;
const HEALTH_RESPONSE_BYTES: usize = 64 * 1024;
const MIN_CONTEXT_TOKENS: u32 = 512;
const MAX_CONTEXT_TOKENS: u32 = 1_048_576;
const MIN_STARTUP_SECONDS: u64 = 1;
const MAX_STARTUP_SECONDS: u64 = 600;
const MAX_EMBEDDING_INPUTS: usize = 16;
const MAX_EMBEDDING_DIMENSIONS: usize = 4096;
const MAX_EMBEDDING_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_EMBEDDING_VERIFICATION_WORKERS: usize = 1;
const MAX_EMBEDDING_THREADS: usize = 4;

#[derive(Parser)]
#[command(
    name = "ca local",
    bin_name = "ca local",
    about = "Manage explicit local GGUF models and supervised llama.cpp runs"
)]
struct RawLocalArgs {
    #[arg(long, value_name = "NAME")]
    profile: Option<String>,

    #[command(subcommand)]
    command: LocalCommand,
}

#[derive(Subcommand)]
enum LocalCommand {
    /// List safe imported model metadata without reading model bytes
    List,
    /// Reverify and inspect one imported model
    Inspect { id: String },
    /// Import one user-provided GGUF file
    Import {
        path: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long, value_name = "MODEL_ID")]
        confirm: String,
    },
    /// Download one exact GGUF after an explicit metadata preview
    Download {
        id: String,
        url: String,
        #[arg(long)]
        sha256: String,
        #[arg(long)]
        bytes: u64,
        #[arg(long)]
        license: String,
        #[arg(long, value_name = "MODEL_ID")]
        confirm: String,
    },
    /// Remove one exact managed model
    Remove {
        id: String,
        #[arg(long, value_name = "MODEL_ID")]
        confirm: String,
    },
    /// Run one managed GGUF through an exact managed llama-server until Ctrl+C
    Serve {
        model: String,
        #[arg(long, value_name = "RUNTIME_ID")]
        runtime: String,
        #[arg(long, default_value_t = 4096, value_parser = clap::value_parser!(u32).range(MIN_CONTEXT_TOKENS as i64..=MAX_CONTEXT_TOKENS as i64))]
        context_size: u32,
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(MIN_STARTUP_SECONDS..=MAX_STARTUP_SECONDS))]
        startup_timeout: u64,
    },
}

pub struct LocalCliArgs {
    active_profile: crate::runtime::ActiveProfile,
    command: LocalCommand,
}

impl std::fmt::Debug for LocalCliArgs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalCliArgs")
            .field("active_profile", &self.active_profile)
            .field("command", &self.command.label())
            .finish()
    }
}

impl LocalCommand {
    fn label(&self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Inspect { .. } => "inspect",
            Self::Import { .. } => "import",
            Self::Download { .. } => "download",
            Self::Remove { .. } => "remove",
            Self::Serve { .. } => "serve",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LocalModelError {
    #[error("local model IDs must be 1-64 lowercase ASCII letters, digits, '-' or '_', starting with a letter")]
    InvalidId,
    #[error("local model operation requires --confirm with the exact model ID")]
    Confirmation,
    #[error("local model input must be one bounded regular non-symlink GGUF file")]
    InvalidInput,
    #[error("local model has an unsupported or invalid GGUF header")]
    InvalidGguf,
    #[error("local model download requires an HTTPS URL, or literal-loopback HTTP, without credentials, query, or fragment")]
    InvalidSource,
    #[error("local model metadata, declared bytes, hash, or license is invalid")]
    InvalidMetadata,
    #[error("local model already exists: {0}")]
    Duplicate(String),
    #[error("local model is not managed: {0}")]
    Missing(String),
    #[error("local model bytes changed or the managed file is invalid")]
    Drift,
    #[error("local model request failed with HTTP {0}")]
    HttpStatus(u16),
    #[error("local model request timed out")]
    Timeout,
    #[error("local model source could not be reached")]
    Connect,
    #[error("another local model mutation is in progress")]
    Busy,
    #[error("local model state is invalid")]
    InvalidState,
    #[error("local model I/O failed: {0}")]
    Io(String),
    #[error("the managed llama-server runtime could not be resolved or reverified")]
    RuntimeUnavailable,
    #[error("the managed local model could not be resolved or reverified")]
    ModelUnavailable,
    #[error("the required local-server sandbox is unavailable")]
    SandboxUnavailable,
    #[error("the managed local server could not be started")]
    ServerStart,
    #[error("the managed local server exited before it became ready")]
    ServerExited,
    #[error("the managed local server did not become ready before the startup deadline")]
    ServerTimeout,
    #[error("the managed local server returned an invalid health or model response")]
    ServerProtocol,
    #[error("the managed local server did not advertise the selected model")]
    ServerModelMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GgufHeader {
    pub version: u32,
    pub tensor_count: u64,
    pub metadata_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ModelSourceKind {
    UserFile,
    ExactDownload,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ModelRecord {
    id: String,
    relative_path: String,
    sha256: String,
    bytes: u64,
    header: GgufHeader,
    license: String,
    source_kind: ModelSourceKind,
    source_name: String,
}

impl std::fmt::Debug for ModelRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelRecord")
            .field("id", &self.id)
            .field("bytes", &self.bytes)
            .field("sha256", &self.sha256)
            .field("header", &self.header)
            .field("license", &self.license)
            .field("source_kind", &self.source_kind)
            .field("source_name", &self.source_name)
            .field("relative_path", &"[MANAGED PATH REDACTED]")
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ModelState {
    version: u32,
    revision: u64,
    models: BTreeMap<String, ModelRecord>,
}

impl Default for ModelState {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            revision: 0,
            models: BTreeMap::new(),
        }
    }
}

/// Safe metadata for a managed GGUF. The original source and managed path are omitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalModelInfo {
    pub id: String,
    pub bytes: u64,
    pub sha256: String,
    pub gguf_version: u32,
    pub tensor_count: u64,
    pub metadata_count: u64,
    pub license: String,
    pub source: &'static str,
}

struct ModelStore {
    root: PathBuf,
    state_path: PathBuf,
}

impl ModelStore {
    fn new(root: PathBuf) -> Self {
        let root = normalize_missing_path(&root).unwrap_or(root);
        Self {
            state_path: root.join("state.json"),
            root,
        }
    }

    fn load(&self) -> Result<ModelState, LocalModelError> {
        if !self.state_path.exists() {
            return Ok(ModelState::default());
        }
        reject_symlink_components(&self.state_path)?;
        let metadata = std::fs::symlink_metadata(&self.state_path)
            .map_err(|error| LocalModelError::Io(error.to_string()))?;
        if !metadata.is_file() || metadata.len() > STATE_MAX_BYTES {
            return Err(LocalModelError::InvalidState);
        }
        check_owner_only(&metadata)?;
        let bytes = read_bounded(&self.state_path, STATE_MAX_BYTES)
            .map_err(|_| LocalModelError::InvalidState)?;
        let state: ModelState =
            serde_json::from_slice(&bytes).map_err(|_| LocalModelError::InvalidState)?;
        validate_state(&state)?;
        Ok(state)
    }

    fn save(&self, state: &mut ModelState) -> Result<(), LocalModelError> {
        ensure_private_dir(&self.root)?;
        state.revision = state.revision.wrapping_add(1);
        let bytes = serde_json::to_vec_pretty(state).map_err(|_| LocalModelError::InvalidState)?;
        if bytes.len() as u64 > STATE_MAX_BYTES {
            return Err(LocalModelError::InvalidState);
        }
        let temporary = self.root.join(format!(".state.{}.tmp", Uuid::new_v4()));
        write_private_new(&temporary, &bytes)?;
        if let Err(error) = std::fs::rename(&temporary, &self.state_path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(LocalModelError::Io(error.to_string()));
        }
        Ok(())
    }

    fn models_root(&self) -> PathBuf {
        self.root.join("models")
    }

    fn lock(&self) -> Result<ModelLock, LocalModelError> {
        ensure_private_dir(&self.root)?;
        let lock = ModelLock::acquire(self.root.join("manager.lock"))?;
        cleanup_staging(&self.root)?;
        Ok(lock)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockOwner {
    pid: u32,
    token: Uuid,
}

struct ModelLock {
    path: PathBuf,
    token: Uuid,
}

impl ModelLock {
    fn acquire(path: PathBuf) -> Result<Self, LocalModelError> {
        for attempt in 0..2 {
            let token = Uuid::new_v4();
            let bytes = serde_json::to_vec(&LockOwner {
                pid: std::process::id(),
                token,
            })
            .map_err(|_| LocalModelError::InvalidState)?;
            match create_private_file(&path) {
                Ok(mut file) => {
                    file.write_all(&bytes)
                        .and_then(|_| file.sync_all())
                        .map_err(|error| LocalModelError::Io(error.to_string()))?;
                    return Ok(Self { path, token });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempt == 0 => {
                    if stale_lock(&path)? {
                        std::fs::remove_file(&path)
                            .map_err(|error| LocalModelError::Io(error.to_string()))?;
                        continue;
                    }
                    return Err(LocalModelError::Busy);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Err(LocalModelError::Busy)
                }
                Err(error) => return Err(LocalModelError::Io(error.to_string())),
            }
        }
        Err(LocalModelError::Busy)
    }
}

impl Drop for ModelLock {
    fn drop(&mut self) {
        let owner = std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<LockOwner>(&bytes).ok());
        if owner.is_some_and(|owner| owner.pid == std::process::id() && owner.token == self.token) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

struct StageGuard {
    staging_root: PathBuf,
    staging: PathBuf,
    active: bool,
}

impl StageGuard {
    fn new(staging_root: PathBuf, staging: PathBuf) -> Self {
        Self {
            staging_root,
            staging,
            active: true,
        }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for StageGuard {
    fn drop(&mut self) {
        if self.active && self.staging.parent() == Some(self.staging_root.as_path()) {
            let _ = std::fs::remove_dir_all(&self.staging);
        }
    }
}

pub fn parse_args<I, T>(args: I) -> Result<LocalCliArgs, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let raw = RawLocalArgs::try_parse_from(args)?;
    let active_profile =
        crate::runtime::ActiveProfile::parse(raw.profile.as_deref()).map_err(|error| {
            <RawLocalArgs as clap::CommandFactory>::command()
                .error(clap::error::ErrorKind::InvalidValue, error.to_string())
        })?;
    if matches!(active_profile, crate::runtime::ActiveProfile::Bare) {
        return Err(<RawLocalArgs as clap::CommandFactory>::command().error(
            clap::error::ErrorKind::ArgumentConflict,
            "--profile bare is non-persistent; managed local models are unavailable",
        ));
    }
    Ok(LocalCliArgs {
        active_profile,
        command: raw.command,
    })
}

pub async fn run_cli(cli: LocalCliArgs) -> i32 {
    match run_cli_inner(cli).await {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ca local: {error}");
            match error {
                LocalModelError::Busy => 4,
                LocalModelError::Timeout
                | LocalModelError::Connect
                | LocalModelError::HttpStatus(_)
                | LocalModelError::ServerTimeout => 6,
                LocalModelError::Io(_) | LocalModelError::InvalidState => 8,
                _ => 2,
            }
        }
    }
}

async fn run_cli_inner(cli: LocalCliArgs) -> Result<(), LocalModelError> {
    let paths = crate::runtime::RuntimePaths::discover()
        .map_err(|error| LocalModelError::Io(error.to_string()))?;
    let store = ModelStore::new(paths.local_models_dir(&cli.active_profile));
    match cli.command {
        LocalCommand::List => print_list(&store.load()?),
        LocalCommand::Inspect { id } => {
            let (info, _) = resolve_from_store(&store, &id)?;
            print_info(&info, true);
            Ok(())
        }
        LocalCommand::Import { path, id, confirm } => import_model(&store, &path, &id, &confirm),
        LocalCommand::Download {
            id,
            url,
            sha256,
            bytes,
            license,
            confirm,
        } => download_model(&store, &id, &url, &sha256, bytes, &license, &confirm).await,
        LocalCommand::Remove { id, confirm } => remove_model(&store, &id, &confirm),
        LocalCommand::Serve {
            model,
            runtime,
            context_size,
            startup_timeout,
        } => {
            let base_paths = crate::runtime::RuntimePaths::discover()
                .map_err(|error| LocalModelError::Io(error.to_string()))?;
            let mut server = start_managed_server(
                &base_paths,
                &cli.active_profile,
                &runtime,
                &model,
                context_size,
                Duration::from_secs(startup_timeout),
            )
            .await?;
            let info = server.info();
            println!("Managed local server ready");
            println!("  provider: {}", info.provider_id);
            println!("  model: {}", info.model_id);
            println!("  endpoint: {}", info.base_url);
            println!("  ownership: process-owned; Ctrl+C stops the full process group");
            let _ = tokio::signal::ctrl_c().await;
            server.shutdown().await;
            println!("Managed local server stopped");
            Ok(())
        }
    }
}

fn print_list(state: &ModelState) -> Result<(), LocalModelError> {
    if state.models.is_empty() {
        println!("No managed local GGUF models");
        return Ok(());
    }
    for record in state.models.values() {
        print_info(&info_from_record(record), false);
    }
    Ok(())
}

fn print_info(info: &LocalModelInfo, detailed: bool) {
    println!(
        "{}\t{} bytes\tGGUF v{}\t{}\t{}",
        info.id, info.bytes, info.gguf_version, info.source, info.license
    );
    if detailed {
        let recommended = recommended_model_bytes(info.bytes);
        println!("  sha256: {}", info.sha256);
        println!("  tensors: {}", info.tensor_count);
        println!("  metadata entries: {}", info.metadata_count);
        println!("  minimum model bytes: {}", info.bytes);
        println!(
            "  coarse model headroom estimate: {recommended} bytes (excludes KV cache and GPU/runtime overhead)"
        );
    }
}

fn import_model(
    store: &ModelStore,
    source: &Path,
    id: &str,
    confirm: &str,
) -> Result<(), LocalModelError> {
    validate_id(id)?;
    require_confirmation(id, confirm)?;
    let source = canonical_input(source)?;
    let metadata = std::fs::metadata(&source).map_err(|_| LocalModelError::InvalidInput)?;
    validate_model_size(metadata.len())?;
    let source_name = safe_source_name(&source);
    let _lock = store.lock()?;
    let mut state = store.load()?;
    if state.models.contains_key(id) {
        return Err(LocalModelError::Duplicate(id.to_string()));
    }
    preview(
        id,
        metadata.len(),
        "user-provided",
        &store.models_root().join(id),
    );
    let (record, destination) = stage_local_file(
        store,
        &mut state,
        id,
        &source_name,
        "user-provided",
        ModelSourceKind::UserFile,
        |target| copy_and_hash(&source, target, metadata.len(), None),
    )?;
    println!(
        "Imported local model {} ({} bytes, SHA-256 {}); nothing was started",
        record.id, record.bytes, record.sha256
    );
    debug_assert!(destination.exists());
    Ok(())
}

async fn download_model(
    store: &ModelStore,
    id: &str,
    url: &str,
    expected_sha256: &str,
    expected_bytes: u64,
    license: &str,
    confirm: &str,
) -> Result<(), LocalModelError> {
    validate_id(id)?;
    require_confirmation(id, confirm)?;
    validate_sha256(expected_sha256)?;
    validate_model_size(expected_bytes)?;
    validate_license(license)?;
    let url = parse_download_url(url)?;
    let source_name = url
        .path_segments()
        .and_then(|mut parts| parts.next_back())
        .filter(|part| !part.is_empty())
        .map(sanitize_name)
        .ok_or(LocalModelError::InvalidSource)?;
    let _lock = store.lock()?;
    let mut state = store.load()?;
    if state.models.contains_key(id) {
        return Err(LocalModelError::Duplicate(id.to_string()));
    }
    preview(id, expected_bytes, license, &store.models_root().join(id));
    let staging_root = store.root.join("staging");
    ensure_private_dir(&staging_root)?;
    let staging = staging_root.join(Uuid::new_v4().to_string());
    ensure_private_dir(&staging)?;
    let mut guard = StageGuard::new(staging_root, staging.clone());
    let target = staging.join("model.gguf");
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(4 * 60 * 60))
        .build()
        .map_err(|_| LocalModelError::Connect)?;
    let (header, actual_sha256, actual_bytes) =
        download_and_hash(&client, url, &target, expected_bytes, expected_sha256).await?;
    let record = ModelRecord {
        id: id.to_string(),
        relative_path: format!("models/{id}/model.gguf"),
        sha256: actual_sha256,
        bytes: actual_bytes,
        header,
        license: license.to_string(),
        source_kind: ModelSourceKind::ExactDownload,
        source_name,
    };
    commit_staged(store, &mut state, record.clone(), &staging, &mut guard)?;
    println!(
        "Downloaded local model {} ({} bytes, SHA-256 {}); nothing was started",
        record.id, record.bytes, record.sha256
    );
    Ok(())
}

fn stage_local_file<F>(
    store: &ModelStore,
    state: &mut ModelState,
    id: &str,
    source_name: &str,
    license: &str,
    source_kind: ModelSourceKind,
    write: F,
) -> Result<(ModelRecord, PathBuf), LocalModelError>
where
    F: FnOnce(&Path) -> Result<(GgufHeader, String, u64), LocalModelError>,
{
    let staging_root = store.root.join("staging");
    ensure_private_dir(&staging_root)?;
    let staging = staging_root.join(Uuid::new_v4().to_string());
    ensure_private_dir(&staging)?;
    let mut guard = StageGuard::new(staging_root, staging.clone());
    let target = staging.join("model.gguf");
    let (header, sha256, bytes) = write(&target)?;
    let record = ModelRecord {
        id: id.to_string(),
        relative_path: format!("models/{id}/model.gguf"),
        sha256,
        bytes,
        header,
        license: license.to_string(),
        source_kind,
        source_name: source_name.to_string(),
    };
    let destination = commit_staged(store, state, record.clone(), &staging, &mut guard)?;
    Ok((record, destination))
}

fn commit_staged(
    store: &ModelStore,
    state: &mut ModelState,
    record: ModelRecord,
    staging: &Path,
    guard: &mut StageGuard,
) -> Result<PathBuf, LocalModelError> {
    if state.models.len() >= MAX_MODELS {
        return Err(LocalModelError::InvalidState);
    }
    let models_root = store.models_root();
    ensure_private_dir(&models_root)?;
    let destination = models_root.join(&record.id);
    if destination.exists() {
        return Err(LocalModelError::Duplicate(record.id));
    }
    verify_staged(staging, &record)?;
    std::fs::rename(staging, &destination)
        .map_err(|error| LocalModelError::Io(error.to_string()))?;
    guard.disarm();
    state.models.insert(record.id.clone(), record);
    if let Err(error) = store.save(state) {
        let _ = std::fs::remove_dir_all(&destination);
        return Err(error);
    }
    Ok(destination)
}

fn remove_model(store: &ModelStore, id: &str, confirm: &str) -> Result<(), LocalModelError> {
    validate_id(id)?;
    require_confirmation(id, confirm)?;
    let _lock = store.lock()?;
    let mut state = store.load()?;
    let record = state
        .models
        .get(id)
        .cloned()
        .ok_or_else(|| LocalModelError::Missing(id.to_string()))?;
    let (_, path) = verify_record(store, &record)?;
    println!("Local model removal preview:");
    println!("  model: {id}");
    println!("  bytes removed: {}", record.bytes);
    println!("  exact managed tree: {}", path.parent().unwrap().display());
    println!("  source file, providers, defaults, and other models: unchanged");
    let model_root = path.parent().unwrap().to_path_buf();
    let removing_root = store.root.join("removing");
    ensure_private_dir(&removing_root)?;
    let tombstone = removing_root.join(Uuid::new_v4().to_string());
    std::fs::rename(&model_root, &tombstone)
        .map_err(|error| LocalModelError::Io(error.to_string()))?;
    state.models.remove(id);
    if let Err(error) = store.save(&mut state) {
        let _ = std::fs::rename(&tombstone, &model_root);
        return Err(error);
    }
    if let Err(error) = std::fs::remove_dir_all(&tombstone) {
        let mut rollback = store.load()?;
        rollback.models.insert(id.to_string(), record);
        if store.save(&mut rollback).is_ok() {
            let _ = std::fs::rename(&tombstone, &model_root);
        }
        return Err(LocalModelError::Io(error.to_string()));
    }
    println!("Removed managed local model {id}; the source file and providers were unchanged");
    Ok(())
}

pub fn resolve_model(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    id: &str,
) -> Result<(LocalModelInfo, PathBuf), LocalModelError> {
    resolve_from_store(&ModelStore::new(paths.local_models_dir(profile)), id)
}

fn resolve_from_store(
    store: &ModelStore,
    id: &str,
) -> Result<(LocalModelInfo, PathBuf), LocalModelError> {
    resolve_from_store_with_cancellation(store, id, None)
}

fn resolve_from_store_with_cancellation(
    store: &ModelStore,
    id: &str,
    cancelled: Option<&AtomicBool>,
) -> Result<(LocalModelInfo, PathBuf), LocalModelError> {
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(LocalModelError::ModelUnavailable);
    }
    validate_id(id)?;
    let state = store.load()?;
    let record = state
        .models
        .get(id)
        .ok_or_else(|| LocalModelError::Missing(id.to_string()))?;
    let (_, path) = verify_record_with_cancellation(store, record, cancelled)?;
    Ok((info_from_record(record), path))
}

#[allow(dead_code)] // safe embedding boundary for later TUI/status surfaces
pub fn model_infos(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
) -> Result<Vec<LocalModelInfo>, LocalModelError> {
    let store = ModelStore::new(paths.local_models_dir(profile));
    Ok(store
        .load()?
        .models
        .values()
        .map(info_from_record)
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalServerInfo {
    pub provider_id: String,
    pub model_id: String,
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRunOptions {
    pub model_id: String,
    pub runtime_id: String,
    pub context_size: u32,
    pub startup_timeout: Duration,
}

impl LocalRunOptions {
    pub fn validate(&self) -> Result<(), LocalModelError> {
        validate_id(&self.model_id)?;
        validate_id(&self.runtime_id)?;
        if !(MIN_CONTEXT_TOKENS..=MAX_CONTEXT_TOKENS).contains(&self.context_size)
            || !(Duration::from_secs(MIN_STARTUP_SECONDS)
                ..=Duration::from_secs(MAX_STARTUP_SECONDS))
                .contains(&self.startup_timeout)
        {
            return Err(LocalModelError::InvalidMetadata);
        }
        Ok(())
    }
}

#[allow(dead_code)] // retained safe log snapshot is exposed on explicit status/failure paths
pub struct ManagedLocalServer {
    child: tokio::process::Child,
    process_group: Option<i32>,
    output_tasks: Vec<tokio::task::JoinHandle<()>>,
    stdout: Arc<Mutex<BoundedLog>>,
    stderr: Arc<Mutex<BoundedLog>>,
    temporary: tempfile::TempDir,
    runtime_path: String,
    model_path: String,
    info: LocalServerInfo,
}

impl std::fmt::Debug for ManagedLocalServer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ManagedLocalServer")
            .field("info", &self.info)
            .field("process_group", &self.process_group.is_some())
            .field("runtime", &"[MANAGED RUNTIME PATH REDACTED]")
            .field("model", &"[MANAGED MODEL PATH REDACTED]")
            .finish()
    }
}

impl ManagedLocalServer {
    pub fn info(&self) -> LocalServerInfo {
        self.info.clone()
    }

    pub fn provider_config(&self) -> crate::config::ProviderConfig {
        crate::config::ProviderConfig {
            base_url: self.info.base_url.clone(),
            api_key: String::new(),
            protocol: crate::agent::ProviderProtocol::ChatCompletions,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec![self.info.model_id.clone()],
            timeout: crate::config::ProviderTimeoutConfig::default(),
        }
    }

    #[allow(dead_code)] // production foreground mode currently reports only typed readiness/errors
    pub async fn safe_logs(&self) -> String {
        let stdout = self.stdout.lock().await.render("stdout");
        let stderr = self.stderr.lock().await.render("stderr");
        redact_owned_paths(
            &format!("{stdout}\n{stderr}"),
            &self.runtime_path,
            &self.model_path,
            self.temporary.path(),
        )
    }

    pub async fn shutdown(&mut self) {
        terminate_process_group(self.process_group, nix::sys::signal::Signal::SIGTERM);
        if tokio::time::timeout(Duration::from_secs(2), self.child.wait())
            .await
            .is_err()
        {
            terminate_process_group(self.process_group, nix::sys::signal::Signal::SIGKILL);
            let _ = self.child.start_kill();
            let _ = self.child.wait().await;
        }
        for task in self.output_tasks.drain(..) {
            task.abort();
        }
        self.process_group = None;
    }
}

impl Drop for ManagedLocalServer {
    fn drop(&mut self) {
        terminate_process_group(self.process_group, nix::sys::signal::Signal::SIGKILL);
        let _ = self.child.start_kill();
        for task in &self.output_tasks {
            task.abort();
        }
    }
}

#[derive(Debug)]
struct BoundedLog {
    head: Vec<u8>,
    tail: Vec<u8>,
    total: usize,
}

impl BoundedLog {
    fn new() -> Self {
        Self {
            head: Vec::new(),
            tail: Vec::new(),
            total: 0,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        self.total = self.total.saturating_add(bytes.len());
        let head_capacity = LOG_BYTES_PER_STREAM.div_ceil(2);
        let tail_capacity = LOG_BYTES_PER_STREAM / 2;
        let head_remaining = head_capacity.saturating_sub(self.head.len());
        self.head
            .extend_from_slice(&bytes[..bytes.len().min(head_remaining)]);
        self.tail.extend_from_slice(bytes);
        if self.tail.len() > tail_capacity {
            self.tail.drain(..self.tail.len() - tail_capacity);
        }
    }

    fn render(&self, label: &str) -> String {
        let bytes = if self.total <= LOG_BYTES_PER_STREAM {
            let mut bytes = self.head.clone();
            let tail_start = self.total.saturating_sub(self.tail.len());
            let overlap = self.head.len().saturating_sub(tail_start);
            bytes.extend_from_slice(&self.tail[overlap.min(self.tail.len())..]);
            bytes
        } else {
            let mut bytes = self.head.clone();
            bytes.extend_from_slice(
                format!(
                    "\n[{label}: {} bytes omitted]\n",
                    self.total.saturating_sub(LOG_BYTES_PER_STREAM)
                )
                .as_bytes(),
            );
            bytes.extend_from_slice(&self.tail);
            bytes
        };
        crate::terminal_text::sanitize_terminal_text(&String::from_utf8_lossy(&bytes))
    }
}

pub async fn start_managed_server(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    runtime_id: &str,
    model_id: &str,
    context_size: u32,
    startup_timeout: Duration,
) -> Result<ManagedLocalServer, LocalModelError> {
    start_managed_server_mode(
        paths,
        profile,
        runtime_id,
        model_id,
        context_size,
        startup_timeout,
        LocalServerMode::Chat,
    )
    .await
}

#[derive(Clone, Copy)]
enum LocalServerMode {
    Chat,
    Embeddings,
}

#[allow(clippy::too_many_arguments)]
async fn start_managed_server_mode(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    runtime_id: &str,
    model_id: &str,
    context_size: u32,
    startup_timeout: Duration,
    mode: LocalServerMode,
) -> Result<ManagedLocalServer, LocalModelError> {
    let options = LocalRunOptions {
        model_id: model_id.to_string(),
        runtime_id: runtime_id.to_string(),
        context_size,
        startup_timeout,
    };
    options.validate()?;
    let (_, model) =
        resolve_model(paths, profile, model_id).map_err(|_| LocalModelError::ModelUnavailable)?;
    let runtime =
        crate::external_runtime::resolve_entrypoint(paths, profile, runtime_id, "llama-server")
            .map_err(|_| LocalModelError::RuntimeUnavailable)?;
    start_server_with_resolved_mode(
        runtime,
        model,
        options,
        crate::sandbox::SandboxMode::Required,
        mode,
    )
    .await
}

#[cfg(test)]
async fn start_server_with_resolved(
    runtime: PathBuf,
    model: PathBuf,
    options: LocalRunOptions,
    sandbox_mode: crate::sandbox::SandboxMode,
) -> Result<ManagedLocalServer, LocalModelError> {
    start_server_with_resolved_mode(runtime, model, options, sandbox_mode, LocalServerMode::Chat)
        .await
}

async fn start_server_with_resolved_mode(
    runtime: PathBuf,
    model: PathBuf,
    options: LocalRunOptions,
    sandbox_mode: crate::sandbox::SandboxMode,
    mode: LocalServerMode,
) -> Result<ManagedLocalServer, LocalModelError> {
    options.validate()?;
    let runtime = canonical_executable(&runtime)?;
    let model = canonical_regular(&model, MAX_MODEL_BYTES)?;
    parse_gguf_header(&read_header(&model)?)?;
    let port = reserve_loopback_port().await?;
    let temporary = tempfile::Builder::new()
        .prefix("ca-local-server-")
        .tempdir()
        .map_err(|_| LocalModelError::ServerStart)?;
    let mut arguments = vec![
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--model".into(),
        model.to_string_lossy().into_owned(),
        "--alias".into(),
        options.model_id.clone(),
        "--ctx-size".into(),
        options.context_size.to_string(),
    ];
    match mode {
        LocalServerMode::Chat => arguments.push("--jinja".into()),
        LocalServerMode::Embeddings => arguments.extend([
            "--embedding".into(),
            "--pooling".into(),
            "mean".into(),
            "--threads".into(),
            MAX_EMBEDDING_THREADS.to_string(),
            "--threads-batch".into(),
            MAX_EMBEDDING_THREADS.to_string(),
        ]),
    }
    let prepared = crate::sandbox::SandboxBackend::new(sandbox_mode)
        .prepare_process(
            &runtime,
            &arguments,
            &crate::sandbox::SandboxPolicy {
                roots: vec![
                    runtime
                        .parent()
                        .ok_or(LocalModelError::ServerStart)?
                        .to_path_buf(),
                    model
                        .parent()
                        .ok_or(LocalModelError::ServerStart)?
                        .to_path_buf(),
                ],
                read_only_roots: Vec::new(),
                temporary_directory: temporary.path().to_path_buf(),
                network_allowed: true,
                hide_other_processes: false,
            },
        )
        .map_err(|_| LocalModelError::SandboxUnavailable)?;
    let mut command = tokio::process::Command::new(prepared.program);
    command
        .args(prepared.arguments)
        .current_dir(temporary.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("TMPDIR", temporary.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    let mut child = command.spawn().map_err(|_| LocalModelError::ServerStart)?;
    let process_group = child.id().and_then(|id| i32::try_from(id).ok());
    let stdout_log = Arc::new(Mutex::new(BoundedLog::new()));
    let stderr_log = Arc::new(Mutex::new(BoundedLog::new()));
    let mut output_tasks = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        output_tasks.push(spawn_log_reader(stdout, stdout_log.clone()));
    }
    if let Some(stderr) = child.stderr.take() {
        output_tasks.push(spawn_log_reader(stderr, stderr_log.clone()));
    }
    let info = LocalServerInfo {
        provider_id: format!("local-managed-{port}"),
        model_id: options.model_id.clone(),
        base_url: format!("http://127.0.0.1:{port}/v1"),
    };
    let mut server = ManagedLocalServer {
        child,
        process_group,
        output_tasks,
        stdout: stdout_log,
        stderr: stderr_log,
        temporary,
        runtime_path: runtime.to_string_lossy().into_owned(),
        model_path: model.to_string_lossy().into_owned(),
        info,
    };
    if let Err(error) = wait_until_ready(&mut server, options.startup_timeout).await {
        server.shutdown().await;
        return Err(error);
    }
    Ok(server)
}

/// Owns a supervised embedding subprocess. Only its allocated loopback endpoint
/// is contacted; dropping the owner terminates the process group.
pub struct ManagedEmbeddingServer {
    server: ManagedLocalServer,
    client: reqwest::Client,
    model_digest: String,
}

impl std::fmt::Debug for ManagedEmbeddingServer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ManagedEmbeddingServer")
            .finish_non_exhaustive()
    }
}

struct EmbeddingVerificationCancellation(Arc<AtomicBool>);

impl Drop for EmbeddingVerificationCancellation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub async fn start_managed_embedding_server(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    runtime_id: &str,
    model_id: &str,
    context_size: u32,
    startup_timeout: Duration,
) -> Result<ManagedEmbeddingServer, LocalModelError> {
    let options = LocalRunOptions {
        model_id: model_id.into(),
        runtime_id: runtime_id.into(),
        context_size,
        startup_timeout,
    };
    options.validate()?;
    // Only embedding preparation is admitted here. The permit belongs to the
    // actual blocking worker, so abandoning its waiter cannot admit more hashes.
    static VERIFICATION_WORKERS: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
        std::sync::LazyLock::new(|| {
            Arc::new(tokio::sync::Semaphore::new(
                MAX_EMBEDDING_VERIFICATION_WORKERS,
            ))
        });
    let permit = tokio::time::timeout(
        options.startup_timeout,
        VERIFICATION_WORKERS.clone().acquire_owned(),
    )
    .await
    .map_err(|_| LocalModelError::RuntimeUnavailable)?
    .map_err(|_| LocalModelError::RuntimeUnavailable)?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let _cancel_on_drop = EmbeddingVerificationCancellation(cancelled.clone());
    let paths = paths.clone();
    let profile = profile.clone();
    let runtime_id = runtime_id.to_string();
    let model_id = model_id.to_string();
    // Model hashing and runtime verification can inspect large artifacts; keep
    // that work off the async executor and retain this exact verified binding.
    let (model_digest, model, runtime) = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        #[cfg(test)]
        tests::pause_embedding_verification(&runtime_id);
        let (info, model) = resolve_from_store_with_cancellation(
            &ModelStore::new(paths.local_models_dir(&profile)),
            &model_id,
            Some(cancelled.as_ref()),
        )
        .map_err(|_| LocalModelError::ModelUnavailable)?;
        if cancelled.load(Ordering::Acquire) {
            return Err(LocalModelError::ModelUnavailable);
        }
        let runtime = crate::external_runtime::resolve_entrypoint(
            &paths,
            &profile,
            &runtime_id,
            "llama-server",
        )
        .map_err(|_| LocalModelError::RuntimeUnavailable)?;
        if cancelled.load(Ordering::Acquire) {
            return Err(LocalModelError::RuntimeUnavailable);
        }
        Ok::<_, LocalModelError>((info.sha256, model, runtime))
    })
    .await
    .map_err(|_| LocalModelError::RuntimeUnavailable)??;
    let server = start_server_with_resolved_mode(
        runtime,
        model,
        options,
        crate::sandbox::SandboxMode::Required,
        LocalServerMode::Embeddings,
    )
    .await?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| LocalModelError::ServerStart)?;
    let mut owned = ManagedEmbeddingServer {
        server,
        client,
        model_digest,
    };
    if let Err(error) = owned.embed(&["ca embedding readiness".to_string()]).await {
        owned.server.shutdown().await;
        return Err(error);
    }
    Ok(owned)
}

impl ManagedEmbeddingServer {
    pub fn model_digest(&self) -> &str {
        &self.model_digest
    }

    #[cfg(all(test, unix))]
    pub(crate) async fn fixture(root: &Path) -> Self {
        Self::fixture_with_calibration(root, false).await
    }

    #[cfg(all(test, unix))]
    pub(crate) async fn fixture_with_calibration(root: &Path, calibrated: bool) -> Self {
        let runtime = tests::fake_llama_server(root, None);
        let model = root.join("embedding-fixture.gguf");
        std::fs::write(&model, tests::gguf(3, 1, 2, b"fixture")).unwrap();
        let server = start_server_with_resolved_mode(
            runtime,
            model,
            LocalRunOptions {
                model_id: if calibrated {
                    "embedding-calibrated"
                } else {
                    "embedding"
                }
                .into(),
                runtime_id: "fixture".into(),
                context_size: 2048,
                startup_timeout: Duration::from_secs(5),
            },
            crate::sandbox::SandboxMode::PolicyOnly,
            LocalServerMode::Embeddings,
        )
        .await
        .unwrap();
        Self {
            server,
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap(),
            model_digest: "fixture".into(),
        }
    }

    pub async fn embed(&self, input: &[String]) -> Result<Vec<Vec<f32>>, LocalModelError> {
        if input.is_empty()
            || input.len() > MAX_EMBEDDING_INPUTS
            || input
                .iter()
                .any(|text| text.is_empty() || text.len() > 4096)
            || input.iter().map(String::len).sum::<usize>() > 32 * 1024
        {
            return Err(LocalModelError::InvalidInput);
        }
        let response = self.client.post(format!("{}/embeddings", self.server.info.base_url))
            .json(&serde_json::json!({"model":self.server.info.model_id,"input":input,"encoding_format":"float"}))
            .send().await.map_err(classify_request_error)?;
        if !response.status().is_success() {
            return Err(LocalModelError::ServerProtocol);
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_EMBEDDING_RESPONSE_BYTES as u64)
        {
            return Err(LocalModelError::ServerProtocol);
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(classify_request_error)?;
            if bytes.len().saturating_add(chunk.len()) > MAX_EMBEDDING_RESPONSE_BYTES {
                return Err(LocalModelError::ServerProtocol);
            }
            bytes.extend_from_slice(&chunk);
        }
        decode_embedding_response(&bytes, input.len())
    }
}

fn decode_embedding_response(
    bytes: &[u8],
    expected: usize,
) -> Result<Vec<Vec<f32>>, LocalModelError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| LocalModelError::ServerProtocol)?;
    let data = value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or(LocalModelError::ServerProtocol)?;
    if data.len() != expected {
        return Err(LocalModelError::ServerProtocol);
    }
    let mut result = vec![None; expected];
    let mut dimensions = None;
    for item in data {
        let index = item
            .get("index")
            .and_then(serde_json::Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < expected)
            .ok_or(LocalModelError::ServerProtocol)?;
        if result[index].is_some() {
            return Err(LocalModelError::ServerProtocol);
        }
        let values = item
            .get("embedding")
            .and_then(serde_json::Value::as_array)
            .ok_or(LocalModelError::ServerProtocol)?;
        if values.is_empty()
            || values.len() > MAX_EMBEDDING_DIMENSIONS
            || dimensions.is_some_and(|dimensions| dimensions != values.len())
        {
            return Err(LocalModelError::ServerProtocol);
        }
        dimensions = Some(values.len());
        let vector = values
            .iter()
            .map(|value| {
                value
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .ok_or(LocalModelError::ServerProtocol)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
        if !norm.is_finite() || norm <= f64::EPSILON {
            return Err(LocalModelError::ServerProtocol);
        }
        result[index] = Some(
            vector
                .into_iter()
                .map(|value| (value / norm) as f32)
                .collect(),
        );
    }
    result
        .into_iter()
        .map(|vector| vector.ok_or(LocalModelError::ServerProtocol))
        .collect()
}

fn spawn_log_reader<R>(mut reader: R, log: Arc<Mutex<BoundedLog>>) -> tokio::task::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buffer = [0_u8; 8192];
        while let Ok(read) = reader.read(&mut buffer).await {
            if read == 0 {
                break;
            }
            log.lock().await.push(&buffer[..read]);
        }
    })
}

async fn wait_until_ready(
    server: &mut ManagedLocalServer,
    timeout: Duration,
) -> Result<(), LocalModelError> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|_| LocalModelError::ServerStart)?;
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if server
            .child
            .try_wait()
            .map_err(|_| LocalModelError::ServerExited)?
            .is_some()
        {
            return Err(LocalModelError::ServerExited);
        }
        match check_ready(&client, &server.info).await {
            Ok(true) => return Ok(()),
            Ok(false) | Err(LocalModelError::Connect | LocalModelError::Timeout) => {}
            Err(error) => return Err(error),
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(LocalModelError::ServerTimeout);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn check_ready(
    client: &reqwest::Client,
    info: &LocalServerInfo,
) -> Result<bool, LocalModelError> {
    let health_url = format!("{}/health", info.base_url.trim_end_matches("/v1"));
    let response = client
        .get(health_url)
        .send()
        .await
        .map_err(classify_request_error)?;
    if response.status().as_u16() == 503 {
        return Ok(false);
    }
    if !response.status().is_success() {
        return Err(LocalModelError::ServerProtocol);
    }
    let bytes = bounded_response(response).await?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| LocalModelError::ServerProtocol)?;
    if !value
        .get("status")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|status| matches!(status, "ok" | "ready"))
    {
        return Err(LocalModelError::ServerProtocol);
    }
    let response = client
        .get(format!("{}/models", info.base_url))
        .send()
        .await
        .map_err(classify_request_error)?;
    if !response.status().is_success() {
        return Err(LocalModelError::ServerProtocol);
    }
    let bytes = bounded_response(response).await?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| LocalModelError::ServerProtocol)?;
    let found = value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|models| {
            models.iter().any(|model| {
                model.get("id").and_then(serde_json::Value::as_str) == Some(info.model_id.as_str())
            })
        });
    if found {
        Ok(true)
    } else {
        Err(LocalModelError::ServerModelMismatch)
    }
}

async fn bounded_response(response: reqwest::Response) -> Result<Vec<u8>, LocalModelError> {
    if response
        .content_length()
        .is_some_and(|bytes| bytes > HEALTH_RESPONSE_BYTES as u64)
    {
        return Err(LocalModelError::ServerProtocol);
    }
    let mut stream = response.bytes_stream();
    let mut output = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(classify_request_error)?;
        if output.len().saturating_add(chunk.len()) > HEALTH_RESPONSE_BYTES {
            return Err(LocalModelError::ServerProtocol);
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

async fn reserve_loopback_port() -> Result<u16, LocalModelError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| LocalModelError::ServerStart)?;
    let port = listener
        .local_addr()
        .map_err(|_| LocalModelError::ServerStart)?
        .port();
    drop(listener);
    Ok(port)
}

fn canonical_executable(path: &Path) -> Result<PathBuf, LocalModelError> {
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(LocalModelError::RuntimeUnavailable);
    }
    let path = path
        .canonicalize()
        .map_err(|_| LocalModelError::RuntimeUnavailable)?;
    let metadata = std::fs::metadata(&path).map_err(|_| LocalModelError::RuntimeUnavailable)?;
    if !metadata.is_file() {
        return Err(LocalModelError::RuntimeUnavailable);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(LocalModelError::RuntimeUnavailable);
        }
    }
    Ok(path)
}

fn read_header(path: &Path) -> Result<Vec<u8>, LocalModelError> {
    let mut header = vec![0_u8; HEADER_BYTES];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|_| LocalModelError::ModelUnavailable)?;
    Ok(header)
}

fn terminate_process_group(process_group: Option<i32>, signal: nix::sys::signal::Signal) {
    #[cfg(unix)]
    if let Some(process_group) = process_group {
        let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(process_group), signal);
    }
    #[cfg(not(unix))]
    {
        let _ = process_group;
        let _ = signal;
    }
}

fn redact_owned_paths(text: &str, runtime: &str, model: &str, temporary: &Path) -> String {
    let temporary = temporary.to_string_lossy();
    text.replace(runtime, "[MANAGED RUNTIME]")
        .replace(model, "[MANAGED MODEL]")
        .replace(temporary.as_ref(), "[TEMPORARY DIRECTORY]")
}

fn info_from_record(record: &ModelRecord) -> LocalModelInfo {
    LocalModelInfo {
        id: record.id.clone(),
        bytes: record.bytes,
        sha256: record.sha256.clone(),
        gguf_version: record.header.version,
        tensor_count: record.header.tensor_count,
        metadata_count: record.header.metadata_count,
        license: record.license.clone(),
        source: match record.source_kind {
            ModelSourceKind::UserFile => "user file",
            ModelSourceKind::ExactDownload => "exact download",
        },
    }
}

fn verify_staged(root: &Path, record: &ModelRecord) -> Result<(), LocalModelError> {
    let expected = BTreeSet::from([PathBuf::from("model.gguf")]);
    if collect_files(root)? != expected {
        return Err(LocalModelError::Drift);
    }
    let path = canonical_regular(&root.join("model.gguf"), record.bytes)?;
    let (header, sha256, bytes) = hash_and_header(&path, record.bytes)?;
    if header != record.header || sha256 != record.sha256 || bytes != record.bytes {
        return Err(LocalModelError::Drift);
    }
    Ok(())
}

fn verify_record(
    store: &ModelStore,
    record: &ModelRecord,
) -> Result<(GgufHeader, PathBuf), LocalModelError> {
    verify_record_with_cancellation(store, record, None)
}

fn verify_record_with_cancellation(
    store: &ModelStore,
    record: &ModelRecord,
    cancelled: Option<&AtomicBool>,
) -> Result<(GgufHeader, PathBuf), LocalModelError> {
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(LocalModelError::ModelUnavailable);
    }
    validate_record(record)?;
    let relative = Path::new(&record.relative_path);
    let path = store.root.join(relative);
    let path = canonical_regular(&path, record.bytes)?;
    let canonical_root = store
        .root
        .canonicalize()
        .map_err(|_| LocalModelError::Drift)?;
    if !path.starts_with(&canonical_root) {
        return Err(LocalModelError::Drift);
    }
    let model_root = path.parent().ok_or(LocalModelError::Drift)?;
    if collect_files(model_root)? != BTreeSet::from([PathBuf::from("model.gguf")]) {
        return Err(LocalModelError::Drift);
    }
    let metadata = std::fs::metadata(&path).map_err(|_| LocalModelError::Drift)?;
    check_owner_only(&metadata).map_err(|_| LocalModelError::Drift)?;
    let (header, sha256, bytes) =
        hash_and_header_with_cancellation(&path, record.bytes, cancelled)?;
    if bytes != record.bytes || sha256 != record.sha256 || header != record.header {
        return Err(LocalModelError::Drift);
    }
    Ok((header, path))
}

fn copy_and_hash(
    source: &Path,
    target: &Path,
    expected_bytes: u64,
    expected_sha256: Option<&str>,
) -> Result<(GgufHeader, String, u64), LocalModelError> {
    let mut input = File::open(source).map_err(|_| LocalModelError::InvalidInput)?;
    let mut output =
        create_private_file(target).map_err(|error| LocalModelError::Io(error.to_string()))?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut prefix = Vec::with_capacity(HEADER_BYTES);
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|_| LocalModelError::InvalidInput)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or(LocalModelError::InvalidInput)?;
        if total > expected_bytes || total > MAX_MODEL_BYTES {
            return Err(LocalModelError::InvalidInput);
        }
        if prefix.len() < HEADER_BYTES {
            let needed = HEADER_BYTES - prefix.len();
            prefix.extend_from_slice(&buffer[..read.min(needed)]);
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| LocalModelError::Io(error.to_string()))?;
        hasher.update(&buffer[..read]);
    }
    output
        .sync_all()
        .map_err(|error| LocalModelError::Io(error.to_string()))?;
    let sha256 = digest_string(hasher);
    if total != expected_bytes || expected_sha256.is_some_and(|expected| expected != sha256) {
        return Err(LocalModelError::InvalidInput);
    }
    Ok((parse_gguf_header(&prefix)?, sha256, total))
}

async fn download_and_hash(
    client: &reqwest::Client,
    url: reqwest::Url,
    target: &Path,
    expected_bytes: u64,
    expected_sha256: &str,
) -> Result<(GgufHeader, String, u64), LocalModelError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(classify_request_error)?;
    if !response.status().is_success() {
        return Err(LocalModelError::HttpStatus(response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|bytes| bytes > expected_bytes || bytes > MAX_MODEL_BYTES)
    {
        return Err(LocalModelError::InvalidMetadata);
    }
    let mut output =
        create_private_file(target).map_err(|error| LocalModelError::Io(error.to_string()))?;
    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut prefix = Vec::with_capacity(HEADER_BYTES);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(classify_request_error)?;
        total = total
            .checked_add(chunk.len() as u64)
            .ok_or(LocalModelError::InvalidMetadata)?;
        if total > expected_bytes || total > MAX_MODEL_BYTES {
            return Err(LocalModelError::InvalidMetadata);
        }
        if prefix.len() < HEADER_BYTES {
            let needed = HEADER_BYTES - prefix.len();
            prefix.extend_from_slice(&chunk[..chunk.len().min(needed)]);
        }
        output
            .write_all(&chunk)
            .map_err(|error| LocalModelError::Io(error.to_string()))?;
        hasher.update(&chunk);
    }
    output
        .sync_all()
        .map_err(|error| LocalModelError::Io(error.to_string()))?;
    let sha256 = digest_string(hasher);
    if total != expected_bytes || sha256 != expected_sha256 {
        return Err(LocalModelError::InvalidMetadata);
    }
    Ok((parse_gguf_header(&prefix)?, sha256, total))
}

fn hash_and_header(
    path: &Path,
    expected_bytes: u64,
) -> Result<(GgufHeader, String, u64), LocalModelError> {
    hash_and_header_with_cancellation(path, expected_bytes, None)
}

fn hash_and_header_with_cancellation(
    path: &Path,
    expected_bytes: u64,
    cancelled: Option<&AtomicBool>,
) -> Result<(GgufHeader, String, u64), LocalModelError> {
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(LocalModelError::ModelUnavailable);
    }
    let mut input = File::open(path).map_err(|_| LocalModelError::Drift)?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut prefix = Vec::with_capacity(HEADER_BYTES);
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(LocalModelError::ModelUnavailable);
        }
        let read = input
            .read(&mut buffer)
            .map_err(|_| LocalModelError::Drift)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or(LocalModelError::Drift)?;
        if total > expected_bytes || total > MAX_MODEL_BYTES {
            return Err(LocalModelError::Drift);
        }
        if prefix.len() < HEADER_BYTES {
            let needed = HEADER_BYTES - prefix.len();
            prefix.extend_from_slice(&buffer[..read.min(needed)]);
        }
        hasher.update(&buffer[..read]);
    }
    if total != expected_bytes {
        return Err(LocalModelError::Drift);
    }
    Ok((parse_gguf_header(&prefix)?, digest_string(hasher), total))
}

fn parse_gguf_header(bytes: &[u8]) -> Result<GgufHeader, LocalModelError> {
    if bytes.len() < HEADER_BYTES || &bytes[..4] != b"GGUF" {
        return Err(LocalModelError::InvalidGguf);
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let tensor_count = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    let metadata_count = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
    if !matches!(version, 2 | 3) || tensor_count > 1_000_000 || metadata_count > 1_000_000 {
        return Err(LocalModelError::InvalidGguf);
    }
    Ok(GgufHeader {
        version,
        tensor_count,
        metadata_count,
    })
}

fn validate_state(state: &ModelState) -> Result<(), LocalModelError> {
    if state.version != STORE_VERSION || state.models.len() > MAX_MODELS {
        return Err(LocalModelError::InvalidState);
    }
    for (id, record) in &state.models {
        validate_record(record).map_err(|_| LocalModelError::InvalidState)?;
        if id != &record.id {
            return Err(LocalModelError::InvalidState);
        }
    }
    Ok(())
}

fn validate_record(record: &ModelRecord) -> Result<(), LocalModelError> {
    validate_id(&record.id)?;
    validate_sha256(&record.sha256)?;
    validate_model_size(record.bytes)?;
    validate_license(&record.license)?;
    parse_relative(&record.relative_path)?;
    if record.relative_path != format!("models/{}/model.gguf", record.id)
        || sanitize_name(&record.source_name) != record.source_name
        || !matches!(record.header.version, 2 | 3)
        || record.header.tensor_count > 1_000_000
        || record.header.metadata_count > 1_000_000
    {
        return Err(LocalModelError::InvalidState);
    }
    Ok(())
}

fn preview(id: &str, bytes: u64, license: &str, destination: &Path) {
    println!("Local model write preview:");
    println!("  model: {id}");
    println!("  GGUF bytes: {bytes}");
    println!("  license/source label: {license}");
    println!("  destination: {}", destination.display());
    println!(
        "  coarse model headroom estimate: {} bytes (excludes KV cache and GPU/runtime overhead)",
        recommended_model_bytes(bytes)
    );
}

fn recommended_model_bytes(bytes: u64) -> u64 {
    bytes.saturating_add(bytes.saturating_add(3) / 4)
}

fn canonical_input(path: &Path) -> Result<PathBuf, LocalModelError> {
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(LocalModelError::InvalidInput);
    }
    let path = path
        .canonicalize()
        .map_err(|_| LocalModelError::InvalidInput)?;
    reject_symlink_components(&path).map_err(|_| LocalModelError::InvalidInput)?;
    let metadata = std::fs::metadata(&path).map_err(|_| LocalModelError::InvalidInput)?;
    if !metadata.is_file() {
        return Err(LocalModelError::InvalidInput);
    }
    validate_model_size(metadata.len())?;
    Ok(path)
}

fn parse_download_url(value: &str) -> Result<reqwest::Url, LocalModelError> {
    let url = reqwest::Url::parse(value).map_err(|_| LocalModelError::InvalidSource)?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path_segments().is_none()
    {
        return Err(LocalModelError::InvalidSource);
    }
    match url.scheme() {
        "https" if url.host_str().is_some() => {}
        "http" => {
            let ip = url
                .host_str()
                .map(|host| host.trim_matches(['[', ']']))
                .and_then(|host| host.parse::<IpAddr>().ok())
                .ok_or(LocalModelError::InvalidSource)?;
            if !ip.is_loopback() {
                return Err(LocalModelError::InvalidSource);
            }
        }
        _ => return Err(LocalModelError::InvalidSource),
    }
    Ok(url)
}

fn validate_model_size(bytes: u64) -> Result<(), LocalModelError> {
    if bytes < HEADER_BYTES as u64 || bytes > MAX_MODEL_BYTES {
        Err(LocalModelError::InvalidMetadata)
    } else {
        Ok(())
    }
}

fn validate_license(value: &str) -> Result<(), LocalModelError> {
    if value.trim().is_empty()
        || value.len() > 128
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        Err(LocalModelError::InvalidMetadata)
    } else {
        Ok(())
    }
}

fn validate_id(value: &str) -> Result<(), LocalModelError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        });
    if valid {
        Ok(())
    } else {
        Err(LocalModelError::InvalidId)
    }
}

fn validate_sha256(value: &str) -> Result<(), LocalModelError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(LocalModelError::InvalidMetadata)
    }
}

fn require_confirmation(id: &str, confirm: &str) -> Result<(), LocalModelError> {
    if id == confirm {
        Ok(())
    } else {
        Err(LocalModelError::Confirmation)
    }
}

fn safe_source_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(sanitize_name)
        .unwrap_or_else(|| "user-file.gguf".into())
}

fn sanitize_name(value: &str) -> String {
    let mut output = value
        .chars()
        .filter(|character| !character.is_control() && *character != '/' && *character != '\\')
        .take(128)
        .collect::<String>();
    if output.trim().is_empty() {
        output = "model.gguf".into();
    }
    output
}

fn collect_files(root: &Path) -> Result<BTreeSet<PathBuf>, LocalModelError> {
    let mut files = BTreeSet::new();
    for entry in std::fs::read_dir(root).map_err(|_| LocalModelError::Drift)? {
        let entry = entry.map_err(|_| LocalModelError::Drift)?;
        let kind = entry.file_type().map_err(|_| LocalModelError::Drift)?;
        if !kind.is_file() || kind.is_symlink() {
            return Err(LocalModelError::Drift);
        }
        files.insert(PathBuf::from(entry.file_name()));
    }
    Ok(files)
}

fn canonical_regular(path: &Path, max: u64) -> Result<PathBuf, LocalModelError> {
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(LocalModelError::Drift);
    }
    let path = path.canonicalize().map_err(|_| LocalModelError::Drift)?;
    reject_symlink_components(&path)?;
    let metadata = std::fs::metadata(&path).map_err(|_| LocalModelError::Drift)?;
    if !metadata.is_file() || metadata.len() > max {
        return Err(LocalModelError::Drift);
    }
    Ok(path)
}

fn create_private_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn write_private_new(path: &Path, bytes: &[u8]) -> Result<(), LocalModelError> {
    let mut file =
        create_private_file(path).map_err(|error| LocalModelError::Io(error.to_string()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| LocalModelError::Io(error.to_string()))
}

fn ensure_private_dir(path: &Path) -> Result<(), LocalModelError> {
    reject_symlink_components(path)?;
    std::fs::create_dir_all(path).map_err(|error| LocalModelError::Io(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| LocalModelError::Io(error.to_string()))?;
    }
    Ok(())
}

fn check_owner_only(metadata: &std::fs::Metadata) -> Result<(), LocalModelError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(LocalModelError::InvalidState);
        }
    }
    let _ = metadata;
    Ok(())
}

fn reject_symlink_components(path: &Path) -> Result<(), LocalModelError> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if std::fs::symlink_metadata(ancestor)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(LocalModelError::Drift);
        }
    }
    Ok(())
}

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, LocalModelError> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(max + 1).read_to_end(&mut bytes))
        .map_err(|error| LocalModelError::Io(error.to_string()))?;
    if bytes.len() as u64 > max {
        return Err(LocalModelError::InvalidState);
    }
    Ok(bytes)
}

fn stale_lock(path: &Path) -> Result<bool, LocalModelError> {
    reject_symlink_components(path)?;
    let bytes = read_bounded(path, 4096)?;
    let owner: LockOwner = serde_json::from_slice(&bytes).map_err(|_| LocalModelError::Busy)?;
    #[cfg(unix)]
    {
        let pid = i32::try_from(owner.pid).map_err(|_| LocalModelError::Busy)?;
        match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None) {
            Ok(()) | Err(nix::errno::Errno::EPERM) => Ok(false),
            Err(nix::errno::Errno::ESRCH) => Ok(true),
            Err(_) => Err(LocalModelError::Busy),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = owner;
        Err(LocalModelError::Busy)
    }
}

fn cleanup_staging(root: &Path) -> Result<(), LocalModelError> {
    let staging = root.join("staging");
    if !staging.exists() {
        return Ok(());
    }
    reject_symlink_components(&staging)?;
    for entry in
        std::fs::read_dir(&staging).map_err(|error| LocalModelError::Io(error.to_string()))?
    {
        let entry = entry.map_err(|error| LocalModelError::Io(error.to_string()))?;
        let kind = entry
            .file_type()
            .map_err(|error| LocalModelError::Io(error.to_string()))?;
        let valid = entry
            .file_name()
            .to_str()
            .is_some_and(|name| Uuid::parse_str(name).is_ok());
        if !valid || !kind.is_dir() || kind.is_symlink() {
            return Err(LocalModelError::Drift);
        }
        std::fs::remove_dir_all(entry.path())
            .map_err(|error| LocalModelError::Io(error.to_string()))?;
    }
    Ok(())
}

fn parse_relative(value: &str) -> Result<(), LocalModelError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\\')
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        Err(LocalModelError::InvalidState)
    } else {
        Ok(())
    }
}

fn digest_string(hasher: Sha256) -> String {
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn classify_request_error(error: reqwest::Error) -> LocalModelError {
    if error.is_timeout() {
        LocalModelError::Timeout
    } else {
        LocalModelError::Connect
    }
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
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct EmbeddingVerificationProbe {
        started: std::sync::atomic::AtomicUsize,
        released: Mutex<bool>,
        wake: std::sync::Condvar,
    }

    fn embedding_verification_probes(
    ) -> &'static Mutex<BTreeMap<String, Arc<EmbeddingVerificationProbe>>> {
        static PROBES: std::sync::OnceLock<
            Mutex<BTreeMap<String, Arc<EmbeddingVerificationProbe>>>,
        > = std::sync::OnceLock::new();
        PROBES.get_or_init(Mutex::default)
    }

    pub(super) fn pause_embedding_verification(runtime_id: &str) {
        let Some(probe) = embedding_verification_probes()
            .lock()
            .unwrap()
            .get(runtime_id)
            .cloned()
        else {
            return;
        };
        probe
            .started
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let released = probe.released.lock().unwrap();
        let _ = probe
            .wake
            .wait_timeout_while(released, Duration::from_secs(5), |released| !*released)
            .unwrap();
    }

    struct EmbeddingVerificationProbeGuard {
        runtime_id: String,
        probe: Arc<EmbeddingVerificationProbe>,
    }

    impl Drop for EmbeddingVerificationProbeGuard {
        fn drop(&mut self) {
            *self.probe.released.lock().unwrap() = true;
            self.probe.wake.notify_all();
            embedding_verification_probes()
                .lock()
                .unwrap()
                .remove(&self.runtime_id);
        }
    }

    #[tokio::test]
    async fn cancelled_embedding_verification_keeps_worker_admission_until_retirement() {
        use std::sync::atomic::Ordering;

        let temporary = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(temporary.path());
        let runtime_id = format!("embedding-verification-{}", Uuid::new_v4());
        let probe = Arc::new(EmbeddingVerificationProbe::default());
        embedding_verification_probes()
            .lock()
            .unwrap()
            .insert(runtime_id.clone(), probe.clone());
        let release = EmbeddingVerificationProbeGuard {
            runtime_id: runtime_id.clone(),
            probe: probe.clone(),
        };
        let start = move || {
            let paths = paths.clone();
            let runtime_id = runtime_id.clone();
            tokio::spawn(async move {
                start_managed_embedding_server(
                    &paths,
                    &crate::runtime::ActiveProfile::Default,
                    &runtime_id,
                    "unavailable-model",
                    2048,
                    Duration::from_secs(1),
                )
                .await
            })
        };
        let first = start();
        tokio::time::timeout(Duration::from_secs(2), async {
            while probe.started.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("first verification worker entered");
        first.abort();
        assert!(matches!(first.await, Err(error) if error.is_cancelled()));
        let replacement = start();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            probe.started.load(Ordering::SeqCst),
            1,
            "aborted waiter must not release the actual verification worker admission"
        );
        drop(release);
        assert!(tokio::time::timeout(Duration::from_secs(2), replacement)
            .await
            .expect("replacement resumes after the first worker retires")
            .unwrap()
            .is_err());
    }

    #[test]
    fn embedding_hash_cancellation_preserves_the_ordinary_verification_path() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("model.gguf");
        let bytes = gguf(3, 1, 2, &vec![b'x'; 256 * 1024]);
        std::fs::write(&path, &bytes).unwrap();
        let cancelled = AtomicBool::new(true);
        assert!(matches!(
            hash_and_header_with_cancellation(&path, bytes.len() as u64, Some(&cancelled)),
            Err(LocalModelError::ModelUnavailable)
        ));
        cancelled.store(false, Ordering::Release);
        assert_eq!(
            hash_and_header_with_cancellation(&path, bytes.len() as u64, Some(&cancelled)).unwrap(),
            hash_and_header(&path, bytes.len() as u64).unwrap()
        );
    }

    pub(super) fn gguf(version: u32, tensors: u64, metadata: u64, tail: &[u8]) -> Vec<u8> {
        let mut bytes = b"GGUF".to_vec();
        bytes.extend_from_slice(&version.to_le_bytes());
        bytes.extend_from_slice(&tensors.to_le_bytes());
        bytes.extend_from_slice(&metadata.to_le_bytes());
        bytes.extend_from_slice(tail);
        bytes
    }

    #[cfg(unix)]
    pub(super) fn fake_llama_server(root: &Path, advertised_model: Option<&str>) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = root.join("llama-server");
        let advertised = advertised_model
            .map(|value| format!("alias = {value:?}"))
            .unwrap_or_else(|| "alias = args[args.index('--alias') + 1]".to_string());
        let python = ["/usr/bin/python3", "/usr/local/bin/python3"]
            .into_iter()
            .find(|candidate| Path::new(candidate).is_file())
            .expect("Python 3 test fixture executable");
        let script = format!(
            r#"#!/bin/sh
exec {python} - "$@" <<'PY'
import http.server, json, os, sys
args = sys.argv[1:]
port = int(args[args.index('--port') + 1])
{advertised}
print(json.dumps({{"args": args, "env": sorted(os.environ.keys())}}), flush=True)
class Handler(http.server.BaseHTTPRequestHandler):
    health_count = 0
    def log_message(self, *unused): pass
    def send_json(self, status, value):
        body = json.dumps(value).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def do_GET(self):
        if self.path == '/health':
            Handler.health_count += 1
            if Handler.health_count == 1:
                self.send_json(503, {{'status': 'loading'}})
            else:
                self.send_json(200, {{'status': 'ok'}})
        elif self.path == '/v1/models':
            self.send_json(200, {{'data': [{{'id': alias}}]}})
        else:
            self.send_json(404, {{'error': 'not found'}})
    def do_POST(self):
        if self.path != '/v1/embeddings' or '--embedding' not in args:
            self.send_json(404, {{'error': 'unsupported'}})
            return
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if 'fixture-missing-vector' in request['input']:
            self.send_json(200, {{'data': []}})
            return
        if alias == 'embedding-calibrated':
            def fixture_vector(text):
                value = text.lower()
                if 'publishing' in value or 'release automation' in value or 'signed tags' in value: index = 0
                elif 'compiler diagnostics' in value: index = 1
                elif 'validación' in value: index = 2
                elif 'компилятора' in value: index = 3
                elif '编译器' in value or '检查类型错误' in value: index = 4
                elif value.startswith('explain the outcome'): index = 5
                elif 'amber comet' in value: index = 6
                else: index = 7
                vector = [0.0] * 9
                vector[index] = 0.916515138991168
                vector[8] = 0.4
                return vector
            self.send_json(200, {{'data': [{{'index': index, 'embedding': fixture_vector(text)}} for index, text in enumerate(request['input'])]}})
            return
        self.send_json(200, {{'data': [{{'index': index, 'embedding': [3.0, 4.0]}} for index, text in enumerate(request['input'])]}})
http.server.HTTPServer(('127.0.0.1', port), Handler).serve_forever()
PY
"#
        );
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[test]
    fn bare_rejects_persistent_local_model_management() {
        assert!(parse_args(["ca", "--profile", "bare", "list"]).is_err());
    }

    #[test]
    fn embedding_response_restores_indices_normalizes_and_rejects_malformed_vectors() {
        let vectors = decode_embedding_response(
            br#"{"data":[{"index":1,"embedding":[0,2]},{"index":0,"embedding":[3,4]}]}"#,
            2,
        )
        .unwrap();
        assert_eq!(vectors, vec![vec![0.6, 0.8], vec![0.0, 1.0]]);
        for invalid in [
            br#"{"data":[]}"#.as_slice(),
            br#"{"data":[{"index":0,"embedding":[0,0]}]}"#.as_slice(),
            br#"{"data":[{"index":2,"embedding":[1,2]}]}"#.as_slice(),
            br#"{"data":[{"index":0,"embedding":["NaN",2]}]}"#.as_slice(),
            br#"{"data":[{"index":0,"embedding":[1e309,2]}]}"#.as_slice(),
        ] {
            assert!(decode_embedding_response(invalid, 1).is_err());
        }
        assert!(decode_embedding_response(
            br#"{"data":[{"index":0,"embedding":[1,2]},{"index":0,"embedding":[1,2]}]}"#,
            2
        )
        .is_err());
        assert!(decode_embedding_response(
            br#"{"data":[{"index":0,"embedding":[1,2]},{"index":1,"embedding":[1]}]}"#,
            2
        )
        .is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn embedding_mode_has_explicit_flags_bounded_requests_and_owned_cleanup() {
        let temporary = tempfile::tempdir().unwrap();
        let runtime = fake_llama_server(temporary.path(), None);
        let model = temporary.path().join("embedding.gguf");
        std::fs::write(&model, gguf(3, 1, 2, b"payload")).unwrap();
        let server = start_server_with_resolved_mode(
            runtime,
            model,
            LocalRunOptions {
                model_id: "embedding".into(),
                runtime_id: "fixture".into(),
                context_size: 2048,
                startup_timeout: Duration::from_secs(5),
            },
            crate::sandbox::SandboxMode::PolicyOnly,
            LocalServerMode::Embeddings,
        )
        .await
        .unwrap();
        let logs = server.safe_logs().await;
        assert!(
            logs.contains("--embedding") && logs.contains("--pooling") && logs.contains("mean")
        );
        let launch: serde_json::Value = logs
            .lines()
            .find_map(|line| serde_json::from_str(line).ok())
            .expect("fixture launch arguments");
        let arguments = launch["args"].as_array().unwrap();
        for flag in ["--threads", "--threads-batch"] {
            let position = arguments
                .iter()
                .position(|argument| argument == flag)
                .unwrap_or_else(|| panic!("embedding runtime must explicitly bound {flag}"));
            assert_eq!(arguments[position + 1], "4");
        }
        assert!(!logs.contains("--jinja"));
        let owned = ManagedEmbeddingServer {
            server,
            model_digest: "fixture".into(),
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap(),
        };
        assert_eq!(
            owned
                .embed(&["first".into(), "second".into()])
                .await
                .unwrap(),
            vec![vec![0.6, 0.8], vec![0.6, 0.8]]
        );
        assert!(owned.embed(&[]).await.is_err());
        assert!(owned.embed(&["x".repeat(4097)]).await.is_err());
        let group = owned.server.process_group.unwrap();
        drop(owned);
        for _ in 0..20 {
            if matches!(
                nix::sys::signal::kill(nix::unistd::Pid::from_raw(group), None),
                Err(nix::errno::Errno::ESRCH)
            ) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("embedding owner leaked its process group");
    }

    #[test]
    fn gguf_header_is_bounded_and_versioned() {
        assert_eq!(
            parse_gguf_header(&gguf(3, 10, 20, b"tail")).unwrap(),
            GgufHeader {
                version: 3,
                tensor_count: 10,
                metadata_count: 20
            }
        );
        for bytes in [
            b"no".to_vec(),
            gguf(1, 1, 1, b""),
            gguf(3, 1_000_001, 1, b""),
        ] {
            assert!(parse_gguf_header(&bytes).is_err());
        }
    }

    #[test]
    fn source_urls_and_debug_are_safe() {
        assert!(parse_download_url("https://example.invalid/model.gguf").is_ok());
        assert!(parse_download_url("http://127.0.0.1:8080/model.gguf").is_ok());
        for invalid in [
            "http://example.invalid/model.gguf",
            "http://localhost:8080/model.gguf",
            "https://user@example.invalid/model.gguf",
            "https://example.invalid/model.gguf?token=x",
            "file:///tmp/model.gguf",
        ] {
            assert!(parse_download_url(invalid).is_err());
        }
        let parsed = parse_args([
            "ca",
            "download",
            "demo",
            "https://secret.example.invalid/model.gguf",
            "--sha256",
            &"a".repeat(64),
            "--bytes",
            "24",
            "--license",
            "MIT",
            "--confirm",
            "demo",
        ])
        .unwrap();
        assert!(!format!("{parsed:?}").contains("secret.example.invalid"));
    }

    #[test]
    fn import_resolve_drift_and_remove_are_exact_and_source_preserving() {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("user-model.gguf");
        let bytes = gguf(3, 2, 4, b"bounded payload");
        std::fs::write(&source, &bytes).unwrap();
        let store = ModelStore::new(temporary.path().join("store"));

        import_model(&store, &source, "demo", "demo").unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        let (info, managed) = resolve_from_store(&store, "demo").unwrap();
        assert_eq!(info.id, "demo");
        assert_eq!(info.bytes, bytes.len() as u64);
        assert_eq!(info.source, "user file");
        assert_eq!(std::fs::read(&managed).unwrap(), bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&managed).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(store.root.clone())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        assert!(matches!(
            import_model(&store, &source, "demo", "demo"),
            Err(LocalModelError::Duplicate(_))
        ));

        let extra = managed.parent().unwrap().join("unexpected");
        std::fs::write(&extra, b"drift").unwrap();
        assert!(matches!(
            resolve_from_store(&store, "demo"),
            Err(LocalModelError::Drift)
        ));
        std::fs::remove_file(extra).unwrap();

        remove_model(&store, "demo", "demo").unwrap();
        assert!(source.exists());
        assert!(!managed.exists());
        assert!(store.load().unwrap().models.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn import_rejects_symlink_and_wrong_confirmation_without_writing() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("model.gguf");
        std::fs::write(&source, gguf(3, 1, 1, b"payload")).unwrap();
        let alias = temporary.path().join("alias.gguf");
        symlink(&source, &alias).unwrap();
        let store = ModelStore::new(temporary.path().join("store"));

        assert!(matches!(
            import_model(&store, &alias, "demo", "demo"),
            Err(LocalModelError::InvalidInput)
        ));
        assert!(matches!(
            import_model(&store, &source, "demo", "wrong"),
            Err(LocalModelError::Confirmation)
        ));
        assert!(!store.root.exists());
    }

    #[tokio::test]
    async fn exact_loopback_download_succeeds_and_http_body_stays_out_of_errors() {
        let bytes = gguf(3, 3, 5, b"download payload");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let request_log = requests.clone();
        let response_bytes = bytes.clone();
        let server = tokio::spawn(async move {
            for status in [200_u16, 401_u16] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = vec![0_u8; 4096];
                let read = tokio::io::AsyncReadExt::read(&mut socket, &mut request)
                    .await
                    .unwrap();
                request.truncate(read);
                request_log.lock().unwrap().push(request);
                let body = if status == 200 {
                    response_bytes.as_slice()
                } else {
                    b"TOP-SECRET-RESPONSE-MARKER"
                };
                let header = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                tokio::io::AsyncWriteExt::write_all(&mut socket, header.as_bytes())
                    .await
                    .unwrap();
                tokio::io::AsyncWriteExt::write_all(&mut socket, body)
                    .await
                    .unwrap();
            }
        });
        let temporary = tempfile::tempdir().unwrap();
        let store = ModelStore::new(temporary.path().join("store"));
        let hash = digest_string({
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            hasher
        });
        let url = format!("http://{address}/model.gguf");

        download_model(
            &store,
            "downloaded",
            &url,
            &hash,
            bytes.len() as u64,
            "test-license",
            "downloaded",
        )
        .await
        .unwrap();
        let (info, path) = resolve_from_store(&store, "downloaded").unwrap();
        assert_eq!(info.source, "exact download");
        assert_eq!(std::fs::read(path).unwrap(), bytes);

        let error = download_model(
            &store,
            "rejected",
            &url,
            &hash,
            info.bytes,
            "test-license",
            "rejected",
        )
        .await
        .unwrap_err();
        assert!(matches!(error, LocalModelError::HttpStatus(401)));
        assert!(!error.to_string().contains("TOP-SECRET"));
        assert!(!store.load().unwrap().models.contains_key("rejected"));
        assert_eq!(requests.lock().unwrap().len(), 2);
        server.await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn supervised_server_uses_fixed_loopback_args_clean_env_safe_logs_and_cleanup() {
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempfile::tempdir().unwrap();
        let runtime = fake_llama_server(temporary.path(), None);
        let model = temporary.path().join("model.gguf");
        std::fs::write(&model, gguf(3, 1, 2, b"payload")).unwrap();
        std::fs::set_permissions(&model, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::env::set_var("CA_LOC089_MUST_NOT_REACH_CHILD", "secret-value");
        let mut server = start_server_with_resolved(
            runtime.clone(),
            model.clone(),
            LocalRunOptions {
                model_id: "demo".into(),
                runtime_id: "fake-runtime".into(),
                context_size: 2048,
                startup_timeout: Duration::from_secs(5),
            },
            crate::sandbox::SandboxMode::PolicyOnly,
        )
        .await
        .unwrap();
        std::env::remove_var("CA_LOC089_MUST_NOT_REACH_CHILD");

        let info = server.info();
        assert_eq!(info.model_id, "demo");
        assert!(info.base_url.starts_with("http://127.0.0.1:"));
        assert!(info.base_url.ends_with("/v1"));
        let provider = server.provider_config();
        assert!(provider.api_key.is_empty());
        assert_eq!(provider.models, ["demo"]);
        assert_eq!(provider.base_url, info.base_url);
        let process_group = server.process_group.unwrap();
        let logs = server.safe_logs().await;
        assert!(logs.contains("--host"));
        assert!(logs.contains("127.0.0.1"));
        assert!(logs.contains("--ctx-size"));
        assert!(logs.contains("2048"));
        assert!(logs.contains("--jinja"));
        assert!(logs.contains("[MANAGED MODEL]"));
        assert!(!logs.contains(&model.to_string_lossy().to_string()));
        assert!(!logs.contains(&runtime.to_string_lossy().to_string()));
        assert!(!logs.contains("CA_LOC089_MUST_NOT_REACH_CHILD"));
        assert!(!logs.contains("secret-value"));

        server.shutdown().await;
        assert!(matches!(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(process_group), None),
            Err(nix::errno::Errno::ESRCH)
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mismatched_advertised_model_fails_and_terminates_the_server() {
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempfile::tempdir().unwrap();
        let runtime = fake_llama_server(temporary.path(), Some("other-model"));
        let model = temporary.path().join("model.gguf");
        std::fs::write(&model, gguf(3, 1, 2, b"payload")).unwrap();
        std::fs::set_permissions(&model, std::fs::Permissions::from_mode(0o600)).unwrap();
        let error = start_server_with_resolved(
            runtime,
            model,
            LocalRunOptions {
                model_id: "demo".into(),
                runtime_id: "fake-runtime".into(),
                context_size: 2048,
                startup_timeout: Duration::from_secs(5),
            },
            crate::sandbox::SandboxMode::PolicyOnly,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, LocalModelError::ServerModelMismatch));
    }

    #[test]
    fn bounded_logs_preserve_head_and_tail_with_an_exact_omission_marker() {
        let mut log = BoundedLog::new();
        let mut input = vec![b'h'; LOG_BYTES_PER_STREAM];
        input.extend(vec![b't'; 1234]);
        log.push(&input);
        let rendered = log.render("stdout");
        assert!(rendered.starts_with('h'));
        assert!(rendered.ends_with('t'));
        assert!(rendered.contains("[stdout: 1234 bytes omitted]"));
    }
}
