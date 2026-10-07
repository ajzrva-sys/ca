//! Typed remote comparison, read-only RPC, and digest-bound synchronization.
//!
//! This module deliberately has no arbitrary-command surface. Every host,
//! endpoint, path, and direction comes from a private operator-authored
//! manifest; wire requests carry safe names and closed enums only.

use crate::torrent::{
    RtorrentEndpointV1, SshHostV1, TorrentControlRequest, TorrentLocationV1, TorrentQueryResult,
    TransmissionEndpointV1,
};
use anyhow::{bail, Context, Result};
use ca_automation_protocol::{
    validate_remote_automation_request, RemoteAutomationActionReceiptV1, RemoteAutomationActionV1,
    RemoteAutomationReceiptV1, RemoteAutomationRequestV1, RemoteAutomationStatusV1,
    RemoteRpcOperationV1, TorrentControlActionV1, TorrentInventoryV1, TorrentMutationReceiptV1,
    MAX_MECHANISTIC_OUTPUT_BYTES, REMOTE_AUTOMATION_RECEIPT_SCHEMA,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

const REMOTE_CONFIG_SCHEMA: &str = "codex-team.remote_config.v1";
const MAX_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_PROCESS_OUTPUT_BYTES: usize = 1024 * 1024;
const DEFAULT_TIMEOUT_SECONDS: u64 = 900;
const MAX_TIMEOUT_SECONDS: u64 = 7_200;
const DEFAULT_MAX_CHANGES: usize = 1_000;
const MAX_CHANGES: usize = 10_000;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RemoteConfigV1 {
    schema: String,
    #[serde(default)]
    ssh_hosts: BTreeMap<String, SshHostV1>,
    #[serde(default)]
    transmission: BTreeMap<String, TransmissionEndpointV1>,
    #[serde(default)]
    rtorrent: BTreeMap<String, RtorrentEndpointV1>,
    #[serde(default)]
    torrent_locations: BTreeMap<String, TorrentLocationV1>,
    #[serde(default)]
    syncs: BTreeMap<String, SyncProfileV1>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SyncProfileV1 {
    /// `local` or a key from `ssh_hosts`.
    controller: String,
    source: String,
    destination: String,
    #[serde(default)]
    torrent_endpoint: Option<String>,
    #[serde(default)]
    delete: bool,
    #[serde(default = "default_timeout_seconds")]
    timeout_seconds: u64,
    #[serde(default = "default_max_changes")]
    max_changes: usize,
}

pub(crate) fn validate_config_text(text: &str) -> Result<()> {
    let config: RemoteConfigV1 = toml::from_str(text).context("parse private remote manifest")?;
    validate_config(&config)
}

#[derive(Debug, Clone)]
pub(crate) struct WorkflowRemoteSummary {
    pub(crate) binding_digest: String,
    pub(crate) ssh_hosts: BTreeSet<String>,
    pub(crate) transmission: BTreeSet<String>,
    pub(crate) rtorrent: BTreeSet<String>,
    pub(crate) rtorrent_ssh_hosts: BTreeMap<String, String>,
    pub(crate) torrent_locations: BTreeMap<String, String>,
    pub(crate) syncs: BTreeMap<String, WorkflowSyncProfileSummary>,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkflowSyncProfileSummary {
    pub(crate) controller: String,
    pub(crate) destination: String,
    pub(crate) delete: bool,
    pub(crate) torrent_endpoint: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkflowSyncPlan {
    pub(crate) entries: Vec<String>,
    pub(crate) digest: String,
    pub(crate) predicted_bytes: u64,
    pub(crate) has_conflicts: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkflowSshProbe {
    pub(crate) summary: String,
    pub(crate) identity_digest: String,
}

pub(crate) fn workflow_config_summary(path: &Path) -> Result<WorkflowRemoteSummary> {
    let config = load_config(path)?;
    Ok(WorkflowRemoteSummary {
        binding_digest: digest_json(&config)?,
        ssh_hosts: config.ssh_hosts.keys().cloned().collect(),
        transmission: config.transmission.keys().cloned().collect(),
        rtorrent: config.rtorrent.keys().cloned().collect(),
        rtorrent_ssh_hosts: config
            .rtorrent
            .iter()
            .filter_map(|(name, endpoint)| match &endpoint.transport {
                crate::torrent::RtorrentTransportV1::SshLoopbackScgi { ssh_host, .. } => {
                    Some((name.clone(), ssh_host.clone()))
                }
                _ => None,
            })
            .collect(),
        torrent_locations: config
            .torrent_locations
            .iter()
            .map(|(name, location)| (name.clone(), location.endpoint.clone()))
            .collect(),
        syncs: config
            .syncs
            .iter()
            .map(|(name, profile)| {
                (
                    name.clone(),
                    WorkflowSyncProfileSummary {
                        controller: profile.controller.clone(),
                        destination: profile.destination.clone(),
                        delete: profile.delete,
                        torrent_endpoint: profile.torrent_endpoint.clone(),
                    },
                )
            })
            .collect(),
    })
}

pub(crate) fn workflow_local_roots(path: &Path) -> Result<Vec<PathBuf>> {
    let config = load_config(path)?;
    let mut roots = BTreeSet::new();
    for profile in config.syncs.values() {
        if profile.controller != "local" {
            continue;
        }
        for value in [&profile.source, &profile.destination] {
            if value.contains(':') {
                continue;
            }
            let candidate = Path::new(value);
            if !candidate.is_absolute() || candidate == Path::new("/") {
                bail!("local workflow roots must be absolute and cannot be the filesystem root")
            }
            let canonical = candidate.canonicalize().with_context(|| {
                format!(
                    "configured local workflow root does not exist: {}",
                    candidate.display()
                )
            })?;
            roots.insert(canonical);
        }
    }
    Ok(roots.into_iter().collect())
}

pub(crate) fn workflow_credential_env(path: &Path) -> Result<Vec<String>> {
    let config = load_config(path)?;
    let mut names = BTreeSet::new();
    for endpoint in config.transmission.values() {
        names.extend(endpoint.username_env.iter().cloned());
        names.extend(endpoint.password_env.iter().cloned());
    }
    for endpoint in config.rtorrent.values() {
        if let crate::torrent::RtorrentTransportV1::HttpXmlRpc {
            username_env,
            password_env,
            ..
        } = &endpoint.transport
        {
            names.extend(username_env.iter().cloned());
            names.extend(password_env.iter().cloned());
        }
    }
    anyhow::ensure!(
        names.len() <= 64,
        "workflow credential environment exceeds its bound"
    );
    Ok(names.into_iter().collect())
}

pub(crate) async fn workflow_probe_ssh(path: &Path, host_name: &str) -> Result<WorkflowSshProbe> {
    let config = load_config(path)?;
    let host = config
        .ssh_hosts
        .get(host_name)
        .with_context(|| format!("unknown SSH host `{host_name}`"))?;
    let executable = fixed_executable(&["/usr/bin/ssh", "/bin/ssh"])?;
    let mut command = Command::new(executable);
    command
        .arg("-v")
        .args(ssh_arguments(host))
        .arg("--")
        .arg(&host.destination)
        .arg("/usr/bin/true")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = bounded_process_output(
        &mut command,
        None,
        Duration::from_secs(30),
        MAX_PROCESS_OUTPUT_BYTES,
    )
    .await
    .context("run fixed SSH identity probe")?;
    if !output.status.success() {
        bail!("SSH identity probe failed")
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let identity = stderr
        .lines()
        .find_map(|line| {
            line.split_once("Server host key:")
                .map(|(_, value)| value.trim())
        })
        .filter(|value| !value.is_empty())
        .context("SSH identity probe did not report a host-key fingerprint")?;
    Ok(WorkflowSshProbe {
        summary: format!("SSH host `{host_name}` accepted a non-interactive identity probe"),
        identity_digest: format!("{:x}", Sha256::digest(identity.as_bytes())),
    })
}

pub(crate) async fn workflow_query_transmission(
    path: &Path,
    endpoint: &str,
    operation: RemoteRpcOperationV1,
) -> Result<Value> {
    let config = load_config(path)?;
    if !config.transmission.contains_key(endpoint) {
        bail!("workflow query_transmission requires a Transmission endpoint")
    }
    torrent_query(&config, endpoint, operation).await
}

pub(crate) async fn workflow_query_torrents(
    path: &Path,
    endpoint: &str,
) -> Result<TorrentInventoryV1> {
    let config = load_config(path)?;
    match torrent_query_result(&config, endpoint, RemoteRpcOperationV1::Torrents).await? {
        TorrentQueryResult::Torrents(inventory) => Ok(inventory),
        _ => bail!("torrent query did not return an inventory"),
    }
}

pub(crate) async fn workflow_control_torrents(
    path: &Path,
    endpoint: &str,
    target_hashes: Vec<String>,
    expected_selection: &Value,
    control: TorrentControlActionV1,
) -> Result<TorrentMutationReceiptV1> {
    let config = load_config(path)?;
    let expected_snapshots = expected_selection
        .as_array()
        .context("torrent selection is not an array")?
        .iter()
        .cloned()
        .map(serde_json::from_value)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("torrent selection contained a malformed snapshot")?;
    torrent_control(
        &config,
        endpoint,
        target_hashes,
        expected_snapshots,
        control,
    )
    .await
}

pub(crate) async fn workflow_plan_sync(
    path: &Path,
    profile: &str,
    keep_both: bool,
    selection: Option<&Value>,
    maximum_files: Option<usize>,
) -> Result<WorkflowSyncPlan> {
    let config = load_config(path)?;
    let manifest = selection
        .map(|selection| workflow_file_manifest(&config, profile, selection, maximum_files))
        .transpose()?;
    let plan = workflow_sync_plan(&config, profile, keep_both, manifest.as_ref()).await?;
    let has_conflicts = plan.entries.iter().any(|entry| {
        let itemized = entry.split('\t').next().unwrap_or_default();
        itemized.starts_with('>') && !itemized.contains("+++++++++")
    });
    Ok(WorkflowSyncPlan {
        entries: plan.entries,
        digest: plan.digest,
        predicted_bytes: plan.predicted_bytes,
        has_conflicts,
    })
}

pub(crate) async fn workflow_apply_sync(
    path: &Path,
    profile: &str,
    accepted_plan_digest: &str,
    keep_both: bool,
    selection: Option<&Value>,
    maximum_files: Option<usize>,
) -> Result<()> {
    let config = load_config(path)?;
    let manifest = selection
        .map(|selection| workflow_file_manifest(&config, profile, selection, maximum_files))
        .transpose()?;
    let plan = workflow_sync_plan(&config, profile, keep_both, manifest.as_ref()).await?;
    require_accepted_plan(&plan.digest, accepted_plan_digest)?;
    if !plan.entries.is_empty() {
        let backup_dir = keep_both.then(|| {
            format!(
                ".codex-team-conflicts/{}",
                &accepted_plan_digest[..accepted_plan_digest.len().min(16)]
            )
        });
        apply_sync_with_backup(&config, profile, backup_dir.as_deref(), manifest.as_ref()).await?;
    }
    Ok(())
}

async fn workflow_sync_plan(
    config: &RemoteConfigV1,
    name: &str,
    keep_both: bool,
    manifest: Option<&WorkflowFileManifest>,
) -> Result<SyncPlan> {
    let profile = config
        .syncs
        .get(name)
        .with_context(|| format!("unknown remote sync profile `{name}`"))?;
    let output = run_rsync(config, profile, true, None, manifest).await?;
    let (entries, predicted_bytes) = parse_rsync_plan_with_bytes(&output, profile.max_changes)?;
    let digest = crate::json_identity::digest_value(&json!({
        "schema": "codex-team.workflow_sync_plan.v1",
        "profile": name,
        "controller": profile.controller,
        "source": profile.source,
        "destination": profile.destination,
        "delete": profile.delete,
        "keep_both": keep_both,
        "file_manifest_digest": manifest.map(|value| &value.digest),
        "selected_files": manifest.map_or(0, |value| value.entries.len()),
        "selected_bytes": manifest.map_or(0, |value| value.total_bytes),
        "entries": entries,
    }))?;
    Ok(SyncPlan {
        entries,
        digest,
        predicted_bytes,
    })
}

#[derive(Debug, Clone)]
struct WorkflowFileManifest {
    entries: Vec<String>,
    bytes: Vec<u8>,
    digest: String,
    total_bytes: u64,
}

fn workflow_file_manifest(
    config: &RemoteConfigV1,
    profile_name: &str,
    selection: &Value,
    maximum_files: Option<usize>,
) -> Result<WorkflowFileManifest> {
    let maximum_files = maximum_files.context("torrent-backed sync requires max_files")?;
    let profile = config
        .syncs
        .get(profile_name)
        .with_context(|| format!("unknown remote sync profile `{profile_name}`"))?;
    let endpoint = profile
        .torrent_endpoint
        .as_deref()
        .context("selection-aware sync profile lacks a torrent endpoint binding")?;
    let torrents = selection
        .as_array()
        .context("torrent selection is not an array")?;
    let source_path = profile
        .source
        .rsplit_once(':')
        .map_or(profile.source.as_str(), |(_, path)| path);
    let source_path = source_path.trim_end_matches('/');
    let mut files = BTreeMap::<String, u64>::new();
    for torrent in torrents {
        let location_name = torrent
            .get("location_name")
            .and_then(Value::as_str)
            .context("selected torrent is not bound to an approved named location")?;
        let location = config
            .torrent_locations
            .get(location_name)
            .context("selected torrent location is not configured")?;
        if location.endpoint != endpoint || location.path.trim_end_matches('/') != source_path {
            bail!("selected torrent lies outside the sync profile source root")
        }
        for file in torrent
            .get("files")
            .and_then(Value::as_array)
            .context("selected torrent omitted its bounded file inventory")?
        {
            let relative = file
                .get("path")
                .and_then(Value::as_str)
                .context("selected torrent file omitted its relative path")?;
            validate_relative_manifest_path(relative)?;
            let size = file
                .get("length_bytes")
                .and_then(Value::as_u64)
                .context("selected torrent file omitted its size")?;
            if let Some(existing) = files.insert(relative.to_string(), size) {
                if existing != size {
                    bail!("selected torrent file path has conflicting sizes")
                }
            }
            if files.len() > maximum_files {
                bail!("selected torrent files exceeded max_files")
            }
        }
    }
    let entries = files.keys().cloned().collect::<Vec<_>>();
    let total_bytes = files.values().copied().sum();
    let digest = crate::json_identity::digest_value(&json!({
        "schema": "codex-team.torrent_file_manifest.v1",
        "profile": profile_name,
        "endpoint": endpoint,
        "files": files,
    }))?;
    let mut bytes = Vec::new();
    for entry in &entries {
        bytes.extend_from_slice(entry.as_bytes());
        bytes.push(0);
    }
    Ok(WorkflowFileManifest {
        entries,
        bytes,
        digest,
        total_bytes,
    })
}

fn validate_relative_manifest_path(value: &str) -> Result<()> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > 4_096
        || value.chars().any(char::is_control)
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::RootDir
                    | Component::Prefix(_)
                    | Component::ParentDir
                    | Component::CurDir
            )
        })
    {
        bail!("selected torrent file path escaped its source root")
    }
    Ok(())
}

pub(crate) async fn workflow_verify_sha256(
    path: &Path,
    profile_name: &str,
    maximum_files: usize,
) -> Result<usize> {
    let config = load_config(path)?;
    let profile = config
        .syncs
        .get(profile_name)
        .with_context(|| format!("unknown sync profile `{profile_name}`"))?;
    if profile.controller == "local"
        && !profile.source.contains(':')
        && !profile.destination.contains(':')
    {
        let source = Path::new(&profile.source);
        let destination = Path::new(&profile.destination);
        let source_hashes = sha256_tree(source, maximum_files)?;
        let destination_hashes = sha256_tree(destination, maximum_files)?;
        if source_hashes != destination_hashes {
            bail!("SHA-256 verification found a content mismatch")
        }
        return Ok(source_hashes.len());
    }
    let mut list_args = rsync_arguments(profile, true, None);
    list_args.insert(0, OsString::from("--list-only"));
    let listing = run_rsync_arguments(&config, profile, list_args, None).await?;
    let listed_files = String::from_utf8(listing)
        .context("SHA-256 inventory listing was not UTF-8")?
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_end().ends_with('/'))
        .count();
    if listed_files > maximum_files {
        bail!("SHA-256 verification exceeded its file bound")
    }
    let mut checksum_args = rsync_arguments(profile, true, None);
    checksum_args.insert(0, OsString::from("--checksum-choice=sha256"));
    checksum_args.insert(0, OsString::from("--checksum"));
    let output = run_rsync_arguments(&config, profile, checksum_args, None).await?;
    let (differences, _) = parse_rsync_plan_with_bytes(&output, profile.max_changes)?;
    if !differences.is_empty() {
        bail!("SHA-256 verification found a content mismatch")
    }
    Ok(listed_files)
}

fn sha256_tree(root: &Path, maximum_files: usize) -> Result<BTreeMap<PathBuf, String>> {
    let metadata = std::fs::symlink_metadata(root)
        .with_context(|| format!("inspect SHA-256 root {}", root.display()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!("SHA-256 root must be a real directory")
    }
    let canonical = root.canonicalize().context("canonicalize SHA-256 root")?;
    let mut hashes = BTreeMap::new();
    for entry in walkdir::WalkDir::new(&canonical)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| entry.depth() == 0 || entry.file_name() != ".codex-team-conflicts")
    {
        let entry = entry.context("walk SHA-256 tree")?;
        let metadata = entry.metadata().context("inspect SHA-256 entry")?;
        if metadata.file_type().is_symlink() {
            bail!("SHA-256 verification refuses symlinks")
        }
        if metadata.is_dir() {
            continue;
        }
        if !metadata.is_file() {
            bail!("SHA-256 verification refuses special files")
        }
        if hashes.len() == maximum_files {
            bail!("SHA-256 verification exceeded its file bound")
        }
        let relative = entry
            .path()
            .strip_prefix(&canonical)
            .context("SHA-256 entry escaped its root")?
            .to_path_buf();
        let mut file = std::fs::File::open(entry.path()).context("open SHA-256 input")?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).context("read SHA-256 input")?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        hashes.insert(relative, format!("{:x}", hasher.finalize()));
    }
    Ok(hashes)
}

const fn default_timeout_seconds() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}

const fn default_max_changes() -> usize {
    DEFAULT_MAX_CHANGES
}

pub async fn execute_file(
    config_path: &Path,
    request: RemoteAutomationRequestV1,
) -> Result<RemoteAutomationReceiptV1> {
    validate_remote_automation_request(&request).map_err(anyhow::Error::msg)?;
    let config = load_config(config_path)?;
    execute(&config, request).await
}

async fn execute(
    config: &RemoteConfigV1,
    request: RemoteAutomationRequestV1,
) -> Result<RemoteAutomationReceiptV1> {
    let started_at = Utc::now();
    let started = Instant::now();
    let request_digest = digest_json(&request)?;
    let mut action_results = Vec::with_capacity(request.actions.len());
    let mut total_bytes = 0_usize;
    for (index, action) in request.actions.iter().enumerate() {
        let action_started = Instant::now();
        let outcome = execute_action(config, action).await;
        let (status, summary, mut entries, plan_digest, torrent_inventory, torrent_mutation) =
            match outcome {
                Ok(value) => (
                    RemoteAutomationStatusV1::Completed,
                    value.summary,
                    value.entries,
                    value.plan_digest,
                    value.torrent_inventory,
                    value.torrent_mutation,
                ),
                Err(error) => (
                    RemoteAutomationStatusV1::Failed,
                    bounded_text(&format!("{error:#}"), 1_024),
                    Vec::new(),
                    None,
                    None,
                    None,
                ),
            };
        let mut truncated = false;
        while receipt_text_bytes(&summary, &entries) + total_bytes > MAX_MECHANISTIC_OUTPUT_BYTES {
            if entries.pop().is_none() {
                break;
            }
            truncated = true;
        }
        total_bytes = total_bytes.saturating_add(receipt_text_bytes(&summary, &entries));
        action_results.push(RemoteAutomationActionReceiptV1 {
            action_index: index as u8,
            kind: action.kind_name().to_string(),
            status,
            summary,
            entries,
            truncated,
            plan_digest,
            torrent_inventory,
            torrent_mutation,
            duration_millis: action_started.elapsed().as_millis() as u64,
        });
    }
    let status = if action_results
        .iter()
        .all(|result| result.status == RemoteAutomationStatusV1::Completed)
    {
        RemoteAutomationStatusV1::Completed
    } else {
        RemoteAutomationStatusV1::Failed
    };
    let mut receipt = RemoteAutomationReceiptV1 {
        schema: REMOTE_AUTOMATION_RECEIPT_SCHEMA.to_string(),
        request_digest,
        status,
        action_results,
        output_bytes: total_bytes as u64,
        started_at: started_at.to_rfc3339(),
        finished_at: Utc::now().to_rfc3339(),
        elapsed_millis: started.elapsed().as_millis() as u64,
    };
    bound_remote_receipt(&mut receipt)?;
    Ok(receipt)
}

fn bound_remote_receipt(receipt: &mut RemoteAutomationReceiptV1) -> Result<()> {
    receipt.output_bytes = 0;
    loop {
        let length = serde_json::to_vec(receipt)?.len();
        if length <= MAX_MECHANISTIC_OUTPUT_BYTES {
            let exact = length as u64;
            if receipt.output_bytes == exact {
                return Ok(());
            }
            receipt.output_bytes = exact;
            continue;
        }
        let mut reduced = false;
        for action in receipt.action_results.iter_mut().rev() {
            if action.entries.pop().is_some() {
                action.truncated = true;
                reduced = true;
                break;
            }
            if let Some(inventory) = action.torrent_inventory.as_mut() {
                if shrink_torrent_inventory(inventory) {
                    action.truncated = true;
                    reduced = true;
                    break;
                }
            }
        }
        if !reduced {
            bail!("remote automation receipt exceeds its 64 KiB public bound")
        }
    }
}

fn shrink_torrent_inventory(inventory: &mut TorrentInventoryV1) -> bool {
    if let Some(torrent) = inventory
        .torrents
        .iter_mut()
        .rev()
        .find(|torrent| !torrent.files.is_empty())
    {
        torrent.files.clear();
        torrent.files_truncated = true;
        inventory.truncated = true;
        return true;
    }
    if inventory.torrents.pop().is_some() {
        inventory.truncated = true;
        return true;
    }
    false
}

struct ActionOutcome {
    summary: String,
    entries: Vec<String>,
    plan_digest: Option<String>,
    torrent_inventory: Option<TorrentInventoryV1>,
    torrent_mutation: Option<TorrentMutationReceiptV1>,
}

async fn execute_action(
    config: &RemoteConfigV1,
    action: &RemoteAutomationActionV1,
) -> Result<ActionOutcome> {
    match action {
        RemoteAutomationActionV1::ListRemoteAutomation => {
            let mut entries = config
                .syncs
                .keys()
                .map(|name| format!("sync profile: {name}"))
                .chain(
                    config
                        .transmission
                        .keys()
                        .map(|name| format!("Transmission endpoint: {name}")),
                )
                .chain(
                    config
                        .rtorrent
                        .keys()
                        .map(|name| format!("rTorrent endpoint: {name}")),
                )
                .collect::<Vec<_>>();
            entries.sort();
            Ok(ActionOutcome {
                summary: format!(
                    "{} configured remote automation target{}",
                    entries.len(),
                    if entries.len() == 1 { "" } else { "s" }
                ),
                entries,
                plan_digest: None,
                torrent_inventory: None,
                torrent_mutation: None,
            })
        }
        RemoteAutomationActionV1::CompareRemoteFiles { profile }
        | RemoteAutomationActionV1::PlanRemoteSync { profile } => {
            let plan = sync_plan(config, profile).await?;
            Ok(ActionOutcome {
                summary: format!(
                    "{} pending change{} for remote sync profile `{profile}`",
                    plan.entries.len(),
                    if plan.entries.len() == 1 { "" } else { "s" }
                ),
                entries: plan.entries,
                plan_digest: Some(plan.digest),
                torrent_inventory: None,
                torrent_mutation: None,
            })
        }
        RemoteAutomationActionV1::ApplyRemoteSync {
            profile,
            accepted_plan_digest,
        } => {
            let plan = sync_plan(config, profile).await?;
            require_accepted_plan(&plan.digest, accepted_plan_digest)?;
            if plan.entries.is_empty() {
                return Ok(ActionOutcome {
                    summary: format!("remote sync profile `{profile}` is already current"),
                    entries: Vec::new(),
                    plan_digest: Some(plan.digest),
                    torrent_inventory: None,
                    torrent_mutation: None,
                });
            }
            apply_sync(config, profile).await?;
            Ok(ActionOutcome {
                summary: format!(
                    "applied {} approved change{} for remote sync profile `{profile}`",
                    plan.entries.len(),
                    if plan.entries.len() == 1 { "" } else { "s" }
                ),
                entries: plan.entries,
                plan_digest: Some(plan.digest),
                torrent_inventory: None,
                torrent_mutation: None,
            })
        }
        RemoteAutomationActionV1::QueryRemoteRpc {
            endpoint,
            operation,
        } => {
            let result = torrent_query_result(config, endpoint, *operation).await?;
            let torrent_inventory = result.inventory().cloned();
            let normalized = normalize_json(result.into_json()?);
            Ok(ActionOutcome {
                summary: format!(
                    "read-only torrent {} query completed for `{endpoint}`",
                    rpc_operation_name(*operation)
                ),
                entries: render_rpc_entries(*operation, &normalized),
                plan_digest: None,
                torrent_inventory,
                torrent_mutation: None,
            })
        }
        RemoteAutomationActionV1::CompareRemoteRpc {
            left_endpoint,
            right_endpoint,
            operation,
        } => {
            let (left, right) = tokio::try_join!(
                torrent_query(config, left_endpoint, *operation),
                torrent_query(config, right_endpoint, *operation)
            )?;
            let entries = compare_rpc_values(*operation, &left, &right);
            Ok(ActionOutcome {
                summary: if entries.is_empty() {
                    format!(
                        "`{left_endpoint}` and `{right_endpoint}` match for {}",
                        rpc_operation_name(*operation)
                    )
                } else {
                    format!(
                        "{} {} difference{} between `{left_endpoint}` and `{right_endpoint}`",
                        entries.len(),
                        rpc_operation_name(*operation),
                        if entries.len() == 1 { "" } else { "s" }
                    )
                },
                entries,
                plan_digest: None,
                torrent_inventory: None,
                torrent_mutation: None,
            })
        }
    }
}

struct SyncPlan {
    entries: Vec<String>,
    digest: String,
    predicted_bytes: u64,
}

async fn sync_plan(config: &RemoteConfigV1, name: &str) -> Result<SyncPlan> {
    let profile = config
        .syncs
        .get(name)
        .with_context(|| format!("unknown remote sync profile `{name}`"))?;
    let output = run_rsync(config, profile, true, None, None).await?;
    let (entries, predicted_bytes) = parse_rsync_plan_with_bytes(&output, profile.max_changes)?;
    let digest = crate::json_identity::digest_value(&json!({
        "schema": "codex-team.remote_sync_plan.v1",
        "profile": name,
        "controller": profile.controller,
        "source": profile.source,
        "destination": profile.destination,
        "delete": profile.delete,
        "entries": entries,
    }))?;
    Ok(SyncPlan {
        entries,
        digest,
        predicted_bytes,
    })
}

async fn apply_sync(config: &RemoteConfigV1, name: &str) -> Result<()> {
    apply_sync_with_backup(config, name, None, None).await
}

async fn apply_sync_with_backup(
    config: &RemoteConfigV1,
    name: &str,
    backup_dir: Option<&str>,
    manifest: Option<&WorkflowFileManifest>,
) -> Result<()> {
    let profile = config
        .syncs
        .get(name)
        .with_context(|| format!("unknown remote sync profile `{name}`"))?;
    let _ = run_rsync(config, profile, false, backup_dir, manifest).await?;
    Ok(())
}

async fn run_rsync(
    config: &RemoteConfigV1,
    profile: &SyncProfileV1,
    dry_run: bool,
    backup_dir: Option<&str>,
    manifest: Option<&WorkflowFileManifest>,
) -> Result<Vec<u8>> {
    let mut args = rsync_arguments(profile, dry_run, backup_dir);
    if manifest.is_some() {
        let boundary = args
            .iter()
            .position(|argument| argument == "--")
            .context("rsync argument boundary is missing")?;
        args.insert(boundary, OsString::from("--from0"));
        args.insert(boundary + 1, OsString::from("--files-from=-"));
    }
    run_rsync_arguments(
        config,
        profile,
        args,
        manifest.map(|value| value.bytes.clone()),
    )
    .await
}

async fn run_rsync_arguments(
    config: &RemoteConfigV1,
    profile: &SyncProfileV1,
    args: Vec<OsString>,
    stdin_bytes: Option<Vec<u8>>,
) -> Result<Vec<u8>> {
    let mut command = if profile.controller == "local" {
        let executable = fixed_executable(&["/usr/bin/rsync", "/bin/rsync"])?;
        let mut command = Command::new(executable);
        command.args(&args);
        command
    } else {
        let host = config
            .ssh_hosts
            .get(&profile.controller)
            .with_context(|| format!("unknown SSH controller `{}`", profile.controller))?;
        let executable = fixed_executable(&["/usr/bin/ssh", "/bin/ssh"])?;
        let mut command = Command::new(executable);
        command.args(ssh_arguments(host));
        command.arg("--");
        command.arg(&host.destination);
        command.arg(remote_rsync_command(&args));
        command
    };
    command
        .stdin(if stdin_bytes.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = bounded_process_output(
        &mut command,
        stdin_bytes,
        Duration::from_secs(profile.timeout_seconds),
        MAX_PROCESS_OUTPUT_BYTES,
    )
    .await
    .context("run fixed remote sync transport")?;
    if !output.status.success() {
        let detail = bounded_text(&String::from_utf8_lossy(&output.stderr), 1_024);
        bail!("remote sync transport failed: {detail}");
    }
    Ok(output.stdout)
}

struct RemoteProcessGuard {
    pid: i32,
    group: bool,
    completed: bool,
}

#[derive(Debug)]
struct RemoteOutputLimit;
impl std::fmt::Display for RemoteOutputLimit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("remote transport output exceeded its admitted bound")
    }
}
impl std::error::Error for RemoteOutputLimit {}

fn terminate_admitted_transport_group() -> Result<()> {
    #[cfg(unix)]
    {
        let store = crate::runtime::current_run_store()
            .context("transport budget cleanup has no admitted execution")?;
        anyhow::ensure!(
            !store.context().resources.is_empty(),
            "transport cleanup has no ownership proof"
        );
        for resource in &store.context().resources {
            store.require_adopted_resource(resource)?;
        }
        let group = unsafe { libc::getpgrp() };
        anyhow::ensure!(group > 0, "transport cleanup group is invalid");
        // The ca parent owns leases outside this proven worker group. Killing
        // the worker preserves Running checkpoints for parent reconciliation.
        if unsafe { libc::kill(-group, libc::SIGKILL) } != 0 {
            return Err(std::io::Error::last_os_error()).context("stop admitted transport group");
        }
        Ok(())
    }
    #[cfg(not(unix))]
    bail!("admitted transport cleanup is unavailable")
}
impl Drop for RemoteProcessGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if self.group || !self.completed {
            // Legacy transports own a separate group; admitted native commands
            // retain the ca worker group and its parent drains descendants.
            unsafe { libc::kill(if self.group { -self.pid } else { self.pid }, libc::SIGKILL) };
        }
    }
}

pub(crate) async fn bounded_process_output(
    command: &mut Command,
    stdin_bytes: Option<Vec<u8>>,
    timeout: Duration,
    limit: usize,
) -> Result<std::process::Output> {
    let own_group = !crate::runtime::worker_contained();
    #[cfg(unix)]
    if own_group {
        command.process_group(0);
    }
    command.kill_on_drop(true);
    let mut child = command.spawn().context("start bounded remote transport")?;
    let mut guard = RemoteProcessGuard {
        pid: child
            .id()
            .context("remote transport has no process identity")? as i32,
        group: own_group,
        completed: false,
    };
    let stdout = child
        .stdout
        .take()
        .context("remote transport omitted stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("remote transport omitted stderr")?;
    let stdin = child.stdin.take();
    let total = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let stdout_total = total.clone();
    let result = tokio::time::timeout(timeout, async {
        let input = async move {
            if let Some(bytes) = stdin_bytes {
                let mut stdin = stdin.context("remote transport omitted stdin")?;
                stdin.write_all(&bytes).await?;
                stdin.shutdown().await?;
            }
            Ok::<_, anyhow::Error>(())
        };
        let (status, stdout, stderr, ()) = tokio::try_join!(
            async { child.wait().await.map_err(anyhow::Error::from) },
            read_process_output(stdout, stdout_total, limit),
            read_process_output(stderr, total, limit),
            input,
        )?;
        Ok::<_, anyhow::Error>(std::process::Output {
            status,
            stdout,
            stderr,
        })
    })
    .await;
    match result {
        Ok(Ok(output)) => {
            guard.completed = true;
            Ok(output)
        }
        error => {
            let budget_exceeded = match &error {
                Err(_) => true,
                Ok(Err(error)) => error.is::<RemoteOutputLimit>(),
                _ => false,
            };
            if budget_exceeded && crate::runtime::worker_contained() {
                terminate_admitted_transport_group()?;
            }
            // Dropping the guard/kill-on-drop child stops owned work immediately
            // rather than waiting for a flooding or stalled pipe to reach EOF.
            let _ = child.start_kill();
            guard.completed = child.try_wait().ok().flatten().is_some();
            match error {
                Ok(Err(error)) => Err(error),
                Err(_) => bail!("remote transport timed out"),
                Ok(Ok(_)) => unreachable!(),
            }
        }
    }
}

pub(crate) async fn read_process_output(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    total: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    limit: usize,
) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let count = reader.read(&mut chunk).await?;
        if count == 0 {
            return Ok(output);
        }
        let previous = total.fetch_add(count, std::sync::atomic::Ordering::Relaxed);
        if count > limit.saturating_sub(previous) {
            return Err(RemoteOutputLimit.into());
        }
        output.extend_from_slice(&chunk[..count]);
    }
}

fn rsync_arguments(
    profile: &SyncProfileV1,
    dry_run: bool,
    backup_dir: Option<&str>,
) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("--archive"),
        OsString::from("--partial"),
        OsString::from("--itemize-changes"),
        OsString::from("--out-format=%i\t%l\t%n%L"),
    ];
    if dry_run {
        args.push(OsString::from("--dry-run"));
    }
    if profile.delete {
        args.push(OsString::from("--delete"));
    }
    if let Some(backup_dir) = backup_dir {
        args.push(OsString::from("--backup"));
        args.push(OsString::from(format!("--backup-dir={backup_dir}")));
    }
    args.push(OsString::from("--"));
    args.push(OsString::from(&profile.source));
    args.push(OsString::from(&profile.destination));
    args
}

fn ssh_arguments(host: &SshHostV1) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("-T"),
        OsString::from("-o"),
        OsString::from("BatchMode=yes"),
        OsString::from("-o"),
        OsString::from("ClearAllForwardings=yes"),
        OsString::from("-o"),
        OsString::from("ConnectTimeout=10"),
        OsString::from("-o"),
        OsString::from("ConnectionAttempts=1"),
        OsString::from("-o"),
        OsString::from("PermitLocalCommand=no"),
        OsString::from("-o"),
        OsString::from("RequestTTY=no"),
    ];
    if let Some(port) = host.port {
        args.push(OsString::from("-p"));
        args.push(OsString::from(port.to_string()));
    }
    args
}

fn remote_rsync_command(args: &[OsString]) -> String {
    std::iter::once("/usr/bin/rsync".to_string())
        .chain(args.iter().map(|arg| shell_quote(&arg.to_string_lossy())))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
fn parse_rsync_plan(output: &[u8], max_changes: usize) -> Result<Vec<String>> {
    parse_rsync_plan_with_bytes(output, max_changes).map(|(entries, _)| entries)
}

fn parse_rsync_plan_with_bytes(output: &[u8], max_changes: usize) -> Result<(Vec<String>, u64)> {
    let text = std::str::from_utf8(output).context("rsync plan output is not UTF-8")?;
    let mut entries = Vec::new();
    let mut predicted_bytes = 0_u64;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if entries.len() == max_changes {
            bail!("remote sync plan exceeds its configured change bound");
        }
        if line.len() > 4_096 || line.chars().any(|character| character == '\0') {
            bail!("remote sync plan contains an invalid entry");
        }
        if let Some(length) = line.split('\t').nth(1).and_then(|value| value.parse().ok()) {
            predicted_bytes = predicted_bytes.saturating_add(length);
        }
        entries.push(bounded_text(line, 1_024));
    }
    Ok((entries, predicted_bytes))
}

fn require_accepted_plan(current: &str, accepted: &str) -> Result<()> {
    if current != accepted {
        bail!("remote sync plan changed; inspect and approve the fresh plan digest");
    }
    Ok(())
}

async fn torrent_query_result(
    config: &RemoteConfigV1,
    endpoint_name: &str,
    operation: RemoteRpcOperationV1,
) -> Result<TorrentQueryResult> {
    let locations = config.torrent_locations.clone();
    if let Some(endpoint) = config.transmission.get(endpoint_name) {
        return crate::torrent::query_transmission(
            endpoint_name.to_string(),
            endpoint.clone(),
            operation,
            locations,
        )
        .await;
    }
    if let Some(endpoint) = config.rtorrent.get(endpoint_name) {
        return crate::torrent::query_rtorrent(
            endpoint_name.to_string(),
            endpoint.clone(),
            config.ssh_hosts.clone(),
            operation,
            locations,
        )
        .await;
    }
    bail!("unknown torrent endpoint `{endpoint_name}`")
}

async fn torrent_query(
    config: &RemoteConfigV1,
    endpoint_name: &str,
    operation: RemoteRpcOperationV1,
) -> Result<Value> {
    let value = torrent_query_result(config, endpoint_name, operation)
        .await?
        .into_json()?;
    Ok(normalize_json(value))
}

async fn torrent_control(
    config: &RemoteConfigV1,
    endpoint_name: &str,
    target_hashes: Vec<String>,
    expected_snapshots: Vec<ca_automation_protocol::TorrentSnapshotV1>,
    control: TorrentControlActionV1,
) -> Result<TorrentMutationReceiptV1> {
    let locations = config.torrent_locations.clone();
    let resolved_location = match &control {
        TorrentControlActionV1::AdoptLocation { location } => {
            let configured = config
                .torrent_locations
                .get(location)
                .with_context(|| format!("unknown named torrent location `{location}`"))?;
            if configured.endpoint != endpoint_name {
                bail!("named torrent location belongs to a different endpoint")
            }
            Some(configured.path.clone())
        }
        _ => None,
    };
    let request = TorrentControlRequest {
        endpoint_name: endpoint_name.to_string(),
        locations,
        target_hashes,
        expected_snapshots,
        control,
        resolved_location,
    };
    if let Some(endpoint) = config.transmission.get(endpoint_name) {
        return crate::torrent::control_transmission(endpoint.clone(), request).await;
    }
    if let Some(endpoint) = config.rtorrent.get(endpoint_name) {
        return crate::torrent::control_rtorrent(
            endpoint.clone(),
            config.ssh_hosts.clone(),
            request,
        )
        .await;
    }
    bail!("unknown torrent endpoint `{endpoint_name}`")
}

fn normalize_json(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted = map
                .into_iter()
                .map(|(key, value)| (key, normalize_json(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(values) => {
            let mut normalized = values.into_iter().map(normalize_json).collect::<Vec<_>>();
            normalized.sort_by_key(|value| serde_json::to_string(value).unwrap_or_default());
            Value::Array(normalized)
        }
        other => other,
    }
}

fn render_rpc_entries(operation: RemoteRpcOperationV1, value: &Value) -> Vec<String> {
    match operation {
        RemoteRpcOperationV1::Torrents => value
            .get("torrents")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(1_000)
            .map(|torrent| {
                let name = torrent
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unnamed");
                let hash = torrent
                    .get("info_hash")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let status = torrent
                    .get("state")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let percent = torrent
                    .get("completion_millis")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as f64
                    / 10.0;
                bounded_text(
                    &format!(
                        "{name} | {} | status {status} | {:.1}%",
                        short_hash(hash),
                        percent
                    ),
                    1_024,
                )
            })
            .collect(),
        RemoteRpcOperationV1::Session => {
            let kind = value
                .get("client_kind")
                .and_then(Value::as_str)
                .unwrap_or("torrent client");
            let version = value
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let protocol = value
                .get("protocol")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            vec![format!("{kind} {version} | {protocol}")]
        }
        RemoteRpcOperationV1::FreeSpace => {
            let size = value.get("size-bytes").and_then(Value::as_u64).unwrap_or(0);
            let total = value
                .get("total-size")
                .or_else(|| value.get("total_size"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            vec![format!("free {size} bytes | total {total} bytes")]
        }
    }
}

fn compare_rpc_values(operation: RemoteRpcOperationV1, left: &Value, right: &Value) -> Vec<String> {
    if operation != RemoteRpcOperationV1::Torrents {
        return if left != right {
            vec!["normalized read-only RPC results differ".to_string()]
        } else {
            Vec::new()
        };
    }
    let torrent_keys = |value: &Value| {
        value
            .get("torrents")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|torrent| {
                torrent
                    .get("info_hash")
                    .and_then(Value::as_str)
                    .or_else(|| torrent.get("name").and_then(Value::as_str))
                    .map(str::to_string)
            })
            .collect::<BTreeSet<_>>()
    };
    let left_keys = torrent_keys(left);
    let right_keys = torrent_keys(right);
    left_keys
        .difference(&right_keys)
        .take(500)
        .map(|key| format!("left only: {}", short_hash(key)))
        .chain(
            right_keys
                .difference(&left_keys)
                .take(500)
                .map(|key| format!("right only: {}", short_hash(key))),
        )
        .collect()
}

fn short_hash(value: &str) -> &str {
    value.get(..value.len().min(12)).unwrap_or(value)
}

fn rpc_operation_name(operation: RemoteRpcOperationV1) -> &'static str {
    match operation {
        RemoteRpcOperationV1::Session => "session",
        RemoteRpcOperationV1::Torrents => "torrent",
        RemoteRpcOperationV1::FreeSpace => "free-space",
    }
}

fn load_config(path: &Path) -> Result<RemoteConfigV1> {
    if !path.is_absolute() {
        bail!("remote automation config path must be absolute");
    }
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("inspect remote automation config {}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        bail!("remote automation config must be one regular non-symlink file");
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        bail!("remote automation config exceeds its 64 KiB bound");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 || metadata.mode() & 0o077 != 0 {
            bail!("remote automation config must be private and have exactly one hard link");
        }
    }
    let bytes = std::fs::read(path).context("read remote automation config")?;
    let text = std::str::from_utf8(&bytes).context("remote automation config is not UTF-8")?;
    let config: RemoteConfigV1 = toml::from_str(text).context("parse remote automation config")?;
    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &RemoteConfigV1) -> Result<()> {
    if config.schema != REMOTE_CONFIG_SCHEMA {
        bail!("unsupported remote automation config schema");
    }
    if config.ssh_hosts.len() > 32
        || config.transmission.len() > 32
        || config.rtorrent.len() > 32
        || config.torrent_locations.len() > 128
        || config.syncs.len() > 64
    {
        bail!("remote automation config exceeds endpoint/profile bounds");
    }
    for (name, host) in &config.ssh_hosts {
        validate_name(name)?;
        validate_ssh_destination(&host.destination)?;
        if host.port == Some(0) {
            bail!("SSH port must be nonzero");
        }
    }
    for (name, endpoint) in &config.transmission {
        validate_name(name)?;
        crate::torrent::validate_transmission_endpoint(endpoint)?;
        if config.rtorrent.contains_key(name) {
            bail!("torrent endpoint names must be unique across clients")
        }
    }
    for (name, endpoint) in &config.rtorrent {
        validate_name(name)?;
        crate::torrent::validate_rtorrent_endpoint(endpoint, &config.ssh_hosts)?;
    }
    for (name, location) in &config.torrent_locations {
        validate_name(name)?;
        if !config.transmission.contains_key(&location.endpoint)
            && !config.rtorrent.contains_key(&location.endpoint)
        {
            bail!("torrent location `{name}` names an unknown endpoint")
        }
        validate_private_absolute_path("torrent location", &location.path)?;
    }
    for (name, profile) in &config.syncs {
        validate_name(name)?;
        if profile.controller != "local" && !config.ssh_hosts.contains_key(&profile.controller) {
            bail!("sync profile `{name}` names an unknown SSH controller");
        }
        validate_sync_operand("source", &profile.source)?;
        validate_sync_operand("destination", &profile.destination)?;
        if let Some(endpoint) = &profile.torrent_endpoint {
            if !config.transmission.contains_key(endpoint)
                && !config.rtorrent.contains_key(endpoint)
            {
                bail!("sync profile `{name}` names an unknown torrent endpoint")
            }
        }
        validate_timeout(profile.timeout_seconds)?;
        if profile.max_changes == 0 || profile.max_changes > MAX_CHANGES {
            bail!("sync max_changes must be between 1 and {MAX_CHANGES}");
        }
    }
    Ok(())
}

fn validate_private_absolute_path(label: &str, value: &str) -> Result<()> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > 4_096
        || value.chars().any(char::is_control)
        || !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        bail!("{label} must be one normalized absolute path")
    }
    Ok(())
}

fn validate_name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        bail!("remote names must contain 1-64 safe ASCII identifier bytes");
    }
    Ok(())
}

fn validate_ssh_destination(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 255
        || value.starts_with('-')
        || !value.is_ascii()
        || value.matches('@').count() > 1
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-@".contains(&byte))
    {
        bail!("SSH destination contains unsupported characters");
    }
    Ok(())
}

fn validate_timeout(value: u64) -> Result<()> {
    if value == 0 || value > MAX_TIMEOUT_SECONDS {
        bail!("remote timeout must be between 1 and {MAX_TIMEOUT_SECONDS} seconds");
    }
    Ok(())
}

fn validate_sync_operand(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 4_096
        || value.starts_with('-')
        || !value.is_ascii()
        || value.chars().any(char::is_control)
    {
        bail!("sync {label} is invalid");
    }
    let path_text = value.rsplit_once(':').map_or(value, |(_, path)| path);
    let path = Path::new(path_text);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("sync {label} must contain an absolute path without traversal");
    }
    if let Some((host, _)) = value.rsplit_once(':') {
        validate_ssh_destination(host)?;
    }
    Ok(())
}

fn fixed_executable(candidates: &[&str]) -> Result<PathBuf> {
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .context("required fixed remote transport executable is unavailable")
}

fn digest_json(value: &impl serde::Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

fn receipt_text_bytes(summary: &str, entries: &[String]) -> usize {
    summary.len() + entries.iter().map(String::len).sum::<usize>()
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let mut chars = sanitized.trim().chars();
    let bounded = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn remote_process_output_caps_both_streams_before_eof() -> Result<()> {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "printf abc; printf def >&2; sleep 30"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();
        let error = bounded_process_output(&mut command, None, Duration::from_secs(5), 5)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("output exceeded"));
        assert!(started.elapsed() < Duration::from_secs(2));
        Ok(())
    }
    #[tokio::test]
    async fn remote_transport_reads_input_and_both_streams_concurrently() -> Result<()> {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "printf diagnostic >&2; cat"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let input = vec![b'x'; 64 * 1024];
        let output = bounded_process_output(
            &mut command,
            Some(input.clone()),
            Duration::from_secs(5),
            input.len() + 10,
        )
        .await?;
        assert!(output.status.success());
        assert_eq!(output.stdout, input);
        assert_eq!(output.stderr, b"diagnostic");
        Ok(())
    }
    #[tokio::test]
    async fn remote_timeout_stops_an_owned_transport_before_its_effect() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let marker = temporary.path().join("late-effect");
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "sleep .3; printf late > \"$1\"", "fixture"])
            .arg(&marker)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        assert!(
            bounded_process_output(&mut command, None, Duration::from_millis(40), 1024,)
                .await
                .is_err()
        );
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(!marker.exists());
        Ok(())
    }

    fn config() -> RemoteConfigV1 {
        RemoteConfigV1 {
            schema: REMOTE_CONFIG_SCHEMA.into(),
            ssh_hosts: BTreeMap::from([(
                "server".into(),
                SshHostV1 {
                    destination: "ops@example.test".into(),
                    port: Some(2222),
                },
            )]),
            transmission: BTreeMap::from([(
                "seedbox".into(),
                TransmissionEndpointV1 {
                    url: "https://seedbox.example.test/transmission/rpc".into(),
                    username_env: Some("SEEDBOX_USER".into()),
                    password_env: Some("SEEDBOX_PASSWORD".into()),
                    protocol: ca_automation_protocol::TransmissionProtocolPreferenceV1::Auto,
                    timeout_seconds: 30,
                },
            )]),
            rtorrent: BTreeMap::new(),
            torrent_locations: BTreeMap::new(),
            syncs: BTreeMap::from([(
                "server_to_seedbox".into(),
                SyncProfileV1 {
                    controller: "server".into(),
                    source: "/srv/media/".into(),
                    destination: "seed@seedbox:/downloads/".into(),
                    torrent_endpoint: None,
                    delete: false,
                    timeout_seconds: 900,
                    max_changes: 100,
                },
            )]),
        }
    }

    #[test]
    fn config_rejects_command_shaped_values_and_credential_urls() {
        assert!(validate_config(&config()).is_ok());
        let mut invalid = config();
        invalid.ssh_hosts.get_mut("server").unwrap().destination = "host; touch /tmp/no".into();
        assert!(validate_config(&invalid).is_err());
        let mut invalid = config();
        invalid.transmission.get_mut("seedbox").unwrap().url =
            "https://user:secret@example.test/rpc".into();
        assert!(validate_config(&invalid).is_err());
        let mut invalid = config();
        invalid
            .syncs
            .get_mut("server_to_seedbox")
            .unwrap()
            .destination = "seed@seedbox:../../etc".into();
        assert!(validate_config(&invalid).is_err());
    }

    #[test]
    fn fixed_ssh_and_rsync_arguments_quote_configured_paths() {
        let config = config();
        let profile = &config.syncs["server_to_seedbox"];
        let args = rsync_arguments(profile, true, None);
        let rendered = args
            .iter()
            .map(|value| value.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert!(rendered.contains(&"--dry-run".to_string()));
        assert!(!rendered.contains(&"--delete".to_string()));
        assert_eq!(rendered[rendered.len() - 2], "/srv/media/");
        assert_eq!(rendered.last().unwrap(), "seed@seedbox:/downloads/");
        let remote = remote_rsync_command(&args);
        assert!(remote.starts_with("/usr/bin/rsync '--archive'"));
        assert!(!remote.contains(';'));
        let ssh = ssh_arguments(&config.ssh_hosts["server"]);
        assert!(ssh.iter().any(|value| value == "BatchMode=yes"));
        assert!(ssh.iter().any(|value| value == "2222"));
        let keep_both = rsync_arguments(
            profile,
            false,
            Some(".codex-team-conflicts/0123456789abcdef"),
        );
        assert!(keep_both.iter().any(|value| value == "--backup"));
        assert!(keep_both
            .iter()
            .any(|value| { value == "--backup-dir=.codex-team-conflicts/0123456789abcdef" }));
    }

    #[test]
    fn rsync_plan_is_bounded_and_digest_stable() {
        let output = b">f+++++++++\t12\tnew file.mkv\n.d..t......\t0\tdir/\n";
        let entries = parse_rsync_plan(output, 2).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(parse_rsync_plan(output, 1).is_err());
        let first = digest_json(&entries).unwrap();
        let second = digest_json(&entries).unwrap();
        assert_eq!(first, second);
        assert!(require_accepted_plan(&first, &second).is_ok());
        assert!(require_accepted_plan(&first, &"b".repeat(64)).is_err());
    }

    #[test]
    fn torrent_comparisons_are_normalized_by_info_hash() {
        let left = json!({"torrents":[{"info_hash":"aaa","name":"A"}]});
        let right =
            json!({"torrents":[{"name":"B","info_hash":"bbb"},{"name":"A","info_hash":"aaa"}]});
        let differences = compare_rpc_values(RemoteRpcOperationV1::Torrents, &left, &right);
        assert_eq!(differences, vec!["right only: bbb"]);
    }

    #[test]
    fn selection_aware_manifest_is_sorted_bounded_and_source_contained() {
        let mut config = config();
        let profile = config.syncs.get_mut("server_to_seedbox").unwrap();
        profile.source = "/downloads/".into();
        profile.torrent_endpoint = Some("seedbox".into());
        config.torrent_locations.insert(
            "downloads".into(),
            TorrentLocationV1 {
                endpoint: "seedbox".into(),
                path: "/downloads".into(),
            },
        );
        let selection = json!([{
            "location_name":"downloads",
            "files":[
                {"path":"B/file.bin","length_bytes":2},
                {"path":"A/file.bin","length_bytes":1}
            ]
        }]);
        let manifest =
            workflow_file_manifest(&config, "server_to_seedbox", &selection, Some(2)).unwrap();
        assert_eq!(manifest.entries, vec!["A/file.bin", "B/file.bin"]);
        assert_eq!(manifest.total_bytes, 3);
        assert_eq!(manifest.bytes, b"A/file.bin\0B/file.bin\0");
        assert!(workflow_file_manifest(&config, "server_to_seedbox", &selection, Some(1)).is_err());
        let traversal = json!([{
            "location_name":"downloads",
            "files":[{"path":"../escape","length_bytes":1}]
        }]);
        assert!(workflow_file_manifest(&config, "server_to_seedbox", &traversal, Some(2)).is_err());
        let wrong_location = json!([{
            "location_name":"other",
            "files":[{"path":"file.bin","length_bytes":1}]
        }]);
        assert!(
            workflow_file_manifest(&config, "server_to_seedbox", &wrong_location, Some(2)).is_err()
        );
    }

    #[test]
    fn torrent_endpoint_names_are_global_and_scgi_is_never_externally_exposed() {
        let mut value = config();
        value.rtorrent.insert(
            "seedbox".into(),
            RtorrentEndpointV1 {
                transport: crate::torrent::RtorrentTransportV1::LoopbackTcpScgi { port: 5000 },
                automation_label_slot: None,
                timeout_seconds: 30,
            },
        );
        assert!(validate_config(&value).is_err());

        let mut value = config();
        value.rtorrent.insert(
            "rtorrent".into(),
            RtorrentEndpointV1 {
                transport: crate::torrent::RtorrentTransportV1::HttpXmlRpc {
                    url: "http://seedbox.example.test/RPC2".into(),
                    username_env: None,
                    password_env: None,
                },
                automation_label_slot: None,
                timeout_seconds: 30,
            },
        );
        assert!(validate_config(&value).is_err());
    }

    #[test]
    fn typed_torrent_inventory_is_truncated_inside_the_public_receipt_bound() {
        let files = (0..2_000)
            .map(|index| ca_automation_protocol::TorrentFileV1 {
                path: format!("large/fixture-{index:04}-{}.bin", "x".repeat(80)),
                length_bytes: 1,
                completed_bytes: 1,
                priority: ca_automation_protocol::TorrentPriorityV1::Normal,
            })
            .collect();
        let inventory = TorrentInventoryV1 {
            endpoint: "seedbox".into(),
            identity: ca_automation_protocol::TorrentClientIdentityV1 {
                kind: ca_automation_protocol::TorrentClientKindV1::Rtorrent,
                protocol: ca_automation_protocol::TorrentProtocolV1::RtorrentXmlRpc,
                version: Some("0.9.8".into()),
                fingerprint: "a".repeat(64),
            },
            torrents: vec![ca_automation_protocol::TorrentSnapshotV1 {
                info_hash: "b".repeat(40),
                name: "large fixture".into(),
                state: ca_automation_protocol::TorrentStateV1::Seeding,
                completed: true,
                completion_millis: 1_000,
                total_bytes: 2_000,
                left_bytes: 0,
                ratio_millis: 1_000,
                priority: ca_automation_protocol::TorrentPriorityV1::Normal,
                labels: vec!["archive".into()],
                tracker_summary: None,
                location_name: Some("completed".into()),
                location_fingerprint: Some("d".repeat(64)),
                files,
                files_truncated: false,
            }],
            truncated: false,
        };
        let mut receipt = RemoteAutomationReceiptV1 {
            schema: REMOTE_AUTOMATION_RECEIPT_SCHEMA.into(),
            request_digest: "c".repeat(64),
            status: RemoteAutomationStatusV1::Completed,
            action_results: vec![RemoteAutomationActionReceiptV1 {
                action_index: 0,
                kind: "query_remote_rpc".into(),
                status: RemoteAutomationStatusV1::Completed,
                summary: "bounded inventory".into(),
                entries: Vec::new(),
                truncated: false,
                plan_digest: None,
                torrent_inventory: Some(inventory),
                torrent_mutation: None,
                duration_millis: 1,
            }],
            output_bytes: 0,
            started_at: "2026-08-27T00:00:00Z".into(),
            finished_at: "2026-08-27T00:00:01Z".into(),
            elapsed_millis: 1,
        };
        bound_remote_receipt(&mut receipt).unwrap();
        assert!(serde_json::to_vec(&receipt).unwrap().len() <= MAX_MECHANISTIC_OUTPUT_BYTES);
        assert!(receipt.action_results[0].truncated);
        let inventory = receipt.action_results[0]
            .torrent_inventory
            .as_ref()
            .unwrap();
        assert!(inventory.truncated);
        assert!(inventory.torrents[0].files_truncated);
        assert!(inventory.torrents[0].files.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn config_file_must_be_private_regular_and_single_linked() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("remote.toml");
        std::fs::write(
            &path,
            "schema = 'codex-team.remote_config.v1'\n[ssh_hosts.server]\ndestination = 'ops@example.test'\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(load_config(&path).is_ok());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_config(&path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn workflow_local_roots_are_exact_and_remote_controller_paths_are_not_host_grants() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let destination = directory.path().join("destination");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&destination).unwrap();
        let path = directory.path().join("remote.toml");
        std::fs::write(
            &path,
            format!(
                "schema = 'codex-team.remote_config.v1'\n[syncs.local]\ncontroller = 'local'\nsource = '{}/'\ndestination = '{}/'\n[syncs.remote]\ncontroller = 'server'\nsource = '/remote/source/'\ndestination = '/remote/destination/'\n[ssh_hosts.server]\ndestination = 'ops@example.test'\n",
                source.display(),
                destination.display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let roots = workflow_local_roots(&path).unwrap();
        assert_eq!(
            roots,
            vec![
                source.canonicalize().unwrap(),
                destination.canonicalize().unwrap()
            ]
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
        );
    }
}
