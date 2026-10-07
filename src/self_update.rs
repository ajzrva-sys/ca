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

const STATE_VERSION: u32 = 1;
const STATE_MAX_BYTES: u64 = 4 * 1024 * 1024;
const MANIFEST_MAX_BYTES: usize = 2 * 1024 * 1024;
const BINARY_MAX_BYTES: u64 = 1024 * 1024 * 1024;
const KEY_MAX_BYTES: u64 = 4096;
const ENVELOPE_SCHEMA: &str = "ca.update.envelope.v1";
const MANIFEST_SCHEMA: &str = "ca.update.manifest.v1";

#[derive(Parser)]
#[command(
    name = "ca update",
    bin_name = "ca update",
    about = "Explicitly verify, stage, install, and roll back signed ca releases"
)]
struct RawUpdateArgs {
    #[command(subcommand)]
    command: UpdateCommand,
}

#[derive(Subcommand)]
enum UpdateCommand {
    /// Pin one exact release source and Ed25519 public key without fetching
    Trust {
        source: String,
        public_key: PathBuf,
        #[arg(long, value_name = "LITERAL")]
        confirm: String,
    },
    /// Show safe local update state without network or writes
    Status,
    /// Explicitly fetch and verify the signed release manifest
    Check,
    /// Persist the exact release channel used when no version is pinned
    Channel { name: String },
    /// Pin one exact verified version, or clear the pin
    Pin { version: Option<String> },
    /// Download and retain one exact verified release without executing it
    Download {
        #[arg(long)]
        version: Option<String>,
        #[arg(long, value_name = "VERSION")]
        confirm: String,
    },
    /// Atomically replace only the invoking executable with an already staged release
    Install {
        #[arg(long)]
        version: Option<String>,
        #[arg(long, value_name = "VERSION")]
        confirm: String,
    },
    /// Atomically restore the retained verified previous executable
    Rollback {
        #[arg(long, value_name = "VERSION")]
        confirm: String,
    },
}

pub struct UpdateCliArgs {
    command: UpdateCommand,
}

impl std::fmt::Debug for UpdateCliArgs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UpdateCliArgs")
            .field("command", &self.command.label())
            .finish()
    }
}

impl UpdateCommand {
    fn label(&self) -> &'static str {
        match self {
            Self::Trust { .. } => "trust",
            Self::Status => "status",
            Self::Check => "check",
            Self::Channel { .. } => "channel",
            Self::Pin { .. } => "pin",
            Self::Download { .. } => "download",
            Self::Install { .. } => "install",
            Self::Rollback { .. } => "rollback",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("update trust requires --confirm ca-update")]
    Confirmation,
    #[error("release source must be an exact HTTPS base URL, or literal-loopback HTTP, without credentials, query, or fragment")]
    InvalidSource,
    #[error("release trust key must be one bounded regular non-symlink base64 Ed25519 public key")]
    InvalidKey,
    #[error("signed release manifest is invalid, stale, or incompatible with this platform")]
    InvalidManifest,
    #[error("signed release manifest verification failed")]
    Signature,
    #[error("no release authority is configured; run `ca update trust` first")]
    MissingTrust,
    #[error("a different release authority is already pinned; authority rotation requires a separate recovery flow")]
    AuthorityAlreadyPinned,
    #[error("no verified release manifest is available; run `ca update check` first")]
    MissingManifest,
    #[error("release channel or version is invalid or unavailable")]
    MissingRelease,
    #[error("update action requires --confirm with the exact release version")]
    VersionConfirmation,
    #[error("the selected release is not staged; run `ca update download` first")]
    NotStaged,
    #[error("staged or rollback executable bytes changed or are invalid")]
    Drift,
    #[error("the invoking executable is not a replaceable regular non-symlink file")]
    InvalidExecutable,
    #[error("release request failed with HTTP {0}")]
    HttpStatus(u16),
    #[error("release request timed out")]
    Timeout,
    #[error("release source could not be reached")]
    Connect,
    #[error("another update mutation is in progress")]
    Busy,
    #[error("update state is invalid")]
    InvalidState,
    #[error("update I/O failed: {0}")]
    Io(String),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Authority {
    source: String,
    public_key: String,
}

impl std::fmt::Debug for Authority {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Authority")
            .field("source", &"[REDACTED]")
            .field("public_key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Release {
    version: String,
    target: String,
    artifact: String,
    sha256: String,
    bytes: u64,
    #[serde(default)]
    notes: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateState {
    version: u32,
    revision: u64,
    authority: Option<Authority>,
    verified_revision: u64,
    verified_manifest_sha256: Option<String>,
    channels: BTreeMap<String, String>,
    releases: BTreeMap<String, Release>,
    channel: String,
    pin: Option<String>,
    staged: BTreeMap<String, StagedBinary>,
    installed_version: Option<String>,
    rollback: Option<RetainedBinary>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            revision: 0,
            authority: None,
            verified_revision: 0,
            verified_manifest_sha256: None,
            channels: BTreeMap::new(),
            releases: BTreeMap::new(),
            channel: "stable".into(),
            pin: None,
            staged: BTreeMap::new(),
            installed_version: None,
            rollback: None,
        }
    }
}

impl std::fmt::Debug for UpdateState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UpdateState")
            .field("version", &self.version)
            .field("revision", &self.revision)
            .field("authority", &self.authority)
            .field("verified_revision", &self.verified_revision)
            .field(
                "verified_manifest",
                &self.verified_manifest_sha256.as_ref().map(|_| "[VERIFIED]"),
            )
            .field("channels", &self.channels)
            .field("release_count", &self.releases.len())
            .field("channel", &self.channel)
            .field("pin", &self.pin)
            .field("staged_versions", &self.staged.keys().collect::<Vec<_>>())
            .field("installed_version", &self.installed_version)
            .field(
                "rollback_version",
                &self.rollback.as_ref().map(|item| &item.version),
            )
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StagedBinary {
    version: String,
    relative_path: String,
    sha256: String,
    bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedBinary {
    version: String,
    relative_path: String,
    sha256: String,
    bytes: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema: String,
    payload: String,
    signature: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    revision: u64,
    channels: BTreeMap<String, String>,
    releases: Vec<Release>,
}

struct UpdateStore {
    root: PathBuf,
    state_path: PathBuf,
}

impl UpdateStore {
    fn new(root: PathBuf) -> Self {
        let root = normalize_missing_path(&root).unwrap_or(root);
        Self {
            state_path: root.join("state.json"),
            root,
        }
    }

    fn load(&self) -> Result<UpdateState, UpdateError> {
        if !self.state_path.exists() {
            return Ok(UpdateState::default());
        }
        reject_symlink_components(&self.state_path)?;
        let metadata =
            std::fs::metadata(&self.state_path).map_err(|_| UpdateError::InvalidState)?;
        check_private(&metadata)?;
        if !metadata.is_file() || metadata.len() > STATE_MAX_BYTES {
            return Err(UpdateError::InvalidState);
        }
        let bytes = read_bounded(&self.state_path, STATE_MAX_BYTES)?;
        let state: UpdateState =
            serde_json::from_slice(&bytes).map_err(|_| UpdateError::InvalidState)?;
        validate_state(&state)?;
        Ok(state)
    }

    fn save(&self, state: &mut UpdateState) -> Result<(), UpdateError> {
        ensure_private_dir(&self.root)?;
        state.revision = state.revision.wrapping_add(1);
        let bytes = serde_json::to_vec_pretty(state).map_err(|_| UpdateError::InvalidState)?;
        if bytes.len() as u64 > STATE_MAX_BYTES {
            return Err(UpdateError::InvalidState);
        }
        let temporary = self.root.join(format!(".state.{}.tmp", Uuid::new_v4()));
        write_private_new(&temporary, &bytes, false)?;
        std::fs::rename(&temporary, &self.state_path).map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            UpdateError::Io(error.to_string())
        })?;
        File::open(&self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| UpdateError::Io(error.to_string()))
    }

    fn lock(&self) -> Result<UpdateLock, UpdateError> {
        ensure_private_dir(&self.root)?;
        UpdateLock::acquire(self.root.join("manager.lock"))
    }
}

#[derive(Serialize, Deserialize)]
struct LockOwner {
    pid: u32,
    token: Uuid,
}

struct UpdateLock {
    path: PathBuf,
    token: Uuid,
}

impl UpdateLock {
    fn acquire(path: PathBuf) -> Result<Self, UpdateError> {
        for attempt in 0..2 {
            let token = Uuid::new_v4();
            let bytes = serde_json::to_vec(&LockOwner {
                pid: std::process::id(),
                token,
            })
            .map_err(|_| UpdateError::InvalidState)?;
            match create_private_file(&path, false) {
                Ok(mut file) => {
                    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
                        drop(file);
                        let _ = std::fs::remove_file(&path);
                        return Err(UpdateError::Io(error.to_string()));
                    }
                    return Ok(Self { path, token });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempt == 0 => {
                    if stale_lock(&path)? {
                        std::fs::remove_file(&path)
                            .map_err(|error| UpdateError::Io(error.to_string()))?;
                        continue;
                    }
                    return Err(UpdateError::Busy);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Err(UpdateError::Busy);
                }
                Err(error) => return Err(UpdateError::Io(error.to_string())),
            }
        }
        Err(UpdateError::Busy)
    }
}

impl Drop for UpdateLock {
    fn drop(&mut self) {
        let owner = std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<LockOwner>(&bytes).ok());
        if owner.is_some_and(|owner| owner.pid == std::process::id() && owner.token == self.token) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub fn parse_args<I, T>(args: I) -> Result<UpdateCliArgs, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    RawUpdateArgs::try_parse_from(args).map(|raw| UpdateCliArgs {
        command: raw.command,
    })
}

pub async fn run_cli(cli: UpdateCliArgs) -> i32 {
    match run_cli_inner(cli).await {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ca update: {error}");
            match error {
                UpdateError::Timeout | UpdateError::Connect | UpdateError::HttpStatus(_) => 6,
                UpdateError::Io(_) | UpdateError::InvalidState => 8,
                UpdateError::Busy => 4,
                _ => 2,
            }
        }
    }
}

async fn run_cli_inner(cli: UpdateCliArgs) -> Result<(), UpdateError> {
    let paths = crate::runtime::RuntimePaths::discover()
        .map_err(|error| UpdateError::Io(error.to_string()))?;
    let store = UpdateStore::new(paths.self_update_dir());
    match cli.command {
        UpdateCommand::Trust {
            source,
            public_key,
            confirm,
        } => trust(&store, &source, &public_key, &confirm),
        UpdateCommand::Status => status(&store),
        UpdateCommand::Check => check(&store).await,
        UpdateCommand::Channel { name } => set_channel(&store, &name),
        UpdateCommand::Pin { version } => set_pin(&store, version.as_deref()),
        UpdateCommand::Download { version, confirm } => {
            download(&store, version.as_deref(), &confirm).await
        }
        UpdateCommand::Install { version, confirm } => {
            let executable = std::env::current_exe().map_err(|_| UpdateError::InvalidExecutable)?;
            install(&store, &executable, version.as_deref(), &confirm)
        }
        UpdateCommand::Rollback { confirm } => {
            let executable = std::env::current_exe().map_err(|_| UpdateError::InvalidExecutable)?;
            rollback(&store, &executable, &confirm)
        }
    }
}

fn trust(store: &UpdateStore, source: &str, key: &Path, confirm: &str) -> Result<(), UpdateError> {
    if confirm != "ca-update" {
        return Err(UpdateError::Confirmation);
    }
    let source = parse_source(source)?;
    let key = read_public_key(key)?;
    let _lock = store.lock()?;
    let mut state = store.load()?;
    let authority = Authority {
        source: source.to_string(),
        public_key: base64::engine::general_purpose::STANDARD.encode(key),
    };
    if let Some(existing) = &state.authority {
        if existing.source != authority.source || existing.public_key != authority.public_key {
            return Err(UpdateError::AuthorityAlreadyPinned);
        }
        println!("The same ca release authority is already pinned; nothing changed");
        return Ok(());
    }
    state.authority = Some(authority);
    state.verified_revision = 0;
    state.verified_manifest_sha256 = None;
    state.channels.clear();
    state.releases.clear();
    state.pin = None;
    store.save(&mut state)?;
    println!("Pinned ca release authority; no manifest or binary was fetched");
    Ok(())
}

fn status(store: &UpdateStore) -> Result<(), UpdateError> {
    let state = store.load()?;
    println!("ca update status");
    println!(
        "  authority: {}",
        if state.authority.is_some() {
            "configured"
        } else {
            "none"
        }
    );
    println!("  channel: {}", state.channel);
    println!("  pin: {}", state.pin.as_deref().unwrap_or("none"));
    println!(
        "  verified manifest: {}",
        if state.verified_revision == 0 {
            "none".into()
        } else {
            format!("revision {}", state.verified_revision)
        }
    );
    println!("  staged releases: {}", state.staged.len());
    println!(
        "  installed by manager: {}",
        state.installed_version.as_deref().unwrap_or("none")
    );
    println!(
        "  rollback: {}",
        state
            .rollback
            .as_ref()
            .map(|item| item.version.as_str())
            .unwrap_or("none")
    );
    Ok(())
}

async fn check(store: &UpdateStore) -> Result<(), UpdateError> {
    let _lock = store.lock()?;
    let mut state = store.load()?;
    let authority = state.authority.clone().ok_or(UpdateError::MissingTrust)?;
    let base = parse_source(&authority.source)?;
    let url = base
        .join("releases.json")
        .map_err(|_| UpdateError::InvalidSource)?;
    let bytes = fetch_bounded(url, MANIFEST_MAX_BYTES).await?;
    let (manifest, payload_sha256) = verify_manifest(&authority, &bytes)?;
    if manifest.revision < state.verified_revision
        || (manifest.revision == state.verified_revision
            && state.verified_manifest_sha256.as_deref() != Some(&payload_sha256))
    {
        return Err(UpdateError::InvalidManifest);
    }
    if manifest.revision == state.verified_revision {
        println!(
            "Ca release manifest revision {} is already verified; nothing changed",
            state.verified_revision
        );
        return Ok(());
    }
    state.verified_revision = manifest.revision;
    state.verified_manifest_sha256 = Some(payload_sha256);
    state.channels = manifest.channels;
    state.releases = manifest
        .releases
        .into_iter()
        .map(|release| (release.version.clone(), release))
        .collect();
    if state
        .pin
        .as_ref()
        .is_some_and(|version| !state.releases.contains_key(version))
    {
        state.pin = None;
    }
    store.save(&mut state)?;
    println!(
        "Verified ca release manifest revision {} ({} compatible releases); nothing was downloaded or installed",
        state.verified_revision,
        state.releases.len()
    );
    Ok(())
}

fn set_channel(store: &UpdateStore, name: &str) -> Result<(), UpdateError> {
    validate_name(name)?;
    let _lock = store.lock()?;
    let mut state = store.load()?;
    if state.verified_revision != 0 && !state.channels.contains_key(name) {
        return Err(UpdateError::MissingRelease);
    }
    state.channel = name.to_string();
    store.save(&mut state)?;
    println!("Update channel set to {name}; no network request or install occurred");
    Ok(())
}

fn set_pin(store: &UpdateStore, version: Option<&str>) -> Result<(), UpdateError> {
    let _lock = store.lock()?;
    let mut state = store.load()?;
    if let Some(version) = version {
        validate_version(version)?;
        if !state.releases.contains_key(version) {
            return Err(UpdateError::MissingRelease);
        }
        state.pin = Some(version.to_string());
        println!("Pinned exact release {version}; no download or install occurred");
    } else {
        state.pin = None;
        println!("Cleared exact release pin; no download or install occurred");
    }
    store.save(&mut state)
}

async fn download(
    store: &UpdateStore,
    requested: Option<&str>,
    confirm: &str,
) -> Result<(), UpdateError> {
    let _lock = store.lock()?;
    let mut state = store.load()?;
    let authority = state.authority.clone().ok_or(UpdateError::MissingTrust)?;
    let release = selected_release(&state, requested)?.clone();
    if confirm != release.version {
        return Err(UpdateError::VersionConfirmation);
    }
    let base = parse_source(&authority.source)?;
    let url = join_artifact(&base, &release.artifact)?;
    println!("Release download preview:");
    println!("  version: {}", release.version);
    println!("  target: {}", release.target);
    println!("  bytes: {}", release.bytes);
    println!("  SHA-256: {}", release.sha256);
    println!("  install: no (staging only)");
    let staging_root = store.root.join("staging");
    ensure_private_dir(&staging_root)?;
    let staging = staging_root.join(Uuid::new_v4().to_string());
    ensure_private_dir(&staging)?;
    let target = staging.join("ca");
    let result = download_binary(url, &target, &release).await;
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    let retained_root = store
        .root
        .join("releases")
        .join(format!("{}-{}", release.version, release.sha256));
    ensure_private_dir(retained_root.parent().ok_or(UpdateError::InvalidState)?)?;
    if retained_root.exists() {
        verify_file(&retained_root.join("ca"), release.bytes, &release.sha256)?;
        let _ = std::fs::remove_dir_all(&staging);
    } else {
        std::fs::rename(&staging, &retained_root)
            .map_err(|error| UpdateError::Io(error.to_string()))?;
    }
    state.staged.insert(
        release.version.clone(),
        StagedBinary {
            version: release.version.clone(),
            relative_path: format!("releases/{}-{}/ca", release.version, release.sha256),
            sha256: release.sha256.clone(),
            bytes: release.bytes,
        },
    );
    store.save(&mut state)?;
    println!(
        "Staged verified ca release {}; the invoking executable was unchanged",
        release.version
    );
    Ok(())
}

fn install(
    store: &UpdateStore,
    executable: &Path,
    requested: Option<&str>,
    confirm: &str,
) -> Result<(), UpdateError> {
    let _lock = store.lock()?;
    let mut state = store.load()?;
    let release = selected_release(&state, requested)?.clone();
    if confirm != release.version {
        return Err(UpdateError::VersionConfirmation);
    }
    let staged = state
        .staged
        .get(&release.version)
        .cloned()
        .ok_or(UpdateError::NotStaged)?;
    if staged.version != release.version
        || staged.bytes != release.bytes
        || staged.sha256 != release.sha256
    {
        return Err(UpdateError::Drift);
    }
    let staged_path =
        verified_relative(store, &staged.relative_path, staged.bytes, &staged.sha256)?;
    let executable = canonical_executable(executable)?;
    let old_bytes = std::fs::metadata(&executable)
        .map_err(|_| UpdateError::InvalidExecutable)?
        .len();
    if old_bytes == 0 || old_bytes > BINARY_MAX_BYTES {
        return Err(UpdateError::InvalidExecutable);
    }
    let old_hash = sha256_file(&executable, old_bytes)?;
    println!("Release install preview:");
    println!("  version: {}", release.version);
    println!("  exact invoking executable: {}", executable.display());
    println!("  replacement bytes: {}", release.bytes);
    println!("  rollback bytes retained: {old_bytes}");
    let backup_root = store.root.join("backups").join(Uuid::new_v4().to_string());
    ensure_private_dir(&backup_root)?;
    let backup_path = backup_root.join("ca");
    copy_exact(&executable, &backup_path, old_bytes, &old_hash, true)?;
    if let Err(error) =
        replace_executable(&staged_path, &executable, release.bytes, &release.sha256)
    {
        let _ = replace_executable(&backup_path, &executable, old_bytes, &old_hash);
        return Err(error);
    }
    let previous_version = state
        .installed_version
        .clone()
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let previous_rollback = state.rollback.clone();
    state.rollback = Some(RetainedBinary {
        version: previous_version,
        relative_path: relative_string(store, &backup_path)?,
        sha256: old_hash.clone(),
        bytes: old_bytes,
    });
    state.installed_version = Some(release.version.clone());
    if let Err(error) = store.save(&mut state) {
        let _ = replace_executable(&backup_path, &executable, old_bytes, &old_hash);
        return Err(error);
    }
    if let Some(previous) = previous_rollback {
        if let Ok(path) = safe_relative_path(store, &previous.relative_path) {
            if let Some(root) = path.parent() {
                let _ = std::fs::remove_dir_all(root);
            }
        }
    }
    println!(
        "Installed verified ca release {}; restart ca to use it",
        release.version
    );
    Ok(())
}

fn rollback(store: &UpdateStore, executable: &Path, confirm: &str) -> Result<(), UpdateError> {
    let _lock = store.lock()?;
    let mut state = store.load()?;
    let retained = state.rollback.clone().ok_or(UpdateError::NotStaged)?;
    if confirm != retained.version {
        return Err(UpdateError::VersionConfirmation);
    }
    let backup = verified_relative(
        store,
        &retained.relative_path,
        retained.bytes,
        &retained.sha256,
    )?;
    let executable = canonical_executable(executable)?;
    let current_bytes = std::fs::metadata(&executable)
        .map_err(|_| UpdateError::InvalidExecutable)?
        .len();
    let current_hash = sha256_file(&executable, current_bytes)?;
    let redo_root = store.root.join("backups").join(Uuid::new_v4().to_string());
    ensure_private_dir(&redo_root)?;
    let redo = redo_root.join("ca");
    copy_exact(&executable, &redo, current_bytes, &current_hash, true)?;
    if let Err(error) = replace_executable(&backup, &executable, retained.bytes, &retained.sha256) {
        let _ = replace_executable(&redo, &executable, current_bytes, &current_hash);
        return Err(error);
    }
    let current_version = state
        .installed_version
        .clone()
        .unwrap_or_else(|| "unknown".into());
    let old_backup_root = backup.parent().map(Path::to_path_buf);
    state.installed_version = Some(retained.version.clone());
    state.rollback = Some(RetainedBinary {
        version: current_version,
        relative_path: relative_string(store, &redo)?,
        sha256: current_hash.clone(),
        bytes: current_bytes,
    });
    if let Err(error) = store.save(&mut state) {
        let _ = replace_executable(&redo, &executable, current_bytes, &current_hash);
        return Err(error);
    }
    if let Some(root) = old_backup_root {
        let _ = std::fs::remove_dir_all(root);
    }
    println!(
        "Rolled back to verified ca release {}; restart ca to use it",
        retained.version
    );
    Ok(())
}

fn verify_manifest(authority: &Authority, bytes: &[u8]) -> Result<(Manifest, String), UpdateError> {
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|_| UpdateError::InvalidManifest)?;
    if envelope.schema != ENVELOPE_SCHEMA {
        return Err(UpdateError::InvalidManifest);
    }
    let payload = base64::engine::general_purpose::STANDARD
        .decode(envelope.payload)
        .map_err(|_| UpdateError::InvalidManifest)?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(envelope.signature)
        .map_err(|_| UpdateError::InvalidManifest)?;
    let key = base64::engine::general_purpose::STANDARD
        .decode(&authority.public_key)
        .map_err(|_| UpdateError::InvalidState)?;
    UnparsedPublicKey::new(&ED25519, key)
        .verify(&payload, &signature)
        .map_err(|_| UpdateError::Signature)?;
    let manifest: Manifest =
        serde_json::from_slice(&payload).map_err(|_| UpdateError::InvalidManifest)?;
    validate_manifest(&manifest)?;
    let payload_sha256 = Sha256::digest(&payload)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((manifest, payload_sha256))
}

fn validate_manifest(manifest: &Manifest) -> Result<(), UpdateError> {
    if manifest.schema != MANIFEST_SCHEMA
        || manifest.revision == 0
        || manifest.releases.is_empty()
        || manifest.releases.len() > 512
        || manifest.channels.is_empty()
        || manifest.channels.len() > 32
    {
        return Err(UpdateError::InvalidManifest);
    }
    let mut versions = BTreeSet::new();
    for release in &manifest.releases {
        validate_release(release)?;
        if !versions.insert(release.version.clone()) {
            return Err(UpdateError::InvalidManifest);
        }
    }
    for (channel, version) in &manifest.channels {
        validate_name(channel)?;
        if !versions.contains(version) {
            return Err(UpdateError::InvalidManifest);
        }
    }
    Ok(())
}

fn validate_release(release: &Release) -> Result<(), UpdateError> {
    validate_version(&release.version)?;
    validate_artifact(&release.artifact)?;
    validate_sha256(&release.sha256)?;
    if release.target != current_target()
        || release.bytes == 0
        || release.bytes > BINARY_MAX_BYTES
        || release.notes.len() > 16 * 1024
        || release.notes.chars().any(|character| character == '\0')
    {
        return Err(UpdateError::InvalidManifest);
    }
    Ok(())
}

fn validate_state(state: &UpdateState) -> Result<(), UpdateError> {
    if state.version != STATE_VERSION
        || state.channel.is_empty()
        || state.staged.len() > 512
        || state.releases.len() > 512
    {
        return Err(UpdateError::InvalidState);
    }
    validate_name(&state.channel).map_err(|_| UpdateError::InvalidState)?;
    match (state.verified_revision, &state.verified_manifest_sha256) {
        (0, None) => {}
        (0, Some(_)) | (_, None) => return Err(UpdateError::InvalidState),
        (_, Some(digest)) => validate_sha256(digest).map_err(|_| UpdateError::InvalidState)?,
    }
    if let Some(authority) = &state.authority {
        parse_source(&authority.source).map_err(|_| UpdateError::InvalidState)?;
        let key = base64::engine::general_purpose::STANDARD
            .decode(&authority.public_key)
            .map_err(|_| UpdateError::InvalidState)?;
        if key.len() != 32 {
            return Err(UpdateError::InvalidState);
        }
    }
    if state.verified_revision == 0 {
        if !state.channels.is_empty() || !state.releases.is_empty() || state.pin.is_some() {
            return Err(UpdateError::InvalidState);
        }
    } else if state.authority.is_none() || state.channels.is_empty() || state.releases.is_empty() {
        return Err(UpdateError::InvalidState);
    }
    for (version, release) in &state.releases {
        validate_release(release).map_err(|_| UpdateError::InvalidState)?;
        if version != &release.version {
            return Err(UpdateError::InvalidState);
        }
    }
    for (channel, version) in &state.channels {
        validate_name(channel).map_err(|_| UpdateError::InvalidState)?;
        if !state.releases.contains_key(version) {
            return Err(UpdateError::InvalidState);
        }
    }
    if state
        .pin
        .as_ref()
        .is_some_and(|version| !state.releases.contains_key(version))
    {
        return Err(UpdateError::InvalidState);
    }
    for staged in state.staged.values() {
        validate_retained(
            &staged.version,
            &staged.relative_path,
            staged.bytes,
            &staged.sha256,
        )?;
    }
    if let Some(retained) = &state.rollback {
        validate_retained(
            &retained.version,
            &retained.relative_path,
            retained.bytes,
            &retained.sha256,
        )?;
    }
    Ok(())
}

fn validate_retained(
    version: &str,
    relative: &str,
    bytes: u64,
    sha256: &str,
) -> Result<(), UpdateError> {
    validate_version(version).map_err(|_| UpdateError::InvalidState)?;
    validate_relative(relative).map_err(|_| UpdateError::InvalidState)?;
    validate_sha256(sha256).map_err(|_| UpdateError::InvalidState)?;
    if bytes == 0 || bytes > BINARY_MAX_BYTES {
        return Err(UpdateError::InvalidState);
    }
    Ok(())
}

fn selected_release<'a>(
    state: &'a UpdateState,
    requested: Option<&str>,
) -> Result<&'a Release, UpdateError> {
    if state.verified_revision == 0 {
        return Err(UpdateError::MissingManifest);
    }
    let version = requested
        .or(state.pin.as_deref())
        .or_else(|| state.channels.get(&state.channel).map(String::as_str))
        .ok_or(UpdateError::MissingRelease)?;
    validate_version(version)?;
    state
        .releases
        .get(version)
        .ok_or(UpdateError::MissingRelease)
}

async fn fetch_bounded(url: reqwest::Url, max: usize) -> Result<Vec<u8>, UpdateError> {
    let client = http_client()?;
    let response = client.get(url).send().await.map_err(classify_request)?;
    if !response.status().is_success() {
        return Err(UpdateError::HttpStatus(response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|bytes| bytes > max as u64)
    {
        return Err(UpdateError::InvalidManifest);
    }
    let mut stream = response.bytes_stream();
    let mut output = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(classify_request)?;
        if output.len().saturating_add(chunk.len()) > max {
            return Err(UpdateError::InvalidManifest);
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

async fn download_binary(
    url: reqwest::Url,
    target: &Path,
    release: &Release,
) -> Result<(), UpdateError> {
    let response = http_client()?
        .get(url)
        .send()
        .await
        .map_err(classify_request)?;
    if !response.status().is_success() {
        return Err(UpdateError::HttpStatus(response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|bytes| bytes > release.bytes)
    {
        return Err(UpdateError::Drift);
    }
    let mut file =
        create_private_file(target, true).map_err(|error| UpdateError::Io(error.to_string()))?;
    let mut stream = response.bytes_stream();
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(classify_request)?;
        total = total
            .checked_add(chunk.len() as u64)
            .ok_or(UpdateError::Drift)?;
        if total > release.bytes {
            return Err(UpdateError::Drift);
        }
        file.write_all(&chunk)
            .map_err(|error| UpdateError::Io(error.to_string()))?;
        hasher.update(&chunk);
    }
    file.sync_all()
        .map_err(|error| UpdateError::Io(error.to_string()))?;
    if total != release.bytes || digest(hasher) != release.sha256 {
        return Err(UpdateError::Drift);
    }
    Ok(())
}

fn http_client() -> Result<reqwest::Client, UpdateError> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(10 * 60))
        .build()
        .map_err(|_| UpdateError::Connect)
}

fn classify_request(error: reqwest::Error) -> UpdateError {
    if error.is_timeout() {
        UpdateError::Timeout
    } else {
        UpdateError::Connect
    }
}

fn parse_source(value: &str) -> Result<reqwest::Url, UpdateError> {
    let url = reqwest::Url::parse(value).map_err(|_| UpdateError::InvalidSource)?;
    if url.cannot_be_a_base()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().ends_with('/')
    {
        return Err(UpdateError::InvalidSource);
    }
    match url.scheme() {
        "https" => {}
        "http" => {
            let ip = url
                .host_str()
                .map(|host| host.trim_matches(['[', ']']))
                .and_then(|host| host.parse::<IpAddr>().ok())
                .ok_or(UpdateError::InvalidSource)?;
            if !ip.is_loopback() {
                return Err(UpdateError::InvalidSource);
            }
        }
        _ => return Err(UpdateError::InvalidSource),
    }
    Ok(url)
}

fn join_artifact(base: &reqwest::Url, relative: &str) -> Result<reqwest::Url, UpdateError> {
    validate_artifact(relative)?;
    let url = base
        .join(relative)
        .map_err(|_| UpdateError::InvalidSource)?;
    if url.scheme() != base.scheme()
        || url.host_str() != base.host_str()
        || url.port_or_known_default() != base.port_or_known_default()
        || !url.path().starts_with(base.path())
    {
        return Err(UpdateError::InvalidSource);
    }
    Ok(url)
}

fn read_public_key(path: &Path) -> Result<Vec<u8>, UpdateError> {
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(UpdateError::InvalidKey);
    }
    let bytes = read_bounded(path, KEY_MAX_BYTES).map_err(|_| UpdateError::InvalidKey)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| UpdateError::InvalidKey)?;
    let key = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|_| UpdateError::InvalidKey)?;
    if key.len() != 32 {
        return Err(UpdateError::InvalidKey);
    }
    Ok(key)
}

fn verified_relative(
    store: &UpdateStore,
    relative: &str,
    bytes: u64,
    sha256: &str,
) -> Result<PathBuf, UpdateError> {
    let path = safe_relative_path(store, relative)?;
    verify_file(&path, bytes, sha256)?;
    Ok(path)
}

fn safe_relative_path(store: &UpdateStore, relative: &str) -> Result<PathBuf, UpdateError> {
    validate_relative(relative)?;
    let canonical_root = store.root.canonicalize().map_err(|_| UpdateError::Drift)?;
    let path = store
        .root
        .join(relative)
        .canonicalize()
        .map_err(|_| UpdateError::Drift)?;
    if !path.starts_with(&canonical_root) {
        return Err(UpdateError::Drift);
    }
    Ok(path)
}

fn relative_string(store: &UpdateStore, path: &Path) -> Result<String, UpdateError> {
    path.strip_prefix(&store.root)
        .map_err(|_| UpdateError::InvalidState)?
        .to_str()
        .map(str::to_string)
        .ok_or(UpdateError::InvalidState)
}

pub(crate) fn verify_file(path: &Path, bytes: u64, sha256: &str) -> Result<(), UpdateError> {
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(UpdateError::Drift);
    }
    let metadata = std::fs::metadata(path).map_err(|_| UpdateError::Drift)?;
    check_private(&metadata).map_err(|_| UpdateError::Drift)?;
    if !metadata.is_file() || metadata.len() != bytes || sha256_file(path, bytes)? != sha256 {
        return Err(UpdateError::Drift);
    }
    Ok(())
}

fn canonical_executable(path: &Path) -> Result<PathBuf, UpdateError> {
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(UpdateError::InvalidExecutable);
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| UpdateError::InvalidExecutable)?;
    let metadata = std::fs::metadata(&canonical).map_err(|_| UpdateError::InvalidExecutable)?;
    if !metadata.is_file() {
        return Err(UpdateError::InvalidExecutable);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o111 == 0
            || metadata.permissions().mode() & 0o022 != 0
        {
            return Err(UpdateError::InvalidExecutable);
        }
    }
    Ok(canonical)
}

pub(crate) fn replace_executable(
    source: &Path,
    executable: &Path,
    bytes: u64,
    sha256: &str,
) -> Result<(), UpdateError> {
    verify_file(source, bytes, sha256)?;
    let parent = executable.parent().ok_or(UpdateError::InvalidExecutable)?;
    let temporary = parent.join(format!(".ca-update-{}", Uuid::new_v4()));
    copy_exact(source, &temporary, bytes, sha256, true)?;
    if let Err(error) = std::fs::rename(&temporary, executable) {
        let _ = std::fs::remove_file(&temporary);
        return Err(UpdateError::Io(error.to_string()));
    }
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| UpdateError::Io(error.to_string()))?;
    verify_file(executable, bytes, sha256)
}

fn stale_lock(path: &Path) -> Result<bool, UpdateError> {
    reject_symlink_components(path)?;
    let bytes = read_bounded(path, 4096)?;
    let owner: LockOwner = serde_json::from_slice(&bytes).map_err(|_| UpdateError::Busy)?;
    #[cfg(unix)]
    {
        let pid = i32::try_from(owner.pid).map_err(|_| UpdateError::Busy)?;
        match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None) {
            Ok(()) | Err(nix::errno::Errno::EPERM) => Ok(false),
            Err(nix::errno::Errno::ESRCH) => Ok(true),
            Err(_) => Err(UpdateError::Busy),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = owner;
        Err(UpdateError::Busy)
    }
}

pub(crate) fn copy_exact(
    source: &Path,
    target: &Path,
    bytes: u64,
    sha256: &str,
    executable: bool,
) -> Result<(), UpdateError> {
    let mut input = File::open(source).map_err(|error| UpdateError::Io(error.to_string()))?;
    let mut output = create_private_file(target, executable)
        .map_err(|error| UpdateError::Io(error.to_string()))?;
    let mut buffer = [0_u8; 128 * 1024];
    let mut total = 0_u64;
    let mut hasher = Sha256::new();
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|error| UpdateError::Io(error.to_string()))?;
        if read == 0 {
            break;
        }
        total = total.checked_add(read as u64).ok_or(UpdateError::Drift)?;
        if total > bytes {
            return Err(UpdateError::Drift);
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| UpdateError::Io(error.to_string()))?;
        hasher.update(&buffer[..read]);
    }
    output
        .sync_all()
        .map_err(|error| UpdateError::Io(error.to_string()))?;
    if total != bytes || digest(hasher) != sha256 {
        return Err(UpdateError::Drift);
    }
    Ok(())
}

pub(crate) fn sha256_file(path: &Path, max: u64) -> Result<String, UpdateError> {
    let mut file = File::open(path).map_err(|error| UpdateError::Io(error.to_string()))?;
    let mut buffer = [0_u8; 128 * 1024];
    let mut total = 0_u64;
    let mut hasher = Sha256::new();
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| UpdateError::Io(error.to_string()))?;
        if read == 0 {
            break;
        }
        total = total.checked_add(read as u64).ok_or(UpdateError::Drift)?;
        if total > max {
            return Err(UpdateError::Drift);
        }
        hasher.update(&buffer[..read]);
    }
    if total != max {
        return Err(UpdateError::Drift);
    }
    Ok(digest(hasher))
}

pub(crate) fn digest(hasher: Sha256) -> String {
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validate_name(value: &str) -> Result<(), UpdateError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        Err(UpdateError::MissingRelease)
    } else {
        Ok(())
    }
}

fn validate_version(value: &str) -> Result<(), UpdateError> {
    validate_name(value)
}

fn validate_artifact(value: &str) -> Result<(), UpdateError> {
    if value.is_empty()
        || value.len() > 1024
        || value.starts_with('/')
        || value.contains(['\\', '?', '#', '%'])
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        Err(UpdateError::InvalidManifest)
    } else {
        Ok(())
    }
}

fn validate_relative(value: &str) -> Result<(), UpdateError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\\')
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        Err(UpdateError::InvalidState)
    } else {
        Ok(())
    }
}

fn validate_sha256(value: &str) -> Result<(), UpdateError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(UpdateError::InvalidManifest)
    }
}

fn current_target() -> String {
    format!(
        "{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH.replace('_', "-")
    )
}

pub(crate) fn create_private_file(path: &Path, executable: bool) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if executable { 0o700 } else { 0o600 });
    }
    let _ = executable;
    options.open(path)
}

fn write_private_new(path: &Path, bytes: &[u8], executable: bool) -> Result<(), UpdateError> {
    let mut file = create_private_file(path, executable)
        .map_err(|error| UpdateError::Io(error.to_string()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| UpdateError::Io(error.to_string()))
}

fn ensure_private_dir(path: &Path) -> Result<(), UpdateError> {
    reject_symlink_components(path)?;
    std::fs::create_dir_all(path).map_err(|error| UpdateError::Io(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| UpdateError::Io(error.to_string()))?;
    }
    Ok(())
}

pub(crate) fn check_private(metadata: &std::fs::Metadata) -> Result<(), UpdateError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(UpdateError::InvalidState);
        }
    }
    let _ = metadata;
    Ok(())
}

fn reject_symlink_components(path: &Path) -> Result<(), UpdateError> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if std::fs::symlink_metadata(ancestor)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(UpdateError::Drift);
        }
    }
    Ok(())
}

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, UpdateError> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(max + 1).read_to_end(&mut bytes))
        .map_err(|error| UpdateError::Io(error.to_string()))?;
    if bytes.len() as u64 > max {
        return Err(UpdateError::InvalidState);
    }
    Ok(bytes)
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

    fn pair() -> Ed25519KeyPair {
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap()
    }

    fn release(version: &str, bytes: &[u8]) -> Release {
        Release {
            version: version.into(),
            target: current_target(),
            artifact: format!("artifacts/ca-{version}"),
            sha256: Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            bytes: bytes.len() as u64,
            notes: "safe notes".into(),
        }
    }

    fn envelope(pair: &Ed25519KeyPair, revision: u64, release: Release) -> Vec<u8> {
        let payload = serde_json::to_vec(&Manifest {
            schema: MANIFEST_SCHEMA.into(),
            revision,
            channels: BTreeMap::from([("stable".into(), release.version.clone())]),
            releases: vec![release],
        })
        .unwrap();
        serde_json::to_vec(&Envelope {
            schema: ENVELOPE_SCHEMA.into(),
            payload: base64::engine::general_purpose::STANDARD.encode(&payload),
            signature: base64::engine::general_purpose::STANDARD
                .encode(pair.sign(&payload).as_ref()),
        })
        .unwrap()
    }

    async fn server(
        responses: Vec<(&'static str, Vec<u8>)>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for (status, body) in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 2048];
                loop {
                    let read = socket.read(&mut buffer).await.unwrap();
                    assert!(read > 0);
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                requests.push(
                    String::from_utf8_lossy(&request)
                        .lines()
                        .next()
                        .unwrap()
                        .to_string(),
                );
                let headers = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(headers.as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
            }
            requests
        });
        (format!("http://{address}/releases/"), task)
    }

    #[test]
    fn status_is_no_write_and_sources_are_exact() {
        let root = tempfile::tempdir().unwrap();
        let store = UpdateStore::new(root.path().join("updates"));
        status(&store).unwrap();
        assert!(!store.root.exists());
        assert!(parse_source("https://example.invalid/releases/").is_ok());
        assert!(parse_source("http://127.0.0.1:8000/releases/").is_ok());
        for invalid in [
            "http://example.invalid/releases/",
            "http://localhost:8000/releases/",
            "https://user@example.invalid/releases/",
            "https://example.invalid/releases/?token=x",
            "file:///tmp/releases/",
        ] {
            assert!(parse_source(invalid).is_err());
        }
    }

    #[test]
    fn signed_manifest_is_closed_targeted_and_debug_redacted() {
        let pair = pair();
        let binary = b"binary";
        let authority = Authority {
            source: "https://secret.example.invalid/releases/".into(),
            public_key: base64::engine::general_purpose::STANDARD
                .encode(pair.public_key().as_ref()),
        };
        let (manifest, _) =
            verify_manifest(&authority, &envelope(&pair, 1, release("1.0.0", binary))).unwrap();
        assert_eq!(manifest.channels["stable"], "1.0.0");
        assert!(!format!("{authority:?}").contains("secret.example.invalid"));
        let mut bad = release("1.0.0", binary);
        bad.target = "wrong-target".into();
        assert!(verify_manifest(&authority, &envelope(&pair, 2, bad)).is_err());

        let mut untrusted_state = UpdateState {
            verified_revision: 1,
            verified_manifest_sha256: Some("a".repeat(64)),
            ..UpdateState::default()
        };
        let release = release("1.0.0", binary);
        untrusted_state
            .channels
            .insert("stable".into(), release.version.clone());
        untrusted_state
            .releases
            .insert(release.version.clone(), release);
        assert!(matches!(
            validate_state(&untrusted_state),
            Err(UpdateError::InvalidState)
        ));
    }

    #[tokio::test]
    async fn explicit_check_and_download_make_only_exact_requests_and_stage_verified_bytes() {
        let root = tempfile::tempdir().unwrap();
        let store = UpdateStore::new(root.path().join("updates"));
        let pair = pair();
        let binary = b"#!/bin/sh\nprintf signed-update\n".to_vec();
        let release = release("2.0.0", &binary);
        let manifest = envelope(&pair, 1, release.clone());
        let (source, requests) = server(vec![
            ("200 OK", manifest.clone()),
            ("200 OK", manifest),
            ("200 OK", binary.clone()),
        ])
        .await;
        let key = root.path().join("key");
        std::fs::write(
            &key,
            base64::engine::general_purpose::STANDARD.encode(pair.public_key().as_ref()),
        )
        .unwrap();

        trust(&store, &source, &key, "ca-update").unwrap();
        check(&store).await.unwrap();
        check(&store).await.unwrap();
        download(&store, Some("2.0.0"), "2.0.0").await.unwrap();
        let state = store.load().unwrap();
        let staged = state.staged.get("2.0.0").unwrap();
        let path =
            verified_relative(&store, &staged.relative_path, staged.bytes, &staged.sha256).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), binary);
        assert_eq!(
            requests.await.unwrap(),
            [
                "GET /releases/releases.json HTTP/1.1",
                "GET /releases/releases.json HTTP/1.1",
                "GET /releases/artifacts/ca-2.0.0 HTTP/1.1"
            ]
        );
    }

    #[tokio::test]
    async fn http_failure_discards_response_body_and_leaves_no_staged_release() {
        let root = tempfile::tempdir().unwrap();
        let store = UpdateStore::new(root.path().join("updates"));
        let pair = pair();
        let binary = b"expected".to_vec();
        let release = release("2.0.0", &binary);
        let manifest = envelope(&pair, 1, release.clone());
        let (source, requests) = server(vec![
            ("200 OK", manifest),
            ("401 Unauthorized", b"SECRET-BODY-MARKER".to_vec()),
        ])
        .await;
        let mut state = UpdateState {
            authority: Some(Authority {
                source,
                public_key: base64::engine::general_purpose::STANDARD
                    .encode(pair.public_key().as_ref()),
            }),
            ..UpdateState::default()
        };
        store.save(&mut state).unwrap();
        check(&store).await.unwrap();
        let error = download(&store, Some("2.0.0"), "2.0.0").await.unwrap_err();
        assert!(matches!(error, UpdateError::HttpStatus(401)));
        assert!(!error.to_string().contains("SECRET-BODY"));
        assert!(store.load().unwrap().staged.is_empty());
        assert_eq!(requests.await.unwrap().len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn staged_install_and_rollback_replace_only_the_exact_executable() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let store = UpdateStore::new(root.path().join("updates"));
        let executable = root.path().join("ca-copy");
        let old = b"#!/bin/sh\nprintf old\n";
        let new = b"#!/bin/sh\nprintf new\n";
        std::fs::write(&executable, old).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let release = release("2.0.0", new);
        let release_root = store
            .root
            .join("releases")
            .join(format!("{}-{}", release.version, release.sha256));
        ensure_private_dir(&release_root).unwrap();
        let staged_path = release_root.join("ca");
        write_private_new(&staged_path, new, true).unwrap();
        let mut state = UpdateState {
            authority: Some(Authority {
                source: "https://example.invalid/releases/".into(),
                public_key: base64::engine::general_purpose::STANDARD.encode([0_u8; 32]),
            }),
            verified_revision: 1,
            verified_manifest_sha256: Some("a".repeat(64)),
            ..UpdateState::default()
        };
        state
            .channels
            .insert("stable".into(), release.version.clone());
        state
            .releases
            .insert(release.version.clone(), release.clone());
        state.staged.insert(
            release.version.clone(),
            StagedBinary {
                version: release.version.clone(),
                relative_path: relative_string(&store, &staged_path).unwrap(),
                sha256: release.sha256.clone(),
                bytes: release.bytes,
            },
        );
        store.save(&mut state).unwrap();

        install(&store, &executable, Some("2.0.0"), "2.0.0").unwrap();
        assert_eq!(std::fs::read(&executable).unwrap(), new);
        let rollback_version = store.load().unwrap().rollback.unwrap().version;
        rollback(&store, &executable, &rollback_version).unwrap();
        assert_eq!(std::fs::read(&executable).unwrap(), old);
        assert_eq!(
            std::fs::metadata(&executable).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn trust_is_idempotent_and_refuses_authority_rotation() {
        let root = tempfile::tempdir().unwrap();
        let store = UpdateStore::new(root.path().join("updates"));
        let first = pair();
        let second = pair();
        let first_key = root.path().join("first-key");
        let second_key = root.path().join("second-key");
        std::fs::write(
            &first_key,
            base64::engine::general_purpose::STANDARD.encode(first.public_key().as_ref()),
        )
        .unwrap();
        std::fs::write(
            &second_key,
            base64::engine::general_purpose::STANDARD.encode(second.public_key().as_ref()),
        )
        .unwrap();

        trust(
            &store,
            "https://example.invalid/releases/",
            &first_key,
            "ca-update",
        )
        .unwrap();
        let revision = store.load().unwrap().revision;
        trust(
            &store,
            "https://example.invalid/releases/",
            &first_key,
            "ca-update",
        )
        .unwrap();
        assert_eq!(store.load().unwrap().revision, revision);
        assert!(matches!(
            trust(
                &store,
                "https://example.invalid/releases/",
                &second_key,
                "ca-update"
            ),
            Err(UpdateError::AuthorityAlreadyPinned)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn install_rejects_staged_bytes_from_an_older_manifest_and_writable_executables() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let store = UpdateStore::new(root.path().join("updates"));
        let executable = root.path().join("ca-copy");
        std::fs::write(&executable, b"old").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let old_release = release("2.0.0", b"old-release");
        let current_release = release("2.0.0", b"changed-release");
        let mut state = UpdateState {
            authority: Some(Authority {
                source: "https://example.invalid/releases/".into(),
                public_key: base64::engine::general_purpose::STANDARD.encode([0_u8; 32]),
            }),
            verified_revision: 2,
            verified_manifest_sha256: Some("b".repeat(64)),
            ..UpdateState::default()
        };
        state
            .channels
            .insert("stable".into(), current_release.version.clone());
        state
            .releases
            .insert(current_release.version.clone(), current_release);
        state.staged.insert(
            old_release.version.clone(),
            StagedBinary {
                version: old_release.version,
                relative_path: "releases/unused/ca".into(),
                sha256: old_release.sha256,
                bytes: old_release.bytes,
            },
        );
        store.save(&mut state).unwrap();
        assert!(matches!(
            install(&store, &executable, Some("2.0.0"), "2.0.0"),
            Err(UpdateError::Drift)
        ));
        assert_eq!(std::fs::read(&executable).unwrap(), b"old");

        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o722)).unwrap();
        assert!(matches!(
            canonical_executable(&executable),
            Err(UpdateError::InvalidExecutable)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn abandoned_update_lock_is_recovered() {
        let root = tempfile::tempdir().unwrap();
        let store = UpdateStore::new(root.path().join("updates"));
        ensure_private_dir(&store.root).unwrap();
        let lock_path = store.root.join("manager.lock");
        write_private_new(
            &lock_path,
            &serde_json::to_vec(&LockOwner {
                pid: i32::MAX as u32,
                token: Uuid::new_v4(),
            })
            .unwrap(),
            false,
        )
        .unwrap();
        let lock = store.lock().unwrap();
        drop(lock);
        assert!(!lock_path.exists());
    }

    #[test]
    fn leading_parser_never_keeps_source_or_key_in_debug() {
        let parsed = parse_args([
            "ca",
            "trust",
            "https://secret.example.invalid/releases/",
            "/tmp/key",
            "--confirm",
            "ca-update",
        ])
        .unwrap();
        let debug = format!("{parsed:?}");
        assert!(!debug.contains("secret.example.invalid"));
        assert!(!debug.contains("/tmp/key"));
    }
}
