//! Digest-bound creation of private, typed automation configuration.
//!
//! The model supplies a closed setup draft. CA supplies the active profile,
//! pinned repository, and executable paths out of band. This module generates
//! TOML itself, previews exact effects without writing, and applies only the
//! recomputed approved digest.

use anyhow::{bail, Context, Result};
use ca_automation_protocol::{
    validate_automation_setup_request, AutomationRtorrentEndpointV1, AutomationRtorrentTransportV1,
    AutomationSetupDraftV1, AutomationSetupOperationV1, AutomationSetupReceiptV1,
    AutomationSetupRequestV1, AutomationSetupStatusV1, AutomationSshHostV1,
    AutomationSyncProfileV1, AutomationTorrentLocationV1, AutomationTransmissionEndpointV1,
    AUTOMATION_SETUP_RECEIPT_SCHEMA,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use toml_edit::{value, DocumentMut, Item, Table};

const MAX_PROFILE_BYTES: u64 = 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

#[derive(Debug, Serialize)]
struct GeneratedRemoteConfig<'a> {
    schema: &'static str,
    ssh_hosts: BTreeMap<&'a str, GeneratedSshHost<'a>>,
    transmission: BTreeMap<&'a str, GeneratedTransmission<'a>>,
    rtorrent: BTreeMap<&'a str, GeneratedRtorrent<'a>>,
    torrent_locations: BTreeMap<&'a str, GeneratedTorrentLocation<'a>>,
    syncs: BTreeMap<&'a str, GeneratedSync<'a>>,
}

#[derive(Debug, Serialize)]
struct GeneratedSshHost<'a> {
    destination: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
}

#[derive(Debug, Serialize)]
struct GeneratedTransmission<'a> {
    url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    username_env: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    password_env: Option<&'a str>,
    protocol: ca_automation_protocol::TransmissionProtocolPreferenceV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
struct GeneratedRtorrent<'a> {
    transport: GeneratedRtorrentTransport<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    automation_label_slot: Option<ca_automation_protocol::RtorrentCustomSlotV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum GeneratedRtorrentTransport<'a> {
    HttpXmlRpc {
        url: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        username_env: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        password_env: Option<&'a str>,
    },
    UnixScgi {
        socket: &'a str,
    },
    LoopbackTcpScgi {
        port: u16,
    },
    SshLoopbackScgi {
        ssh_host: &'a str,
        port: u16,
    },
}

#[derive(Debug, Serialize)]
struct GeneratedTorrentLocation<'a> {
    endpoint: &'a str,
    path: &'a str,
}

#[derive(Debug, Serialize)]
struct GeneratedSync<'a> {
    controller: &'a str,
    source: &'a str,
    destination: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    torrent_endpoint: Option<&'a str>,
    delete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_changes: Option<usize>,
}

struct SetupPlan {
    receipt: AutomationSetupReceiptV1,
    manifest_path: PathBuf,
    manifest_bytes: Vec<u8>,
    previous_manifest_bytes: Option<Vec<u8>>,
    previous_profile_bytes: Vec<u8>,
    profile_bytes: Vec<u8>,
}

struct GeneratedManifest<'a> {
    name: &'a str,
    bytes: Vec<u8>,
    environment_names: Vec<String>,
    effects: Vec<String>,
}

pub fn execute(
    profile_config: &Path,
    profile_label: &str,
    repository: &Path,
    request: &AutomationSetupRequestV1,
) -> Result<AutomationSetupReceiptV1> {
    validate_automation_setup_request(request).map_err(anyhow::Error::msg)?;
    let plan = build_plan(profile_config, profile_label, repository, request)?;
    match &request.operation {
        AutomationSetupOperationV1::Preview => Ok(plan.receipt),
        AutomationSetupOperationV1::Apply {
            accepted_plan_digest,
        } => {
            if accepted_plan_digest != &plan.receipt.plan_digest {
                bail!("automation setup changed after preview; preview it again");
            }
            apply_plan(profile_config, &plan)?;
            Ok(AutomationSetupReceiptV1 {
                status: AutomationSetupStatusV1::Applied,
                ..plan.receipt
            })
        }
    }
}

fn build_plan(
    profile_config: &Path,
    profile_label: &str,
    repository: &Path,
    request: &AutomationSetupRequestV1,
) -> Result<SetupPlan> {
    validate_profile_label(profile_label)?;
    let profile_config = absolute_clean_path(profile_config, "profile config")?;
    let repository = repository
        .canonicalize()
        .context("canonicalize pinned repository")?;
    if !repository.is_dir() {
        bail!("pinned repository is not a directory");
    }
    let profile_bytes = read_private_regular(&profile_config, MAX_PROFILE_BYTES, "profile config")?;
    let profile_text =
        std::str::from_utf8(&profile_bytes).context("profile config is not UTF-8")?;
    let mut document = profile_text
        .parse::<DocumentMut>()
        .context("profile config is not valid TOML")?;
    validate_delegate_profile(&document, &repository)?;

    let generated = generate_manifest(&request.draft)?;
    let manifest_name = generated.name;
    let manifest_bytes = generated.bytes;
    let environment_names = generated.environment_names;
    let effects = generated.effects;
    if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
        bail!("generated automation manifest exceeds 64 KiB");
    }
    crate::remote::validate_config_text(
        std::str::from_utf8(&manifest_bytes).expect("generated manifest is UTF-8"),
    )?;
    let manifest_path = manifest_path(&profile_config, profile_label, manifest_name)?;
    let prior_manifest = read_optional_private_regular(&manifest_path, MAX_MANIFEST_BYTES)?;
    patch_profile(&mut document, &manifest_path, &environment_names)?;
    let updated_profile = document.to_string().into_bytes();
    let executable = std::env::current_exe().context("resolve codex-team executable")?;
    let executable = executable
        .canonicalize()
        .context("canonicalize codex-team executable")?;
    let digest = digest_parts(&[
        b"codex-team.automation_setup_plan.v1",
        &profile_bytes,
        prior_manifest.as_deref().unwrap_or(b"<absent>"),
        &manifest_bytes,
        &updated_profile,
        repository.as_os_str().as_encoded_bytes(),
        executable.as_os_str().as_encoded_bytes(),
        profile_label.as_bytes(),
    ]);
    Ok(SetupPlan {
        receipt: AutomationSetupReceiptV1 {
            schema: AUTOMATION_SETUP_RECEIPT_SCHEMA.to_string(),
            status: AutomationSetupStatusV1::Previewed,
            template: request.draft.template_name().to_string(),
            manifest_name: manifest_name.to_string(),
            plan_digest: digest,
            effects,
            environment_names,
            profile_reload_required: true,
        },
        manifest_path,
        manifest_bytes,
        previous_manifest_bytes: prior_manifest,
        previous_profile_bytes: profile_bytes,
        profile_bytes: updated_profile,
    })
}

fn generate_manifest(draft: &AutomationSetupDraftV1) -> Result<GeneratedManifest<'_>> {
    match draft {
        AutomationSetupDraftV1::RemoteAutomation {
            manifest_name,
            ssh_hosts,
            transmission,
            rtorrent,
            torrent_locations,
            syncs,
        } => {
            let hosts = ssh_hosts
                .iter()
                .map(|entry: &AutomationSshHostV1| {
                    (
                        entry.name.as_str(),
                        GeneratedSshHost {
                            destination: &entry.destination,
                            port: entry.port,
                        },
                    )
                })
                .collect();
            let endpoints = transmission
                .iter()
                .map(|entry: &AutomationTransmissionEndpointV1| {
                    (
                        entry.name.as_str(),
                        GeneratedTransmission {
                            url: &entry.url,
                            username_env: entry.username_env.as_deref(),
                            password_env: entry.password_env.as_deref(),
                            protocol: entry.protocol,
                            timeout_seconds: entry.timeout_seconds,
                        },
                    )
                })
                .collect();
            let rtorrent_endpoints = rtorrent
                .iter()
                .map(|entry: &AutomationRtorrentEndpointV1| {
                    let transport = match &entry.transport {
                        AutomationRtorrentTransportV1::HttpXmlRpc {
                            url,
                            username_env,
                            password_env,
                        } => GeneratedRtorrentTransport::HttpXmlRpc {
                            url,
                            username_env: username_env.as_deref(),
                            password_env: password_env.as_deref(),
                        },
                        AutomationRtorrentTransportV1::UnixScgi { socket } => {
                            GeneratedRtorrentTransport::UnixScgi { socket }
                        }
                        AutomationRtorrentTransportV1::LoopbackTcpScgi { port } => {
                            GeneratedRtorrentTransport::LoopbackTcpScgi { port: *port }
                        }
                        AutomationRtorrentTransportV1::SshLoopbackScgi { ssh_host, port } => {
                            GeneratedRtorrentTransport::SshLoopbackScgi {
                                ssh_host,
                                port: *port,
                            }
                        }
                    };
                    (
                        entry.name.as_str(),
                        GeneratedRtorrent {
                            transport,
                            automation_label_slot: entry.automation_label_slot,
                            timeout_seconds: entry.timeout_seconds,
                        },
                    )
                })
                .collect();
            let locations = torrent_locations
                .iter()
                .map(|entry: &AutomationTorrentLocationV1| {
                    (
                        entry.name.as_str(),
                        GeneratedTorrentLocation {
                            endpoint: &entry.endpoint,
                            path: &entry.path,
                        },
                    )
                })
                .collect();
            let profiles = syncs
                .iter()
                .map(|entry: &AutomationSyncProfileV1| {
                    (
                        entry.name.as_str(),
                        GeneratedSync {
                            controller: &entry.controller,
                            source: &entry.source,
                            destination: &entry.destination,
                            torrent_endpoint: entry.torrent_endpoint.as_deref(),
                            delete: entry.delete,
                            timeout_seconds: entry.timeout_seconds,
                            max_changes: entry.max_changes,
                        },
                    )
                })
                .collect();
            let generated = GeneratedRemoteConfig {
                schema: "codex-team.remote_config.v1",
                ssh_hosts: hosts,
                transmission: endpoints,
                rtorrent: rtorrent_endpoints,
                torrent_locations: locations,
                syncs: profiles,
            };
            let mut environment_names = transmission
                .iter()
                .flat_map(|endpoint| {
                    [endpoint.username_env.clone(), endpoint.password_env.clone()]
                        .into_iter()
                        .flatten()
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            environment_names.extend(rtorrent.iter().flat_map(|endpoint| {
                match &endpoint.transport {
                    AutomationRtorrentTransportV1::HttpXmlRpc {
                        username_env,
                        password_env,
                        ..
                    } => [username_env.clone(), password_env.clone()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>(),
                    _ => Vec::new(),
                }
            }));
            environment_names.sort();
            environment_names.dedup();
            let mut effects = Vec::new();
            effects.extend(
                ssh_hosts
                    .iter()
                    .map(|entry| format!("configure SSH host `{}`", entry.name)),
            );
            effects.extend(
                transmission.iter().map(|entry| {
                    format!("configure governed Transmission endpoint `{}`", entry.name)
                }),
            );
            effects.extend(
                rtorrent
                    .iter()
                    .map(|entry| format!("configure governed rTorrent endpoint `{}`", entry.name)),
            );
            effects.extend(
                torrent_locations
                    .iter()
                    .map(|entry| format!("configure named torrent location `{}`", entry.name)),
            );
            effects.extend(
                syncs
                    .iter()
                    .map(|entry| format!("configure remote sync profile `{}`", entry.name)),
            );
            effects.push("enable typed remote automation in the active profile".into());
            Ok(GeneratedManifest {
                name: manifest_name,
                bytes: toml::to_string_pretty(&generated)?.into_bytes(),
                environment_names,
                effects,
            })
        }
    }
}

fn validate_delegate_profile(document: &DocumentMut, repository: &Path) -> Result<()> {
    let mechanistic = document
        .get("mcp")
        .and_then(Item::as_table_like)
        .and_then(|mcp| mcp.get("mechanistic"))
        .and_then(Item::as_table_like)
        .context("active profile must enable base mechanistic delegation first")?;
    if mechanistic.get("enabled").and_then(Item::as_bool) != Some(true) {
        bail!("active profile must enable base mechanistic delegation first");
    }
    let server_name = mechanistic
        .get("server")
        .and_then(Item::as_str)
        .context("mechanistic server name is missing")?;
    let server = document
        .get("mcp")
        .and_then(Item::as_table_like)
        .and_then(|mcp| mcp.get("servers"))
        .and_then(Item::as_table_like)
        .and_then(|servers| servers.get(server_name))
        .and_then(Item::as_table_like)
        .context("mechanistic delegate server is missing")?;
    if server.get("transport").and_then(Item::as_str) != Some("stdio")
        || server.get("network").and_then(Item::as_bool) != Some(false)
        || server.get("enabled").and_then(Item::as_bool) != Some(true)
    {
        bail!("mechanistic delegate server must be enabled local stdio with network disabled");
    }
    let configured_executable = server
        .get("command")
        .and_then(Item::as_str)
        .context("mechanistic delegate server command is missing")?;
    let configured_executable = Path::new(configured_executable)
        .canonicalize()
        .context("canonicalize configured codex-team executable")?;
    let current_executable = std::env::current_exe()?
        .canonicalize()
        .context("canonicalize running codex-team executable")?;
    if configured_executable != current_executable {
        bail!("mechanistic delegate server command does not match the running codex-team binary");
    }
    let args = server
        .get("args")
        .and_then(Item::as_array)
        .context("mechanistic delegate server args are missing")?
        .iter()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    let configured_repository = args
        .windows(2)
        .find(|pair| pair[0] == "--repo")
        .and_then(|pair| Path::new(pair[1]).canonicalize().ok());
    let exact = configured_repository.as_deref() == Some(repository)
        && args.windows(2).any(|pair| pair == ["--access", "delegate"])
        && args.first() == Some(&"mcp");
    if !exact {
        bail!("mechanistic delegate server is not pinned to this repository in delegate mode");
    }
    Ok(())
}

fn patch_profile(
    document: &mut DocumentMut,
    manifest_path: &Path,
    environment_names: &[String],
) -> Result<()> {
    ensure_table(document.as_table_mut(), "mcp")?;
    let mcp = document["mcp"]
        .as_table_mut()
        .context("mcp must be a table")?;
    ensure_table(mcp, "mechanistic")?;
    let mechanistic = mcp["mechanistic"]
        .as_table_mut()
        .context("mcp.mechanistic must be a table")?;
    ensure_table(mechanistic, "remote")?;
    let remote = mechanistic["remote"]
        .as_table_mut()
        .context("mcp.mechanistic.remote must be a table")?;
    remote["enabled"] = value(true);
    remote["config"] = value(manifest_path.to_string_lossy().to_string());
    if !remote.contains_key("max_wait_seconds") {
        remote["max_wait_seconds"] = value(1_800_i64);
    }
    ensure_table(remote, "env")?;
    let env = remote["env"]
        .as_table_mut()
        .context("mcp.mechanistic.remote.env must be a table")?;
    for name in environment_names {
        env[name] = value(format!("${{{name}}}"));
    }
    Ok(())
}

fn ensure_table(parent: &mut Table, key: &str) -> Result<()> {
    if !parent.contains_key(key) {
        parent.insert(key, Item::Table(Table::new()));
    }
    if !parent[key].is_table() {
        bail!("{key} must be a table");
    }
    Ok(())
}

fn apply_plan(profile_config: &Path, plan: &SetupPlan) -> Result<()> {
    let current_profile =
        read_private_regular(profile_config, MAX_PROFILE_BYTES, "profile config")?;
    let current_manifest = read_optional_private_regular(&plan.manifest_path, MAX_MANIFEST_BYTES)?;
    if current_profile != plan.previous_profile_bytes
        || current_manifest != plan.previous_manifest_bytes
    {
        bail!("automation setup inputs changed after validation; preview again");
    }
    let previous_manifest = current_manifest;
    create_private_parents(&plan.manifest_path)?;
    atomic_write(&plan.manifest_path, &plan.manifest_bytes, 0o600)?;
    if let Err(error) = atomic_write(profile_config, &plan.profile_bytes, 0o600) {
        match previous_manifest {
            Some(bytes) => {
                let _ = atomic_write(&plan.manifest_path, &bytes, 0o600);
            }
            None => {
                let _ = std::fs::remove_file(&plan.manifest_path);
            }
        }
        return Err(error).context("write active profile after automation manifest");
    }
    Ok(())
}

fn manifest_path(profile_config: &Path, profile_label: &str, name: &str) -> Result<PathBuf> {
    let parent = profile_config
        .parent()
        .context("profile config has no parent")?;
    let config_root = if profile_label == "default" {
        parent
    } else {
        if parent.file_name().and_then(|value| value.to_str()) != Some("profiles") {
            bail!("named profile config must be inside the profiles directory");
        }
        parent
            .parent()
            .context("named profile config has no root")?
    };
    Ok(config_root
        .join("automation")
        .join(profile_label)
        .join(format!("{name}.toml")))
}

fn create_private_parents(path: &Path) -> Result<()> {
    let parent = path.parent().context("automation manifest has no parent")?;
    std::fs::create_dir_all(parent).context("create automation config directory")?;
    let automation_root = parent
        .parent()
        .context("automation manifest directory has no root")?;
    for directory in [automation_root, parent] {
        let metadata = std::fs::symlink_metadata(directory)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("automation config directory is unsafe");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

fn read_private_regular(path: &Path, maximum: u64, label: &str) -> Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path).with_context(|| format!("read {label}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > maximum {
        bail!("{label} must be a bounded regular non-symlink file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 || metadata.mode() & 0o077 != 0 {
            bail!("{label} must be private and have exactly one hard link");
        }
    }
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        bail!("{label} exceeds its size bound");
    }
    Ok(bytes)
}

fn read_optional_private_regular(path: &Path, maximum: u64) -> Result<Option<Vec<u8>>> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => read_private_regular(path, maximum, "automation manifest").map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let parent = path.parent().context("write target has no parent")?;
    let temp = parent.join(format!(".automation-setup-{}.tmp", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    let result = (|| -> Result<()> {
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn absolute_clean_path(path: &Path, label: &str) -> Result<PathBuf> {
    if !path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        bail!("{label} must be an absolute normalized path");
    }
    Ok(path.to_path_buf())
}

fn validate_profile_label(label: &str) -> Result<()> {
    if label == "bare"
        || label.is_empty()
        || label.len() > 64
        || (label != "default"
            && (!label.as_bytes()[0].is_ascii_lowercase()
                || !label.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'-' | b'_')
                })))
    {
        bail!("automation setup requires a writable default or named profile");
    }
    Ok(())
}

fn digest_parts(parts: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part);
    }
    format!("{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ca_automation_protocol::{
        AutomationRtorrentEndpointV1, AutomationRtorrentTransportV1, AutomationSetupDraftV1,
        AutomationSetupOperationV1, AutomationTorrentLocationV1, RtorrentCustomSlotV1,
    };
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn profile(binary: &Path, repository: &Path) -> String {
        format!(
            "[mcp.servers.codex_team_delegate]\ntransport = 'stdio'\ncommand = '{}'\nargs = ['mcp', '--repo', '{}', '--access', 'delegate']\nenabled = true\nnetwork = false\n\n[mcp.mechanistic]\nenabled = true\nserver = 'codex_team_delegate'\n",
            binary.display(),
            repository.display()
        )
    }

    fn request(operation: AutomationSetupOperationV1) -> AutomationSetupRequestV1 {
        AutomationSetupRequestV1 {
            schema: ca_automation_protocol::AUTOMATION_SETUP_REQUEST_SCHEMA.into(),
            operation,
            draft: AutomationSetupDraftV1::RemoteAutomation {
                manifest_name: "seedbox".into(),
                ssh_hosts: vec![AutomationSshHostV1 {
                    name: "server".into(),
                    destination: "ops@example.test".into(),
                    port: None,
                }],
                transmission: vec![AutomationTransmissionEndpointV1 {
                    name: "seedbox".into(),
                    url: "https://seedbox.example/transmission/rpc".into(),
                    username_env: Some("SEEDBOX_USER".into()),
                    password_env: Some("SEEDBOX_PASSWORD".into()),
                    protocol: ca_automation_protocol::TransmissionProtocolPreferenceV1::Auto,
                    timeout_seconds: None,
                }],
                rtorrent: Vec::new(),
                torrent_locations: Vec::new(),
                syncs: vec![AutomationSyncProfileV1 {
                    name: "server_to_seedbox".into(),
                    controller: "server".into(),
                    source: "/data/completed/".into(),
                    destination: "seedbox:/data/incoming/".into(),
                    torrent_endpoint: None,
                    delete: false,
                    timeout_seconds: None,
                    max_changes: None,
                }],
            },
        }
    }

    #[test]
    fn generated_manifest_binds_rtorrent_transport_location_and_sync_without_secret_values() {
        let draft = AutomationSetupDraftV1::RemoteAutomation {
            manifest_name: "rtorrent_seedbox".into(),
            ssh_hosts: Vec::new(),
            transmission: Vec::new(),
            rtorrent: vec![AutomationRtorrentEndpointV1 {
                name: "seedbox".into(),
                transport: AutomationRtorrentTransportV1::HttpXmlRpc {
                    url: "https://seedbox.example/RPC2".into(),
                    username_env: Some("RTORRENT_USER".into()),
                    password_env: Some("RTORRENT_PASSWORD".into()),
                },
                automation_label_slot: Some(RtorrentCustomSlotV1::Custom3),
                timeout_seconds: Some(30),
            }],
            torrent_locations: vec![AutomationTorrentLocationV1 {
                name: "completed".into(),
                endpoint: "seedbox".into(),
                path: "/srv/downloads/completed".into(),
            }],
            syncs: vec![AutomationSyncProfileV1 {
                name: "completed_home".into(),
                controller: "local".into(),
                source: "/srv/downloads/completed/".into(),
                destination: "/media/incoming/".into(),
                torrent_endpoint: Some("seedbox".into()),
                delete: false,
                timeout_seconds: Some(60),
                max_changes: Some(100),
            }],
        };
        let generated = generate_manifest(&draft).unwrap();
        let text = String::from_utf8(generated.bytes).unwrap();
        crate::remote::validate_config_text(&text).unwrap();
        assert!(text.contains("[rtorrent.seedbox.transport]"));
        assert!(text.contains("kind = \"http_xml_rpc\""));
        assert!(text.contains("automation_label_slot = \"custom3\""));
        assert!(text.contains("torrent_endpoint = \"seedbox\""));
        assert!(text.contains("[torrent_locations.completed]"));
        assert_eq!(
            generated.environment_names,
            vec!["RTORRENT_PASSWORD", "RTORRENT_USER"]
        );
        assert!(!text.contains("credential-value"));
    }

    #[test]
    fn preview_is_no_write_and_apply_is_digest_bound() {
        let temp = tempfile::tempdir().unwrap();
        let repository = temp.path().join("repo");
        std::fs::create_dir(&repository).unwrap();
        let profiles = temp.path().join("profiles");
        std::fs::create_dir(&profiles).unwrap();
        let profile_path = profiles.join("work.toml");
        std::fs::write(
            &profile_path,
            profile(&std::env::current_exe().unwrap(), &repository),
        )
        .unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&profile_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let preview = execute(
            &profile_path,
            "work",
            &repository,
            &request(AutomationSetupOperationV1::Preview),
        )
        .unwrap();
        let manifest = temp.path().join("automation/work/seedbox.toml");
        assert!(!manifest.exists());
        let applied = execute(
            &profile_path,
            "work",
            &repository,
            &request(AutomationSetupOperationV1::Apply {
                accepted_plan_digest: preview.plan_digest,
            }),
        )
        .unwrap();
        assert_eq!(applied.status, AutomationSetupStatusV1::Applied);
        assert!(manifest.is_file());
        let updated = std::fs::read_to_string(&profile_path).unwrap();
        assert!(updated.contains("[mcp.mechanistic.remote]"));
        assert!(updated.contains("SEEDBOX_PASSWORD = \"${SEEDBOX_PASSWORD}\""));
        let manifest_before = std::fs::read(&manifest).unwrap();
        let profile_before = std::fs::read(&profile_path).unwrap();
        let second_preview = execute(
            &profile_path,
            "work",
            &repository,
            &request(AutomationSetupOperationV1::Preview),
        )
        .unwrap();
        execute(
            &profile_path,
            "work",
            &repository,
            &request(AutomationSetupOperationV1::Apply {
                accepted_plan_digest: second_preview.plan_digest,
            }),
        )
        .unwrap();
        assert_eq!(std::fs::read(&manifest).unwrap(), manifest_before);
        assert_eq!(std::fs::read(&profile_path).unwrap(), profile_before);
        #[cfg(unix)]
        {
            assert_eq!(
                std::fs::metadata(&manifest).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(manifest.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn drift_and_unsafe_profile_refuse_without_manifest_write() {
        let temp = tempfile::tempdir().unwrap();
        let repository = temp.path().join("repo");
        std::fs::create_dir(&repository).unwrap();
        let profiles = temp.path().join("profiles");
        std::fs::create_dir(&profiles).unwrap();
        let profile_path = profiles.join("work.toml");
        std::fs::write(
            &profile_path,
            profile(&std::env::current_exe().unwrap(), &repository),
        )
        .unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&profile_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let preview = execute(
            &profile_path,
            "work",
            &repository,
            &request(AutomationSetupOperationV1::Preview),
        )
        .unwrap();
        std::fs::write(&profile_path, "[mcp]\n").unwrap();
        let error = execute(
            &profile_path,
            "work",
            &repository,
            &request(AutomationSetupOperationV1::Apply {
                accepted_plan_digest: preview.plan_digest,
            }),
        )
        .unwrap_err();
        assert!(error.to_string().contains("base mechanistic delegation"));
        assert!(!temp.path().join("automation/work/seedbox.toml").exists());
    }
}
