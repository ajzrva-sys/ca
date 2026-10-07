use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

const MANIFEST_VERSION: u32 = 1;
const STORE_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
const MAX_STORE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_BUNDLE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_BUNDLE_FILES: usize = 1024;
const MAX_PLUGINS: usize = 128;
const MAX_EPHEMERAL_PLUGINS: usize = 32;

/// Versioned lifecycle facade made available to trusted in-process plugin
/// adapters. Declarative plugin manifests do not receive this authority by
/// default; the host must pass the facade explicitly.
#[allow(dead_code)] // constructed only by optional trusted in-process adapters
pub fn agent_lifecycle_api(
    manager: std::sync::Arc<crate::agent::subagent::AgentManager>,
) -> crate::agent::lifecycle::AgentLifecycleService {
    crate::agent::lifecycle::AgentLifecycleService::new(manager)
}

#[derive(Debug, Parser)]
#[command(
    name = "ca plugin",
    bin_name = "ca plugin",
    about = "Manage explicit local plugin bundles"
)]
struct RawPluginArgs {
    #[arg(long, value_name = "NAME")]
    profile: Option<String>,

    #[command(subcommand)]
    command: PluginCommand,
}

#[derive(Debug, Subcommand)]
enum PluginCommand {
    /// List safe plugin metadata
    List,
    /// Validate and add a local plugin manifest, disabled
    Add { manifest: PathBuf },
    /// Show safe contributions, capabilities, and collisions
    Show { id: String },
    /// Enable an unchanged collision-free bundle
    Enable {
        id: String,
        #[arg(long, value_name = "ID")]
        confirm: String,
    },
    /// Disable a plugin bundle
    Disable { id: String },
    /// Forget a disabled plugin without deleting its source
    Remove {
        id: String,
        #[arg(long, value_name = "ID")]
        confirm: String,
    },
    /// Resolve an enabled plugin workflow to its exact local file
    Workflow { name: String },
    /// Apply an enabled plugin's alias for an existing built-in theme
    Theme { name: String },
    /// Manage signed curated catalog snapshots and staged versions
    Catalog {
        #[command(subcommand)]
        command: crate::plugin_catalog::CatalogCommand,
    },
}

#[derive(Debug)]
pub struct PluginCliArgs {
    active_profile: crate::runtime::ActiveProfile,
    command: PluginCommand,
}

pub fn parse_args<I, T>(args: I) -> Result<PluginCliArgs, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let raw = RawPluginArgs::try_parse_from(args)?;
    let profile =
        crate::runtime::ActiveProfile::parse(raw.profile.as_deref()).map_err(|error| {
            <RawPluginArgs as clap::CommandFactory>::command()
                .error(clap::error::ErrorKind::InvalidValue, error.to_string())
        })?;
    if matches!(profile, crate::runtime::ActiveProfile::Bare) {
        return Err(<RawPluginArgs as clap::CommandFactory>::command().error(
            clap::error::ErrorKind::ArgumentConflict,
            "--profile bare disables extensions; plugins are unavailable",
        ));
    }
    Ok(PluginCliArgs {
        active_profile: profile,
        command: raw.command,
    })
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    #[serde(default = "manifest_version")]
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub mcp: HashMap<String, crate::config::McpServerConfig>,
    #[serde(default)]
    pub hooks: Vec<crate::config::HookConfig>,
    #[serde(default)]
    pub workflows: HashMap<String, String>,
    #[serde(default)]
    pub themes: HashMap<String, crate::ui_settings::UiTheme>,
}

impl std::fmt::Debug for PluginManifest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginManifest")
            .field("version", &self.version)
            .field("id", &self.id)
            .field("name", &self.name)
            .field("description", &self.description)
            .field("skill_root_count", &self.skills.len())
            .field("mcp_names", &sorted_keys(&self.mcp))
            .field("hook_count", &self.hooks.len())
            .field("workflow_names", &sorted_keys(&self.workflows))
            .field("theme_names", &sorted_keys(&self.themes))
            .finish()
    }
}

fn manifest_version() -> u32 {
    MANIFEST_VERSION
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PluginRecord {
    manifest_path: PathBuf,
    bundle_root: PathBuf,
    digest: String,
    #[serde(default)]
    catalog_digest: Option<String>,
    manifest: PluginManifest,
    enabled: bool,
}

impl std::fmt::Debug for PluginRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginRecord")
            .field("id", &self.manifest.id)
            .field("bundle_root", &self.bundle_root)
            .field("digest", &self.digest)
            .field("catalog_content_pinned", &self.catalog_digest.is_some())
            .field("enabled", &self.enabled)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PluginStoreData {
    version: u32,
    revision: u64,
    records: Vec<PluginRecord>,
}

impl Default for PluginStoreData {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            revision: 0,
            records: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginCatalog {
    pub workflows: BTreeMap<String, PathBuf>,
    pub themes: BTreeMap<String, crate::ui_settings::UiTheme>,
    pub enabled: Vec<String>,
    pub ephemeral: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("plugin manifest must be a bounded regular non-symlink .toml or .json file")]
    InvalidManifest,
    #[error("plugin manifest could not be parsed: {0}")]
    Parse(String),
    #[error("unsupported plugin manifest version: {0}")]
    UnsupportedVersion(u32),
    #[error("plugin IDs and contribution names must be 1-64 lowercase ASCII letters, digits, '-' or '_', starting with a letter")]
    InvalidId,
    #[error("plugin metadata is invalid")]
    InvalidMetadata,
    #[error("plugin resource is invalid or escapes its bundle: {0}")]
    InvalidResource(String),
    #[error("plugin MCP definition is invalid: {0}")]
    InvalidMcp(String),
    #[error("plugin hook definition is invalid")]
    InvalidHook,
    #[error("plugin bundle exceeds its file or byte bound")]
    BundleLimit,
    #[error("plugin already exists: {0}")]
    Duplicate(String),
    #[error("plugin was not found: {0}")]
    Missing(String),
    #[error("plugin activation/removal requires --confirm with the exact plugin ID")]
    Confirmation,
    #[error("plugin changed after it was added: {0}")]
    Drift(String),
    #[error("plugin contribution collision: {0}")]
    Collision(String),
    #[error("plugin must be disabled before removal")]
    EnabledRemoval,
    #[error("plugin registry is invalid: {0}")]
    State(String),
    #[error("plugin I/O failed: {0}")]
    Io(String),
    #[error("plugin catalog failed: {0}")]
    Catalog(String),
}

#[derive(Debug)]
struct PluginStore {
    dir: PathBuf,
    path: PathBuf,
}

impl PluginStore {
    fn new(dir: PathBuf) -> Self {
        let dir = normalize_missing_path(&dir).unwrap_or(dir);
        Self {
            path: dir.join("plugins.json"),
            dir,
        }
    }

    fn load(&self) -> Result<PluginStoreData, PluginError> {
        if !self.path.exists() {
            return Ok(PluginStoreData::default());
        }
        reject_symlink_components(&self.path)?;
        let metadata = std::fs::symlink_metadata(&self.path)
            .map_err(|error| PluginError::Io(error.to_string()))?;
        if !metadata.is_file() || metadata.len() > MAX_STORE_BYTES {
            return Err(PluginError::State(
                "registry is not a bounded regular file".into(),
            ));
        }
        check_owner_only(&metadata)?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(&self.path)
            .and_then(|file| file.take(MAX_STORE_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|error| PluginError::Io(error.to_string()))?;
        if bytes.len() as u64 > MAX_STORE_BYTES {
            return Err(PluginError::State("registry is too large".into()));
        }
        let data: PluginStoreData = serde_json::from_slice(&bytes)
            .map_err(|error| PluginError::State(error.to_string()))?;
        if data.version != STORE_VERSION || data.records.len() > MAX_PLUGINS {
            return Err(PluginError::State(
                "unsupported or oversized registry".into(),
            ));
        }
        Ok(data)
    }

    fn save(&self, data: &mut PluginStoreData) -> Result<(), PluginError> {
        ensure_private_dir(&self.dir)?;
        data.revision = data.revision.wrapping_add(1);
        let bytes = serde_json::to_vec_pretty(data)
            .map_err(|error| PluginError::State(error.to_string()))?;
        if bytes.len() as u64 > MAX_STORE_BYTES {
            return Err(PluginError::State("registry is too large".into()));
        }
        let temporary = self.dir.join(format!(".plugins.{}.tmp", Uuid::new_v4()));
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
                .map_err(|error| PluginError::Io(error.to_string()))?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|error| PluginError::Io(error.to_string()))?;
            std::fs::rename(&temporary, &self.path)
                .map_err(|error| PluginError::Io(error.to_string()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

pub async fn run_cli(cli: PluginCliArgs) -> i32 {
    match run_cli_inner(cli) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ca plugin: {error}");
            match error {
                PluginError::Io(_) | PluginError::State(_) => 8,
                _ => 2,
            }
        }
    }
}

fn run_cli_inner(cli: PluginCliArgs) -> Result<(), PluginError> {
    let paths = crate::runtime::RuntimePaths::discover()
        .map_err(|error| PluginError::Io(error.to_string()))?;
    let store = PluginStore::new(paths.plugins_dir(&cli.active_profile));
    let config_paths = paths.clone().for_profile(&cli.active_profile);
    let config = crate::config::ConfigStore::for_profile(
        config_paths.config_file.clone(),
        cli.active_profile.clone(),
    )
    .load()
    .map_err(|error| PluginError::State(error.to_string()))?;
    match cli.command {
        PluginCommand::List => {
            print_list(&store.load()?);
            Ok(())
        }
        PluginCommand::Add { manifest } => {
            let record = load_manifest(&manifest)?;
            let id = record.manifest.id.clone();
            let mut data = store.load()?;
            if data.records.iter().any(|entry| entry.manifest.id == id) {
                return Err(PluginError::Duplicate(id));
            }
            if data.records.len() >= MAX_PLUGINS {
                return Err(PluginError::State("plugin limit reached".into()));
            }
            data.records.push(record);
            data.records
                .sort_by(|left, right| left.manifest.id.cmp(&right.manifest.id));
            store.save(&mut data)?;
            println!("Added plugin {id} (disabled)");
            Ok(())
        }
        PluginCommand::Show { id } => {
            validate_id(&id)?;
            let data = store.load()?;
            let record = find_record(&data, &id)?;
            print_show(record, &data, &config, &paths);
            Ok(())
        }
        PluginCommand::Enable { id, confirm } => {
            if id != confirm {
                return Err(PluginError::Confirmation);
            }
            let mut data = store.load()?;
            let index = find_index(&data, &id)?;
            verify_digest(&data.records[index])?;
            collision_check(index, &data, &config, &paths)?;
            data.records[index].enabled = true;
            store.save(&mut data)?;
            println!("Enabled plugin {id}; changes apply on the next ca launch");
            Ok(())
        }
        PluginCommand::Disable { id } => {
            let mut data = store.load()?;
            let index = find_index(&data, &id)?;
            data.records[index].enabled = false;
            store.save(&mut data)?;
            println!("Disabled plugin {id}; changes apply on the next ca launch");
            Ok(())
        }
        PluginCommand::Remove { id, confirm } => {
            if id != confirm {
                return Err(PluginError::Confirmation);
            }
            let mut data = store.load()?;
            let index = find_index(&data, &id)?;
            if data.records[index].enabled {
                return Err(PluginError::EnabledRemoval);
            }
            data.records.remove(index);
            store.save(&mut data)?;
            println!("Removed plugin {id}; source bundle was not deleted");
            Ok(())
        }
        PluginCommand::Workflow { name } => {
            let mut composed = config.clone();
            let catalog = compose_enabled(&paths, &cli.active_profile, &mut composed)?;
            let path = catalog
                .workflows
                .get(&name)
                .ok_or_else(|| PluginError::Missing(name.clone()))?;
            println!("{}", path.display());
            Ok(())
        }
        PluginCommand::Theme { name } => {
            let mut composed = config.clone();
            let catalog = compose_enabled(&paths, &cli.active_profile, &mut composed)?;
            let theme = catalog
                .themes
                .get(&name)
                .copied()
                .ok_or_else(|| PluginError::Missing(name.clone()))?;
            crate::config::ConfigStore::for_profile(config_paths.config_file, cli.active_profile)
                .update_ui_theme(theme)
                .map_err(|error| PluginError::Io(error.to_string()))?;
            println!("Theme alias {name} applied as {}", theme.label());
            Ok(())
        }
        PluginCommand::Catalog { command } => {
            crate::plugin_catalog::run(command, &paths, &cli.active_profile)
                .map_err(|error| PluginError::Catalog(error.to_string()))
        }
    }
}

pub(crate) fn run_tui_action(
    base_paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    arguments: &str,
) -> Result<String, PluginError> {
    if matches!(profile, crate::runtime::ActiveProfile::Bare) {
        return Err(PluginError::State(
            "plugins are unavailable in the bare profile".into(),
        ));
    }
    let paths = base_paths.clone().for_profile(profile);
    let store = PluginStore::new(paths.plugins_dir(profile));
    let config =
        crate::config::ConfigStore::for_profile(paths.config_file.clone(), profile.clone())
            .load()
            .map_err(|error| PluginError::State(error.to_string()))?;
    let input = arguments.trim();
    if input.is_empty() || input == "list" {
        return Ok(render_list(&store.load()?, None));
    }
    if let Some(query) = input.strip_prefix("search ").map(str::trim) {
        if query.is_empty() || query.len() > 128 || query.chars().any(char::is_control) {
            return Err(PluginError::InvalidMetadata);
        }
        return Ok(render_list(&store.load()?, Some(query)));
    }
    if let Some(id) = input.strip_prefix("show ").map(str::trim) {
        validate_id(id)?;
        let data = store.load()?;
        return Ok(render_show(find_record(&data, id)?, &data, &config, &paths));
    }
    if let Some(path) = input.strip_prefix("add ").map(str::trim) {
        if path.is_empty() {
            return Err(PluginError::InvalidManifest);
        }
        let record = load_manifest(Path::new(path))?;
        let id = record.manifest.id.clone();
        let mut data = store.load()?;
        if data.records.iter().any(|entry| entry.manifest.id == id) {
            return Err(PluginError::Duplicate(id));
        }
        if data.records.len() >= MAX_PLUGINS {
            return Err(PluginError::State("plugin limit reached".into()));
        }
        data.records.push(record);
        data.records
            .sort_by(|left, right| left.manifest.id.cmp(&right.manifest.id));
        store.save(&mut data)?;
        return Ok(format!(
            "Added plugin {id} (disabled)\n\n{}",
            render_list(&data, None)
        ));
    }
    if let Some(rest) = input.strip_prefix("enable ").map(str::trim) {
        let (id, confirm) = parse_id_confirmation(rest)?;
        let data = store.load()?;
        if confirm.is_none() {
            let record = find_record(&data, id)?;
            return Ok(format!(
                "Plugin enable preview\n\n{}\n\nConfirm with: /plugin enable {id} confirm {id}",
                render_show(record, &data, &config, &paths)
            ));
        }
        let mut data = data;
        let index = find_index(&data, id)?;
        verify_digest(&data.records[index])?;
        collision_check(index, &data, &config, &paths)?;
        data.records[index].enabled = true;
        store.save(&mut data)?;
        return Ok(format!(
            "Enabled plugin {id}; changes apply on the next ca launch\n\n{}",
            render_list(&data, None)
        ));
    }
    if let Some(id) = input.strip_prefix("disable ").map(str::trim) {
        validate_id(id)?;
        let mut data = store.load()?;
        let index = find_index(&data, id)?;
        data.records[index].enabled = false;
        store.save(&mut data)?;
        return Ok(format!(
            "Disabled plugin {id}; changes apply on the next ca launch\n\n{}",
            render_list(&data, None)
        ));
    }
    if let Some(rest) = input.strip_prefix("remove ").map(str::trim) {
        let (id, confirm) = parse_id_confirmation(rest)?;
        let mut data = store.load()?;
        let index = find_index(&data, id)?;
        if data.records[index].enabled {
            return Err(PluginError::EnabledRemoval);
        }
        if confirm.is_none() {
            return Ok(format!(
                "Plugin removal preview\n\n{}\n\nConfirm with: /plugin remove {id} confirm {id}",
                render_show(&data.records[index], &data, &config, &paths)
            ));
        }
        data.records.remove(index);
        store.save(&mut data)?;
        return Ok(format!(
            "Removed plugin {id}; source bundle was not deleted\n\n{}",
            render_list(&data, None)
        ));
    }
    if let Some(rest) = input.strip_prefix("catalog").map(str::trim) {
        return match parse_tui_catalog(rest)? {
            TuiCatalogAction::Run(command) => {
                crate::plugin_catalog::run_for_tui(command, base_paths, profile)
                    .map_err(|error| PluginError::Catalog(error.to_string()))
            }
            TuiCatalogAction::Preview(preview) => Ok(preview),
        };
    }
    Err(PluginError::State(
        "usage: /plugin [list|search <query>|show <id>|add <manifest>|enable <id> [confirm <id>]|disable <id>|remove <id> [confirm <id>]|catalog <action>]"
            .into(),
    ))
}

fn parse_id_confirmation(input: &str) -> Result<(&str, Option<&str>), PluginError> {
    let fields = input.split_ascii_whitespace().collect::<Vec<_>>();
    let (id, confirmation) = match fields.as_slice() {
        [id] => (*id, None),
        [id, "confirm", confirm] if id == confirm => (*id, Some(*confirm)),
        [_, "confirm", _] => return Err(PluginError::Confirmation),
        _ => return Err(PluginError::InvalidMetadata),
    };
    validate_id(id)?;
    Ok((id, confirmation))
}

enum TuiCatalogAction {
    Run(crate::plugin_catalog::CatalogCommand),
    Preview(String),
}

fn parse_tui_catalog(input: &str) -> Result<TuiCatalogAction, PluginError> {
    let fields = input.split_ascii_whitespace().collect::<Vec<_>>();
    let invalid = || {
        PluginError::Catalog(
            "usage: /plugin catalog [list [id]|trust <id> <key> [confirm <id>]|untrust <id> [confirm <id>]|sync <id> <index>|install <catalog/id> [confirm <id>]|update <catalog/id> [confirm <id>]|rollback <id> <version> [confirm <id>]]"
                .into(),
        )
    };
    match fields.as_slice() {
        [] | ["list"] => Ok(TuiCatalogAction::Run(
            crate::plugin_catalog::CatalogCommand::List { id: None },
        )),
        ["list", id] => Ok(TuiCatalogAction::Run(
            crate::plugin_catalog::CatalogCommand::List {
            id: Some((*id).into()),
        })),
        ["trust", id, key] => Ok(TuiCatalogAction::Preview(format!(
            "Catalog trust preview: {id} from {key}. Confirm with: /plugin catalog trust {id} {key} confirm {id}"
        ))),
        ["trust", id, key, "confirm", confirm] => {
            Ok(TuiCatalogAction::Run(crate::plugin_catalog::CatalogCommand::Trust {
                id: (*id).into(),
                public_key: PathBuf::from(key),
                confirm: (*confirm).into(),
            }))
        }
        ["untrust", id] => Ok(TuiCatalogAction::Preview(format!(
            "Catalog untrust preview: {id}. Confirm with: /plugin catalog untrust {id} confirm {id}"
        ))),
        ["untrust", id, "confirm", confirm] => {
            Ok(TuiCatalogAction::Run(crate::plugin_catalog::CatalogCommand::Untrust {
                id: (*id).into(),
                confirm: (*confirm).into(),
            }))
        }
        ["sync", id, index] => Ok(TuiCatalogAction::Run(
            crate::plugin_catalog::CatalogCommand::Sync {
            id: (*id).into(),
            index: PathBuf::from(index),
        })),
        [action @ ("install" | "update"), name] => {
            let plugin = name.rsplit('/').next().ok_or_else(invalid)?;
            Ok(TuiCatalogAction::Preview(format!(
                "Catalog {action} preview: {name}. Confirm with: /plugin catalog {action} {name} confirm {plugin}"
            )))
        }
        ["install", name, "confirm", confirm] => {
            Ok(TuiCatalogAction::Run(crate::plugin_catalog::CatalogCommand::Install {
                name: (*name).into(),
                confirm: (*confirm).into(),
            }))
        }
        ["update", name, "confirm", confirm] => {
            Ok(TuiCatalogAction::Run(crate::plugin_catalog::CatalogCommand::Update {
                name: (*name).into(),
                confirm: (*confirm).into(),
            }))
        }
        ["rollback", plugin, version] => Ok(TuiCatalogAction::Preview(format!(
            "Catalog rollback preview: {plugin} to {version}. Confirm with: /plugin catalog rollback {plugin} {version} confirm {plugin}"
        ))),
        ["rollback", plugin, version, "confirm", confirm] => {
            Ok(TuiCatalogAction::Run(crate::plugin_catalog::CatalogCommand::Rollback {
                plugin: (*plugin).into(),
                to: (*version).into(),
                confirm: (*confirm).into(),
            }))
        }
        _ => Err(invalid()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CatalogRegistration {
    Install,
    Replace,
}

pub(crate) fn register_catalog_manifest(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    manifest_path: &Path,
    installs_root: &Path,
    mode: CatalogRegistration,
) -> Result<String, PluginError> {
    let mut record = load_manifest(manifest_path)?;
    record.enabled = false;
    record.catalog_digest = Some(full_bundle_digest(&record.bundle_root)?);
    let id = record.manifest.id.clone();
    let store = PluginStore::new(paths.plugins_dir(profile));
    let mut data = store.load()?;
    match (find_index(&data, &id), mode) {
        (Err(PluginError::Missing(_)), CatalogRegistration::Install) => {
            if data.records.len() >= MAX_PLUGINS {
                return Err(PluginError::State("plugin limit reached".into()));
            }
            data.records.push(record);
        }
        (Ok(_), CatalogRegistration::Install) => return Err(PluginError::Duplicate(id)),
        (Ok(index), CatalogRegistration::Replace) => {
            if !data.records[index].manifest_path.starts_with(installs_root) {
                return Err(PluginError::Catalog(
                    "catalog updates cannot replace a local-source plugin".into(),
                ));
            }
            data.records[index] = record;
        }
        (Err(error), CatalogRegistration::Replace) => return Err(error),
        (Err(error), CatalogRegistration::Install) => return Err(error),
    }
    data.records
        .sort_by(|left, right| left.manifest.id.cmp(&right.manifest.id));
    store.save(&mut data)?;
    Ok(id)
}

pub fn compose_enabled(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    config: &mut crate::config::CaConfig,
) -> Result<PluginCatalog, PluginError> {
    compose_enabled_with_ephemeral(paths, profile, config, &[])
}

pub fn compose_enabled_with_ephemeral(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    config: &mut crate::config::CaConfig,
    ephemeral_dirs: &[PathBuf],
) -> Result<PluginCatalog, PluginError> {
    if matches!(profile, crate::runtime::ActiveProfile::Bare) {
        if !ephemeral_dirs.is_empty() {
            return Err(PluginError::State(
                "bare profiles reject process-only plugin directories".into(),
            ));
        }
        return Ok(PluginCatalog::default());
    }
    let store = PluginStore::new(paths.plugins_dir(profile));
    let mut data = store.load()?;
    let persistent_count = data.records.len();
    let ephemeral = load_ephemeral_records(ephemeral_dirs)?;
    for record in ephemeral {
        if data
            .records
            .iter()
            .any(|entry| entry.manifest.id == record.manifest.id)
        {
            return Err(PluginError::Duplicate(record.manifest.id));
        }
        data.records.push(record);
    }
    let mut catalog = PluginCatalog::default();
    for (index, record) in data
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.enabled)
    {
        verify_digest(record)?;
        collision_check(index, &data, config, paths)?;
        for root in resolved_skill_roots(record)? {
            if !config.skills.compatibility_roots.contains(&root) {
                config.skills.compatibility_roots.push(root);
            }
        }
        for (name, server) in &record.manifest.mcp {
            let mut server = server.clone();
            for value in server.env.values_mut() {
                *value = crate::config::expand_env_vars(value);
            }
            config.mcp.servers.insert(name.clone(), server);
        }
        config.hooks.extend(record.manifest.hooks.clone());
        for (name, relative) in &record.manifest.workflows {
            catalog.workflows.insert(
                format!("{}/{}", record.manifest.id, name),
                resolve_resource(&record.bundle_root, relative, false)?,
            );
        }
        for (name, theme) in &record.manifest.themes {
            catalog
                .themes
                .insert(format!("{}/{}", record.manifest.id, name), *theme);
        }
        catalog.enabled.push(record.manifest.id.clone());
        if index >= persistent_count {
            catalog.ephemeral.push(record.manifest.id.clone());
        }
    }
    Ok(catalog)
}

fn load_ephemeral_records(directories: &[PathBuf]) -> Result<Vec<PluginRecord>, PluginError> {
    if directories.len() > MAX_EPHEMERAL_PLUGINS {
        return Err(PluginError::BundleLimit);
    }
    let mut canonical_roots = BTreeSet::new();
    let mut records = Vec::with_capacity(directories.len());
    for requested in directories {
        let metadata =
            std::fs::symlink_metadata(requested).map_err(|_| PluginError::InvalidManifest)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(PluginError::InvalidManifest);
        }
        let root = requested
            .canonicalize()
            .map_err(|_| PluginError::InvalidManifest)?;
        reject_symlink_components(&root)?;
        if !canonical_roots.insert(root.clone()) {
            return Err(PluginError::Duplicate("ephemeral plugin directory".into()));
        }
        let candidates = [root.join("plugin.toml"), root.join("plugin.json")]
            .into_iter()
            .filter(|path| path.exists())
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return Err(PluginError::InvalidManifest);
        }
        let mut record = load_manifest(&candidates[0])?;
        record.enabled = true;
        records.push(record);
    }
    Ok(records)
}

fn load_manifest(path: &Path) -> Result<PluginRecord, PluginError> {
    let exact = std::fs::symlink_metadata(path).map_err(|_| PluginError::InvalidManifest)?;
    if exact.file_type().is_symlink() {
        return Err(PluginError::InvalidManifest);
    }
    let path = path
        .canonicalize()
        .map_err(|_| PluginError::InvalidManifest)?;
    reject_symlink_components(&path)?;
    let metadata = std::fs::metadata(&path).map_err(|_| PluginError::InvalidManifest)?;
    if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
        return Err(PluginError::InvalidManifest);
    }
    let extension = path.extension().and_then(|value| value.to_str());
    if !matches!(extension, Some("toml" | "json")) {
        return Err(PluginError::InvalidManifest);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(&path)
        .and_then(|file| file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| PluginError::InvalidManifest)?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(PluginError::InvalidManifest);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| PluginError::InvalidManifest)?;
    let manifest: PluginManifest = if extension == Some("toml") {
        toml::from_str(text).map_err(|_| PluginError::Parse("invalid TOML".into()))?
    } else {
        serde_json::from_str(text).map_err(|_| PluginError::Parse("invalid JSON".into()))?
    };
    let bundle_root = path
        .parent()
        .ok_or(PluginError::InvalidManifest)?
        .to_path_buf();
    validate_manifest(&manifest, &bundle_root)?;
    let digest = bundle_digest(&path, &manifest, &bundle_root)?;
    Ok(PluginRecord {
        manifest_path: path,
        bundle_root,
        digest,
        catalog_digest: None,
        manifest,
        enabled: false,
    })
}

fn validate_manifest(manifest: &PluginManifest, root: &Path) -> Result<(), PluginError> {
    if manifest.version != MANIFEST_VERSION {
        return Err(PluginError::UnsupportedVersion(manifest.version));
    }
    validate_id(&manifest.id)?;
    if manifest.name.trim().is_empty()
        || manifest.name.len() > 128
        || manifest.name.chars().any(char::is_control)
        || manifest.description.len() > 1024
        || manifest.description.chars().any(char::is_control)
    {
        return Err(PluginError::InvalidMetadata);
    }
    if manifest.skills.len() > 32
        || manifest.mcp.len() > 32
        || manifest.hooks.len() > 64
        || manifest.workflows.len() > 64
        || manifest.themes.len() > 32
    {
        return Err(PluginError::BundleLimit);
    }
    let mut skill_roots = HashSet::new();
    for value in &manifest.skills {
        let path = resolve_resource(root, value, true)?;
        if !skill_roots.insert(path) {
            return Err(PluginError::Collision("duplicate skill root".into()));
        }
    }
    for (name, server) in &manifest.mcp {
        validate_id(name)?;
        crate::mcp::validate_server_config(server)
            .map_err(|error| PluginError::InvalidMcp(error.to_string()))?;
        if server.oauth.client_secret.is_some()
            || server
                .env
                .values()
                .chain(server.headers.values())
                .any(|value| !is_environment_reference(value))
        {
            return Err(PluginError::InvalidMcp(
                "plugin MCP credentials and environment values must use ${ENV_NAME} references"
                    .into(),
            ));
        }
    }
    for hook in &manifest.hooks {
        if crate::hooks::HookEvent::parse(&hook.event).is_none()
            || hook.command.trim().is_empty()
            || hook.command.len() > 4096
            || hook.command.contains('\0')
            || hook.timeout_secs == 0
            || hook.timeout_secs > 600
        {
            return Err(PluginError::InvalidHook);
        }
    }
    for (name, path) in &manifest.workflows {
        validate_id(name)?;
        let path = resolve_resource(root, path, false)?;
        let workspace = crate::workspace::WorkspaceContext::from_primary(root.into(), root.into());
        crate::workflow::load_workflow(&workspace, path.to_string_lossy().as_ref())
            .map_err(|_| PluginError::InvalidResource(name.clone()))?;
    }
    for name in manifest.themes.keys() {
        validate_id(name)?;
    }
    Ok(())
}

fn verify_digest(record: &PluginRecord) -> Result<(), PluginError> {
    let digest = bundle_digest(&record.manifest_path, &record.manifest, &record.bundle_root)?;
    let catalog_matches = record.catalog_digest.as_ref().is_none_or(|expected| {
        full_bundle_digest(&record.bundle_root).is_ok_and(|digest| digest == *expected)
    });
    if digest == record.digest && catalog_matches {
        Ok(())
    } else {
        Err(PluginError::Drift(record.manifest.id.clone()))
    }
}

fn bundle_digest(
    manifest_path: &Path,
    manifest: &PluginManifest,
    root: &Path,
) -> Result<String, PluginError> {
    let mut paths = vec![manifest_path.to_path_buf()];
    for skill_root in &manifest.skills {
        collect_tree(&resolve_resource(root, skill_root, true)?, &mut paths)?;
    }
    for workflow in manifest.workflows.values() {
        paths.push(resolve_resource(root, workflow, false)?);
    }
    paths.sort();
    paths.dedup();
    digest_paths(root, paths)
}

fn full_bundle_digest(root: &Path) -> Result<String, PluginError> {
    let mut paths = Vec::new();
    collect_tree(root, &mut paths)?;
    paths.sort();
    paths.dedup();
    digest_paths(root, paths)
}

fn digest_paths(root: &Path, paths: Vec<PathBuf>) -> Result<String, PluginError> {
    if paths.len() > MAX_BUNDLE_FILES {
        return Err(PluginError::BundleLimit);
    }
    let mut total = 0_u64;
    let mut digest = Sha256::new();
    for path in paths {
        let metadata = std::fs::metadata(&path)
            .map_err(|_| PluginError::InvalidResource(path.display().to_string()))?;
        if !metadata.is_file() {
            continue;
        }
        total = total.saturating_add(metadata.len());
        if total > MAX_BUNDLE_BYTES {
            return Err(PluginError::BundleLimit);
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| PluginError::InvalidResource(path.display().to_string()))?;
        digest.update(relative.as_os_str().as_encoded_bytes());
        let mut file = File::open(&path).map_err(|error| PluginError::Io(error.to_string()))?;
        let mut buffer = [0_u8; 8192];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|error| PluginError::Io(error.to_string()))?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn collect_tree(path: &Path, paths: &mut Vec<PathBuf>) -> Result<(), PluginError> {
    reject_symlink_components(path)?;
    for entry in std::fs::read_dir(path).map_err(|error| PluginError::Io(error.to_string()))? {
        let entry = entry.map_err(|error| PluginError::Io(error.to_string()))?;
        let metadata = entry
            .file_type()
            .map_err(|error| PluginError::Io(error.to_string()))?;
        if metadata.is_symlink() {
            return Err(PluginError::InvalidResource(
                entry.path().display().to_string(),
            ));
        }
        if metadata.is_dir() {
            collect_tree(&entry.path(), paths)?;
        } else if metadata.is_file() {
            paths.push(entry.path());
        } else {
            return Err(PluginError::InvalidResource(
                entry.path().display().to_string(),
            ));
        }
        if paths.len() > MAX_BUNDLE_FILES {
            return Err(PluginError::BundleLimit);
        }
    }
    Ok(())
}

fn collision_check(
    candidate: usize,
    data: &PluginStoreData,
    config: &crate::config::CaConfig,
    paths: &crate::runtime::RuntimePaths,
) -> Result<(), PluginError> {
    let record = &data.records[candidate];
    let mut mcp = config.mcp.servers.keys().cloned().collect::<BTreeSet<_>>();
    let mut hooks = config
        .hooks
        .iter()
        .map(|hook| (hook.event.clone(), hook.command.clone()))
        .collect::<HashSet<_>>();
    let mut workflows = BTreeSet::new();
    let mut themes = BTreeSet::new();
    let mut skills = base_skill_claims(config, paths);
    for (index, other) in data.records.iter().enumerate() {
        if index == candidate || !other.enabled {
            continue;
        }
        mcp.extend(other.manifest.mcp.keys().cloned());
        hooks.extend(
            other
                .manifest
                .hooks
                .iter()
                .map(|hook| (hook.event.clone(), hook.command.clone())),
        );
        workflows.extend(other.manifest.workflows.keys().cloned());
        themes.extend(other.manifest.themes.keys().cloned());
        skills.extend(skill_claims(other)?);
    }
    for name in record.manifest.mcp.keys() {
        if mcp.contains(name) {
            return Err(PluginError::Collision(format!("MCP server {name}")));
        }
    }
    for hook in &record.manifest.hooks {
        if hooks.contains(&(hook.event.clone(), hook.command.clone())) {
            return Err(PluginError::Collision(format!("hook event {}", hook.event)));
        }
    }
    for name in record.manifest.workflows.keys() {
        if workflows.contains(name) {
            return Err(PluginError::Collision(format!("workflow {name}")));
        }
    }
    for name in record.manifest.themes.keys() {
        if themes.contains(name) {
            return Err(PluginError::Collision(format!("theme {name}")));
        }
    }
    for identity in skill_claims(record)? {
        if skills.contains(&identity) {
            return Err(PluginError::Collision(format!("skill identity {identity}")));
        }
    }
    Ok(())
}

fn base_skill_claims(
    config: &crate::config::CaConfig,
    paths: &crate::runtime::RuntimePaths,
) -> BTreeSet<String> {
    let compatibility = config
        .skills
        .compatibility_roots
        .iter()
        .map(|path| {
            if path.is_absolute() {
                path.clone()
            } else {
                paths.config_root.join(path)
            }
        })
        .collect();
    let registry = crate::skills::SkillRegistry::discover(
        crate::skills::SkillRoots {
            project: None,
            project_boundary: None,
            user: Some(paths.user_skills_dir()),
            compatibility,
        },
        &config.skills.disabled,
    );
    registry
        .entries()
        .flat_map(|skill| std::iter::once(skill.id.clone()).chain(skill.aliases.iter().cloned()))
        .collect()
}

fn skill_claims(record: &PluginRecord) -> Result<BTreeSet<String>, PluginError> {
    let registry = crate::skills::SkillRegistry::discover(
        crate::skills::SkillRoots {
            project: None,
            project_boundary: None,
            user: None,
            compatibility: resolved_skill_roots(record)?,
        },
        &[],
    );
    if !registry.diagnostics().is_empty() {
        return Err(PluginError::InvalidResource(
            "plugin skill catalog contains invalid or colliding entries".into(),
        ));
    }
    Ok(registry
        .entries()
        .flat_map(|skill| std::iter::once(skill.id.clone()).chain(skill.aliases.iter().cloned()))
        .collect())
}

fn resolved_skill_roots(record: &PluginRecord) -> Result<Vec<PathBuf>, PluginError> {
    record
        .manifest
        .skills
        .iter()
        .map(|path| resolve_resource(&record.bundle_root, path, true))
        .collect()
}

fn resolve_resource(root: &Path, value: &str, directory: bool) -> Result<PathBuf, PluginError> {
    if value.is_empty() || value.len() > 4096 {
        return Err(PluginError::InvalidResource(value.into()));
    }
    let relative = Path::new(value);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(PluginError::InvalidResource(value.into()));
    }
    let unresolved = root.join(relative);
    reject_symlink_components(&unresolved)?;
    let path = unresolved
        .canonicalize()
        .map_err(|_| PluginError::InvalidResource(value.into()))?;
    if !path.starts_with(root) || (directory && !path.is_dir()) || (!directory && !path.is_file()) {
        return Err(PluginError::InvalidResource(value.into()));
    }
    Ok(path)
}

fn print_list(data: &PluginStoreData) {
    println!("{}", render_list(data, None));
}

fn render_list(data: &PluginStoreData, filter: Option<&str>) -> String {
    let filter = filter.map(str::to_ascii_lowercase);
    let lines = data
        .records
        .iter()
        .filter(|record| {
            filter.as_ref().is_none_or(|query| {
                record.manifest.id.to_ascii_lowercase().contains(query)
                    || record.manifest.name.to_ascii_lowercase().contains(query)
                    || record
                        .manifest
                        .description
                        .to_ascii_lowercase()
                        .contains(query)
                    || capability_labels(&record.manifest)
                        .iter()
                        .any(|label| label.contains(query))
            })
        })
        .map(|record| {
            format!(
                "{}\t{}\t{}\t{}",
                record.manifest.id,
                if record.enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                record.manifest.name,
                capability_labels(&record.manifest).join(",")
            )
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        if filter.is_some() {
            "No plugins match the current search".into()
        } else {
            "No plugins configured".into()
        }
    } else {
        lines.join("\n")
    }
}

fn print_show(
    record: &PluginRecord,
    data: &PluginStoreData,
    config: &crate::config::CaConfig,
    paths: &crate::runtime::RuntimePaths,
) {
    println!("{}", render_show(record, data, config, paths));
}

fn render_show(
    record: &PluginRecord,
    data: &PluginStoreData,
    config: &crate::config::CaConfig,
    paths: &crate::runtime::RuntimePaths,
) -> String {
    let mut lines = vec![format!("{} ({})", record.manifest.name, record.manifest.id)];
    lines.push(format!(
        "State: {}",
        if record.enabled {
            "enabled"
        } else {
            "disabled"
        }
    ));
    lines.push(format!(
        "Capabilities: {}",
        capability_labels(&record.manifest).join(", ")
    ));
    lines.push(format!("Skills: {}", record.manifest.skills.len()));
    lines.push(format!(
        "MCP: {}",
        sorted_keys(&record.manifest.mcp).join(", ")
    ));
    lines.push(format!("Hooks: {}", record.manifest.hooks.len()));
    lines.push(format!(
        "Workflows: {}",
        sorted_keys(&record.manifest.workflows).join(", ")
    ));
    lines.push(format!(
        "Themes: {}",
        sorted_keys(&record.manifest.themes).join(", ")
    ));
    if !record.manifest.description.is_empty() {
        lines.push(format!("Description: {}", record.manifest.description));
    }
    let index = data
        .records
        .iter()
        .position(|candidate| candidate.manifest.id == record.manifest.id)
        .expect("record came from store");
    lines.push(format!(
        "Collisions: {}",
        collision_check(index, data, config, paths)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_else(|| "none".into())
    ));
    lines.push(format!(
        "Integrity: {}",
        if verify_digest(record).is_ok() {
            "unchanged"
        } else {
            "changed"
        }
    ));
    lines.push(format!("Digest: {}", &record.digest[..16]));
    lines.push(format!(
        "Source: {}",
        if record.catalog_digest.is_some() {
            "signed catalog"
        } else {
            "local manifest"
        }
    ));
    lines.join("\n")
}

fn capability_labels(manifest: &PluginManifest) -> Vec<&'static str> {
    let mut labels = Vec::new();
    if !manifest.skills.is_empty() || !manifest.workflows.is_empty() {
        labels.push("filesystem-read");
    }
    if manifest
        .mcp
        .values()
        .any(|server| matches!(server.transport, crate::config::McpTransport::Stdio))
        || !manifest.hooks.is_empty()
    {
        labels.push("process");
    }
    if manifest.mcp.values().any(|server| server.network) {
        labels.push("network");
    }
    if !manifest.hooks.is_empty() {
        labels.push("hooks");
    }
    if !manifest.themes.is_empty() {
        labels.push("display");
    }
    if labels.is_empty() {
        labels.push("metadata-only");
    }
    labels
}

fn find_record<'a>(data: &'a PluginStoreData, id: &str) -> Result<&'a PluginRecord, PluginError> {
    data.records
        .iter()
        .find(|record| record.manifest.id == id)
        .ok_or_else(|| PluginError::Missing(id.to_string()))
}

fn find_index(data: &PluginStoreData, id: &str) -> Result<usize, PluginError> {
    validate_id(id)?;
    data.records
        .iter()
        .position(|record| record.manifest.id == id)
        .ok_or_else(|| PluginError::Missing(id.to_string()))
}

fn validate_id(value: &str) -> Result<(), PluginError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        });
    if valid {
        Ok(())
    } else {
        Err(PluginError::InvalidId)
    }
}

fn is_environment_reference(value: &str) -> bool {
    let Some(name) = value
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
    else {
        return false;
    };
    !name.is_empty()
        && name.len() <= 128
        && name.as_bytes()[0].is_ascii_uppercase()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn sorted_keys<T>(values: &HashMap<String, T>) -> Vec<String> {
    let mut keys = values.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    keys
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

fn ensure_private_dir(path: &Path) -> Result<(), PluginError> {
    reject_symlink_components(path)?;
    std::fs::create_dir_all(path).map_err(|error| PluginError::Io(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| PluginError::Io(error.to_string()))?;
    }
    Ok(())
}

fn reject_symlink_components(path: &Path) -> Result<(), PluginError> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if std::fs::symlink_metadata(ancestor)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(PluginError::InvalidResource(path.display().to_string()));
        }
    }
    Ok(())
}

fn check_owner_only(metadata: &std::fs::Metadata) -> Result<(), PluginError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(PluginError::State(
                "registry must be current-user-owned and mode 0600".into(),
            ));
        }
    }
    let _ = metadata;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_and_invalid_names_are_rejected_before_storage() {
        assert!(parse_args(["ca", "--profile", "bare", "list"]).is_err());
        assert!(validate_id("safe-plugin").is_ok());
        assert!(validate_id("../escape").is_err());
    }

    #[test]
    fn ephemeral_plugins_compose_without_persisting_registry_state() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let bundle = root.path().join("bundle");
        let skill = bundle.join("skills/ephemeral-skill");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: ephemeral-skill\ndescription: process only\n---\nbody\n",
        )
        .unwrap();
        std::fs::write(
            bundle.join("plugin.toml"),
            "version = 1\nid = \"ephemeral\"\nname = \"Ephemeral\"\nskills = [\"skills\"]\n",
        )
        .unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(&home);
        let profile = crate::runtime::ActiveProfile::Default;
        let mut config = crate::config::CaConfig::default_config();

        let catalog = compose_enabled_with_ephemeral(
            &paths,
            &profile,
            &mut config,
            std::slice::from_ref(&bundle),
        )
        .unwrap();

        assert_eq!(catalog.enabled, ["ephemeral"]);
        assert_eq!(catalog.ephemeral, ["ephemeral"]);
        assert_eq!(
            config.skills.compatibility_roots,
            [bundle.canonicalize().unwrap().join("skills")]
        );
        assert!(!paths.plugins_dir(&profile).join("plugins.json").exists());

        let mut next_config = crate::config::CaConfig::default_config();
        let next = compose_enabled(&paths, &profile, &mut next_config).unwrap();
        assert!(next.enabled.is_empty());
        assert!(next_config.skills.compatibility_roots.is_empty());
    }

    #[test]
    fn ephemeral_directories_reject_ambiguity_aliases_bare_and_collisions() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path().join("home"));
        let profile = crate::runtime::ActiveProfile::Default;
        let ambiguous = root.path().join("ambiguous");
        std::fs::create_dir_all(&ambiguous).unwrap();
        let body = "{\"version\":1,\"id\":\"one\",\"name\":\"One\"}";
        std::fs::write(ambiguous.join("plugin.json"), body).unwrap();
        std::fs::write(
            ambiguous.join("plugin.toml"),
            "version = 1\nid = \"one\"\nname = \"One\"\n",
        )
        .unwrap();
        assert!(matches!(
            compose_enabled_with_ephemeral(
                &paths,
                &profile,
                &mut crate::config::CaConfig::default_config(),
                std::slice::from_ref(&ambiguous),
            ),
            Err(PluginError::InvalidManifest)
        ));

        std::fs::remove_file(ambiguous.join("plugin.json")).unwrap();
        assert!(matches!(
            compose_enabled_with_ephemeral(
                &paths,
                &profile,
                &mut crate::config::CaConfig::default_config(),
                &[ambiguous.clone(), ambiguous.clone()],
            ),
            Err(PluginError::Duplicate(_))
        ));
        assert!(matches!(
            compose_enabled_with_ephemeral(
                &paths,
                &crate::runtime::ActiveProfile::Bare,
                &mut crate::config::CaConfig::default_config(),
                std::slice::from_ref(&ambiguous),
            ),
            Err(PluginError::State(_))
        ));

        let other = root.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(
            other.join("plugin.toml"),
            "version = 1\nid = \"one\"\nname = \"Other\"\n",
        )
        .unwrap();
        assert!(matches!(
            compose_enabled_with_ephemeral(
                &paths,
                &profile,
                &mut crate::config::CaConfig::default_config(),
                &[ambiguous, other],
            ),
            Err(PluginError::Duplicate(id)) if id == "one"
        ));

        #[cfg(unix)]
        {
            let alias = root.path().join("alias");
            std::os::unix::fs::symlink(root.path().join("ambiguous"), &alias).unwrap();
            assert!(matches!(
                compose_enabled_with_ephemeral(
                    &paths,
                    &profile,
                    &mut crate::config::CaConfig::default_config(),
                    &[alias],
                ),
                Err(PluginError::InvalidManifest)
            ));
        }
    }

    #[test]
    fn tui_manager_browses_previews_mutates_and_revalidates_like_cli() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let bundle = root.path().join("bundle");
        std::fs::create_dir_all(&bundle).unwrap();
        let manifest = bundle.join("plugin.toml");
        std::fs::write(
            &manifest,
            "version = 1\nid = \"local\"\nname = \"Local helper\"\ndescription = \"Review helper\"\n",
        )
        .unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(&home);
        let profile = crate::runtime::ActiveProfile::Default;

        let added =
            run_tui_action(&paths, &profile, &format!("add {}", manifest.display())).unwrap();
        assert!(added.contains("Added plugin local (disabled)"));
        assert!(run_tui_action(&paths, &profile, "search review")
            .unwrap()
            .contains("local\tdisabled\tLocal helper"));
        let preview = run_tui_action(&paths, &profile, "enable local").unwrap();
        assert!(preview.contains("Plugin enable preview"));
        assert!(preview.contains("Digest:"));
        assert!(preview.contains("/plugin enable local confirm local"));
        assert!(run_tui_action(&paths, &profile, "list")
            .unwrap()
            .contains("local\tdisabled"));

        std::fs::write(
            &manifest,
            "version = 1\nid = \"local\"\nname = \"Changed\"\n",
        )
        .unwrap();
        assert!(matches!(
            run_tui_action(&paths, &profile, "enable local confirm local"),
            Err(PluginError::Drift(id)) if id == "local"
        ));
        std::fs::write(
            &manifest,
            "version = 1\nid = \"local\"\nname = \"Local helper\"\ndescription = \"Review helper\"\n",
        )
        .unwrap();
        assert!(
            run_tui_action(&paths, &profile, "enable local confirm local")
                .unwrap()
                .contains("Enabled plugin local")
        );
        assert!(run_tui_action(&paths, &profile, "disable local")
            .unwrap()
            .contains("Disabled plugin local"));
        assert!(run_tui_action(&paths, &profile, "remove local")
            .unwrap()
            .contains("Plugin removal preview"));
        assert!(run_tui_action(&paths, &profile, "remove local confirm wrong").is_err());
        assert!(
            run_tui_action(&paths, &profile, "remove local confirm local")
                .unwrap()
                .contains("Removed plugin local")
        );
        assert_eq!(
            run_tui_action(&paths, &profile, "list").unwrap(),
            "No plugins configured"
        );
    }

    #[test]
    fn tui_catalog_actions_preview_and_use_the_signed_local_store() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path());
        let profile = crate::runtime::ActiveProfile::Default;
        assert_eq!(
            run_tui_action(&paths, &profile, "catalog list").unwrap(),
            "No catalog trust roots configured"
        );
        let preview = run_tui_action(&paths, &profile, "catalog install official/daily").unwrap();
        assert!(preview.contains("Catalog install preview"));
        assert!(preview.contains("confirm daily"));
        assert!(run_tui_action(&paths, &crate::runtime::ActiveProfile::Bare, "list").is_err());
    }

    #[test]
    fn capability_preview_is_derived_without_exposing_commands() {
        let manifest = PluginManifest {
            version: 1,
            id: "safe".into(),
            name: "Safe".into(),
            description: String::new(),
            skills: vec![],
            mcp: HashMap::new(),
            hooks: vec![crate::config::HookConfig {
                event: "session_start".into(),
                command: "SENTINEL SECRET COMMAND".into(),
                enabled: true,
                timeout_secs: 1,
                fail_closed: false,
            }],
            workflows: HashMap::new(),
            themes: HashMap::new(),
        };
        assert_eq!(capability_labels(&manifest), vec!["process", "hooks"]);
        assert!(!format!("{manifest:?}").contains("SENTINEL"));
    }

    #[test]
    fn registry_is_profile_scoped_owner_only_and_atomic() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path());
        assert_ne!(
            paths.plugins_dir(&crate::runtime::ActiveProfile::Default),
            paths.plugins_dir(&crate::runtime::ActiveProfile::Named("work".into()))
        );
        let store = PluginStore::new(paths.plugins_dir(&crate::runtime::ActiveProfile::Default));
        let mut data = PluginStoreData::default();
        store.save(&mut data).unwrap();
        assert_eq!(store.load().unwrap().revision, 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                std::fs::metadata(&store.path).unwrap().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn manifest_rejects_native_fields_traversal_and_detects_content_drift() {
        let root = tempfile::tempdir().unwrap();
        let manifest = root.path().join("plugin.toml");
        std::fs::write(
            &manifest,
            "version = 1\nid = \"safe\"\nname = \"Safe\"\nnative_library = \"evil.dylib\"\n",
        )
        .unwrap();
        assert!(matches!(
            load_manifest(&manifest),
            Err(PluginError::Parse(_))
        ));
        std::fs::write(
            &manifest,
            "version = 1\nid = \"safe\"\nname = \"Safe\"\nskills = [\"../escape\"]\n",
        )
        .unwrap();
        assert!(matches!(
            load_manifest(&manifest),
            Err(PluginError::InvalidResource(_))
        ));

        let skills = root.path().join("skills/one");
        std::fs::create_dir_all(&skills).unwrap();
        std::fs::write(
            skills.join("SKILL.md"),
            "---\nname: one\ndescription: one\n---\nbody",
        )
        .unwrap();
        std::fs::write(
            &manifest,
            "version = 1\nid = \"safe\"\nname = \"Safe\"\nskills = [\"skills\"]\n",
        )
        .unwrap();
        let record = load_manifest(&manifest).unwrap();
        assert!(verify_digest(&record).is_ok());
        std::fs::write(
            skills.join("SKILL.md"),
            "---\nname: one\ndescription: one\n---\nchanged",
        )
        .unwrap();
        assert!(matches!(verify_digest(&record), Err(PluginError::Drift(_))));
    }

    #[test]
    fn exact_skill_collisions_block_activation() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        for (bundle, id, skill_name) in [
            (&first, "first", "shared-skill"),
            (&second, "second", "shared-skill"),
        ] {
            let skill = bundle.join(format!("skills/{skill_name}"));
            std::fs::create_dir_all(&skill).unwrap();
            std::fs::write(
                skill.join("SKILL.md"),
                format!("---\nname: {skill_name}\ndescription: shared\n---\nbody"),
            )
            .unwrap();
            std::fs::write(
                bundle.join("plugin.toml"),
                format!("version = 1\nid = \"{id}\"\nname = \"{id}\"\nskills = [\"skills\"]\n"),
            )
            .unwrap();
        }
        let mut one = load_manifest(&first.join("plugin.toml")).unwrap();
        one.enabled = true;
        let two = load_manifest(&second.join("plugin.toml")).unwrap();
        let data = PluginStoreData {
            version: 1,
            revision: 0,
            records: vec![one, two],
        };
        assert!(matches!(
            collision_check(
                1,
                &data,
                &crate::config::CaConfig::default_config(),
                &crate::runtime::RuntimePaths::from_home(root.path()),
            ),
            Err(PluginError::Collision(_))
        ));
    }

    #[test]
    fn plugin_mcp_values_must_be_environment_references() {
        let root = tempfile::tempdir().unwrap();
        let manifest = PluginManifest {
            version: 1,
            id: "safe".into(),
            name: "Safe".into(),
            description: String::new(),
            skills: Vec::new(),
            mcp: HashMap::from([(
                "helper".into(),
                crate::config::McpServerConfig {
                    transport: crate::config::McpTransport::Stdio,
                    command: "/bin/echo".into(),
                    args: Vec::new(),
                    env: HashMap::from([("TOKEN".into(), "literal-secret".into())]),
                    url: None,
                    headers: HashMap::new(),
                    enabled: true,
                    timeout_secs: 1,
                    network: false,
                    oauth: Default::default(),
                },
            )]),
            hooks: Vec::new(),
            workflows: HashMap::new(),
            themes: HashMap::new(),
        };
        assert!(matches!(
            validate_manifest(&manifest, root.path()),
            Err(PluginError::InvalidMcp(_))
        ));

        let mut referenced = manifest;
        referenced.mcp.get_mut("helper").unwrap().env =
            HashMap::from([("TOKEN".into(), "${PLUGIN_TOKEN}".into())]);
        assert!(validate_manifest(&referenced, root.path()).is_ok());
        assert!(!is_environment_reference("${lowercase}"));
        assert!(!is_environment_reference("prefix-${PLUGIN_TOKEN}"));
    }

    #[test]
    fn configured_mcp_and_user_skill_collisions_block_activation() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path());
        let user_skill = paths.user_skills_dir().join("shared-skill");
        std::fs::create_dir_all(&user_skill).unwrap();
        std::fs::write(
            user_skill.join("SKILL.md"),
            "---\nname: shared-skill\ndescription: user skill\n---\nbody",
        )
        .unwrap();

        let bundle = root.path().join("bundle");
        let plugin_skill = bundle.join("skills/shared-skill");
        std::fs::create_dir_all(&plugin_skill).unwrap();
        std::fs::write(
            plugin_skill.join("SKILL.md"),
            "---\nname: shared-skill\ndescription: plugin skill\n---\nbody",
        )
        .unwrap();
        std::fs::write(
            bundle.join("plugin.toml"),
            r#"version = 1
id = "safe"
name = "Safe"
skills = ["skills"]

[mcp.helper]
command = "/bin/echo"
timeout_secs = 1
env = { TOKEN = "${PLUGIN_TOKEN}" }
"#,
        )
        .unwrap();
        let record = load_manifest(&bundle.join("plugin.toml")).unwrap();
        let data = PluginStoreData {
            version: 1,
            revision: 0,
            records: vec![record.clone()],
        };
        let mut config = crate::config::CaConfig::default_config();
        assert!(matches!(
            collision_check(0, &data, &config, &paths),
            Err(PluginError::Collision(message)) if message.contains("skill identity")
        ));

        std::fs::remove_dir_all(paths.user_skills_dir()).unwrap();
        config
            .mcp
            .servers
            .insert("helper".into(), record.manifest.mcp["helper"].clone());
        assert!(matches!(
            collision_check(0, &data, &config, &paths),
            Err(PluginError::Collision(message)) if message.contains("MCP server helper")
        ));
    }

    #[test]
    fn enabled_bundle_composes_only_existing_extension_types() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path());
        let bundle = root.path().join("bundle");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(
            bundle.join("plugin.toml"),
            r#"version = 1
id = "safe"
name = "Safe"

[mcp.helper]
command = "/bin/echo"
timeout_secs = 1
env = { TOKEN = "${PLUGIN_TOKEN}" }

[[hooks]]
event = "session_start"
command = "/bin/echo ready"
timeout_secs = 1
"#,
        )
        .unwrap();
        let mut record = load_manifest(&bundle.join("plugin.toml")).unwrap();
        record.enabled = true;
        let store = PluginStore::new(paths.plugins_dir(&crate::runtime::ActiveProfile::Default));
        store
            .save(&mut PluginStoreData {
                version: STORE_VERSION,
                revision: 0,
                records: vec![record],
            })
            .unwrap();

        let mut config = crate::config::CaConfig::default_config();
        let catalog =
            compose_enabled(&paths, &crate::runtime::ActiveProfile::Default, &mut config).unwrap();
        assert_eq!(catalog.enabled, vec!["safe"]);
        assert!(config.mcp.servers.contains_key("helper"));
        assert_eq!(config.hooks.len(), 1);
        assert_eq!(config.hooks[0].event, "session_start");
    }

    #[test]
    fn catalog_registration_pins_every_staged_file_and_stays_disabled() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(root.path());
        let profile = crate::runtime::ActiveProfile::Default;
        let installs = root.path().join("catalog/installs");
        let bundle = installs.join("safe/1.0.0-hash");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(
            bundle.join("plugin.toml"),
            "version = 1\nid = \"safe\"\nname = \"Safe\"\n",
        )
        .unwrap();
        std::fs::write(bundle.join("undeclared-helper.sh"), "one").unwrap();
        register_catalog_manifest(
            &paths,
            &profile,
            &bundle.join("plugin.toml"),
            &installs,
            CatalogRegistration::Install,
        )
        .unwrap();
        let store = PluginStore::new(paths.plugins_dir(&profile));
        let data = store.load().unwrap();
        assert!(!data.records[0].enabled);
        assert!(verify_digest(&data.records[0]).is_ok());
        std::fs::write(bundle.join("undeclared-helper.sh"), "two").unwrap();
        assert!(matches!(
            verify_digest(&data.records[0]),
            Err(PluginError::Drift(_))
        ));
    }
}
