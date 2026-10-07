//! Closed, bounded torrent RPC adapters.
//!
//! This module owns protocol negotiation and transports. Callers provide only
//! private, validated named-resource records and closed typed operations; there
//! is deliberately no generic method-name, raw XML, URL, or command surface.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use ca_automation_protocol::{
    RemoteRpcOperationV1, RtorrentCustomSlotV1, TorrentClientIdentityV1, TorrentClientKindV1,
    TorrentControlActionV1, TorrentFileV1, TorrentInventoryV1, TorrentMutationReceiptV1,
    TorrentPriorityV1, TorrentProtocolV1, TorrentSnapshotV1, TorrentStateV1,
    TransmissionProtocolPreferenceV1, MAX_TORRENT_FILES, MAX_TORRENT_LABELS,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error as StdError;
use std::ffi::OsString;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpStream};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use xmlrpc::{Request as XmlRequest, Transport as XmlTransport, Value as XmlValue};

const MAX_RPC_BYTES: u64 = 1024 * 1024;
const MAX_TORRENTS: usize = 512;
const MAX_XML_DEPTH: usize = 16;
const MAX_XML_NODES: usize = 50_000;
const MAX_REMOTE_TEXT: usize = 512;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SshHostV1 {
    pub(crate) destination: String,
    #[serde(default)]
    pub(crate) port: Option<u16>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TransmissionEndpointV1 {
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) username_env: Option<String>,
    #[serde(default)]
    pub(crate) password_env: Option<String>,
    #[serde(default)]
    pub(crate) protocol: TransmissionProtocolPreferenceV1,
    #[serde(default = "default_timeout_seconds")]
    pub(crate) timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RtorrentTransportV1 {
    HttpXmlRpc {
        url: String,
        #[serde(default)]
        username_env: Option<String>,
        #[serde(default)]
        password_env: Option<String>,
    },
    UnixScgi {
        socket: String,
    },
    LoopbackTcpScgi {
        port: u16,
    },
    SshLoopbackScgi {
        ssh_host: String,
        port: u16,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RtorrentEndpointV1 {
    pub(crate) transport: RtorrentTransportV1,
    #[serde(default)]
    pub(crate) automation_label_slot: Option<RtorrentCustomSlotV1>,
    #[serde(default = "default_timeout_seconds")]
    pub(crate) timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TorrentLocationV1 {
    pub(crate) endpoint: String,
    pub(crate) path: String,
}

#[derive(Debug, Clone)]
pub(crate) enum TorrentQueryResult {
    Session(JsonValue),
    Torrents(TorrentInventoryV1),
    FreeSpace(JsonValue),
}

#[derive(Debug, Clone)]
pub(crate) struct TorrentControlRequest {
    pub(crate) endpoint_name: String,
    pub(crate) locations: BTreeMap<String, TorrentLocationV1>,
    pub(crate) target_hashes: Vec<String>,
    pub(crate) expected_snapshots: Vec<TorrentSnapshotV1>,
    pub(crate) control: TorrentControlActionV1,
    pub(crate) resolved_location: Option<String>,
}

impl TorrentQueryResult {
    pub(crate) fn into_json(self) -> Result<JsonValue> {
        match self {
            Self::Session(value) | Self::FreeSpace(value) => Ok(value),
            Self::Torrents(value) => {
                serde_json::to_value(value).context("serialize torrent inventory")
            }
        }
    }

    pub(crate) fn inventory(&self) -> Option<&TorrentInventoryV1> {
        match self {
            Self::Torrents(inventory) => Some(inventory),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct TransmissionProbe {
    protocol: TorrentProtocolV1,
    version: Option<String>,
    fingerprint: String,
}

fn transmission_probe_cache() -> &'static Mutex<HashMap<String, TransmissionProbe>> {
    static CACHE: OnceLock<Mutex<HashMap<String, TransmissionProbe>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn validate_transmission_endpoint(endpoint: &TransmissionEndpointV1) -> Result<()> {
    validate_http_url(&endpoint.url)?;
    validate_credentials(&endpoint.username_env, &endpoint.password_env)?;
    validate_timeout(endpoint.timeout_seconds)
}

pub(crate) fn validate_rtorrent_endpoint(
    endpoint: &RtorrentEndpointV1,
    ssh_hosts: &BTreeMap<String, SshHostV1>,
) -> Result<()> {
    validate_timeout(endpoint.timeout_seconds)?;
    match &endpoint.transport {
        RtorrentTransportV1::HttpXmlRpc {
            url,
            username_env,
            password_env,
        } => {
            validate_http_url(url)?;
            validate_credentials(username_env, password_env)
        }
        RtorrentTransportV1::UnixScgi { socket } => validate_absolute_path(socket, "SCGI socket"),
        RtorrentTransportV1::LoopbackTcpScgi { port } => {
            if *port == 0 {
                bail!("SCGI port must be non-zero")
            }
            Ok(())
        }
        RtorrentTransportV1::SshLoopbackScgi { ssh_host, port } => {
            if *port == 0 || !ssh_hosts.contains_key(ssh_host) {
                bail!("remote SCGI requires a named SSH resource and non-zero loopback port")
            }
            Ok(())
        }
    }
}

pub(crate) async fn query_transmission(
    endpoint_name: String,
    endpoint: TransmissionEndpointV1,
    operation: RemoteRpcOperationV1,
    locations: BTreeMap<String, TorrentLocationV1>,
) -> Result<TorrentQueryResult> {
    tokio::task::spawn_blocking(move || {
        let probe = probe_transmission(&endpoint)?;
        transmission_query_blocking(&endpoint_name, &endpoint, &probe, operation, &locations)
    })
    .await
    .context("join Transmission RPC query")?
}

pub(crate) async fn query_rtorrent(
    endpoint_name: String,
    endpoint: RtorrentEndpointV1,
    ssh_hosts: BTreeMap<String, SshHostV1>,
    operation: RemoteRpcOperationV1,
    locations: BTreeMap<String, TorrentLocationV1>,
) -> Result<TorrentQueryResult> {
    tokio::task::spawn_blocking(move || {
        rtorrent_query_blocking(&endpoint_name, &endpoint, &ssh_hosts, operation, &locations)
    })
    .await
    .context("join rTorrent RPC query")?
}

pub(crate) async fn control_transmission(
    endpoint: TransmissionEndpointV1,
    request: TorrentControlRequest,
) -> Result<TorrentMutationReceiptV1> {
    tokio::task::spawn_blocking(move || {
        let TorrentControlRequest {
            endpoint_name,
            locations,
            target_hashes,
            expected_snapshots,
            control,
            resolved_location,
        } = request;
        let query_endpoint_name = endpoint_name.clone();
        let post_hashes = target_hashes.clone();
        let probe = probe_transmission_current(&endpoint)?;
        let before = transmission_inventory(
            &endpoint_name,
            &endpoint,
            &probe,
            Some(&target_hashes),
            &locations,
        )?;
        require_exact_hashes(&before, &target_hashes)?;
        require_expected_snapshots(&before, &expected_snapshots)?;
        let sent = transmission_mutation(
            &endpoint,
            &probe,
            &target_hashes,
            &control,
            resolved_location.as_deref(),
        );
        resolve_mutation(endpoint_name, before, target_hashes, control, sent, || {
            let post_probe = probe_transmission_current(&endpoint)?;
            transmission_inventory(
                &query_endpoint_name,
                &endpoint,
                &post_probe,
                Some(&post_hashes),
                &locations,
            )
        })
    })
    .await
    .context("join Transmission RPC mutation")?
}

pub(crate) async fn control_rtorrent(
    endpoint: RtorrentEndpointV1,
    ssh_hosts: BTreeMap<String, SshHostV1>,
    request: TorrentControlRequest,
) -> Result<TorrentMutationReceiptV1> {
    tokio::task::spawn_blocking(move || {
        let TorrentControlRequest {
            endpoint_name,
            locations,
            target_hashes,
            expected_snapshots,
            control,
            resolved_location,
        } = request;
        let before = rtorrent_inventory(
            &endpoint_name,
            &endpoint,
            &ssh_hosts,
            &locations,
            Some(&target_hashes),
        )?;
        require_exact_hashes(&before, &target_hashes)?;
        require_expected_snapshots(&before, &expected_snapshots)?;
        let sent = rtorrent_mutation(
            &endpoint,
            &ssh_hosts,
            &target_hashes,
            &control,
            resolved_location.as_deref(),
        );
        resolve_mutation(
            endpoint_name.clone(),
            before,
            target_hashes.clone(),
            control,
            sent,
            || {
                rtorrent_inventory(
                    &endpoint_name,
                    &endpoint,
                    &ssh_hosts,
                    &locations,
                    Some(&target_hashes),
                )
            },
        )
    })
    .await
    .context("join rTorrent RPC mutation")?
}

fn resolve_mutation(
    endpoint: String,
    before: TorrentInventoryV1,
    target_hashes: Vec<String>,
    control: TorrentControlActionV1,
    sent: Result<()>,
    post_query: impl FnOnce() -> Result<TorrentInventoryV1>,
) -> Result<TorrentMutationReceiptV1> {
    let send_failed = sent.is_err();
    if send_failed
        && matches!(
            control,
            TorrentControlActionV1::Verify | TorrentControlActionV1::Reannounce
        )
    {
        return Err(sent.expect_err("checked failed send"));
    }
    let after = post_query().context("inspect torrent state after mutation")?;
    if before.identity != after.identity {
        bail!("torrent client identity or protocol changed during mutation")
    }
    require_exact_hashes(&after, &target_hashes)?;
    let confirmed = mutation_effect_confirmed(&after, &control, &target_hashes);
    let stateful_control = !matches!(
        control,
        TorrentControlActionV1::Verify | TorrentControlActionV1::Reannounce
    );
    if stateful_control && !confirmed {
        if send_failed {
            return Err(sent.expect_err("checked failed send")).context(
                "torrent mutation transport failed and post-error state did not prove the effect",
            );
        }
        bail!("torrent mutation returned success but its post-state was not confirmed")
    }
    let changed_hashes = target_hashes
        .iter()
        .filter(|hash| snapshot_by_hash(&before, hash) != snapshot_by_hash(&after, hash))
        .cloned()
        .collect::<Vec<_>>();
    let target_hashes_digest = digest_json(&target_hashes)?;
    let changed_hashes_digest = digest_json(&changed_hashes)?;
    Ok(TorrentMutationReceiptV1 {
        endpoint,
        identity: after.identity,
        action: control,
        target_hashes,
        changed_hashes,
        target_hashes_digest: Some(target_hashes_digest),
        changed_hashes_digest: Some(changed_hashes_digest),
        post_state_confirmed: confirmed,
        ambiguous_transport_resolved: send_failed && confirmed,
        truncated: false,
    })
}

fn probe_transmission(endpoint: &TransmissionEndpointV1) -> Result<TransmissionProbe> {
    let key = digest_json(endpoint)?;
    if let Some(cached) = transmission_probe_cache()
        .lock()
        .expect("Transmission probe cache poisoned")
        .get(&key)
        .cloned()
    {
        return Ok(cached);
    }
    let candidates: &[TorrentProtocolV1] = match endpoint.protocol {
        TransmissionProtocolPreferenceV1::Auto => &[
            TorrentProtocolV1::TransmissionJsonRpc2,
            TorrentProtocolV1::TransmissionLegacy,
        ],
        TransmissionProtocolPreferenceV1::Legacy => &[TorrentProtocolV1::TransmissionLegacy],
        TransmissionProtocolPreferenceV1::JsonRpc2 => &[TorrentProtocolV1::TransmissionJsonRpc2],
    };
    let mut last_error = None;
    for protocol in candidates {
        match transmission_call(
            endpoint,
            *protocol,
            "session_get",
            json!({"fields":["version","rpc_version_semver"]}),
        ) {
            Ok(value) => {
                let version = json_string(&value, "version");
                let fingerprint = crate::json_identity::digest_value(&json!({
                    "resource": key,
                    "protocol": protocol,
                    "version": version,
                    "rpc_version_semver": json_string(&value, "rpc_version_semver"),
                }))?;
                let probe = TransmissionProbe {
                    protocol: *protocol,
                    version,
                    fingerprint,
                };
                transmission_probe_cache()
                    .lock()
                    .expect("Transmission probe cache poisoned")
                    .insert(key, probe.clone());
                return Ok(probe);
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("Transmission protocol probe failed")))
}

fn probe_transmission_current(endpoint: &TransmissionEndpointV1) -> Result<TransmissionProbe> {
    let key = digest_json(endpoint)?;
    transmission_probe_cache()
        .lock()
        .expect("Transmission probe cache poisoned")
        .remove(&key);
    probe_transmission(endpoint)
}

fn transmission_query_blocking(
    endpoint_name: &str,
    endpoint: &TransmissionEndpointV1,
    probe: &TransmissionProbe,
    operation: RemoteRpcOperationV1,
    locations: &BTreeMap<String, TorrentLocationV1>,
) -> Result<TorrentQueryResult> {
    match operation {
        RemoteRpcOperationV1::Session => Ok(TorrentQueryResult::Session(json!({
            "client_kind": "transmission",
            "protocol": probe.protocol,
            "version": probe.version,
            "identity_fingerprint": probe.fingerprint,
        }))),
        RemoteRpcOperationV1::Torrents => Ok(TorrentQueryResult::Torrents(transmission_inventory(
            endpoint_name,
            endpoint,
            probe,
            None,
            locations,
        )?)),
        RemoteRpcOperationV1::FreeSpace => {
            let value =
                transmission_call(endpoint, probe.protocol, "free_space", json!({"path":"/"}))?;
            Ok(TorrentQueryResult::FreeSpace(json!({
                "path": "/",
                "size-bytes": json_u64_any(&value, &["size_bytes", "size-bytes"]).unwrap_or(0),
                "total-size": json_u64_any(&value, &["total_size", "total-size"]).unwrap_or(0),
            })))
        }
    }
}

fn transmission_inventory(
    endpoint_name: &str,
    endpoint: &TransmissionEndpointV1,
    probe: &TransmissionProbe,
    hashes: Option<&[String]>,
    locations: &BTreeMap<String, TorrentLocationV1>,
) -> Result<TorrentInventoryV1> {
    let snake_fields = [
        "hash_string",
        "name",
        "status",
        "percent_done",
        "total_size",
        "left_until_done",
        "is_finished",
        "labels",
        "download_dir",
        "error",
        "error_string",
        "upload_ratio",
        "bandwidth_priority",
        "tracker_stats",
        "files",
        "file_stats",
    ];
    let legacy_fields = [
        "hashString",
        "name",
        "status",
        "percentDone",
        "totalSize",
        "leftUntilDone",
        "isFinished",
        "labels",
        "downloadDir",
        "error",
        "errorString",
        "uploadRatio",
        "bandwidthPriority",
        "trackerStats",
        "files",
        "fileStats",
    ];
    let fields = if probe.protocol == TorrentProtocolV1::TransmissionJsonRpc2 {
        &snake_fields
    } else {
        &legacy_fields
    };
    let mut params = json!({"fields": fields});
    if let Some(hashes) = hashes.filter(|hashes| !hashes.is_empty()) {
        params["ids"] = json!(hashes);
    }
    let result = transmission_call(endpoint, probe.protocol, "torrent_get", params)?;
    let values = result
        .get("torrents")
        .and_then(JsonValue::as_array)
        .context("Transmission torrent query omitted torrents")?;
    if values.len() > MAX_TORRENTS {
        bail!("Transmission torrent inventory exceeded its 512 item bound")
    }
    let mut total_files = 0_usize;
    let mut torrents = values
        .iter()
        .map(|value| transmission_snapshot(value, endpoint_name, locations, &mut total_files))
        .collect::<Result<Vec<_>>>()?;
    torrents.sort_by(|left, right| left.info_hash.cmp(&right.info_hash));
    reject_duplicate_hashes(&torrents)?;
    Ok(TorrentInventoryV1 {
        endpoint: endpoint_name.to_string(),
        identity: TorrentClientIdentityV1 {
            kind: TorrentClientKindV1::Transmission,
            protocol: probe.protocol,
            version: probe.version.clone(),
            fingerprint: probe.fingerprint.clone(),
        },
        torrents,
        truncated: false,
    })
}

fn transmission_snapshot(
    value: &JsonValue,
    endpoint_name: &str,
    locations: &BTreeMap<String, TorrentLocationV1>,
    total_files: &mut usize,
) -> Result<TorrentSnapshotV1> {
    let info_hash = json_string_any(value, &["hash_string", "hashString"])
        .context("Transmission torrent omitted hash")?
        .to_ascii_lowercase();
    validate_hash(&info_hash)?;
    let status = json_u64_any(value, &["status"]).unwrap_or(0);
    let error = json_u64_any(value, &["error"]).unwrap_or(0);
    let percent = json_f64_any(value, &["percent_done", "percentDone"]).unwrap_or(0.0);
    let left_bytes = json_u64_any(value, &["left_until_done", "leftUntilDone"]).unwrap_or(0);
    let total_bytes = json_u64_any(value, &["total_size", "totalSize"]).unwrap_or(0);
    let completed =
        json_bool_any(value, &["is_finished", "isFinished"]).unwrap_or(false) || percent >= 1.0;
    let state = if error != 0 {
        TorrentStateV1::Error
    } else {
        match status {
            0 => TorrentStateV1::Stopped,
            1 | 2 => TorrentStateV1::Checking,
            3 | 5 => TorrentStateV1::Queued,
            4 => TorrentStateV1::Downloading,
            6 => TorrentStateV1::Seeding,
            _ => TorrentStateV1::Unknown,
        }
    };
    let labels = json_array_any(value, &["labels"])
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .take(MAX_TORRENT_LABELS)
        .map(|label| sanitize_remote_text(label, 128))
        .collect();
    let tracker_summary = json_array_any(value, &["tracker_stats", "trackerStats"])
        .and_then(|trackers| trackers.first())
        .map(|tracker| {
            let host = json_string_any(tracker, &["host"]).unwrap_or_default();
            let result = json_string_any(tracker, &["last_announce_result", "lastAnnounceResult"])
                .unwrap_or_default();
            sanitize_remote_text(&format!("{host}: {result}"), MAX_REMOTE_TEXT)
        })
        .filter(|value| !value.is_empty());
    let location = json_string_any(value, &["download_dir", "downloadDir"]);
    let location_fingerprint = location
        .as_ref()
        .map(|path| {
            crate::json_identity::digest_value(
                &json!({"endpoint": endpoint_name, "location": path}),
            )
        })
        .transpose()?;
    let location_name = location.and_then(|path| location_name(endpoint_name, &path, locations));
    let files = transmission_files(value, total_files)?;
    Ok(TorrentSnapshotV1 {
        info_hash,
        name: sanitize_remote_text(
            &json_string_any(value, &["name"]).unwrap_or_else(|| "unnamed".into()),
            256,
        ),
        state,
        completed,
        completion_millis: (percent.clamp(0.0, 1.0) * 1000.0).round() as u16,
        total_bytes,
        left_bytes,
        ratio_millis: (json_f64_any(value, &["upload_ratio", "uploadRatio"])
            .unwrap_or(0.0)
            .max(0.0)
            * 1000.0)
            .round() as u64,
        priority: transmission_priority(
            json_i64_any(value, &["bandwidth_priority", "bandwidthPriority"]).unwrap_or(0),
        ),
        labels,
        tracker_summary,
        location_name,
        location_fingerprint,
        files,
        files_truncated: false,
    })
}

fn transmission_files(value: &JsonValue, total_files: &mut usize) -> Result<Vec<TorrentFileV1>> {
    let Some(files) = json_array_any(value, &["files"]) else {
        return Ok(Vec::new());
    };
    let stats = json_array_any(value, &["file_stats", "fileStats"]);
    if total_files.saturating_add(files.len()) > MAX_TORRENT_FILES {
        bail!("torrent file inventory exceeded its 10000 file bound")
    }
    let mut normalized = Vec::with_capacity(files.len());
    for (index, file) in files.iter().enumerate() {
        let path = json_string_any(file, &["name"]).context("torrent file omitted its path")?;
        validate_relative_file(&path)?;
        let stat = stats.and_then(|values| values.get(index));
        normalized.push(TorrentFileV1 {
            path,
            length_bytes: json_u64_any(file, &["length"]).unwrap_or(0),
            completed_bytes: json_u64_any(file, &["bytes_completed", "bytesCompleted"])
                .or_else(|| {
                    stat.and_then(|value| {
                        json_u64_any(value, &["bytes_completed", "bytesCompleted"])
                    })
                })
                .unwrap_or(0),
            priority: stat
                .and_then(|value| json_i64_any(value, &["priority"]))
                .map(transmission_priority)
                .unwrap_or(TorrentPriorityV1::Normal),
        });
    }
    *total_files += normalized.len();
    Ok(normalized)
}

fn transmission_mutation(
    endpoint: &TransmissionEndpointV1,
    probe: &TransmissionProbe,
    hashes: &[String],
    control: &TorrentControlActionV1,
    resolved_location: Option<&str>,
) -> Result<()> {
    let (method, params) = match control {
        TorrentControlActionV1::Start => ("torrent_start", json!({"ids": hashes})),
        TorrentControlActionV1::Stop => ("torrent_stop", json!({"ids": hashes})),
        TorrentControlActionV1::Verify => ("torrent_verify", json!({"ids": hashes})),
        TorrentControlActionV1::Reannounce => ("torrent_reannounce", json!({"ids": hashes})),
        TorrentControlActionV1::SetAutomationLabel { label } => (
            "torrent_set",
            json!({"ids": hashes, "labels": label.iter().collect::<Vec<_>>() }),
        ),
        TorrentControlActionV1::SetPriority { priority } => {
            if *priority == TorrentPriorityV1::Off {
                bail!("Transmission does not support an off torrent priority")
            }
            (
                "torrent_set",
                json!({"ids": hashes, "bandwidth_priority": priority_to_i64(*priority)}),
            )
        }
        TorrentControlActionV1::AdoptLocation { .. } => (
            "torrent_set_location",
            json!({
                "ids": hashes,
                "location": resolved_location.context("resolved torrent location is missing")?,
                "move": false,
            }),
        ),
    };
    transmission_call(endpoint, probe.protocol, method, params).map(|_| ())
}

fn transmission_call(
    endpoint: &TransmissionEndpointV1,
    protocol: TorrentProtocolV1,
    snake_method: &str,
    params: JsonValue,
) -> Result<JsonValue> {
    let method = match protocol {
        TorrentProtocolV1::TransmissionJsonRpc2 => snake_method.to_string(),
        TorrentProtocolV1::TransmissionLegacy => snake_method.replace('_', "-"),
        TorrentProtocolV1::RtorrentXmlRpc => bail!("invalid Transmission protocol"),
    };
    let body = if protocol == TorrentProtocolV1::TransmissionJsonRpc2 {
        json!({"jsonrpc":"2.0","method":method,"params":params,"id":1})
    } else {
        json!({"method":method,"arguments":params})
    };
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(endpoint.timeout_seconds))
        .build();
    let authorization = authorization_header(&endpoint.username_env, &endpoint.password_env)?;
    let mut session_id = None;
    for attempt in 0..2 {
        let mut request = agent
            .post(&endpoint.url)
            .set("Content-Type", "application/json");
        if let Some(value) = authorization.as_deref() {
            request = request.set("Authorization", value);
        }
        if let Some(value) = session_id.as_deref() {
            request = request.set("X-Transmission-Session-Id", value);
        }
        match request.send_json(&body) {
            Ok(response) => {
                let value = read_json_response(response)?;
                if protocol == TorrentProtocolV1::TransmissionJsonRpc2 {
                    if let Some(error) = value.get("error") {
                        bail!(
                            "Transmission JSON-RPC error: {}",
                            sanitize_remote_text(&error.to_string(), 256)
                        )
                    }
                    return value
                        .get("result")
                        .cloned()
                        .context("Transmission JSON-RPC response omitted result");
                }
                if value.get("result").and_then(JsonValue::as_str) != Some("success") {
                    bail!("Transmission legacy RPC reported a non-success result")
                }
                return Ok(value.get("arguments").cloned().unwrap_or_else(|| json!({})));
            }
            Err(ureq::Error::Status(409, response)) if attempt == 0 => {
                session_id = response
                    .header("X-Transmission-Session-Id")
                    .map(str::to_string);
                if session_id.is_none() {
                    bail!("Transmission RPC omitted its session identifier")
                }
            }
            Err(ureq::Error::Status(status, _)) => {
                bail!("Transmission RPC returned HTTP status {status}")
            }
            Err(ureq::Error::Transport(_)) => bail!("Transmission RPC transport failed"),
        }
    }
    bail!("Transmission RPC session negotiation failed")
}

fn rtorrent_query_blocking(
    endpoint_name: &str,
    endpoint: &RtorrentEndpointV1,
    ssh_hosts: &BTreeMap<String, SshHostV1>,
    operation: RemoteRpcOperationV1,
    locations: &BTreeMap<String, TorrentLocationV1>,
) -> Result<TorrentQueryResult> {
    match operation {
        RemoteRpcOperationV1::Session => {
            let identity = rtorrent_identity(endpoint, ssh_hosts)?;
            Ok(TorrentQueryResult::Session(json!({
                "client_kind": "rtorrent",
                "protocol": identity.protocol,
                "version": identity.version,
                "identity_fingerprint": identity.fingerprint,
            })))
        }
        RemoteRpcOperationV1::Torrents => Ok(TorrentQueryResult::Torrents(rtorrent_inventory(
            endpoint_name,
            endpoint,
            ssh_hosts,
            locations,
            None,
        )?)),
        RemoteRpcOperationV1::FreeSpace => {
            bail!("rTorrent endpoints do not expose a portable free-space query")
        }
    }
}

fn rtorrent_identity(
    endpoint: &RtorrentEndpointV1,
    ssh_hosts: &BTreeMap<String, SshHostV1>,
) -> Result<TorrentClientIdentityV1> {
    let version = xml_string(xml_call(
        endpoint,
        ssh_hosts,
        XmlRequest::new("system.client_version"),
    )?)?;
    let library = xml_string(xml_call(
        endpoint,
        ssh_hosts,
        XmlRequest::new("system.library_version"),
    )?)?;
    let api = xml_string(xml_call(
        endpoint,
        ssh_hosts,
        XmlRequest::new("system.api_version"),
    )?)?;
    let fingerprint = crate::json_identity::digest_value(&json!({
        "config": digest_json(endpoint)?,
        "version": version,
        "library": library,
        "api": api,
    }))?;
    Ok(TorrentClientIdentityV1 {
        kind: TorrentClientKindV1::Rtorrent,
        protocol: TorrentProtocolV1::RtorrentXmlRpc,
        version: Some(sanitize_remote_text(&version, 128)),
        fingerprint,
    })
}

fn rtorrent_inventory(
    endpoint_name: &str,
    endpoint: &RtorrentEndpointV1,
    ssh_hosts: &BTreeMap<String, SshHostV1>,
    locations: &BTreeMap<String, TorrentLocationV1>,
    hashes: Option<&[String]>,
) -> Result<TorrentInventoryV1> {
    let identity = rtorrent_identity(endpoint, ssh_hosts)?;
    let label_field = rtorrent_label_getter(endpoint.automation_label_slot);
    let mut request = XmlRequest::new("d.multicall2")
        .arg("")
        .arg("main")
        .arg("d.hash=")
        .arg("d.name=")
        .arg("d.state=")
        .arg("d.complete=")
        .arg("d.hashing=")
        .arg("d.size_bytes=")
        .arg("d.left_bytes=")
        .arg("d.ratio=")
        .arg("d.priority=")
        .arg("d.message=")
        .arg("d.directory_base=");
    if let Some(field) = label_field {
        request = request.arg(field);
    }
    let rows = xml_array(xml_call(endpoint, ssh_hosts, request)?)?;
    if rows.len() > MAX_TORRENTS {
        bail!("rTorrent inventory exceeded its 512 item bound")
    }
    let requested = hashes.map(|values| values.iter().cloned().collect::<BTreeSet<_>>());
    let mut torrents = Vec::new();
    let mut total_files = 0_usize;
    for row in rows {
        let columns = xml_array(row)?;
        if columns.len() < 11 {
            bail!("rTorrent multicall row omitted required fields")
        }
        let hash = xml_string(columns[0].clone())?.to_ascii_lowercase();
        validate_hash(&hash)?;
        if requested.as_ref().is_some_and(|set| !set.contains(&hash)) {
            continue;
        }
        let complete = xml_i64(columns[3].clone())? != 0;
        let hashing = xml_i64(columns[4].clone())?;
        let state_value = xml_i64(columns[2].clone())?;
        let message = xml_string(columns[9].clone()).unwrap_or_default();
        let directory = xml_string(columns[10].clone()).unwrap_or_default();
        let location_fingerprint = (!directory.is_empty())
            .then(|| {
                crate::json_identity::digest_value(
                    &json!({"endpoint": endpoint_name, "location": directory.as_str()}),
                )
            })
            .transpose()?;
        let total_bytes = xml_i64(columns[5].clone())?.max(0) as u64;
        let left_bytes = xml_i64(columns[6].clone())?.max(0) as u64;
        let files = rtorrent_files(endpoint, ssh_hosts, &hash, &mut total_files)?;
        let tracker_summary = rtorrent_tracker_summary(endpoint, ssh_hosts, &hash)?;
        let label = columns
            .get(11)
            .cloned()
            .and_then(|value| xml_string(value).ok())
            .filter(|value| !value.is_empty())
            .map(|value| sanitize_remote_text(&value, 128));
        torrents.push(TorrentSnapshotV1 {
            info_hash: hash,
            name: sanitize_remote_text(&xml_string(columns[1].clone())?, 256),
            state: if !message.is_empty() {
                TorrentStateV1::Error
            } else if hashing != 0 {
                TorrentStateV1::Checking
            } else if state_value == 0 {
                TorrentStateV1::Stopped
            } else if complete {
                TorrentStateV1::Seeding
            } else {
                TorrentStateV1::Downloading
            },
            completed: complete,
            completion_millis: if complete {
                1000
            } else {
                total_bytes
                    .saturating_sub(left_bytes)
                    .saturating_mul(1000)
                    .checked_div(total_bytes)
                    .unwrap_or(0)
                    .min(1000) as u16
            },
            total_bytes,
            left_bytes,
            ratio_millis: xml_i64(columns[7].clone())?.max(0) as u64,
            priority: rtorrent_priority(xml_i64(columns[8].clone())?),
            labels: label.into_iter().collect(),
            tracker_summary,
            location_name: location_name(endpoint_name, &directory, locations),
            location_fingerprint,
            files,
            files_truncated: false,
        });
    }
    torrents.sort_by(|left, right| left.info_hash.cmp(&right.info_hash));
    reject_duplicate_hashes(&torrents)?;
    Ok(TorrentInventoryV1 {
        endpoint: endpoint_name.to_string(),
        identity,
        torrents,
        truncated: false,
    })
}

fn rtorrent_tracker_summary(
    endpoint: &RtorrentEndpointV1,
    ssh_hosts: &BTreeMap<String, SshHostV1>,
    hash: &str,
) -> Result<Option<String>> {
    let request = XmlRequest::new("t.multicall")
        .arg(hash)
        .arg("")
        .arg("t.url=")
        .arg("t.is_enabled=");
    let rows = xml_array(xml_call(endpoint, ssh_hosts, request)?)?;
    let Some(first) = rows.into_iter().next() else {
        return Ok(None);
    };
    let columns = xml_array(first)?;
    let url = columns
        .first()
        .cloned()
        .map(xml_string)
        .transpose()?
        .unwrap_or_default();
    let enabled = columns
        .get(1)
        .cloned()
        .map(xml_i64)
        .transpose()?
        .unwrap_or(0)
        != 0;
    Ok(safe_tracker_host(&url).map(|host| {
        sanitize_remote_text(
            &format!("{host} ({})", if enabled { "enabled" } else { "disabled" }),
            MAX_REMOTE_TEXT,
        )
    }))
}

fn safe_tracker_host(url: &str) -> Option<String> {
    let authority = url
        .split_once("://")
        .map_or(url, |(_, remainder)| remainder)
        .split(['/', '?', '#'])
        .next()?;
    let host = authority.rsplit('@').next()?.split(':').next()?;
    (!host.is_empty()).then(|| host.chars().take(255).collect())
}

fn rtorrent_files(
    endpoint: &RtorrentEndpointV1,
    ssh_hosts: &BTreeMap<String, SshHostV1>,
    hash: &str,
    total_files: &mut usize,
) -> Result<Vec<TorrentFileV1>> {
    let request = XmlRequest::new("f.multicall")
        .arg(hash)
        .arg("")
        .arg("f.path=")
        .arg("f.size_bytes=")
        .arg("f.completed_chunks=")
        .arg("f.size_chunks=")
        .arg("f.priority=");
    let rows = xml_array(xml_call(endpoint, ssh_hosts, request)?)?;
    if total_files.saturating_add(rows.len()) > MAX_TORRENT_FILES {
        bail!("torrent file inventory exceeded its 10000 file bound")
    }
    let mut files = Vec::with_capacity(rows.len());
    for row in rows {
        let columns = xml_array(row)?;
        if columns.len() < 5 {
            bail!("rTorrent file multicall row omitted required fields")
        }
        let path = xml_string(columns[0].clone())?;
        validate_relative_file(&path)?;
        let length = xml_i64(columns[1].clone())?.max(0) as u64;
        let completed_chunks = xml_i64(columns[2].clone())?.max(0) as u64;
        let size_chunks = xml_i64(columns[3].clone())?.max(0) as u64;
        let completed_bytes = length
            .saturating_mul(completed_chunks)
            .checked_div(size_chunks)
            .unwrap_or(0);
        files.push(TorrentFileV1 {
            path,
            length_bytes: length,
            completed_bytes,
            priority: rtorrent_priority(xml_i64(columns[4].clone())?),
        });
    }
    *total_files += files.len();
    Ok(files)
}

fn rtorrent_mutation(
    endpoint: &RtorrentEndpointV1,
    ssh_hosts: &BTreeMap<String, SshHostV1>,
    hashes: &[String],
    control: &TorrentControlActionV1,
    resolved_location: Option<&str>,
) -> Result<()> {
    for hash in hashes {
        let request = match control {
            TorrentControlActionV1::Start => XmlRequest::new("d.start").arg(hash.as_str()),
            TorrentControlActionV1::Stop => XmlRequest::new("d.stop").arg(hash.as_str()),
            TorrentControlActionV1::Verify => XmlRequest::new("d.check_hash").arg(hash.as_str()),
            TorrentControlActionV1::Reannounce => {
                XmlRequest::new("d.tracker_announce").arg(hash.as_str())
            }
            TorrentControlActionV1::SetAutomationLabel { label } => {
                let method = rtorrent_label_setter(endpoint.automation_label_slot)
                    .context("rTorrent automation label slot is not configured")?;
                XmlRequest::new(method)
                    .arg(hash.as_str())
                    .arg(label.as_deref().unwrap_or_default())
            }
            TorrentControlActionV1::SetPriority { priority } => XmlRequest::new("d.priority.set")
                .arg(hash.as_str())
                .arg(match priority {
                    TorrentPriorityV1::Off => 0,
                    TorrentPriorityV1::Low => 1,
                    TorrentPriorityV1::Normal => 2,
                    TorrentPriorityV1::High => 3,
                }),
            TorrentControlActionV1::AdoptLocation { .. } => XmlRequest::new("d.directory_base.set")
                .arg(hash.as_str())
                .arg(resolved_location.context("resolved torrent location is missing")?),
        };
        xml_call(endpoint, ssh_hosts, request)?;
    }
    Ok(())
}

fn xml_call(
    endpoint: &RtorrentEndpointV1,
    ssh_hosts: &BTreeMap<String, SshHostV1>,
    request: XmlRequest<'_>,
) -> Result<XmlValue> {
    let value = request
        .call(OwnedRtorrentTransport {
            endpoint: endpoint.clone(),
            ssh_hosts: ssh_hosts.clone(),
        })
        .map_err(|error| {
            anyhow!(
                "rTorrent XML-RPC call failed: {}",
                sanitize_remote_text(&error.to_string(), 256)
            )
        })?;
    let mut nodes = 0_usize;
    validate_xml_value(&value, 0, &mut nodes)?;
    Ok(value)
}

#[derive(Debug, Clone)]
struct OwnedRtorrentTransport {
    endpoint: RtorrentEndpointV1,
    ssh_hosts: BTreeMap<String, SshHostV1>,
}

impl XmlTransport for OwnedRtorrentTransport {
    type Stream = Cursor<Vec<u8>>;

    fn transmit(
        self,
        request: &XmlRequest<'_>,
    ) -> std::result::Result<Self::Stream, Box<dyn StdError + Send + Sync>> {
        let mut body = Vec::new();
        request.write_as_xml(&mut body)?;
        if body.len() as u64 > MAX_RPC_BYTES {
            return Err("rTorrent XML-RPC request exceeded its 1 MiB bound".into());
        }
        let response = match &self.endpoint.transport {
            RtorrentTransportV1::HttpXmlRpc {
                url,
                username_env,
                password_env,
            } => http_xmlrpc(
                url,
                username_env,
                password_env,
                self.endpoint.timeout_seconds,
                &body,
            )?,
            RtorrentTransportV1::UnixScgi { socket } => {
                unix_scgi(socket, self.endpoint.timeout_seconds, &body)?
            }
            RtorrentTransportV1::LoopbackTcpScgi { port } => {
                tcp_scgi(*port, self.endpoint.timeout_seconds, &body)?
            }
            RtorrentTransportV1::SshLoopbackScgi { ssh_host, port } => {
                let host = self
                    .ssh_hosts
                    .get(ssh_host)
                    .ok_or("rTorrent SSH resource is missing")?;
                ssh_scgi(host, *port, self.endpoint.timeout_seconds, &body)?
            }
        };
        validate_xml_envelope(&response)?;
        Ok(Cursor::new(response))
    }
}

fn validate_xml_envelope(bytes: &[u8]) -> std::result::Result<(), Box<dyn StdError + Send + Sync>> {
    let text = std::str::from_utf8(bytes)?;
    let upper = text.to_ascii_uppercase();
    if upper.contains("<!DOCTYPE") || upper.contains("<!ENTITY") {
        return Err("rTorrent XML declarations may not define entities or doctypes".into());
    }
    let mut depth = 0_usize;
    let mut nodes = 0_usize;
    let mut rest = text;
    while let Some(index) = rest.find('<') {
        rest = &rest[index + 1..];
        let Some(end) = rest.find('>') else {
            return Err("rTorrent XML response has an unterminated tag".into());
        };
        let tag = rest[..end].trim();
        rest = &rest[end + 1..];
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        if tag.starts_with('/') {
            depth = depth.saturating_sub(1);
        } else if !tag.ends_with('/') {
            depth = depth.saturating_add(1);
            nodes = nodes.saturating_add(1);
            if depth > MAX_XML_DEPTH || nodes > MAX_XML_NODES {
                return Err("rTorrent XML response exceeded its structural bounds".into());
            }
        }
    }
    Ok(())
}

fn http_xmlrpc(
    url: &str,
    username_env: &Option<String>,
    password_env: &Option<String>,
    timeout_seconds: u64,
    body: &[u8],
) -> std::result::Result<Vec<u8>, Box<dyn StdError + Send + Sync>> {
    validate_http_url(url)?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout_seconds))
        .build();
    let mut request = agent
        .post(url)
        .set("Content-Type", "text/xml; charset=utf-8")
        .set("User-Agent", "codex-team/1");
    if let Some(value) = authorization_header(username_env, password_env)? {
        request = request.set("Authorization", &value);
    }
    let response = request.send_bytes(body).map_err(|error| match error {
        ureq::Error::Status(status, _) => format!("rTorrent HTTP returned status {status}"),
        ureq::Error::Transport(_) => "rTorrent HTTP transport failed".to_string(),
    })?;
    read_response(response).map_err(Into::into)
}

#[cfg(unix)]
fn unix_scgi(
    socket: &str,
    timeout_seconds: u64,
    body: &[u8],
) -> std::result::Result<Vec<u8>, Box<dyn StdError + Send + Sync>> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let path = Path::new(socket);
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_socket()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err("rTorrent Unix SCGI socket owner/type/permissions are unsafe".into());
    }
    let stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_secs(timeout_seconds)))?;
    stream.set_write_timeout(Some(Duration::from_secs(timeout_seconds)))?;
    scgi_stream(stream, timeout_seconds, body)
}

#[cfg(not(unix))]
fn unix_scgi(
    _socket: &str,
    _timeout_seconds: u64,
    _body: &[u8],
) -> std::result::Result<Vec<u8>, Box<dyn StdError + Send + Sync>> {
    Err("Unix SCGI is unavailable on this platform".into())
}

fn tcp_scgi(
    port: u16,
    timeout_seconds: u64,
    body: &[u8],
) -> std::result::Result<Vec<u8>, Box<dyn StdError + Send + Sync>> {
    let stream = TcpStream::connect_timeout(
        &(Ipv4Addr::LOCALHOST, port).into(),
        Duration::from_secs(timeout_seconds.min(30)),
    )?;
    stream.set_read_timeout(Some(Duration::from_secs(timeout_seconds)))?;
    stream.set_write_timeout(Some(Duration::from_secs(timeout_seconds)))?;
    scgi_stream(stream, timeout_seconds, body)
}

fn scgi_stream<S>(
    mut stream: S,
    timeout_seconds: u64,
    body: &[u8],
) -> std::result::Result<Vec<u8>, Box<dyn StdError + Send + Sync>>
where
    S: ScgiStream,
{
    let frame = scgi_frame(body)?;
    stream.write_all(&frame)?;
    stream.shutdown_write()?;
    let mut response = Vec::new();
    stream.take(MAX_RPC_BYTES + 1).read_to_end(&mut response)?;
    if response.len() as u64 > MAX_RPC_BYTES {
        return Err("rTorrent SCGI response exceeded its 1 MiB bound".into());
    }
    let _ = timeout_seconds;
    parse_scgi_response(response)
}

trait ScgiStream: Read + Write {
    fn shutdown_write(&self) -> std::io::Result<()>;
}

impl ScgiStream for TcpStream {
    fn shutdown_write(&self) -> std::io::Result<()> {
        self.shutdown(Shutdown::Write)
    }
}

#[cfg(unix)]
impl ScgiStream for UnixStream {
    fn shutdown_write(&self) -> std::io::Result<()> {
        self.shutdown(Shutdown::Write)
    }
}

fn ssh_scgi(
    host: &SshHostV1,
    port: u16,
    timeout_seconds: u64,
    body: &[u8],
) -> std::result::Result<Vec<u8>, Box<dyn StdError + Send + Sync>> {
    let executable = fixed_executable(&["/usr/bin/ssh", "/bin/ssh"])?;
    let mut command = Command::new(executable);
    command
        .args(ssh_forward_arguments(host, port))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    let mut stdin = child.stdin.take().ok_or("SSH SCGI child omitted stdin")?;
    stdin.write_all(&scgi_frame(body)?)?;
    drop(stdin);
    let stdout = child.stdout.take().ok_or("SSH SCGI child omitted stdout")?;
    let reader = thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut response = Vec::new();
        stdout.take(MAX_RPC_BYTES + 1).read_to_end(&mut response)?;
        Ok(response)
    });
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("rTorrent SSH SCGI timed out".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let response = reader.join().map_err(|_| "SSH SCGI reader panicked")??;
    if !status.success() {
        return Err("rTorrent SSH SCGI transport failed".into());
    }
    if response.len() as u64 > MAX_RPC_BYTES {
        return Err("rTorrent SSH SCGI response exceeded its 1 MiB bound".into());
    }
    parse_scgi_response(response)
}

fn scgi_frame(body: &[u8]) -> std::result::Result<Vec<u8>, Box<dyn StdError + Send + Sync>> {
    let headers = format!(
        "CONTENT_LENGTH\0{}\0SCGI\01\0REQUEST_METHOD\0POST\0REQUEST_URI\0/RPC2\0",
        body.len()
    );
    if headers.len() > 4_096 {
        return Err("SCGI header exceeded its bound".into());
    }
    let mut frame = format!("{}:", headers.len()).into_bytes();
    frame.extend_from_slice(headers.as_bytes());
    frame.push(b',');
    frame.extend_from_slice(body);
    Ok(frame)
}

fn parse_scgi_response(
    response: Vec<u8>,
) -> std::result::Result<Vec<u8>, Box<dyn StdError + Send + Sync>> {
    let marker = b"\r\n\r\n";
    let index = response
        .windows(marker.len())
        .position(|window| window == marker)
        .ok_or("rTorrent SCGI response omitted HTTP headers")?;
    let headers = std::str::from_utf8(&response[..index])?;
    let status_ok = headers.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("status:") && lower.contains("200")
    }) || !headers
        .lines()
        .any(|line| line.to_ascii_lowercase().starts_with("status:"));
    if !status_ok {
        return Err("rTorrent SCGI returned a non-success status".into());
    }
    Ok(response[index + marker.len()..].to_vec())
}

fn validate_xml_value(value: &XmlValue, depth: usize, nodes: &mut usize) -> Result<()> {
    if depth > MAX_XML_DEPTH {
        bail!("rTorrent XML response exceeded its nesting bound")
    }
    *nodes = nodes.saturating_add(1);
    if *nodes > MAX_XML_NODES {
        bail!("rTorrent XML response exceeded its node bound")
    }
    match value {
        XmlValue::Array(values) => {
            if values.len() > MAX_TORRENT_FILES {
                bail!("rTorrent XML collection exceeded its bound")
            }
            for value in values {
                validate_xml_value(value, depth + 1, nodes)?;
            }
        }
        XmlValue::Struct(values) => {
            if values.len() > 512 {
                bail!("rTorrent XML struct exceeded its bound")
            }
            for value in values.values() {
                validate_xml_value(value, depth + 1, nodes)?;
            }
        }
        XmlValue::String(value) if value.len() > MAX_RPC_BYTES as usize => {
            bail!("rTorrent XML string exceeded its bound")
        }
        _ => {}
    }
    Ok(())
}

fn mutation_effect_confirmed(
    after: &TorrentInventoryV1,
    control: &TorrentControlActionV1,
    hashes: &[String],
) -> bool {
    hashes.iter().all(|hash| {
        let Some(next) = snapshot_by_hash(after, hash) else {
            return false;
        };
        match control {
            TorrentControlActionV1::Start => !matches!(next.state, TorrentStateV1::Stopped),
            TorrentControlActionV1::Stop => matches!(next.state, TorrentStateV1::Stopped),
            TorrentControlActionV1::SetAutomationLabel { label } => {
                next.labels == label.iter().cloned().collect::<Vec<_>>()
            }
            TorrentControlActionV1::SetPriority { priority } => next.priority == *priority,
            TorrentControlActionV1::AdoptLocation { location } => {
                next.location_name.as_deref() == Some(location.as_str())
            }
            TorrentControlActionV1::Verify | TorrentControlActionV1::Reannounce => false,
        }
    })
}

fn require_exact_hashes(inventory: &TorrentInventoryV1, hashes: &[String]) -> Result<()> {
    let present = inventory
        .torrents
        .iter()
        .map(|torrent| torrent.info_hash.as_str())
        .collect::<BTreeSet<_>>();
    if hashes.iter().any(|hash| !present.contains(hash.as_str())) {
        bail!("selected torrent state changed or a torrent is missing")
    }
    Ok(())
}

fn require_expected_snapshots(
    inventory: &TorrentInventoryV1,
    expected: &[TorrentSnapshotV1],
) -> Result<()> {
    if expected
        .iter()
        .any(|snapshot| snapshot_by_hash(inventory, &snapshot.info_hash) != Some(snapshot))
    {
        bail!("selected torrent state, location, or file inventory changed after preview")
    }
    Ok(())
}

fn snapshot_by_hash<'a>(
    inventory: &'a TorrentInventoryV1,
    hash: &str,
) -> Option<&'a TorrentSnapshotV1> {
    inventory
        .torrents
        .iter()
        .find(|torrent| torrent.info_hash == hash)
}

fn reject_duplicate_hashes(torrents: &[TorrentSnapshotV1]) -> Result<()> {
    if torrents
        .windows(2)
        .any(|pair| pair[0].info_hash == pair[1].info_hash)
    {
        bail!("torrent inventory contains duplicate info hashes")
    }
    Ok(())
}

fn location_name(
    endpoint: &str,
    path: &str,
    locations: &BTreeMap<String, TorrentLocationV1>,
) -> Option<String> {
    locations.iter().find_map(|(name, location)| {
        (location.endpoint == endpoint && location.path == path).then(|| name.clone())
    })
}

fn rtorrent_label_getter(slot: Option<RtorrentCustomSlotV1>) -> Option<&'static str> {
    slot.map(|slot| match slot {
        RtorrentCustomSlotV1::Custom1 => "d.custom1=",
        RtorrentCustomSlotV1::Custom2 => "d.custom2=",
        RtorrentCustomSlotV1::Custom3 => "d.custom3=",
        RtorrentCustomSlotV1::Custom4 => "d.custom4=",
        RtorrentCustomSlotV1::Custom5 => "d.custom5=",
    })
}

fn rtorrent_label_setter(slot: Option<RtorrentCustomSlotV1>) -> Option<&'static str> {
    slot.map(|slot| match slot {
        RtorrentCustomSlotV1::Custom1 => "d.custom1.set",
        RtorrentCustomSlotV1::Custom2 => "d.custom2.set",
        RtorrentCustomSlotV1::Custom3 => "d.custom3.set",
        RtorrentCustomSlotV1::Custom4 => "d.custom4.set",
        RtorrentCustomSlotV1::Custom5 => "d.custom5.set",
    })
}

fn xml_array(value: XmlValue) -> Result<Vec<XmlValue>> {
    match value {
        XmlValue::Array(values) => Ok(values),
        _ => bail!("rTorrent XML-RPC response had the wrong collection type"),
    }
}

fn xml_string(value: XmlValue) -> Result<String> {
    match value {
        XmlValue::String(value) => Ok(value),
        XmlValue::Int(value) => Ok(value.to_string()),
        XmlValue::Int64(value) => Ok(value.to_string()),
        _ => bail!("rTorrent XML-RPC response had the wrong string type"),
    }
}

fn xml_i64(value: XmlValue) -> Result<i64> {
    match value {
        XmlValue::Int(value) => Ok(i64::from(value)),
        XmlValue::Int64(value) => Ok(value),
        XmlValue::String(value) => value.parse().context("parse rTorrent integer"),
        _ => bail!("rTorrent XML-RPC response had the wrong integer type"),
    }
}

fn read_response(response: ureq::Response) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_RPC_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("read bounded RPC response")?;
    if bytes.len() as u64 > MAX_RPC_BYTES {
        bail!("RPC response exceeded its 1 MiB bound")
    }
    Ok(bytes)
}

fn read_json_response(response: ureq::Response) -> Result<JsonValue> {
    serde_json::from_slice(&read_response(response)?).context("parse Transmission RPC JSON")
}

fn authorization_header(
    username_env: &Option<String>,
    password_env: &Option<String>,
) -> Result<Option<String>> {
    match (username_env, password_env) {
        (None, None) => Ok(None),
        (Some(username_env), Some(password_env)) => {
            let username = std::env::var(username_env).with_context(|| {
                format!("required credential environment `{username_env}` is absent")
            })?;
            let password = std::env::var(password_env).with_context(|| {
                format!("required credential environment `{password_env}` is absent")
            })?;
            Ok(Some(format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"))
            )))
        }
        _ => bail!("RPC credential environment names must be configured together"),
    }
}

fn validate_credentials(username: &Option<String>, password: &Option<String>) -> Result<()> {
    match (username, password) {
        (None, None) => Ok(()),
        (Some(username), Some(password)) => {
            validate_env_name(username)?;
            validate_env_name(password)
        }
        _ => bail!("RPC credential environment names must be configured together"),
    }
}

fn validate_http_url(value: &str) -> Result<()> {
    let loopback_http = value.starts_with("http://127.0.0.1:")
        || value.starts_with("http://localhost:")
        || value.starts_with("http://[::1]:");
    if !(value.starts_with("https://") || loopback_http)
        || value.len() > 2_048
        || value.chars().any(char::is_control)
        || value.contains(['@', '?', '#'])
    {
        bail!("RPC URL must be HTTPS or loopback HTTP without embedded credentials")
    }
    Ok(())
}

fn validate_absolute_path(value: &str, label: &str) -> Result<()> {
    let path = Path::new(value);
    if !path.is_absolute()
        || value.len() > 4_096
        || value.chars().any(char::is_control)
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        bail!("{label} must be one normalized absolute path")
    }
    Ok(())
}

fn validate_relative_file(value: &str) -> Result<()> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > 4_096
        || path.is_absolute()
        || value.chars().any(char::is_control)
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
        bail!("torrent file path must be a contained relative path")
    }
    Ok(())
}

fn validate_hash(value: &str) -> Result<()> {
    ca_automation_protocol::validate_info_hash(value).map_err(anyhow::Error::msg)
}

fn validate_timeout(value: u64) -> Result<()> {
    if value == 0 || value > 7_200 {
        bail!("torrent RPC timeout must be between 1 and 7200 seconds")
    }
    Ok(())
}

fn validate_env_name(value: &str) -> Result<()> {
    let mut bytes = value.bytes();
    if value.is_empty()
        || value.len() > 128
        || !bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_uppercase() || byte == b'_')
        || !bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        bail!("credential environment names must use safe uppercase ASCII identifiers")
    }
    Ok(())
}

fn sanitize_remote_text(value: &str, maximum: usize) -> String {
    let redacted = ["password", "passwd", "token", "secret", "api_key"]
        .iter()
        .fold(value.to_string(), |text, pattern| {
            if text.to_ascii_lowercase().contains(pattern) {
                "[redacted remote metadata]".to_string()
            } else {
                text
            }
        });
    let mut output = redacted
        .chars()
        .filter(|character| !character.is_control())
        .take(maximum)
        .collect::<String>();
    if redacted
        .chars()
        .filter(|character| !character.is_control())
        .count()
        > maximum
    {
        output.push('…');
    }
    output
}

fn transmission_priority(value: i64) -> TorrentPriorityV1 {
    match value {
        -1 => TorrentPriorityV1::Low,
        1 => TorrentPriorityV1::High,
        _ => TorrentPriorityV1::Normal,
    }
}

fn rtorrent_priority(value: i64) -> TorrentPriorityV1 {
    match value {
        0 => TorrentPriorityV1::Off,
        1 => TorrentPriorityV1::Low,
        2 => TorrentPriorityV1::Normal,
        3 => TorrentPriorityV1::High,
        _ => TorrentPriorityV1::Normal,
    }
}

fn priority_to_i64(value: TorrentPriorityV1) -> i64 {
    match value {
        TorrentPriorityV1::Off => 0,
        TorrentPriorityV1::Low => -1,
        TorrentPriorityV1::Normal => 0,
        TorrentPriorityV1::High => 1,
    }
}

fn json_string(value: &JsonValue, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
}

fn json_string_any(value: &JsonValue, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| json_string(value, key))
}

fn json_u64_any(value: &JsonValue, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(JsonValue::as_u64))
}

fn json_i64_any(value: &JsonValue, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(JsonValue::as_i64))
}

fn json_f64_any(value: &JsonValue, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(JsonValue::as_f64))
}

fn json_bool_any(value: &JsonValue, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(JsonValue::as_bool))
}

fn json_array_any<'a>(value: &'a JsonValue, keys: &[&str]) -> Option<&'a Vec<JsonValue>> {
    keys.iter()
        .find_map(|key| value.get(key).and_then(JsonValue::as_array))
}

fn fixed_executable(candidates: &[&str]) -> Result<PathBuf> {
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .context("required fixed SSH executable is unavailable")
}

fn ssh_arguments(host: &SshHostV1) -> Vec<OsString> {
    let mut arguments = vec![
        OsString::from("-T"),
        OsString::from("-oBatchMode=yes"),
        OsString::from("-oClearAllForwardings=yes"),
        OsString::from("-oForwardAgent=no"),
        OsString::from("-oPermitLocalCommand=no"),
    ];
    if let Some(port) = host.port {
        arguments.push(OsString::from("-p"));
        arguments.push(OsString::from(port.to_string()));
    }
    arguments
}

fn ssh_forward_arguments(host: &SshHostV1, port: u16) -> Vec<OsString> {
    let mut arguments = ssh_arguments(host);
    arguments.push(OsString::from("-W"));
    arguments.push(OsString::from(format!("127.0.0.1:{port}")));
    arguments.push(OsString::from("--"));
    arguments.push(OsString::from(&host.destination));
    arguments
}

fn digest_json(value: &impl Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

const fn default_timeout_seconds() -> u64 {
    900
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    fn spawn_http_server(
        responses: Vec<String>,
    ) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_thread = captured.clone();
        let handle = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let request = read_http_request(&mut stream).unwrap();
                captured_thread.lock().unwrap().push(request);
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (format!("http://{address}/rpc"), captured, handle)
    }

    fn read_http_request(stream: &mut TcpStream) -> Result<String> {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
            if let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&bytes[..header_end + 4])?;
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.split_once(':').and_then(|(name, value)| {
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                    })
                    .unwrap_or(0);
                if bytes.len() >= header_end + 4 + length {
                    break;
                }
            }
        }
        Ok(String::from_utf8(bytes)?)
    }

    fn json_response(value: JsonValue) -> String {
        let body = serde_json::to_string(&value).unwrap();
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn xml_response(value: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{value}",
            value.len()
        )
    }

    fn xml_string_response(value: &str) -> String {
        format!(
            "<?xml version=\"1.0\"?><methodResponse><params><param><value><string>{value}</string></value></param></params></methodResponse>"
        )
    }

    fn xml_value_response(value: XmlValue) -> String {
        let mut encoded = Vec::new();
        value.write_as_xml(&mut encoded).unwrap();
        format!(
            "<?xml version=\"1.0\"?><methodResponse><params><param>{}</param></params></methodResponse>",
            String::from_utf8(encoded).unwrap()
        )
    }

    fn transmission_torrent(hash: &str, status: u64) -> JsonValue {
        json!({
            "hash_string":hash,
            "name":"Fixture",
            "status":status,
            "percent_done":0.5,
            "total_size":20,
            "left_until_done":10,
            "labels":[],
            "download_dir":"/downloads",
            "error":0,
            "error_string":"",
            "upload_ratio":0.25,
            "bandwidth_priority":0,
            "tracker_stats":[],
            "files":[],
            "file_stats":[]
        })
    }

    #[test]
    fn scgi_frame_is_a_length_prefixed_netstring() {
        let frame = scgi_frame(b"<methodCall/>").unwrap();
        let colon = frame.iter().position(|byte| *byte == b':').unwrap();
        let length: usize = std::str::from_utf8(&frame[..colon])
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(frame[colon + 1 + length], b',');
        assert_eq!(&frame[colon + 2 + length..], b"<methodCall/>");
        assert!(frame.windows(14).any(|window| window == b"CONTENT_LENGTH"));
    }

    #[test]
    fn direct_rpc_urls_and_torrent_paths_are_fail_closed() {
        assert!(validate_http_url("http://seedbox.example/RPC2").is_err());
        assert!(validate_http_url("https://seedbox.example/RPC2").is_ok());
        assert!(validate_relative_file("movies/title.mkv").is_ok());
        assert!(validate_relative_file("../escape").is_err());
        assert!(validate_relative_file("/absolute").is_err());
    }

    #[test]
    fn remote_text_is_bounded_control_free_and_secret_redacted() {
        assert_eq!(sanitize_remote_text("hello\nworld", 20), "helloworld");
        assert_eq!(
            sanitize_remote_text("password=do-not-leak", 64),
            "[redacted remote metadata]"
        );
        assert_eq!(sanitize_remote_text("abcdef", 3), "abc…");
    }

    #[tokio::test]
    async fn transmission_json_rpc2_negotiates_409_and_normalizes_inventory() {
        let hash = "a".repeat(40);
        let responses = vec![
            "HTTP/1.1 409 Conflict\r\nX-Transmission-Session-Id: test-session\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
            json_response(json!({
                "jsonrpc":"2.0",
                "result":{"version":"4.1.0","rpc_version_semver":"6.0.0"},
                "id":1
            })),
            json_response(json!({
                "jsonrpc":"2.0",
                "result":{"torrents":[{
                    "hash_string":hash,
                    "name":"Movie\nTitle",
                    "status":6,
                    "percent_done":1.0,
                    "total_size":12,
                    "left_until_done":0,
                    "labels":["archive"],
                    "download_dir":"/downloads",
                    "error":0,
                    "error_string":"",
                    "upload_ratio":1.5,
                    "bandwidth_priority":1,
                    "tracker_stats":[{"host":"tracker.example","last_announce_result":"Success"}],
                    "files":[{"name":"Movie/file.mkv","length":12,"bytes_completed":12}],
                    "file_stats":[{"bytes_completed":12,"priority":1}]
                }]},
                "id":1
            })),
        ];
        let (url, captured, server) = spawn_http_server(responses);
        let endpoint = TransmissionEndpointV1 {
            url,
            username_env: None,
            password_env: None,
            protocol: TransmissionProtocolPreferenceV1::Auto,
            timeout_seconds: 5,
        };
        let locations = BTreeMap::from([(
            "downloads".into(),
            TorrentLocationV1 {
                endpoint: "seedbox".into(),
                path: "/downloads".into(),
            },
        )]);
        let TorrentQueryResult::Torrents(inventory) = query_transmission(
            "seedbox".into(),
            endpoint,
            RemoteRpcOperationV1::Torrents,
            locations,
        )
        .await
        .unwrap() else {
            panic!("expected inventory")
        };
        server.join().unwrap();
        assert_eq!(
            inventory.identity.protocol,
            TorrentProtocolV1::TransmissionJsonRpc2
        );
        assert_eq!(inventory.torrents[0].info_hash, "a".repeat(40));
        assert_eq!(inventory.torrents[0].name, "MovieTitle");
        assert_eq!(
            inventory.torrents[0].location_name.as_deref(),
            Some("downloads")
        );
        assert_eq!(
            inventory.torrents[0]
                .location_fingerprint
                .as_ref()
                .unwrap()
                .len(),
            64
        );
        assert_eq!(inventory.torrents[0].files[0].path, "Movie/file.mkv");
        let requests = captured.lock().unwrap();
        assert!(requests[0].contains("\"method\":\"session_get\""));
        assert!(requests[1].contains("X-Transmission-Session-Id: test-session"));
        assert!(requests[2].contains("\"method\":\"torrent_get\""));
        assert!(requests[2].contains("hash_string"));
    }

    #[tokio::test]
    async fn transmission_legacy_protocol_remains_normalized() {
        let hash = "b".repeat(40);
        let responses = vec![
            json_response(json!({
                "result":"success",
                "arguments":{"version":"4.0.6","rpc-version-semver":"5.3.0"}
            })),
            json_response(json!({
                "result":"success",
                "arguments":{"torrents":[{
                    "hashString":hash,
                    "name":"Legacy",
                    "status":0,
                    "percentDone":0.5,
                    "totalSize":20,
                    "leftUntilDone":10,
                    "labels":[],
                    "downloadDir":"/legacy",
                    "error":0,
                    "uploadRatio":0.25,
                    "bandwidthPriority":0,
                    "trackerStats":[],
                    "files":[],
                    "fileStats":[]
                }]}
            })),
        ];
        let (url, captured, server) = spawn_http_server(responses);
        let endpoint = TransmissionEndpointV1 {
            url,
            username_env: None,
            password_env: None,
            protocol: TransmissionProtocolPreferenceV1::Legacy,
            timeout_seconds: 5,
        };
        let TorrentQueryResult::Torrents(inventory) = query_transmission(
            "legacy".into(),
            endpoint,
            RemoteRpcOperationV1::Torrents,
            BTreeMap::new(),
        )
        .await
        .unwrap() else {
            panic!("expected inventory")
        };
        server.join().unwrap();
        assert_eq!(
            inventory.identity.protocol,
            TorrentProtocolV1::TransmissionLegacy
        );
        assert_eq!(inventory.torrents[0].state, TorrentStateV1::Stopped);
        assert_eq!(inventory.torrents[0].completion_millis, 500);
        let requests = captured.lock().unwrap();
        assert!(requests[0].contains("\"method\":\"session-get\""));
        assert!(requests[1].contains("\"method\":\"torrent-get\""));
        assert!(requests[1].contains("hashString"));
    }

    #[tokio::test]
    async fn rtorrent_multicalls_normalize_inventory_files_trackers_and_labels() {
        let hash = "e".repeat(40);
        let torrent_row = XmlValue::Array(vec![
            XmlValue::String(hash.clone()),
            XmlValue::String("Movie\nTitle".into()),
            XmlValue::Int64(1),
            XmlValue::Int64(1),
            XmlValue::Int64(0),
            XmlValue::Int64(12),
            XmlValue::Int64(0),
            XmlValue::Int64(1500),
            XmlValue::Int64(3),
            XmlValue::String(String::new()),
            XmlValue::String("/downloads".into()),
            XmlValue::String("archive".into()),
        ]);
        let file_row = XmlValue::Array(vec![
            XmlValue::String("Movie/file.mkv".into()),
            XmlValue::Int64(12),
            XmlValue::Int64(1),
            XmlValue::Int64(1),
            XmlValue::Int64(2),
        ]);
        let tracker_row = XmlValue::Array(vec![
            XmlValue::String("https://tracker.example/announce?token=private".into()),
            XmlValue::Int64(1),
        ]);
        let responses = vec![
            xml_response(&xml_string_response("0.9.8")),
            xml_response(&xml_string_response("0.13.8")),
            xml_response(&xml_string_response("2")),
            xml_response(&xml_value_response(XmlValue::Array(vec![torrent_row]))),
            xml_response(&xml_value_response(XmlValue::Array(vec![file_row]))),
            xml_response(&xml_value_response(XmlValue::Array(vec![tracker_row]))),
        ];
        let (url, captured, server) = spawn_http_server(responses);
        let endpoint = RtorrentEndpointV1 {
            transport: RtorrentTransportV1::HttpXmlRpc {
                url,
                username_env: None,
                password_env: None,
            },
            automation_label_slot: Some(RtorrentCustomSlotV1::Custom2),
            timeout_seconds: 5,
        };
        let locations = BTreeMap::from([(
            "downloads".into(),
            TorrentLocationV1 {
                endpoint: "seedbox".into(),
                path: "/downloads".into(),
            },
        )]);
        let TorrentQueryResult::Torrents(inventory) = query_rtorrent(
            "seedbox".into(),
            endpoint,
            BTreeMap::new(),
            RemoteRpcOperationV1::Torrents,
            locations,
        )
        .await
        .unwrap() else {
            panic!("expected inventory")
        };
        server.join().unwrap();

        assert_eq!(inventory.identity.kind, TorrentClientKindV1::Rtorrent);
        assert_eq!(
            inventory.identity.protocol,
            TorrentProtocolV1::RtorrentXmlRpc
        );
        let torrent = &inventory.torrents[0];
        assert_eq!(torrent.info_hash, hash);
        assert_eq!(torrent.name, "MovieTitle");
        assert_eq!(torrent.state, TorrentStateV1::Seeding);
        assert_eq!(torrent.labels, vec!["archive"]);
        assert_eq!(torrent.location_name.as_deref(), Some("downloads"));
        assert_eq!(torrent.location_fingerprint.as_ref().unwrap().len(), 64);
        assert_eq!(
            torrent.tracker_summary.as_deref(),
            Some("tracker.example (enabled)")
        );
        assert_eq!(torrent.files[0].path, "Movie/file.mkv");
        assert_eq!(torrent.files[0].completed_bytes, 12);
        let requests = captured.lock().unwrap();
        assert!(requests[3].contains("<methodName>d.multicall2</methodName>"));
        assert!(requests[3].contains("d.custom2="));
        assert!(requests[4].contains("<methodName>f.multicall</methodName>"));
        assert!(requests[5].contains("<methodName>t.multicall</methodName>"));
        assert!(!requests[5].contains("token=private"));
    }

    #[test]
    fn transmission_mutations_are_hash_addressed_and_location_never_moves_data() {
        let hash = "c".repeat(40);
        let (url, captured, server) = spawn_http_server(vec![
            json_response(json!({"jsonrpc":"2.0","result":{},"id":1})),
            json_response(json!({"jsonrpc":"2.0","result":{},"id":1})),
        ]);
        let endpoint = TransmissionEndpointV1 {
            url,
            username_env: None,
            password_env: None,
            protocol: TransmissionProtocolPreferenceV1::JsonRpc2,
            timeout_seconds: 5,
        };
        let probe = TransmissionProbe {
            protocol: TorrentProtocolV1::TransmissionJsonRpc2,
            version: Some("4.1.0".into()),
            fingerprint: "d".repeat(64),
        };
        transmission_mutation(
            &endpoint,
            &probe,
            std::slice::from_ref(&hash),
            &TorrentControlActionV1::Start,
            None,
        )
        .unwrap();
        transmission_mutation(
            &endpoint,
            &probe,
            std::slice::from_ref(&hash),
            &TorrentControlActionV1::AdoptLocation {
                location: "archive".into(),
            },
            Some("/media/archive"),
        )
        .unwrap();
        server.join().unwrap();
        let requests = captured.lock().unwrap();
        assert!(requests[0].contains("\"method\":\"torrent_start\""));
        assert!(requests[0].contains(&hash));
        assert!(requests[1].contains("\"method\":\"torrent_set_location\""));
        assert!(requests[1].contains("\"move\":false"));
        assert!(requests[1].contains("/media/archive"));
    }

    #[tokio::test]
    async fn transmission_ambiguous_idempotent_control_is_resolved_but_verify_is_not_repeated() {
        let hash = "f".repeat(40);
        let before_value = transmission_torrent(&hash, 0);
        let after_value = transmission_torrent(&hash, 4);
        let mut total_files = 0;
        let expected =
            transmission_snapshot(&before_value, "seedbox", &BTreeMap::new(), &mut total_files)
                .unwrap();
        let session = || {
            json_response(json!({
                "jsonrpc":"2.0",
                "result":{"version":"4.1.0","rpc_version_semver":"6.0.0"},
                "id":1
            }))
        };
        let inventory = |torrent: JsonValue| {
            json_response(json!({"jsonrpc":"2.0","result":{"torrents":[torrent]},"id":1}))
        };
        let (url, captured, server) = spawn_http_server(vec![
            session(),
            inventory(before_value.clone()),
            String::new(),
            session(),
            inventory(after_value),
        ]);
        let endpoint = TransmissionEndpointV1 {
            url,
            username_env: None,
            password_env: None,
            protocol: TransmissionProtocolPreferenceV1::JsonRpc2,
            timeout_seconds: 5,
        };
        let receipt = control_transmission(
            endpoint,
            TorrentControlRequest {
                endpoint_name: "seedbox".into(),
                locations: BTreeMap::new(),
                target_hashes: vec![hash.clone()],
                expected_snapshots: vec![expected.clone()],
                control: TorrentControlActionV1::Start,
                resolved_location: None,
            },
        )
        .await
        .unwrap();
        server.join().unwrap();
        assert!(receipt.ambiguous_transport_resolved);
        assert!(receipt.post_state_confirmed);
        assert_eq!(receipt.changed_hashes, vec![hash.clone()]);
        assert_eq!(
            captured
                .lock()
                .unwrap()
                .iter()
                .filter(|request| request.contains("torrent_start"))
                .count(),
            1
        );

        let (url, captured, server) =
            spawn_http_server(vec![session(), inventory(before_value), String::new()]);
        let endpoint = TransmissionEndpointV1 {
            url,
            username_env: None,
            password_env: None,
            protocol: TransmissionProtocolPreferenceV1::JsonRpc2,
            timeout_seconds: 5,
        };
        let error = control_transmission(
            endpoint,
            TorrentControlRequest {
                endpoint_name: "seedbox".into(),
                locations: BTreeMap::new(),
                target_hashes: vec![hash],
                expected_snapshots: vec![expected],
                control: TorrentControlActionV1::Verify,
                resolved_location: None,
            },
        )
        .await
        .unwrap_err();
        server.join().unwrap();
        assert!(error.to_string().contains("transport failed"));
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.contains("torrent_verify"))
                .count(),
            1
        );
    }

    #[test]
    fn duplicate_hashes_and_protocol_identity_drift_fail_closed() {
        let hash = "1".repeat(40);
        let mut total_files = 0;
        let snapshot = transmission_snapshot(
            &transmission_torrent(&hash, 0),
            "seedbox",
            &BTreeMap::new(),
            &mut total_files,
        )
        .unwrap();
        assert!(reject_duplicate_hashes(&[snapshot.clone(), snapshot.clone()]).is_err());
        let before = TorrentInventoryV1 {
            endpoint: "seedbox".into(),
            identity: TorrentClientIdentityV1 {
                kind: TorrentClientKindV1::Transmission,
                protocol: TorrentProtocolV1::TransmissionJsonRpc2,
                version: Some("4.1.0".into()),
                fingerprint: "a".repeat(64),
            },
            torrents: vec![snapshot.clone()],
            truncated: false,
        };
        let after = TorrentInventoryV1 {
            identity: TorrentClientIdentityV1 {
                kind: TorrentClientKindV1::Transmission,
                protocol: TorrentProtocolV1::TransmissionLegacy,
                version: Some("4.0.6".into()),
                fingerprint: "b".repeat(64),
            },
            ..before.clone()
        };
        let error = resolve_mutation(
            "seedbox".into(),
            before,
            vec![hash],
            TorrentControlActionV1::Start,
            Ok(()),
            || Ok(after),
        )
        .unwrap_err();
        assert!(error.to_string().contains("identity or protocol changed"));
    }

    #[test]
    fn rtorrent_http_transport_uses_xmlrpc_codec_and_basic_auth() {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let xml = xml_string_response("0.9.8");
        let (url, captured, server) = spawn_http_server(vec![xml_response(&xml)]);
        std::env::set_var("CODEX_TEAM_TEST_RPC_USER", "fixture-user");
        std::env::set_var("CODEX_TEAM_TEST_RPC_PASSWORD", "fixture-password");
        let endpoint = RtorrentEndpointV1 {
            transport: RtorrentTransportV1::HttpXmlRpc {
                url,
                username_env: Some("CODEX_TEAM_TEST_RPC_USER".into()),
                password_env: Some("CODEX_TEAM_TEST_RPC_PASSWORD".into()),
            },
            automation_label_slot: Some(RtorrentCustomSlotV1::Custom3),
            timeout_seconds: 5,
        };
        let value = xml_call(
            &endpoint,
            &BTreeMap::new(),
            XmlRequest::new("system.client_version"),
        )
        .unwrap();
        std::env::remove_var("CODEX_TEAM_TEST_RPC_USER");
        std::env::remove_var("CODEX_TEAM_TEST_RPC_PASSWORD");
        server.join().unwrap();
        assert_eq!(xml_string(value).unwrap(), "0.9.8");
        let request = &captured.lock().unwrap()[0];
        assert!(request.contains("Authorization: Basic "));
        assert!(request.contains("<methodName>system.client_version</methodName>"));
        assert!(!request.contains("fixture-password"));
    }

    #[test]
    fn rtorrent_mutations_use_only_closed_hash_addressed_methods() {
        let xml = "<?xml version=\"1.0\"?><methodResponse><params><param><value><int>0</int></value></param></params></methodResponse>";
        let (url, captured, server) = spawn_http_server(vec![xml_response(xml)]);
        let endpoint = RtorrentEndpointV1 {
            transport: RtorrentTransportV1::HttpXmlRpc {
                url,
                username_env: None,
                password_env: None,
            },
            automation_label_slot: Some(RtorrentCustomSlotV1::Custom4),
            timeout_seconds: 5,
        };
        let hash = "d".repeat(40);
        rtorrent_mutation(
            &endpoint,
            &BTreeMap::new(),
            std::slice::from_ref(&hash),
            &TorrentControlActionV1::SetAutomationLabel {
                label: Some("archive".into()),
            },
            None,
        )
        .unwrap();
        server.join().unwrap();
        let request = &captured.lock().unwrap()[0];
        assert!(request.contains("<methodName>d.custom4.set</methodName>"));
        assert!(request.contains(&hash));
        assert!(request.contains("archive"));
        assert!(!request.contains("load"));
        assert!(!request.contains("erase"));
    }

    #[test]
    fn loopback_scgi_transport_frames_and_parses_xmlrpc() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_thread = captured.clone();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            stream.read_to_end(&mut request).unwrap();
            captured_thread.lock().unwrap().extend_from_slice(&request);
            let xml = xml_string_response("0.9.8");
            let response = format!("Status: 200 OK\r\nContent-Type: text/xml\r\n\r\n{xml}");
            stream.write_all(response.as_bytes()).unwrap();
        });
        let endpoint = RtorrentEndpointV1 {
            transport: RtorrentTransportV1::LoopbackTcpScgi { port },
            automation_label_slot: None,
            timeout_seconds: 5,
        };
        let value = xml_call(
            &endpoint,
            &BTreeMap::new(),
            XmlRequest::new("system.client_version"),
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(xml_string(value).unwrap(), "0.9.8");
        let request = captured.lock().unwrap();
        assert!(request.windows(4).any(|window| window == b"SCGI"));
        assert!(request
            .windows(b"system.client_version".len())
            .any(|window| window == b"system.client_version"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_scgi_requires_private_owned_socket() {
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::net::UnixListener;
        let temp = tempfile::tempdir().unwrap();
        let socket = temp.path().join("rtorrent.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            stream.read_to_end(&mut request).unwrap();
            let xml = xml_string_response("0.9.8");
            let response = format!("Status: 200 OK\r\n\r\n{xml}");
            stream.write_all(response.as_bytes()).unwrap();
        });
        let endpoint = RtorrentEndpointV1 {
            transport: RtorrentTransportV1::UnixScgi {
                socket: socket.to_string_lossy().into_owned(),
            },
            automation_label_slot: None,
            timeout_seconds: 5,
        };
        assert!(xml_call(
            &endpoint,
            &BTreeMap::new(),
            XmlRequest::new("system.client_version")
        )
        .is_ok());
        server.join().unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(unix_scgi(socket.to_str().unwrap(), 1, b"x").is_err());
    }

    #[test]
    fn xml_bounds_and_fixed_ssh_forwarding_are_closed() {
        assert!(validate_xml_envelope(
            b"<!DOCTYPE value [<!ENTITY xxe SYSTEM 'file:///etc/passwd'>]><value>&xxe;</value>"
        )
        .is_err());
        let mut nested = XmlValue::String("leaf".into());
        for _ in 0..=MAX_XML_DEPTH {
            nested = XmlValue::Array(vec![nested]);
        }
        let mut nodes = 0;
        assert!(validate_xml_value(&nested, 0, &mut nodes).is_err());
        let host = SshHostV1 {
            destination: "seed@example.test".into(),
            port: Some(2222),
        };
        let arguments = ssh_forward_arguments(&host, 5000);
        assert!(arguments.iter().any(|argument| argument == "-T"));
        assert!(arguments.iter().any(|argument| argument == "2222"));
        assert!(arguments.iter().any(|argument| argument == "-W"));
        assert!(arguments
            .iter()
            .any(|argument| argument == "127.0.0.1:5000"));
        assert_eq!(arguments.last().unwrap(), "seed@example.test");
        assert!(!arguments
            .iter()
            .any(|argument| argument.to_string_lossy().contains(';')));
    }

    #[test]
    fn pinned_client_crates_are_conformance_fixtures_not_runtime_transports() {
        assert!(std::any::type_name::<transmission_rpc::TransClient>().contains("TransClient"));
        assert!(std::any::type_name::<rtorrent_xmlrpc_bindings::Server>().contains("Server"));
        assert_eq!(
            rtorrent_label_getter(Some(RtorrentCustomSlotV1::Custom5)),
            Some("d.custom5=")
        );
        assert_eq!(
            rtorrent_label_setter(Some(RtorrentCustomSlotV1::Custom5)),
            Some("d.custom5.set")
        );
    }
}
