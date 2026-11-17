//! Admitted-child network effects. The child names a destination reference.
//! The host connects to the address it already vetted, through its proxy.
#![allow(dead_code)]
use crate::execution_grant::AdmittedExecutionGrant;
use crate::jobs::{JobManager, JobOwner};
use crate::network_broker::{admit, AddressClass, ChildProxy, Destination};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

struct PublishedRoute {
    destination: Destination,
    address: SocketAddr,
    observed_address: SocketAddr,
    owner: String,
    generation: u64,
}

struct EffectRecord {
    body: String,
    uncertain: bool,
}

struct MediationState {
    routes: HashMap<String, PublishedRoute>,
    effects: HashMap<String, EffectRecord>,
    proxy: Option<ChildProxy>,
    live_generation: u64,
    /// Generations whose cleanup was requested but not yet observed.
    unconfirmed: HashSet<u64>,
    confirmed: HashSet<u64>,
    queued_approvals: Vec<(u64, String)>,
}

fn state() -> &'static Mutex<MediationState> {
    static STATE: OnceLock<Mutex<MediationState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(MediationState {
            routes: HashMap::new(),
            effects: HashMap::new(),
            proxy: None,
            live_generation: 1,
            unconfirmed: HashSet::new(),
            confirmed: HashSet::new(),
            queued_approvals: Vec::new(),
        })
    })
}

pub fn queue_approval(generation: u64, id: &str) -> Result<(), String> {
    let mut guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
    if guard.unconfirmed.contains(&generation) {
        return Err("old grant is blocked until cleanup is confirmed".into());
    }
    guard.queued_approvals.push((generation, id.to_string()));
    Ok(())
}

pub fn queued_approvals(generation: u64) -> usize {
    state()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .queued_approvals
        .iter()
        .filter(|(queued, _)| *queued == generation)
        .count()
}

pub fn nested_spawn_allowed(generation: u64) -> bool {
    let guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
    !guard.unconfirmed.contains(&generation)
}

/// The same generation gate mediated execution uses. Unconfirmed cleanup blocks
/// the old grant. After cleanup is confirmed, `ensure_current` still rejects a
/// generation that is no longer live.
pub fn require_current_generation(grant: &AdmittedExecutionGrant) -> Result<u64, String> {
    let guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
    if guard.unconfirmed.contains(&grant.policy_generation) {
        return Err("old grant is blocked until cleanup is confirmed".into());
    }
    let live = guard.live_generation;
    grant
        .ensure_current(live)
        .map_err(|_| "stale grant generation".to_string())?;
    Ok(live)
}

/// Host-only publication of a vetted address. The child never supplies this.
pub fn host_publish_route(
    reference: &str,
    owner: &str,
    protocol: &str,
    hostname: &str,
    address: SocketAddr,
    generation: u64,
    classes: Vec<AddressClass>,
) {
    let mut guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
    guard.routes.insert(
        reference.to_string(),
        PublishedRoute {
            destination: Destination {
                reference: reference.to_string(),
                protocol: protocol.to_string(),
                hostname: hostname.to_string(),
                port: address.port(),
                classes,
                opaque_tunnel: protocol == "https",
            },
            address,
            observed_address: address,
            owner: owner.to_string(),
            generation,
        },
    );
}

/// The host saw a different address after admission. Connect must not follow it.
pub fn host_retarget_published_route(reference: &str, address: SocketAddr) -> bool {
    let mut guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
    let Some(route) = guard.routes.get_mut(reference) else {
        return false;
    };
    route.observed_address = address;
    true
}

pub fn live_proxy_port() -> Option<u16> {
    let guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
    guard
        .proxy
        .as_ref()
        .filter(|proxy| !proxy.is_closed())
        .map(|proxy| proxy.port())
}

pub fn host_open_proxy(generation: u64) -> Result<u16, String> {
    let proxy = ChildProxy::start(generation).map_err(|error| error.to_string())?;
    let port = proxy.port();
    let mut guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
    guard.proxy = Some(proxy);
    Ok(port)
}

fn worker_pids() -> &'static Mutex<Vec<i32>> {
    static PIDS: OnceLock<Mutex<Vec<i32>>> = OnceLock::new();
    PIDS.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn track_worker_pid(pid: i32) {
    if pid > 0 {
        worker_pids()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push(pid);
    }
}

pub fn untrack_worker_pid(pid: i32) {
    worker_pids()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .retain(|tracked| *tracked != pid);
}

pub fn cancel_tracked_workers() -> usize {
    let pids = std::mem::take(
        &mut *worker_pids()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()),
    );
    for pid in &pids {
        unsafe { libc::kill(*pid, libc::SIGKILL) };
    }
    pids.len()
}

pub fn child_network_profile(proxy_port: u16) -> Result<String, crate::sandbox::SandboxError> {
    crate::sandbox::sandbox_profile(&crate::sandbox::SandboxPolicy {
        roots: vec![std::path::PathBuf::from("/tmp/workspace")],
        read_only_roots: Vec::new(),
        temporary_directory: std::path::PathBuf::from("/tmp/job"),
        network_allowed: true,
        host_credentials: false,
        proxy_port: Some(proxy_port),
        hide_other_processes: true,
    })
}

pub async fn execute_mediated(
    grant: &AdmittedExecutionGrant,
    owner: &str,
    name: &str,
    arguments: &Value,
) -> crate::agent::registry::ToolExecutionResult {
    match perform(grant, owner, name, arguments) {
        Ok(body) => crate::agent::registry::ToolExecutionResult::Success(body),
        Err(error) => crate::agent::registry::ToolExecutionResult::Denied(error),
    }
}

fn perform(
    grant: &AdmittedExecutionGrant,
    owner: &str,
    name: &str,
    arguments: &Value,
) -> Result<String, String> {
    if grant.authorize_tool(name).is_err() {
        return Err("tool is outside the child grant".into());
    }
    let live = require_current_generation(grant)?;

    let (reference, payload) = planned_payload(name, arguments)?;
    if !grant.destinations.contains(&reference) {
        return Err("destination reference is not in the child grant".into());
    }
    let mut guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
    let published = guard
        .routes
        .get(&reference)
        .ok_or_else(|| "destination was not published by the host".to_string())?;
    if published.owner != owner || published.generation != grant.policy_generation {
        return Err("destination owner or generation does not match".into());
    }
    if published.destination.protocol != "http" && published.destination.protocol != "https" {
        return Err("alternate socket family is not admitted".into());
    }
    // Admit against the stored address. A later hostname lookup cannot replace it.
    let route = admit(
        &published.destination,
        &reference,
        owner,
        owner,
        &published.destination.protocol,
        published.address.port(),
        published.address,
        grant.policy_generation,
        live,
    )
    .map_err(|error| format!("destination admission failed: {error:?}"))?;
    if published.observed_address != route.address {
        return Err("destination address changed between admission and connect".into());
    }
    let effect_key = format!("{owner}:{name}:{reference}:{}", grant.digest);
    if let Some(existing) = guard.effects.get(&effect_key) {
        if existing.uncertain {
            return Err("uncertain effect is not replayed".into());
        }
        return Ok(existing.body.clone());
    }
    if guard
        .proxy
        .as_ref()
        .is_none_or(|proxy| proxy.generation() != live || proxy.is_closed())
    {
        guard.proxy = Some(ChildProxy::start(live).map_err(|error| error.to_string())?);
    }
    let proxy = guard.proxy.as_ref().unwrap();
    let port = proxy.port();
    proxy.insert(owner, route, live);
    drop(guard);

    let response = exchange(port, &reference, owner, &payload)?;
    if response_status(&response).is_some_and(|status| (300..400).contains(&status)) {
        let mut guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
        guard.effects.insert(
            effect_key,
            EffectRecord {
                body: response,
                uncertain: false,
            },
        );
        return Err("redirect requires a new admission".into());
    }
    let mut guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
    guard.effects.insert(
        effect_key,
        EffectRecord {
            body: response.clone(),
            uncertain: false,
        },
    );
    Ok(response)
}

/// Mark an effect uncertain after a failed local write. Reconciliation may
/// confirm the stored body and must not run it again.
pub fn fail_latest_local_write() -> Option<String> {
    let mut guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
    let key = guard.effects.keys().next().cloned()?;
    if let Some(effect) = guard.effects.get_mut(&key) {
        effect.uncertain = true;
    }
    Some(key)
}

pub fn confirm_uncertain(effect_key: &str) -> Result<String, String> {
    let mut guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
    let effect = guard
        .effects
        .get_mut(effect_key)
        .ok_or_else(|| "no observed effect to reconcile".to_string())?;
    effect.uncertain = false;
    Ok(effect.body.clone())
}

fn planned_payload(name: &str, arguments: &Value) -> Result<(String, String), String> {
    match name {
        "http_request" => {
            let request: crate::mediated_ops::HttpRequest =
                serde_json::from_value(arguments.clone()).map_err(|error| error.to_string())?;
            let planned = crate::mediated_ops::plan_http(&request)?;
            Ok((
                request.destination,
                json!({"op":"http","args":planned.args,"body":request.body}).to_string(),
            ))
        }
        "git_operation" => {
            let request: crate::mediated_ops::GitOperation =
                serde_json::from_value(arguments.clone()).map_err(|error| error.to_string())?;
            let planned = crate::mediated_ops::plan_git(&request)?;
            Ok((
                request.remote,
                json!({"op":"git","args":planned.args}).to_string(),
            ))
        }
        "ssh_operation" => {
            let request: crate::mediated_ops::SshOperation =
                serde_json::from_value(arguments.clone()).map_err(|error| error.to_string())?;
            let planned = crate::mediated_ops::plan_ssh(&request)?;
            Ok((
                request.destination,
                json!({"op":"ssh","args":planned.args}).to_string(),
            ))
        }
        _ => Err("not a mediated child operation".into()),
    }
}

fn exchange(port: u16, reference: &str, owner: &str, payload: &str) -> Result<String, String> {
    let mut last = "proxy did not answer".to_string();
    for _ in 0..20 {
        match exchange_once(port, reference, owner, payload) {
            Ok(response) if response.starts_with("HTTP/") => return Ok(response),
            Ok(response) => last = response,
            Err(error) => last = error,
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(last)
}

fn exchange_once(port: u16, reference: &str, owner: &str, payload: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    // http_proxy is ignored. The only socket is the host proxy.
    let mut client = std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .map_err(|error| error.to_string())?;
    let _ = client.set_read_timeout(Some(Duration::from_secs(2)));
    let header = format!(
        "POST /effect HTTP/1.1\r\nHost: localhost\r\nx-ca-destination: {reference}\r\nx-ca-owner: {owner}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    client
        .write_all(header.as_bytes())
        .map_err(|error| error.to_string())?;
    let mut response = String::new();
    client
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    Ok(response)
}

fn response_status(response: &str) -> Option<u16> {
    let code = response.split_whitespace().nth(1)?;
    code.parse().ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevocationReport {
    pub generation: u64,
    pub cancel_requested: bool,
    pub tunnels_closed: bool,
    pub jobs_terminal: bool,
    pub confirmed: bool,
}

/// Request revocation. Observing terminal jobs is what confirms cleanup.
/// A request without that observation stays unconfirmed and blocks the old grant.
pub async fn revoke_owned(manager: &JobManager, owner: JobOwner) -> RevocationReport {
    let (generation, tunnels_closed) = {
        let mut guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
        guard.live_generation = guard.live_generation.saturating_add(1);
        let tunnels_closed = if let Some(proxy) = guard.proxy.as_ref() {
            proxy.revoke();
            proxy.is_closed()
        } else {
            false
        };
        guard.proxy = None;
        let generation = guard.live_generation - 1;
        guard.unconfirmed.insert(generation);
        guard
            .queued_approvals
            .retain(|(queued, _)| *queued != generation);
        (generation, tunnels_closed)
    };
    let _workers = cancel_tracked_workers();
    let jobs = manager.list_owned(owner).await;
    for job in &jobs {
        if !job.status.is_terminal() {
            let _ = manager.kill_owned(job.id, owner).await;
        }
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut jobs_terminal = false;
    while std::time::Instant::now() < deadline {
        let pending = manager.list_owned(owner).await;
        jobs_terminal = pending.iter().all(|job| job.status.is_terminal());
        if jobs_terminal {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
    if jobs_terminal {
        guard.unconfirmed.remove(&generation);
        guard.confirmed.insert(generation);
    }
    RevocationReport {
        generation,
        cancel_requested: true,
        tunnels_closed,
        jobs_terminal,
        confirmed: jobs_terminal,
    }
}

/// A cancellation request that has not observed cleanup. Old-grant work stays blocked.
pub fn note_cancel_without_confirmation(generation: u64) {
    let mut guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
    guard.unconfirmed.insert(generation);
}

/// Mediation state is process-wide. Tests that publish routes, revoke a
/// generation, or track a worker hold this for the whole test so one case
/// cannot mark another's grant unconfirmed or kill its worker.
#[cfg(test)]
pub struct MediationTestPermit;

#[cfg(test)]
static MEDIATION_TEST_HELD: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
impl MediationTestPermit {
    pub fn acquire() -> Self {
        while MEDIATION_TEST_HELD
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::Acquire,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        Self
    }
}

#[cfg(test)]
impl Drop for MediationTestPermit {
    fn drop(&mut self) {
        MEDIATION_TEST_HELD.store(false, std::sync::atomic::Ordering::Release);
    }
}

pub fn reset_for_test() {
    let mut guard = state().lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(proxy) = guard.proxy.as_ref() {
        proxy.revoke();
    }
    cancel_tracked_workers();
    *guard = MediationState {
        routes: HashMap::new(),
        effects: HashMap::new(),
        proxy: None,
        live_generation: 1,
        unconfirmed: HashSet::new(),
        confirmed: HashSet::new(),
        queued_approvals: Vec::new(),
    };
}

/// Loopback fixture that counts accepted connections and can answer once.
pub fn fixture_server(
    status: u16,
    body: &str,
) -> (SocketAddr, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let hits_thread = std::sync::Arc::clone(&hits);
    let body = body.to_string();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        while let Ok((mut socket, _)) = listener.accept() {
            hits_thread.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut buffer = [0_u8; 2048];
            let _ = socket.read(&mut buffer);
            let payload = format!(
                "HTTP/1.1 {status} fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(payload.as_bytes());
            if (300..400).contains(&status) {
                break;
            }
        }
    });
    (address, hits)
}
