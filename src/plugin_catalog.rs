use base64::Engine as _;
use clap::Subcommand;
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

const STORE_VERSION: u32 = 1;
const ENVELOPE_SCHEMA: &str = "ca.plugin.catalog-envelope.v1";
const PACKAGE_SCHEMA: &str = "ca.plugin.package.v1";
const MAX_KEY_BYTES: u64 = 4096;
const MAX_INDEX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 5 * 1024 * 1024;
const MAX_PACKAGE_CONTENT_BYTES: usize = 3 * 1024 * 1024;
const MAX_STATE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_CATALOGS: usize = 32;
const MAX_ENTRIES: usize = 512;
const MAX_FILES: usize = 1024;

#[derive(Debug, Subcommand)]
pub enum CatalogCommand {
    /// Pin one local Ed25519 catalog trust root
    Trust {
        id: String,
        public_key: PathBuf,
        #[arg(long, value_name = "ID")]
        confirm: String,
    },
    /// Remove a pinned root and cached catalog metadata
    Untrust {
        id: String,
        #[arg(long, value_name = "ID")]
        confirm: String,
    },
    /// Verify and store one newer signed local catalog snapshot
    Sync { id: String, index: PathBuf },
    /// List pinned catalogs and verified plugin versions
    List { id: Option<String> },
    /// Stage and register one verified catalog plugin, disabled
    Install {
        name: String,
        #[arg(long, value_name = "PLUGIN_ID")]
        confirm: String,
    },
    /// Stage and switch an installed catalog plugin to the verified version, disabled
    Update {
        name: String,
        #[arg(long, value_name = "PLUGIN_ID")]
        confirm: String,
    },
    /// Switch an installed catalog plugin to a retained version, disabled
    Rollback {
        plugin: String,
        #[arg(long, value_name = "VERSION")]
        to: String,
        #[arg(long, value_name = "PLUGIN_ID")]
        confirm: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("catalog IDs and plugin IDs must be 1-64 lowercase ASCII letters, digits, '-' or '_', starting with a letter")]
    InvalidId,
    #[error("catalog versions must be 1-64 ASCII letters, digits, '.', '-' or '_'")]
    InvalidVersion,
    #[error("catalog operation requires --confirm with the exact ID")]
    Confirmation,
    #[error("catalog trust key must be a bounded regular non-symlink file containing one base64 Ed25519 public key")]
    InvalidKey,
    #[error("catalog snapshot is invalid or exceeds its bound")]
    InvalidIndex,
    #[error("catalog signature verification failed")]
    Signature,
    #[error("catalog snapshot revision is stale or conflicts with the verified revision")]
    StaleRevision,
    #[error("catalog package is invalid, changed, or exceeds its bound")]
    InvalidPackage,
    #[error("catalog resource is invalid or escapes its source")]
    InvalidResource,
    #[error("catalog is already trusted: {0}")]
    DuplicateCatalog(String),
    #[error("catalog is not trusted: {0}")]
    MissingCatalog(String),
    #[error("plugin is not present in the verified catalog: {0}")]
    MissingPlugin(String),
    #[error("retained catalog version was not found or is ambiguous")]
    MissingVersion,
    #[error("catalog state is invalid")]
    InvalidState,
    #[error("catalog I/O failed: {0}")]
    Io(String),
    #[error("local plugin registration failed: {0}")]
    Plugin(String),
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CatalogEnvelope {
    schema: String,
    payload: String,
    signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CatalogPayload {
    catalog_id: String,
    revision: u64,
    entries: Vec<CatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CatalogEntry {
    plugin_id: String,
    version: String,
    package: String,
    sha256: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct TrustedCatalog {
    public_key: String,
    verified: Option<VerifiedCatalog>,
}

impl std::fmt::Debug for TrustedCatalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrustedCatalog")
            .field("public_key_pinned", &true)
            .field(
                "verified_revision",
                &self.verified.as_ref().map(|catalog| catalog.revision),
            )
            .field(
                "entry_count",
                &self
                    .verified
                    .as_ref()
                    .map_or(0, |catalog| catalog.entries.len()),
            )
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct VerifiedCatalog {
    revision: u64,
    payload_sha256: String,
    source_root: PathBuf,
    entries: Vec<CatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CatalogState {
    version: u32,
    revision: u64,
    catalogs: BTreeMap<String, TrustedCatalog>,
}

impl Default for CatalogState {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            revision: 0,
            catalogs: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PluginPackage {
    schema: String,
    plugin_id: String,
    version: String,
    files: Vec<PackageFile>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PackageFile {
    path: String,
    sha256: String,
    content: String,
}

impl std::fmt::Debug for PackageFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PackageFile")
            .field("path", &self.path)
            .field("sha256", &self.sha256)
            .field("content", &"[PACKAGE CONTENT REDACTED]")
            .finish()
    }
}

struct CatalogStore {
    dir: PathBuf,
    path: PathBuf,
}

impl CatalogStore {
    fn new(dir: PathBuf) -> Self {
        Self {
            path: dir.join("catalogs.json"),
            dir,
        }
    }

    fn load(&self) -> Result<CatalogState, CatalogError> {
        if !self.path.exists() {
            return Ok(CatalogState::default());
        }
        reject_symlink_components(&self.path)?;
        let metadata = std::fs::symlink_metadata(&self.path)
            .map_err(|error| CatalogError::Io(error.to_string()))?;
        if !metadata.is_file() || metadata.len() > MAX_STATE_BYTES {
            return Err(CatalogError::InvalidState);
        }
        check_owner_only(&metadata)?;
        let bytes = read_bounded(&self.path, MAX_STATE_BYTES).map_err(CatalogError::Io)?;
        let state: CatalogState =
            serde_json::from_slice(&bytes).map_err(|_| CatalogError::InvalidState)?;
        if state.version != STORE_VERSION || state.catalogs.len() > MAX_CATALOGS {
            return Err(CatalogError::InvalidState);
        }
        Ok(state)
    }

    fn save(&self, state: &mut CatalogState) -> Result<(), CatalogError> {
        ensure_private_dir(&self.dir)?;
        state.revision = state.revision.wrapping_add(1);
        let bytes = serde_json::to_vec_pretty(state).map_err(|_| CatalogError::InvalidState)?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err(CatalogError::InvalidState);
        }
        let temporary = self.dir.join(format!(".catalogs.{}.tmp", Uuid::new_v4()));
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
                .map_err(|error| CatalogError::Io(error.to_string()))?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|error| CatalogError::Io(error.to_string()))?;
            std::fs::rename(&temporary, &self.path)
                .map_err(|error| CatalogError::Io(error.to_string()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

pub fn run(
    command: CatalogCommand,
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
) -> Result<(), CatalogError> {
    let output = run_for_tui(command, paths, profile)?;
    if !output.is_empty() {
        println!("{output}");
    }
    Ok(())
}

pub(crate) fn run_for_tui(
    command: CatalogCommand,
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
) -> Result<String, CatalogError> {
    if matches!(profile, crate::runtime::ActiveProfile::Bare) {
        return Err(CatalogError::InvalidState);
    }
    let requested_root = paths.plugin_catalog_dir(profile);
    let root = normalize_missing_path(&requested_root).unwrap_or(requested_root);
    let store = CatalogStore::new(root.clone());
    match command {
        CatalogCommand::Trust {
            id,
            public_key,
            confirm,
        } => {
            require_confirmation(&id, &confirm)?;
            validate_id(&id)?;
            let public_key = load_public_key(&public_key)?;
            let mut state = store.load()?;
            if let Some(existing) = state.catalogs.get(&id) {
                if existing.public_key == public_key {
                    return Ok(format!("Catalog {id} trust root is already pinned"));
                }
                return Err(CatalogError::DuplicateCatalog(id));
            }
            if state.catalogs.len() >= MAX_CATALOGS {
                return Err(CatalogError::InvalidState);
            }
            state.catalogs.insert(
                id.clone(),
                TrustedCatalog {
                    public_key,
                    verified: None,
                },
            );
            store.save(&mut state)?;
            Ok(format!(
                "Pinned catalog {id}; no plugins were installed or enabled"
            ))
        }
        CatalogCommand::Untrust { id, confirm } => {
            require_confirmation(&id, &confirm)?;
            validate_id(&id)?;
            let mut state = store.load()?;
            if state.catalogs.remove(&id).is_none() {
                return Err(CatalogError::MissingCatalog(id));
            }
            store.save(&mut state)?;
            Ok(format!(
                "Removed catalog {id} trust and cached metadata; installed bundles were kept"
            ))
        }
        CatalogCommand::Sync { id, index } => sync_catalog(&store, &id, &index),
        CatalogCommand::List { id } => list_catalogs(&store.load()?, id.as_deref()),
        CatalogCommand::Install { name, confirm } => stage_entry(
            &store.load()?,
            &root,
            paths,
            profile,
            &name,
            &confirm,
            crate::plugin::CatalogRegistration::Install,
        ),
        CatalogCommand::Update { name, confirm } => stage_entry(
            &store.load()?,
            &root,
            paths,
            profile,
            &name,
            &confirm,
            crate::plugin::CatalogRegistration::Replace,
        ),
        CatalogCommand::Rollback {
            plugin,
            to,
            confirm,
        } => rollback(&root, paths, profile, &plugin, &to, &confirm),
    }
}

fn sync_catalog(store: &CatalogStore, id: &str, index: &Path) -> Result<String, CatalogError> {
    validate_id(id)?;
    let mut state = store.load()?;
    let trusted = state
        .catalogs
        .get(id)
        .ok_or_else(|| CatalogError::MissingCatalog(id.to_string()))?;
    let public_key = base64::engine::general_purpose::STANDARD
        .decode(&trusted.public_key)
        .map_err(|_| CatalogError::InvalidState)?;
    if public_key.len() != 32 {
        return Err(CatalogError::InvalidState);
    }
    let index =
        canonical_regular(index, MAX_INDEX_BYTES).map_err(|_| CatalogError::InvalidIndex)?;
    let bytes = read_bounded(&index, MAX_INDEX_BYTES).map_err(|_| CatalogError::InvalidIndex)?;
    let envelope: CatalogEnvelope =
        serde_json::from_slice(&bytes).map_err(|_| CatalogError::InvalidIndex)?;
    if envelope.schema != ENVELOPE_SCHEMA {
        return Err(CatalogError::InvalidIndex);
    }
    let payload = base64::engine::general_purpose::STANDARD
        .decode(&envelope.payload)
        .map_err(|_| CatalogError::InvalidIndex)?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(&envelope.signature)
        .map_err(|_| CatalogError::InvalidIndex)?;
    if payload.len() > MAX_PAYLOAD_BYTES || signature.len() != 64 {
        return Err(CatalogError::InvalidIndex);
    }
    UnparsedPublicKey::new(&ED25519, &public_key)
        .verify(&payload, &signature)
        .map_err(|_| CatalogError::Signature)?;
    let mut payload: CatalogPayload =
        serde_json::from_slice(&payload).map_err(|_| CatalogError::InvalidIndex)?;
    validate_payload(&payload, id)?;
    payload.entries.sort_by(|left, right| {
        left.plugin_id
            .cmp(&right.plugin_id)
            .then_with(|| left.version.cmp(&right.version))
    });
    let payload_bytes = base64::engine::general_purpose::STANDARD
        .decode(&envelope.payload)
        .map_err(|_| CatalogError::InvalidIndex)?;
    let payload_sha256 = sha256_hex(&payload_bytes);
    let existing = state
        .catalogs
        .get(id)
        .and_then(|catalog| catalog.verified.as_ref());
    if let Some(existing) = existing {
        if existing.revision == payload.revision && existing.payload_sha256 == payload_sha256 {
            return Ok(format!(
                "Catalog {id} revision {} is already verified",
                payload.revision
            ));
        }
        if payload.revision <= existing.revision {
            return Err(CatalogError::StaleRevision);
        }
    }
    let source_root = index
        .parent()
        .ok_or(CatalogError::InvalidIndex)?
        .canonicalize()
        .map_err(|_| CatalogError::InvalidIndex)?;
    state.catalogs.get_mut(id).unwrap().verified = Some(VerifiedCatalog {
        revision: payload.revision,
        payload_sha256,
        source_root,
        entries: payload.entries,
    });
    store.save(&mut state)?;
    Ok(format!(
        "Verified catalog {id} revision {} ({} plugins); nothing was installed or enabled",
        payload.revision,
        state.catalogs[id].verified.as_ref().unwrap().entries.len()
    ))
}

fn validate_payload(payload: &CatalogPayload, id: &str) -> Result<(), CatalogError> {
    if payload.catalog_id != id
        || payload.revision == 0
        || payload.entries.is_empty()
        || payload.entries.len() > MAX_ENTRIES
    {
        return Err(CatalogError::InvalidIndex);
    }
    let mut identities = BTreeSet::new();
    for entry in &payload.entries {
        validate_id(&entry.plugin_id)?;
        validate_version(&entry.version)?;
        validate_sha256(&entry.sha256)?;
        validate_relative(Path::new(&entry.package))?;
        if Path::new(&entry.package)
            .extension()
            .and_then(|value| value.to_str())
            != Some("json")
            || !identities.insert(entry.plugin_id.clone())
        {
            return Err(CatalogError::InvalidIndex);
        }
    }
    Ok(())
}

fn list_catalogs(state: &CatalogState, selected: Option<&str>) -> Result<String, CatalogError> {
    let mut lines = Vec::new();
    if let Some(id) = selected {
        validate_id(id)?;
        let catalog = state
            .catalogs
            .get(id)
            .ok_or_else(|| CatalogError::MissingCatalog(id.to_string()))?;
        append_catalog(&mut lines, id, catalog);
        return Ok(lines.join("\n"));
    }
    if state.catalogs.is_empty() {
        return Ok("No catalog trust roots configured".into());
    }
    for (id, catalog) in &state.catalogs {
        append_catalog(&mut lines, id, catalog);
    }
    Ok(lines.join("\n"))
}

fn append_catalog(lines: &mut Vec<String>, id: &str, catalog: &TrustedCatalog) {
    let Some(verified) = &catalog.verified else {
        lines.push(format!("{id}\tpinned\tnot synced"));
        return;
    };
    lines.push(format!(
        "{id}\tverified revision {}\t{} plugins",
        verified.revision,
        verified.entries.len()
    ));
    for entry in &verified.entries {
        lines.push(format!(
            "  {}/{}\t{}\t{}",
            id,
            entry.plugin_id,
            entry.version,
            &entry.sha256[..12]
        ));
    }
}

fn stage_entry(
    state: &CatalogState,
    catalog_root: &Path,
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    name: &str,
    confirm: &str,
    mode: crate::plugin::CatalogRegistration,
) -> Result<String, CatalogError> {
    let (catalog_id, plugin_id) = split_name(name)?;
    require_confirmation(plugin_id, confirm)?;
    let verified = state
        .catalogs
        .get(catalog_id)
        .and_then(|catalog| catalog.verified.as_ref())
        .ok_or_else(|| CatalogError::MissingCatalog(catalog_id.to_string()))?;
    let entry = verified
        .entries
        .iter()
        .find(|entry| entry.plugin_id == plugin_id)
        .ok_or_else(|| CatalogError::MissingPlugin(plugin_id.to_string()))?;
    let package_path = resolve_source(&verified.source_root, &entry.package)?;
    let package_bytes =
        read_bounded(&package_path, MAX_PACKAGE_BYTES).map_err(|_| CatalogError::InvalidPackage)?;
    if sha256_hex(&package_bytes) != entry.sha256 {
        return Err(CatalogError::InvalidPackage);
    }
    let package: PluginPackage =
        serde_json::from_slice(&package_bytes).map_err(|_| CatalogError::InvalidPackage)?;
    let files = validate_package(&package, entry)?;
    let installs_root = catalog_root.join("installs");
    ensure_private_dir(&installs_root)?;
    let plugin_root = installs_root.join(plugin_id);
    ensure_private_dir(&plugin_root)?;
    let destination = plugin_root.join(format!("{}-{}", entry.version, entry.sha256));
    if destination.exists() {
        return Err(CatalogError::InvalidPackage);
    }
    let staging_root = catalog_root.join("staging");
    ensure_private_dir(&staging_root)?;
    let staging = staging_root.join(Uuid::new_v4().to_string());
    ensure_private_dir(&staging)?;
    let result = (|| {
        for (relative, bytes) in &files {
            let target = staging.join(relative);
            let parent = target.parent().ok_or(CatalogError::InvalidPackage)?;
            ensure_private_dir(parent)?;
            write_private_file(&target, bytes)?;
        }
        write_private_file(&staging.join(".ca-package.json"), &package_bytes)?;
        std::fs::rename(&staging, &destination)
            .map_err(|error| CatalogError::Io(error.to_string()))?;
        let manifest = manifest_path(&destination)?;
        crate::plugin::register_catalog_manifest(paths, profile, &manifest, &installs_root, mode)
            .map_err(|error| CatalogError::Plugin(error.to_string()))?;
        Ok(())
    })();
    if let Err(error) = result {
        if staging.starts_with(&staging_root) {
            let _ = std::fs::remove_dir_all(&staging);
        }
        if destination.starts_with(&plugin_root) {
            let _ = std::fs::remove_dir_all(&destination);
        }
        return Err(error);
    }
    Ok(format!(
        "{} catalog plugin {plugin_id} {} (disabled); use `ca plugin enable {plugin_id} --confirm {plugin_id}` explicitly",
        match mode {
            crate::plugin::CatalogRegistration::Install => "Installed",
            crate::plugin::CatalogRegistration::Replace => "Updated",
        },
        entry.version
    ))
}

fn rollback(
    catalog_root: &Path,
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    plugin: &str,
    version: &str,
    confirm: &str,
) -> Result<String, CatalogError> {
    validate_id(plugin)?;
    validate_version(version)?;
    require_confirmation(plugin, confirm)?;
    let installs_root = catalog_root.join("installs");
    let plugin_root = installs_root.join(plugin);
    reject_symlink_components(&plugin_root)?;
    let prefix = format!("{version}-");
    let mut matches = std::fs::read_dir(&plugin_root)
        .map_err(|_| CatalogError::MissingVersion)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
                && entry.file_name().to_string_lossy().starts_with(&prefix)
        })
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    matches.sort();
    if matches.len() != 1 {
        return Err(CatalogError::MissingVersion);
    }
    let retained = matches.remove(0);
    let directory_name = retained
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(CatalogError::MissingVersion)?;
    let expected_hash = directory_name
        .strip_prefix(&prefix)
        .ok_or(CatalogError::MissingVersion)?;
    validate_sha256(expected_hash).map_err(|_| CatalogError::MissingVersion)?;
    verify_retained_package(&retained, plugin, version, expected_hash)?;
    let manifest = manifest_path(&retained)?;
    crate::plugin::register_catalog_manifest(
        paths,
        profile,
        &manifest,
        &installs_root,
        crate::plugin::CatalogRegistration::Replace,
    )
    .map_err(|error| CatalogError::Plugin(error.to_string()))?;
    Ok(format!(
        "Rolled back catalog plugin {plugin} to {version} (disabled)"
    ))
}

fn validate_package(
    package: &PluginPackage,
    entry: &CatalogEntry,
) -> Result<Vec<(PathBuf, Vec<u8>)>, CatalogError> {
    if package.schema != PACKAGE_SCHEMA
        || package.plugin_id != entry.plugin_id
        || package.version != entry.version
        || package.files.is_empty()
        || package.files.len() > MAX_FILES
    {
        return Err(CatalogError::InvalidPackage);
    }
    let mut paths = BTreeSet::new();
    let mut total = 0_usize;
    let mut decoded = Vec::with_capacity(package.files.len());
    let mut manifests = 0;
    for file in &package.files {
        let relative = PathBuf::from(&file.path);
        validate_relative(&relative).map_err(|_| CatalogError::InvalidPackage)?;
        validate_sha256(&file.sha256).map_err(|_| CatalogError::InvalidPackage)?;
        if file.path == ".ca-package.json" || !paths.insert(relative.clone()) {
            return Err(CatalogError::InvalidPackage);
        }
        if matches!(file.path.as_str(), "plugin.toml" | "plugin.json") {
            manifests += 1;
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&file.content)
            .map_err(|_| CatalogError::InvalidPackage)?;
        total = total.saturating_add(bytes.len());
        if total > MAX_PACKAGE_CONTENT_BYTES || sha256_hex(&bytes) != file.sha256 {
            return Err(CatalogError::InvalidPackage);
        }
        decoded.push((relative, bytes));
    }
    if manifests != 1 {
        return Err(CatalogError::InvalidPackage);
    }
    Ok(decoded)
}

fn verify_retained_package(
    root: &Path,
    plugin: &str,
    version: &str,
    expected_hash: &str,
) -> Result<(), CatalogError> {
    let receipt = canonical_regular(&root.join(".ca-package.json"), MAX_PACKAGE_BYTES)
        .map_err(|_| CatalogError::InvalidPackage)?;
    if !receipt.starts_with(root) {
        return Err(CatalogError::InvalidPackage);
    }
    let bytes =
        read_bounded(&receipt, MAX_PACKAGE_BYTES).map_err(|_| CatalogError::InvalidPackage)?;
    if sha256_hex(&bytes) != expected_hash {
        return Err(CatalogError::InvalidPackage);
    }
    let package: PluginPackage =
        serde_json::from_slice(&bytes).map_err(|_| CatalogError::InvalidPackage)?;
    let entry = CatalogEntry {
        plugin_id: plugin.into(),
        version: version.into(),
        package: "retained.json".into(),
        sha256: expected_hash.into(),
    };
    let files = validate_package(&package, &entry)?;
    let expected = files
        .iter()
        .map(|(path, _)| path.clone())
        .chain(std::iter::once(PathBuf::from(".ca-package.json")))
        .collect::<BTreeSet<_>>();
    let actual = collect_installed_files(root)?;
    if actual != expected {
        return Err(CatalogError::InvalidPackage);
    }
    for (relative, expected) in files {
        let path = canonical_regular(&root.join(&relative), MAX_PACKAGE_CONTENT_BYTES as u64)
            .map_err(|_| CatalogError::InvalidPackage)?;
        if !path.starts_with(root)
            || read_bounded(&path, MAX_PACKAGE_CONTENT_BYTES as u64)
                .map_err(|_| CatalogError::InvalidPackage)?
                != expected
        {
            return Err(CatalogError::InvalidPackage);
        }
    }
    Ok(())
}

fn collect_installed_files(root: &Path) -> Result<BTreeSet<PathBuf>, CatalogError> {
    fn visit(
        root: &Path,
        directory: &Path,
        files: &mut BTreeSet<PathBuf>,
    ) -> Result<(), CatalogError> {
        for entry in std::fs::read_dir(directory).map_err(|_| CatalogError::InvalidPackage)? {
            let entry = entry.map_err(|_| CatalogError::InvalidPackage)?;
            let kind = entry
                .file_type()
                .map_err(|_| CatalogError::InvalidPackage)?;
            if kind.is_symlink() {
                return Err(CatalogError::InvalidPackage);
            }
            if kind.is_dir() {
                visit(root, &entry.path(), files)?;
            } else if kind.is_file() {
                files.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .map_err(|_| CatalogError::InvalidPackage)?
                        .to_path_buf(),
                );
            } else {
                return Err(CatalogError::InvalidPackage);
            }
            if files.len() > MAX_FILES + 1 {
                return Err(CatalogError::InvalidPackage);
            }
        }
        Ok(())
    }
    let mut files = BTreeSet::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

fn manifest_path(root: &Path) -> Result<PathBuf, CatalogError> {
    let toml = root.join("plugin.toml");
    let json = root.join("plugin.json");
    match (toml.is_file(), json.is_file()) {
        (true, false) => Ok(toml),
        (false, true) => Ok(json),
        _ => Err(CatalogError::InvalidPackage),
    }
}

fn load_public_key(path: &Path) -> Result<String, CatalogError> {
    let path = canonical_regular(path, MAX_KEY_BYTES).map_err(|_| CatalogError::InvalidKey)?;
    let bytes = read_bounded(&path, MAX_KEY_BYTES).map_err(|_| CatalogError::InvalidKey)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| CatalogError::InvalidKey)?;
    let key = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|_| CatalogError::InvalidKey)?;
    if key.len() != 32 {
        return Err(CatalogError::InvalidKey);
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(key))
}

fn resolve_source(root: &Path, relative: &str) -> Result<PathBuf, CatalogError> {
    validate_relative(Path::new(relative))?;
    let path = root.join(relative);
    let canonical =
        canonical_regular(&path, MAX_PACKAGE_BYTES).map_err(|_| CatalogError::InvalidResource)?;
    if !canonical.starts_with(root) {
        return Err(CatalogError::InvalidResource);
    }
    Ok(canonical)
}

fn canonical_regular(path: &Path, max: u64) -> Result<PathBuf, CatalogError> {
    if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(CatalogError::InvalidResource);
    }
    let path = path
        .canonicalize()
        .map_err(|_| CatalogError::InvalidResource)?;
    reject_symlink_components(&path)?;
    let metadata = std::fs::metadata(&path).map_err(|_| CatalogError::InvalidResource)?;
    if !metadata.is_file() || metadata.len() > max {
        return Err(CatalogError::InvalidResource);
    }
    Ok(path)
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

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), CatalogError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| CatalogError::Io(error.to_string()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| CatalogError::Io(error.to_string()))
}

fn ensure_private_dir(path: &Path) -> Result<(), CatalogError> {
    reject_symlink_components(path)?;
    std::fs::create_dir_all(path).map_err(|error| CatalogError::Io(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| CatalogError::Io(error.to_string()))?;
    }
    Ok(())
}

fn check_owner_only(metadata: &std::fs::Metadata) -> Result<(), CatalogError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(CatalogError::InvalidState);
        }
    }
    let _ = metadata;
    Ok(())
}

fn reject_symlink_components(path: &Path) -> Result<(), CatalogError> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if std::fs::symlink_metadata(ancestor)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(CatalogError::InvalidResource);
        }
    }
    Ok(())
}

fn validate_relative(path: &Path) -> Result<(), CatalogError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            !matches!(component, Component::Normal(_))
                || component.as_os_str().to_str().is_none_or(|value| {
                    value.is_empty()
                        || value.len() > 255
                        || value.contains('\\')
                        || value.chars().any(char::is_control)
                })
        })
        || path.as_os_str().as_encoded_bytes().len() > 1024
    {
        return Err(CatalogError::InvalidResource);
    }
    Ok(())
}

fn validate_id(value: &str) -> Result<(), CatalogError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        });
    if valid {
        Ok(())
    } else {
        Err(CatalogError::InvalidId)
    }
}

fn validate_version(value: &str) -> Result<(), CatalogError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(CatalogError::InvalidVersion)
    }
}

fn validate_sha256(value: &str) -> Result<(), CatalogError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(CatalogError::InvalidIndex)
    }
}

fn require_confirmation(id: &str, confirm: &str) -> Result<(), CatalogError> {
    if id == confirm {
        Ok(())
    } else {
        Err(CatalogError::Confirmation)
    }
}

fn split_name(name: &str) -> Result<(&str, &str), CatalogError> {
    let (catalog, plugin) = name.split_once('/').ok_or(CatalogError::InvalidId)?;
    if plugin.contains('/') {
        return Err(CatalogError::InvalidId);
    }
    validate_id(catalog)?;
    validate_id(plugin)?;
    Ok((catalog, plugin))
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

    fn key_pair() -> Ed25519KeyPair {
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap()
    }

    fn write_key(root: &Path, key: &Ed25519KeyPair) -> PathBuf {
        let path = root.join("catalog.pub");
        std::fs::write(
            &path,
            base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref()),
        )
        .unwrap();
        path
    }

    fn package(root: &Path, version: &str, body: &str) -> (String, String) {
        let manifest =
            "version = 1\nid = \"daily\"\nname = \"Daily\"\nskills = [\"skills\"]\n".to_string();
        let skill = format!("---\nname: daily-skill\ndescription: Daily\n---\n{body}");
        let files = [
            ("plugin.toml", manifest),
            ("skills/daily-skill/SKILL.md", skill),
        ]
        .into_iter()
        .map(|(path, content)| PackageFile {
            path: path.into(),
            sha256: sha256_hex(content.as_bytes()),
            content: base64::engine::general_purpose::STANDARD.encode(content),
        })
        .collect();
        let bytes = serde_json::to_vec(&PluginPackage {
            schema: PACKAGE_SCHEMA.into(),
            plugin_id: "daily".into(),
            version: version.into(),
            files,
        })
        .unwrap();
        let name = format!("daily-{version}.json");
        std::fs::write(root.join(&name), &bytes).unwrap();
        (name, sha256_hex(&bytes))
    }

    fn index(
        root: &Path,
        key: &Ed25519KeyPair,
        revision: u64,
        version: &str,
        package: &str,
        hash: &str,
    ) -> PathBuf {
        let payload = serde_json::to_vec(&CatalogPayload {
            catalog_id: "official".into(),
            revision,
            entries: vec![CatalogEntry {
                plugin_id: "daily".into(),
                version: version.into(),
                package: package.into(),
                sha256: hash.into(),
            }],
        })
        .unwrap();
        let envelope = CatalogEnvelope {
            schema: ENVELOPE_SCHEMA.into(),
            payload: base64::engine::general_purpose::STANDARD.encode(&payload),
            signature: base64::engine::general_purpose::STANDARD
                .encode(key.sign(&payload).as_ref()),
        };
        let path = root.join(format!("index-{revision}.json"));
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        path
    }

    fn trust(paths: &crate::runtime::RuntimePaths, key_path: PathBuf) -> Result<(), CatalogError> {
        run(
            CatalogCommand::Trust {
                id: "official".into(),
                public_key: key_path,
                confirm: "official".into(),
            },
            paths,
            &crate::runtime::ActiveProfile::Default,
        )
    }

    #[test]
    fn signature_revision_and_profile_store_are_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path());
        let key = key_pair();
        trust(&paths, write_key(root.path(), &key)).unwrap();
        let (package_name, hash) = package(root.path(), "1.0.0", "one");
        let first = index(root.path(), &key, 1, "1.0.0", &package_name, &hash);
        run(
            CatalogCommand::Sync {
                id: "official".into(),
                index: first.clone(),
            },
            &paths,
            &crate::runtime::ActiveProfile::Default,
        )
        .unwrap();
        run(
            CatalogCommand::Sync {
                id: "official".into(),
                index: first,
            },
            &paths,
            &crate::runtime::ActiveProfile::Default,
        )
        .unwrap();

        let (changed_package, changed_hash) = package(root.path(), "1.0.1", "changed");
        let stale = index(
            root.path(),
            &key,
            1,
            "1.0.1",
            &changed_package,
            &changed_hash,
        );
        assert!(matches!(
            run(
                CatalogCommand::Sync {
                    id: "official".into(),
                    index: stale,
                },
                &paths,
                &crate::runtime::ActiveProfile::Default,
            ),
            Err(CatalogError::StaleRevision)
        ));

        let other = key_pair();
        let conflicting = index(root.path(), &other, 2, "1.0.0", &package_name, &hash);
        assert!(matches!(
            run(
                CatalogCommand::Sync {
                    id: "official".into(),
                    index: conflicting,
                },
                &paths,
                &crate::runtime::ActiveProfile::Default,
            ),
            Err(CatalogError::Signature)
        ));
        let state_path = paths
            .plugin_catalog_dir(&crate::runtime::ActiveProfile::Default)
            .join("catalogs.json");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(std::fs::metadata(state_path).unwrap().mode() & 0o777, 0o600);
        }
        assert!(!paths
            .plugin_catalog_dir(&crate::runtime::ActiveProfile::Named("other".into()))
            .exists());
    }

    #[test]
    fn signed_install_update_and_rollback_remain_disabled_and_retain_versions() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path());
        let profile = crate::runtime::ActiveProfile::Default;
        let key = key_pair();
        trust(&paths, write_key(root.path(), &key)).unwrap();
        let (one_package, one_hash) = package(root.path(), "1.0.0", "one");
        let one_index = index(root.path(), &key, 1, "1.0.0", &one_package, &one_hash);
        run(
            CatalogCommand::Sync {
                id: "official".into(),
                index: one_index,
            },
            &paths,
            &profile,
        )
        .unwrap();
        run(
            CatalogCommand::Install {
                name: "official/daily".into(),
                confirm: "daily".into(),
            },
            &paths,
            &profile,
        )
        .unwrap();

        let (two_package, two_hash) = package(root.path(), "2.0.0", "two");
        let two_index = index(root.path(), &key, 2, "2.0.0", &two_package, &two_hash);
        run(
            CatalogCommand::Sync {
                id: "official".into(),
                index: two_index,
            },
            &paths,
            &profile,
        )
        .unwrap();
        run(
            CatalogCommand::Update {
                name: "official/daily".into(),
                confirm: "daily".into(),
            },
            &paths,
            &profile,
        )
        .unwrap();
        run(
            CatalogCommand::Rollback {
                plugin: "daily".into(),
                to: "1.0.0".into(),
                confirm: "daily".into(),
            },
            &paths,
            &profile,
        )
        .unwrap();

        let registry =
            std::fs::read_to_string(paths.plugins_dir(&profile).join("plugins.json")).unwrap();
        assert!(registry.contains("1.0.0-"));
        assert!(registry.contains("\"enabled\": false"));
        let installs = paths.plugin_catalog_dir(&profile).join("installs/daily");
        assert_eq!(std::fs::read_dir(installs).unwrap().count(), 2);
    }

    #[test]
    fn package_hash_traversal_and_confirmation_fail_before_plugin_registration() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path());
        let profile = crate::runtime::ActiveProfile::Default;
        let key = key_pair();
        trust(&paths, write_key(root.path(), &key)).unwrap();
        let (package_name, hash) = package(root.path(), "1.0.0", "one");
        let signed = index(root.path(), &key, 1, "1.0.0", &package_name, &hash);
        run(
            CatalogCommand::Sync {
                id: "official".into(),
                index: signed,
            },
            &paths,
            &profile,
        )
        .unwrap();
        assert!(matches!(
            run(
                CatalogCommand::Install {
                    name: "official/daily".into(),
                    confirm: "wrong".into(),
                },
                &paths,
                &profile,
            ),
            Err(CatalogError::Confirmation)
        ));
        std::fs::write(root.path().join(&package_name), b"changed").unwrap();
        assert!(matches!(
            run(
                CatalogCommand::Install {
                    name: "official/daily".into(),
                    confirm: "daily".into(),
                },
                &paths,
                &profile,
            ),
            Err(CatalogError::InvalidPackage)
        ));
        assert!(!paths.plugins_dir(&profile).join("plugins.json").exists());

        let invalid = PluginPackage {
            schema: PACKAGE_SCHEMA.into(),
            plugin_id: "daily".into(),
            version: "1.0.0".into(),
            files: vec![PackageFile {
                path: "../plugin.toml".into(),
                sha256: sha256_hex(b"x"),
                content: base64::engine::general_purpose::STANDARD.encode(b"x"),
            }],
        };
        assert!(matches!(
            validate_package(
                &invalid,
                &CatalogEntry {
                    plugin_id: "daily".into(),
                    version: "1.0.0".into(),
                    package: "daily.json".into(),
                    sha256: sha256_hex(b"package"),
                }
            ),
            Err(CatalogError::InvalidPackage)
        ));
    }

    #[test]
    fn catalog_update_cannot_replace_a_local_source_plugin() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path());
        let profile = crate::runtime::ActiveProfile::Default;
        let local = root.path().join("local");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(
            local.join("plugin.toml"),
            "version = 1\nid = \"daily\"\nname = \"Local daily\"\n",
        )
        .unwrap();
        crate::plugin::register_catalog_manifest(
            &paths,
            &profile,
            &local.join("plugin.toml"),
            &root.path().join("unrelated-catalog-root"),
            crate::plugin::CatalogRegistration::Install,
        )
        .unwrap();

        let key = key_pair();
        trust(&paths, write_key(root.path(), &key)).unwrap();
        let (package_name, hash) = package(root.path(), "1.0.0", "catalog");
        let signed = index(root.path(), &key, 1, "1.0.0", &package_name, &hash);
        run(
            CatalogCommand::Sync {
                id: "official".into(),
                index: signed,
            },
            &paths,
            &profile,
        )
        .unwrap();
        assert!(matches!(
            run(
                CatalogCommand::Update {
                    name: "official/daily".into(),
                    confirm: "daily".into(),
                },
                &paths,
                &profile,
            ),
            Err(CatalogError::Plugin(message)) if message.contains("cannot replace a local-source plugin")
        ));
        let registry =
            std::fs::read_to_string(paths.plugins_dir(&profile).join("plugins.json")).unwrap();
        assert!(registry.contains("Local daily"));
        let installs = paths.plugin_catalog_dir(&profile).join("installs/daily");
        assert!(
            !installs.exists() || std::fs::read_dir(installs).unwrap().next().is_none(),
            "failed update must remove its staged catalog version"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_catalog_package_is_rejected_before_staging() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path());
        let profile = crate::runtime::ActiveProfile::Default;
        let key = key_pair();
        trust(&paths, write_key(root.path(), &key)).unwrap();
        let (package_name, hash) = package(root.path(), "1.0.0", "one");
        let real = root.path().join("real-package.json");
        std::fs::rename(root.path().join(&package_name), &real).unwrap();
        symlink(&real, root.path().join(&package_name)).unwrap();
        let signed = index(root.path(), &key, 1, "1.0.0", &package_name, &hash);
        run(
            CatalogCommand::Sync {
                id: "official".into(),
                index: signed,
            },
            &paths,
            &profile,
        )
        .unwrap();
        assert!(matches!(
            run(
                CatalogCommand::Install {
                    name: "official/daily".into(),
                    confirm: "daily".into(),
                },
                &paths,
                &profile,
            ),
            Err(CatalogError::InvalidResource)
        ));
        assert!(!paths.plugins_dir(&profile).join("plugins.json").exists());
    }
}
