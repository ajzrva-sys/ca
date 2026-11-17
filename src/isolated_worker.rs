//! Pre-Tokio worker entry. The flag alone does not create a grant or start a tool.
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;

pub const WORKER_FLAG: &str = "--internal-isolated-worker";
pub const LAUNCH_ENV: &str = "CA_ISOLATED_WORKER_LAUNCH";

static COMMANDS_CONFINED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_commands_confined(confined: bool) {
    COMMANDS_CONFINED.store(confined, std::sync::atomic::Ordering::Release);
}

pub fn commands_are_confined() -> bool {
    COMMANDS_CONFINED.load(std::sync::atomic::Ordering::Acquire)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkerLaunchSpec {
    pub version: u32,
    pub worker_id: String,
    pub owner_id: String,
    pub lineage: Vec<String>,
    pub launch_nonce: String,
    pub policy_generation: u64,
    pub executable_digest: String,
    pub grant_digest: String,
    pub backend: String,
    pub required_capabilities: Vec<String>,
    pub private_directory: String,
    pub channel: String,
    pub limits: WorkerLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkerLimits {
    pub max_turns: u32,
    pub timeout_secs: u64,
    pub output_bytes: u64,
}

pub fn claims_worker_launch(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> bool {
    args.into_iter().any(|arg| arg.as_ref() == WORKER_FLAG)
}

/// Inherited host channels and a valid launch spec are required. A typed flag
/// with no spec exits without dispatching a model-selected command.
pub fn run_inherited() -> i32 {
    match std::env::var(LAUNCH_ENV) {
        Ok(raw) => match serde_json::from_str::<WorkerLaunchSpec>(&raw) {
            Ok(spec) => match spec.validate() {
                Ok(()) => serve_ready(&spec),
                Err(error) => {
                    eprintln!("isolated worker refused: {error}");
                    2
                }
            },
            Err(_) => {
                eprintln!("isolated worker refused: launch spec is invalid");
                2
            }
        },
        Err(_) => {
            eprintln!("isolated worker refused: missing inherited launch spec");
            2
        }
    }
}

impl WorkerLaunchSpec {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != 1
            || self.worker_id.is_empty()
            || self.owner_id.is_empty()
            || self.launch_nonce.len() < 16
            || !is_hex64(&self.executable_digest)
            || !is_hex64(&self.grant_digest)
            || self.backend.is_empty()
            || self.private_directory.is_empty()
            || self.channel.is_empty()
            || self.limits.max_turns == 0
            || self.limits.timeout_secs == 0
            || self.limits.output_bytes == 0
        {
            return Err("launch spec is incomplete");
        }
        if self
            .lineage
            .iter()
            .any(|item| item.is_empty() || item.len() > 128)
        {
            return Err("launch lineage is invalid");
        }
        Ok(())
    }
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadyReport {
    pub worker_id: String,
    pub owner_id: String,
    pub grant_digest: String,
    pub backend: String,
    pub policy_generation: u64,
    /// Effective guarantees observed inside the already confined process.
    /// Absent on an older handshake means none were observed.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Dispatch {
    worker_id: String,
    grant_digest: String,
    command: String,
}

pub fn host_accepts(spec: &WorkerLaunchSpec, ready: &ReadyReport) -> bool {
    spec.worker_id == ready.worker_id
        && spec.owner_id == ready.owner_id
        && spec.grant_digest == ready.grant_digest
        && spec.backend == ready.backend
        && spec.policy_generation == ready.policy_generation
}

pub fn capabilities_satisfied(spec: &WorkerLaunchSpec, ready: &ReadyReport) -> bool {
    spec.required_capabilities
        .iter()
        .all(|required| ready.capabilities.iter().any(|have| have == required))
}

/// The worker writes its identity and waits. A model-selected command runs
/// only after the host has accepted that identity and written a dispatch.
fn serve_ready(spec: &WorkerLaunchSpec) -> i32 {
    let channel = std::path::Path::new(&spec.channel);
    if !channel.is_dir() {
        eprintln!("isolated worker refused: channel is not a private directory");
        return 2;
    }
    let ready = ReadyReport {
        worker_id: spec.worker_id.clone(),
        owner_id: spec.owner_id.clone(),
        grant_digest: spec.grant_digest.clone(),
        backend: spec.backend.clone(),
        policy_generation: spec.policy_generation,
        capabilities: effective_capabilities(),
    };
    if std::fs::write(
        channel.join("ready.json"),
        serde_json::to_vec(&ready).unwrap_or_default(),
    )
    .is_err()
    {
        return 2;
    }
    let dispatch_path = channel.join("dispatch.json");
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(spec.limits.timeout_secs);
    while !dispatch_path.exists() {
        if std::time::Instant::now() > deadline {
            eprintln!("isolated worker refused: host did not accept the ready handshake");
            return 2;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let Ok(bytes) = std::fs::read(&dispatch_path) else {
        return 2;
    };
    let Ok(dispatch) = serde_json::from_slice::<Dispatch>(&bytes) else {
        return 2;
    };
    if dispatch.worker_id != spec.worker_id || dispatch.grant_digest != spec.grant_digest {
        eprintln!("isolated worker refused: dispatch identity does not match");
        return 2;
    }
    let mut stdout = match std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(&dispatch.command)
        .env_clear()
        .env(
            "PATH",
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        )
        .env("HOME", &spec.private_directory)
        .env("TMPDIR", &spec.private_directory)
        .current_dir(&spec.private_directory)
        .output()
    {
        Ok(output) => output,
        Err(_) => return 2,
    };
    stdout.stdout.truncate(spec.limits.output_bytes as usize);
    if std::fs::write(channel.join("result.json"), &stdout.stdout).is_err() {
        return 2;
    }
    i32::from(!stdout.status.success())
}

fn effective_capabilities() -> Vec<String> {
    let mut capabilities = vec!["filesystem".to_string()];
    if process_protection_active() {
        capabilities.push("process-protection".to_string());
    }
    capabilities
}

/// Seatbelt denies the network before this process accepts a command.
/// An unsandboxed process can open a socket, so it must not report this.
fn process_protection_active() -> bool {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    }
    let mut address: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    address.sin_family = libc::AF_INET as libc::sa_family_t;
    address.sin_port = 9u16.to_be();
    address.sin_addr = libc::in_addr {
        s_addr: u32::from(std::net::Ipv4Addr::LOCALHOST).to_be(),
    };
    let connected = unsafe {
        libc::connect(
            fd,
            &address as *const libc::sockaddr_in as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        )
    };
    let error = std::io::Error::last_os_error();
    unsafe { libc::close(fd) };
    connected < 0 && error.raw_os_error() == Some(libc::EPERM)
}

pub struct LaunchedWorker {
    child: std::process::Child,
    channel: std::path::PathBuf,
    ready: ReadyReport,
}

pub fn worker_executable() -> Result<std::path::PathBuf, String> {
    if let Ok(path) = std::env::var("CA_WORKER_TEST_BIN") {
        let path = std::path::PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "isolated worker binary is missing: {}",
            path.display()
        ));
    }
    std::env::current_exe().map_err(|error| error.to_string())
}

pub fn spec_for_grant(
    grant_digest: &str,
    worker_id: &str,
    owner_id: &str,
    policy_generation: u64,
) -> WorkerLaunchSpec {
    WorkerLaunchSpec {
        version: 1,
        worker_id: worker_id.to_string(),
        owner_id: owner_id.to_string(),
        lineage: Vec::new(),
        launch_nonce: format!("{grant_digest}{worker_id}"),
        policy_generation,
        executable_digest: "0".repeat(64),
        grant_digest: grant_digest.to_string(),
        backend: "seatbelt".into(),
        required_capabilities: vec!["filesystem".into(), "process-protection".into()],
        private_directory: ".".into(),
        channel: ".".into(),
        limits: WorkerLimits {
            max_turns: 1,
            timeout_secs: 15,
            output_bytes: 4096,
        },
    }
}

/// Start `sandbox-exec` around the worker before any model-selected command.
pub fn launch_confined(
    executable: &std::path::Path,
    spec: &WorkerLaunchSpec,
) -> Result<LaunchedWorker, String> {
    spec.validate()
        .map_err(|error| format!("isolated worker refused: {error}"))?;
    let channel = std::path::PathBuf::from(&spec.channel);
    if !channel.is_dir() {
        return Err("isolated worker refused: channel is not a private directory".into());
    }
    let _ = std::fs::remove_file(channel.join("ready.json"));
    let _ = std::fs::remove_file(channel.join("dispatch.json"));
    let _ = std::fs::remove_file(channel.join("result.json"));
    let executable = executable
        .canonicalize()
        .map_err(|error| format!("isolated worker refused: {error}"))?;
    let mut roots = vec![channel.clone()];
    if let Some(parent) = executable.parent() {
        roots.push(parent.to_path_buf());
    }
    let policy = crate::sandbox::SandboxPolicy {
        roots,
        read_only_roots: vec![
            std::path::PathBuf::from("/bin"),
            std::path::PathBuf::from("/usr"),
        ],
        temporary_directory: channel.clone(),
        network_allowed: false,
        host_credentials: false,
        proxy_port: None,
        hide_other_processes: true,
    };
    let profile = crate::sandbox::sandbox_profile(&policy)
        .map_err(|error| format!("isolated worker refused: {error}"))?;
    if !profile.contains("(deny network*)") || profile.contains("(allow network*)") {
        return Err(
            "isolated worker refused: seatbelt profile does not confine the network".into(),
        );
    }
    let prepared = crate::sandbox::SandboxBackend::new(crate::sandbox::SandboxMode::Required)
        .prepare_process(&executable, &[WORKER_FLAG.to_string()], &policy)
        .map_err(|error| format!("isolated worker refused: {error}"))?;
    if prepared.program != std::path::Path::new("/usr/bin/sandbox-exec") {
        return Err(
            "isolated worker refused: confinement was not applied before the worker".into(),
        );
    }
    let mut child = std::process::Command::new(prepared.program)
        .args(prepared.arguments)
        .env_clear()
        .env(LAUNCH_ENV, serde_json::to_string(spec).unwrap_or_default())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("isolated worker refused: {error}"))?;
    crate::child_mediation::track_worker_pid(child.id() as i32);
    let ready_path = channel.join("ready.json");
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(spec.limits.timeout_secs);
    while !ready_path.exists() {
        if std::time::Instant::now() > deadline {
            let pid = child.id() as i32;
            let _ = child.kill();
            let _ = child.wait();
            crate::child_mediation::untrack_worker_pid(pid);
            return Err("isolated worker refused: host did not observe a ready handshake".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let ready: ReadyReport =
        serde_json::from_slice(&std::fs::read(&ready_path).unwrap_or_default())
            .map_err(|_| "isolated worker refused: ready identity is unreadable")?;
    Ok(LaunchedWorker {
        child,
        channel,
        ready,
    })
}

/// Write the command only after identity and capabilities match.
/// A mismatch kills the worker and does not run the command in this process.
pub fn dispatch_accepted(
    mut worker: LaunchedWorker,
    expected: &WorkerLaunchSpec,
    command: &str,
) -> Result<Vec<u8>, String> {
    if !host_accepts(expected, &worker.ready) || !capabilities_satisfied(expected, &worker.ready) {
        let pid = worker.child.id() as i32;
        let _ = worker.child.kill();
        let _ = worker.child.wait();
        crate::child_mediation::untrack_worker_pid(pid);
        let _ = std::fs::remove_file(worker.channel.join("dispatch.json"));
        return Err(
            "isolated worker refused: identity or capability mismatch; command was not run and does not fall back in-process"
                .into(),
        );
    }
    let dispatch = Dispatch {
        worker_id: expected.worker_id.clone(),
        grant_digest: expected.grant_digest.clone(),
        command: command.to_string(),
    };
    std::fs::write(
        worker.channel.join("dispatch.json"),
        serde_json::to_vec(&dispatch).unwrap_or_default(),
    )
    .map_err(|error| format!("isolated worker refused: {error}"))?;
    let pid = worker.child.id() as i32;
    let status = worker
        .child
        .wait()
        .map_err(|error| format!("isolated worker refused: {error}"))?;
    crate::child_mediation::untrack_worker_pid(pid);
    let result_path = worker.channel.join("result.json");
    if !result_path.exists() {
        return Err(format!(
            "isolated worker refused: exited {status} without a command result and does not fall back in-process"
        ));
    }
    std::fs::read(&result_path).map_err(|error| format!("isolated worker refused: {error}"))
}

pub fn execute_confined_model_command(
    spec: &WorkerLaunchSpec,
    command: &str,
) -> Result<Vec<u8>, String> {
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let mut spec = spec.clone();
    spec.channel = directory.path().to_string_lossy().into_owned();
    spec.private_directory = spec.channel.clone();
    let executable = worker_executable()?;
    spec.executable_digest = sha256_hex(&executable.to_string_lossy());
    if spec.launch_nonce.len() < 16 {
        spec.launch_nonce = format!("{:064}", spec.policy_generation);
    }
    let launched = launch_confined(&executable, &spec)?;
    dispatch_accepted(launched, &spec, command)
}

fn sha256_hex(value: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> WorkerLaunchSpec {
        WorkerLaunchSpec {
            version: 1,
            worker_id: "worker".into(),
            owner_id: "owner".into(),
            lineage: vec!["parent".into()],
            launch_nonce: "0123456789abcdef".into(),
            policy_generation: 3,
            executable_digest: "a".repeat(64),
            grant_digest: "b".repeat(64),
            backend: "seatbelt".into(),
            required_capabilities: vec!["filesystem".into()],
            private_directory: "/tmp/ca-worker".into(),
            channel: "fd:3".into(),
            limits: WorkerLimits {
                max_turns: 4,
                timeout_secs: 30,
                output_bytes: 1024,
            },
        }
    }

    #[test]
    fn flag_without_a_spec_creates_no_authority() {
        assert!(claims_worker_launch(["ca", WORKER_FLAG]));
        assert!(!claims_worker_launch(["ca", "--help"]));
        let previous = std::env::var_os(LAUNCH_ENV);
        std::env::remove_var(LAUNCH_ENV);
        assert_eq!(run_inherited(), 2);
        if let Some(previous) = previous {
            std::env::set_var(LAUNCH_ENV, previous);
        }
    }

    #[test]
    fn mismatched_spec_is_rejected() {
        let mut broken = spec();
        broken.grant_digest = "short".into();
        assert!(broken.validate().is_err());
        assert!(spec().validate().is_ok());
    }

    #[test]
    fn mismatched_ready_identity_runs_no_command() {
        let directory = tempfile::tempdir().unwrap();
        let mut launch = spec();
        launch.channel = directory.path().to_string_lossy().into_owned();
        launch.limits.timeout_secs = 1;
        let channel = directory.path().to_path_buf();
        let worker = std::thread::spawn(move || serve_ready(&launch));
        let ready_path = channel.join("ready.json");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !ready_path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let ready: ReadyReport =
            serde_json::from_slice(&std::fs::read(&ready_path).unwrap()).unwrap();
        let mut launch = spec();
        launch.channel = channel.to_string_lossy().into_owned();
        assert!(host_accepts(&launch, &ready));
        launch.grant_digest = "c".repeat(64);
        assert!(!host_accepts(&launch, &ready));
        std::fs::write(
            channel.join("dispatch.json"),
            br#"{"worker_id":"worker","grant_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","command":"echo ran > command-ran"}"#,
        )
        .unwrap();
        assert_eq!(worker.join().unwrap(), 2);
        assert!(!channel.join("result.json").exists());
        assert!(!directory.path().join("command-ran").exists());
    }

    #[test]
    fn release_worker_checks_identity_before_any_command() {
        let exe = std::env::var("CA_WORKER_TEST_BIN")
            .unwrap_or_else(|_| "/Users/aji/.local/bin/ca".into());
        assert!(
            std::path::Path::new(&exe).is_file(),
            "worker test binary is missing: {exe}"
        );
        assert!(std::path::Path::new("/usr/bin/sandbox-exec").is_file());
        let directory = tempfile::tempdir().unwrap();
        let mut launch = spec();
        launch.channel = directory.path().to_string_lossy().into_owned();
        launch.private_directory = launch.channel.clone();
        launch.limits.timeout_secs = 2;
        let profile = crate::sandbox::sandbox_profile(&crate::sandbox::SandboxPolicy {
            roots: vec![
                std::path::PathBuf::from(&exe)
                    .parent()
                    .unwrap()
                    .to_path_buf(),
                directory.path().to_path_buf(),
            ],
            read_only_roots: vec![
                std::path::PathBuf::from("/bin"),
                std::path::PathBuf::from("/usr"),
            ],
            temporary_directory: directory.path().to_path_buf(),
            network_allowed: false,
            host_credentials: false,
            proxy_port: None,
            hide_other_processes: true,
        })
        .unwrap();
        assert!(profile.contains("(deny network*)"));
        let mut child = std::process::Command::new("/usr/bin/sandbox-exec")
            .arg("-p")
            .arg(&profile)
            .arg(&exe)
            .arg(WORKER_FLAG)
            .env(LAUNCH_ENV, serde_json::to_string(&launch).unwrap())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let ready_path = directory.path().join("ready.json");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !ready_path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            ready_path.exists(),
            "worker did not report ready before a command"
        );
        let ready: ReadyReport =
            serde_json::from_slice(&std::fs::read(&ready_path).unwrap()).unwrap();
        let mut mismatched = launch.clone();
        mismatched.grant_digest = "c".repeat(64);
        assert!(!host_accepts(&mismatched, &ready));
        std::fs::write(
            directory.path().join("dispatch.json"),
            br#"{"worker_id":"worker","grant_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","command":"echo ran"}"#,
        )
        .unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success());
        assert!(!directory.path().join("result.json").exists());
    }

    fn release_binary() -> std::path::PathBuf {
        if let Ok(path) = std::env::var("CA_WORKER_TEST_BIN") {
            return std::path::PathBuf::from(path);
        }
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/release/ca")
    }

    fn confined_spec(directory: &std::path::Path) -> WorkerLaunchSpec {
        let mut launch = spec();
        launch.channel = directory.to_string_lossy().into_owned();
        launch.private_directory = launch.channel.clone();
        launch.limits.timeout_secs = 5;
        launch.required_capabilities = vec!["filesystem".into(), "process-protection".into()];
        launch
    }

    #[test]
    fn unsandboxed_process_does_not_claim_seatbelt_or_aggregate_resources() {
        let capabilities = effective_capabilities();
        assert!(capabilities.contains(&"filesystem".to_string()));
        assert!(!capabilities.contains(&"process-protection".to_string()));
        assert!(!capabilities.contains(&"aggregate-resources".to_string()));
    }

    #[test]
    fn release_worker_runs_one_command_only_after_confinement_and_acceptance() {
        let _mediation = crate::child_mediation::MediationTestPermit::acquire();
        let executable = release_binary();
        assert!(
            executable.is_file(),
            "worker test binary is missing: {}",
            executable.display()
        );
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("command-ran");
        let quoted = marker.display().to_string().replace('\'', "'\\''");
        let command = format!("printf ran > '{quoted}'; printf ran");
        let launch = confined_spec(directory.path());
        let probe = launch_confined(&executable, &launch).expect("confined launch");
        assert!(
            probe
                .ready
                .capabilities
                .contains(&"process-protection".to_string()),
            "seatbelt was not active before the command: {:?}",
            probe.ready.capabilities
        );
        assert!(!marker.exists(), "command ran before the host accepted");
        let mut rejected = launch.clone();
        rejected.grant_digest = "c".repeat(64);
        let error = dispatch_accepted(probe, &rejected, &command).unwrap_err();
        assert!(error.contains("does not fall back"), "{error}");
        assert!(!marker.exists());
        assert!(!directory.path().join("dispatch.json").exists());
        let mut missing = launch.clone();
        missing.required_capabilities = vec!["aggregate-resources".into()];
        let missing_worker = launch_confined(&executable, &missing).expect("capability launch");
        let error = dispatch_accepted(missing_worker, &missing, &command).unwrap_err();
        assert!(error.contains("does not fall back"), "{error}");
        assert!(!marker.exists());
        let accepted = launch_confined(&executable, &launch).expect("accepted launch");
        assert!(!marker.exists());
        let output = dispatch_accepted(accepted, &launch, &command).expect("accepted command");
        assert_eq!(output, b"ran");
        assert_eq!(std::fs::read(&marker).unwrap(), b"ran");
    }
}
