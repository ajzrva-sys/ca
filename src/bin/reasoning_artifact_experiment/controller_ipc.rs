//! Controller-owned framed IPC for the two RND-097 internal workers.
//!
//! This is deliberately a process boundary, not a convenient in-process call:
//! the provider broker is the only child allowed to resolve configured OpenAI
//! authority, while the tool worker starts with a scrubbed environment.  The
//! controller validates both sides of a frame and never retains child stderr.

use std::ffi::OsStr;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

use crate::reasoning_experiment_protocol::{
    read_ipc_frame, write_ipc_frame, IpcPayloadV1, ProviderCallReceiptV1, ProviderCallSpecV1,
    ToolCallReceiptV1, ToolCallSpecV1, INTERNAL_BROKER_FLAG, INTERNAL_BROKER_PREFLIGHT_FLAG,
    INTERNAL_TOOL_WORKER_FLAG, INTERNAL_TOOL_WORKER_PREFLIGHT_FLAG,
    MAX_PROVIDER_CALL_RECEIPT_FRAME_BYTES, MAX_PROVIDER_CALL_SPEC_FRAME_BYTES,
    MAX_TOOL_CALL_RECEIPT_FRAME_BYTES, MAX_TOOL_CALL_SPEC_FRAME_BYTES,
};

const MAX_WORKER_STDERR_BYTES: usize = 64 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Proves that the isolated worker can resolve both exact canonical OpenAI
/// routes and their credential without emitting metadata or making traffic.
pub fn invoke_provider_preflight(
    worker: &Path,
    experiment_dir: &Path,
    timeout: Duration,
) -> Result<()> {
    validate_executable(worker)?;
    validate_private_directory(experiment_dir)?;
    if timeout.is_zero() {
        bail!("broker preflight timeout must be nonzero");
    }
    let mut child = Command::new(worker)
        .arg(INTERNAL_BROKER_PREFLIGHT_FLAG)
        .current_dir(experiment_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("cannot spawn internal broker preflight")?;
    if !wait_for_exit(&mut child, timeout)?.success() {
        bail!("canonical OpenAI broker preflight failed without provider traffic");
    }
    Ok(())
}

/// Invokes exactly one isolated provider broker call.  This function does not
/// create a provider request body or inspect configuration; it only transports
/// an already validated call to the pre-registered `ca` worker binary.
pub fn invoke_provider_worker(
    worker: &Path,
    experiment_dir: &Path,
    call: &ProviderCallSpecV1,
    timeout: Duration,
) -> Result<ProviderCallReceiptV1> {
    call.validate()
        .map_err(|error| anyhow!("provider call is invalid before broker spawn: {error}"))?;
    // Exercise the exact credential-free worker binary and the required macOS
    // sandbox before checking the time-sensitive paid receipts.  The latter
    // check is deliberately last so a price card cannot age past its boundary
    // while a slower preflight runs.
    invoke_tool_worker_preflight(worker, experiment_dir, Duration::from_secs(10))?;
    crate::pilot::require_paid_dispatch_gates(experiment_dir)?;
    let receipt: ProviderCallReceiptV1 = invoke_framed_child(
        FramedChildConfig {
            program: worker,
            arguments: &[OsStr::new(INTERNAL_BROKER_FLAG)],
            current_dir: experiment_dir,
            request_limit: MAX_PROVIDER_CALL_SPEC_FRAME_BYTES,
            response_limit: MAX_PROVIDER_CALL_RECEIPT_FRAME_BYTES,
            timeout,
            scrub_environment: false,
        },
        call,
    )?;
    receipt
        .validate_against(call)
        .map_err(|error| anyhow!("broker receipt is not bound to its call: {error}"))?;
    Ok(receipt)
}

/// Proves that the registered worker can launch a no-network process through
/// the required macOS sandbox. It inherits no configuration or credentials,
/// emits no output, and performs no provider operation.
pub fn invoke_tool_worker_preflight(
    worker: &Path,
    experiment_dir: &Path,
    timeout: Duration,
) -> Result<()> {
    validate_executable(worker)?;
    validate_private_directory(experiment_dir)?;
    if timeout.is_zero() {
        bail!("tool-worker preflight timeout must be nonzero");
    }
    let mut command = Command::new(worker);
    command
        .arg(INTERNAL_TOOL_WORKER_PREFLIGHT_FLAG)
        .current_dir(experiment_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_clear()
        .env(
            "PATH",
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        );
    let mut child = command
        .spawn()
        .context("cannot spawn internal tool-worker preflight")?;
    if !wait_for_exit(&mut child, timeout)?.success() {
        bail!("required tool-worker sandbox preflight failed without provider traffic");
    }
    Ok(())
}

/// Invokes exactly one isolated tool-worker call.  Unlike the provider worker,
/// this child begins with an empty environment because it has no legitimate
/// need for provider, proxy, SSH, or user-runtime authority.
pub fn invoke_tool_worker(
    worker: &Path,
    experiment_dir: &Path,
    call: &ToolCallSpecV1,
    timeout: Duration,
) -> Result<ToolCallReceiptV1> {
    call.validate()
        .map_err(|error| anyhow!("tool call is invalid before worker spawn: {error}"))?;
    let receipt: ToolCallReceiptV1 = invoke_framed_child(
        FramedChildConfig {
            program: worker,
            arguments: &[OsStr::new(INTERNAL_TOOL_WORKER_FLAG)],
            current_dir: experiment_dir,
            request_limit: MAX_TOOL_CALL_SPEC_FRAME_BYTES,
            response_limit: MAX_TOOL_CALL_RECEIPT_FRAME_BYTES,
            timeout,
            scrub_environment: true,
        },
        call,
    )?;
    receipt
        .validate_against(call)
        .map_err(|error| anyhow!("tool receipt is not bound to its call: {error}"))?;
    Ok(receipt)
}

/// The generic transport is kept private except to its focused test module.
/// It consumes standard input after the one length-prefixed frame, requires a
/// single decodable response frame, drains but never returns stderr, and does
/// not interpret any child output beyond the typed IPC payload.
struct FramedChildConfig<'a> {
    program: &'a Path,
    arguments: &'a [&'a OsStr],
    current_dir: &'a Path,
    request_limit: u32,
    response_limit: u32,
    timeout: Duration,
    scrub_environment: bool,
}

fn invoke_framed_child<I, O>(config: FramedChildConfig<'_>, request: &I) -> Result<O>
where
    I: IpcPayloadV1,
    O: IpcPayloadV1 + Send + 'static,
{
    request
        .validate_ipc_payload()
        .map_err(|error| anyhow!("outbound IPC payload is invalid: {error}"))?;
    validate_executable(config.program)?;
    validate_private_directory(config.current_dir)?;
    if config.timeout.is_zero() {
        bail!("worker timeout must be nonzero");
    }

    let mut command = Command::new(config.program);
    command
        .args(config.arguments)
        .current_dir(config.current_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if config.scrub_environment {
        command.env_clear().env(
            "PATH",
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        );
    }
    let mut child = command.spawn().context("cannot spawn internal worker")?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("internal worker stdin is unavailable"))?;
    write_ipc_frame(&mut stdin, request, config.request_limit)
        .map_err(|_| anyhow!("cannot write internal worker IPC frame"))?;
    drop(stdin);

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("internal worker stdout is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("internal worker stderr is unavailable"))?;
    let response_limit = config.response_limit;
    let (response_tx, response_rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut stdout = stdout;
        let response = read_ipc_frame::<_, O>(&mut stdout, response_limit)
            .map_err(|_| anyhow!("internal worker response frame is invalid"));
        let trailing = match response {
            Ok(response) => {
                let mut byte = [0_u8; 1];
                match stdout.read(&mut byte) {
                    Ok(0) => Ok(response),
                    Ok(_) => Err(anyhow!("internal worker wrote trailing stdout bytes")),
                    Err(error) => Err(error.into()),
                }
            }
            Err(error) => Err(error),
        };
        let _ = response_tx.send(trailing);
    });
    std::thread::spawn(move || {
        let _ = drain_bounded(stderr, MAX_WORKER_STDERR_BYTES);
    });

    let status = wait_for_exit(&mut child, config.timeout)?;
    if !status.success() {
        bail!("internal worker exited unsuccessfully");
    }
    response_rx
        .recv_timeout(Duration::from_millis(250))
        .map_err(|_| anyhow!("internal worker did not finish its response frame"))?
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> Result<std::process::ExitStatus> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("internal worker timed out");
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn drain_bounded(mut reader: impl Read, limit: usize) -> io::Result<()> {
    let mut retained = 0_usize;
    let mut buffer = [0_u8; 4096];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        retained = retained.saturating_add(read);
        if retained > limit {
            // Keep draining to avoid deadlock, but intentionally discard all
            // bytes. Worker stderr can contain provider/server text and is
            // never a controller diagnostic surface.
            retained = limit;
        }
    }
}

fn validate_executable(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!("internal worker must be an absolute executable path");
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("internal worker executable is invalid");
    }
    #[cfg(unix)]
    if std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o111 == 0 {
        bail!("internal worker executable is not executable");
    }
    Ok(())
}

fn validate_private_directory(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("internal worker directory is invalid");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            bail!("internal worker directory is not owner-only");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning_experiment_protocol::{
        ExperimentModelRouteV1, ExperimentPhase, ProviderCallSpecV1, CHEAP_MODEL,
        MAX_INPUT_TOKENS_PER_REQUEST, MAX_OUTPUT_TOKENS_PER_REQUEST, PROVIDER_CALL_SPEC_SCHEMA_V1,
    };
    use sha2::{Digest, Sha256};

    fn digest(byte: char) -> String {
        byte.to_string().repeat(64)
    }

    fn call() -> ProviderCallSpecV1 {
        let request_json = format!(
            r#"{{"model":"{CHEAP_MODEL}","input":[{{"role":"user","content":"fixture"}}],"stream":true,"reasoning":{{"effort":"high"}},"include":["reasoning.encrypted_content"],"max_output_tokens":8192,"store":false,"service_tier":"default"}}"#
        );
        ProviderCallSpecV1 {
            schema_version: PROVIDER_CALL_SPEC_SCHEMA_V1.into(),
            call_id: "provider-call-1".into(),
            channel_id: "channel-1".into(),
            nonce: "nonce-1".into(),
            worker_spec_sha256: digest('a'),
            controller_sha256: digest('b'),
            worker_sha256: digest('c'),
            experiment_id: "pilot-1".into(),
            preregistration_sha256: digest('d'),
            trial_id: "trial-1".into(),
            slot_index: 0,
            phase: ExperimentPhase::Executor,
            semantic_request_index: 0,
            physical_attempt: 1,
            route: ExperimentModelRouteV1::openai(CHEAP_MODEL),
            max_input_tokens: MAX_INPUT_TOKENS_PER_REQUEST,
            max_output_tokens: MAX_OUTPUT_TOKENS_PER_REQUEST,
            store: false,
            request_sha256: hex(&Sha256::digest(request_json.as_bytes())),
            request_json,
            x_client_request_id: "test-client-request-id".into(),
        }
    }

    fn private_directory() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        directory
    }

    #[test]
    fn local_cat_round_trips_one_complete_frame() {
        let directory = private_directory();
        let call = call();
        let received: ProviderCallSpecV1 = invoke_framed_child(
            FramedChildConfig {
                program: Path::new("/bin/cat"),
                arguments: &[],
                current_dir: directory.path(),
                request_limit: MAX_PROVIDER_CALL_SPEC_FRAME_BYTES,
                response_limit: MAX_PROVIDER_CALL_SPEC_FRAME_BYTES,
                timeout: Duration::from_secs(2),
                scrub_environment: true,
            },
            &call,
        )
        .unwrap();
        assert_eq!(received, call);
    }

    #[test]
    fn nonzero_child_never_yields_a_typed_receipt() {
        let directory = private_directory();
        let call = call();
        let result = invoke_framed_child::<ProviderCallSpecV1, ProviderCallSpecV1>(
            FramedChildConfig {
                program: Path::new("/usr/bin/false"),
                arguments: &[],
                current_dir: directory.path(),
                request_limit: MAX_PROVIDER_CALL_SPEC_FRAME_BYTES,
                response_limit: MAX_PROVIDER_CALL_SPEC_FRAME_BYTES,
                timeout: Duration::from_secs(2),
                scrub_environment: true,
            },
            &call,
        );
        assert!(result.is_err());
    }

    fn hex(bytes: &[u8]) -> String {
        const TABLE: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            output.push(TABLE[(byte >> 4) as usize] as char);
            output.push(TABLE[(byte & 0x0f) as usize] as char);
        }
        output
    }
}
