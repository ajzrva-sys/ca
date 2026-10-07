//! The baseline host binary owns verification; candidate worker stdout cannot
//! report a pass. This separate process gives compilation a killable deadline.
use crate::harness_cases::FrozenCodingCase;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const VERIFIER_FLAG: &str = "--internal-harness-verifier";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifyRequest {
    case: FrozenCodingCase,
    repository: PathBuf,
    workspace: PathBuf,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedCodingResult {
    pub case_id: String,
    pub verifier_digest: String,
    pub implementation_digest: String,
    pub public_passed: bool,
    pub hidden_passed: bool,
    pub failure: Option<String>,
    pub build_diagnostic: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum VerifyReply {
    Verified { result: VerifiedCodingResult },
    InfrastructureFailure { reason: String },
}

fn verify_request(request: VerifyRequest) -> Result<VerifiedCodingResult> {
    let implementation_digest = request
        .case
        .check_edits(&request.workspace)
        .context("inspect frozen trial inputs")?;
    let result = crate::harness_cases::verifier::verify_materialized_fixture_with_profile(
        &request.repository,
        &request.case.fixture_id,
        "trial",
        &request.workspace,
        |workspace, temporary, forbidden| {
            let mut read_only_roots = Vec::new();
            if Path::new("/opt/homebrew").is_dir() {
                read_only_roots.push("/opt/homebrew".into());
            }
            let policy = crate::sandbox::SandboxPolicy {
                roots: vec![workspace.into()],
                read_only_roots,
                temporary_directory: temporary.into(),
                network_allowed: false,
                hide_other_processes: true,
            };
            let mut profile =
                crate::sandbox::sandbox_profile(&policy).map_err(|error| error.to_string())?;
            crate::sandbox::append_denied_path_rules(&mut profile, forbidden)
                .map_err(|error| error.to_string())?;
            Ok(profile)
        },
    )
    .map_err(anyhow::Error::msg)
    .context("execute host fixture checks")?;
    if request.case.check_edits(&request.workspace)? != implementation_digest {
        bail!("candidate source changed during verification");
    }
    Ok(VerifiedCodingResult {
        case_id: request.case.case.id,
        verifier_digest: request.case.case.verifier_digest,
        implementation_digest,
        public_passed: result.public_passed,
        hidden_passed: result.hidden_passed,
        failure: result.hidden_failure_code,
        build_diagnostic: result
            .build_diagnostic
            .as_deref()
            .and_then(crate::learning::sanitized_learning_text),
    })
}

pub async fn run_stdio() -> i32 {
    if crate::jobs::inherit_harness_worker_containment().is_err() {
        return 2;
    }
    tokio::task::spawn_blocking(|| -> Result<()> {
        let operation = crate::harness_worker::read_frame(&mut std::io::stdin())
            .map_err(anyhow::Error::from)
            .and_then(verify_request);
        let reply = match operation {
            Ok(result) => VerifyReply::Verified { result },
            Err(error) => VerifyReply::InfrastructureFailure {
                reason: crate::learning::sanitized_learning_text(&format!("{error:#}"))
                    .unwrap_or_else(|| "host verifier failed".into())
                    .chars()
                    .take(1024)
                    .collect(),
            },
        };
        crate::harness_worker::write_frame(&mut std::io::stdout(), &reply)?;
        Ok(())
    })
    .await
    .map_or(1, |result| if result.is_ok() { 0 } else { 1 })
}

struct OwnedGroup(Option<i32>);
impl Drop for OwnedGroup {
    fn drop(&mut self) {
        if let Some(group) = self.0 {
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
        }
    }
}

pub async fn verify_coding_workspace(
    baseline_binary: &Path,
    repository: &Path,
    workspace: &Path,
    case: &FrozenCodingCase,
    deadline: Duration,
) -> Result<VerifiedCodingResult> {
    case.validate()?;
    if deadline.is_zero() || deadline > Duration::from_secs(7200) {
        bail!("invalid verification deadline");
    }
    let expected = case.check_edits(workspace)?;
    let binary = baseline_binary.canonicalize()?;
    let workspace = workspace.canonicalize()?;
    let repository = repository.canonicalize()?;
    let temporary = tempfile::tempdir()?;
    // This is the trusted baseline verifier, not candidate code. It applies the
    // required closed sandbox once to each compiler/probe subprocess; nesting
    // sandbox-exec inside a sandbox is rejected by macOS.
    let mut command = tokio::process::Command::new(binary);
    command
        .arg(VERIFIER_FLAG)
        .current_dir(&workspace)
        .env_clear()
        .env(
            "PATH",
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        )
        .env("TMPDIR", temporary.path())
        .env("CARGO_NET_OFFLINE", "true")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .process_group(crate::jobs::owned_child_process_group());
    if crate::jobs::worker_contained() {
        command.env(
            "CA_HARNESS_PARENT_GROUP",
            crate::jobs::owned_child_process_group().to_string(),
        );
    }
    // The inner host verifier explicitly denies this real home to compiled
    // candidate code and replaces it with its own empty home before execution.
    if let Some(home) = std::env::var_os("HOME") {
        command.env("HOME", home);
    }
    run_verifier_command(command, &repository, &workspace, case, &expected, deadline).await
}

/// Only the trusted baseline verifier may back this transport. A VM caller
/// passes fixed guest paths and the digest of its independently imported bytes.
pub(crate) async fn run_verifier_command(
    mut command: tokio::process::Command,
    repository: &Path,
    workspace: &Path,
    case: &FrozenCodingCase,
    expected: &str,
    deadline: Duration,
) -> Result<VerifiedCodingResult> {
    case.validate()?;
    if deadline.is_zero() || deadline > Duration::from_secs(7200) {
        bail!("invalid verification deadline");
    }
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .process_group(crate::jobs::owned_child_process_group());
    let mut child = command.spawn().context("launch host verifier")?;
    let pid = i32::try_from(child.id().context("missing verifier PID")?)?;
    let group = OwnedGroup((!crate::jobs::worker_contained()).then_some(pid));
    let mut input = child.stdin.take().context("missing verifier input")?;
    let mut output = child.stdout.take().context("missing verifier output")?;
    let mut frame = Vec::new();
    crate::harness_worker::write_frame(
        &mut frame,
        &VerifyRequest {
            case: case.clone(),
            repository: repository.into(),
            workspace: workspace.into(),
        },
    )?;
    let operation = async {
        input.write_all(&frame).await?;
        drop(input);
        let length = output.read_u32().await? as usize;
        if length == 0 || length > 64 * 1024 {
            bail!("invalid host verifier receipt size");
        }
        let mut bytes = vec![0; length];
        output.read_exact(&mut bytes).await?;
        let reply: VerifyReply = serde_json::from_slice(&bytes)?;
        let result = match reply {
            VerifyReply::Verified { result } => result,
            VerifyReply::InfrastructureFailure { reason } => {
                bail!("host verifier infrastructure failure: {reason}")
            }
        };
        let mut extra = [0];
        if output.read(&mut extra).await? != 0 || !child.wait().await?.success() {
            bail!("host verifier did not exit cleanly");
        }
        if result.case_id != case.case.id
            || result.verifier_digest != case.case.verifier_digest
            || result.implementation_digest != expected
        {
            bail!("host verifier receipt binding changed");
        }
        Ok(result)
    };
    let result = tokio::time::timeout(deadline, operation)
        .await
        .context("host verifier deadline exhausted")
        .and_then(|result| result);
    if result.is_err() {
        unsafe {
            libc::kill(group.0.map_or(pid, |owned| -owned), libc::SIGKILL);
        }
        let _ = child.wait().await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires CA_HARNESS_TEST_BINARY naming a freshly built ca and macOS sandbox"]
    async fn hrs253_real_host_verifier_distinguishes_all_six_bugs_from_repairs() {
        let binary =
            PathBuf::from(std::env::var_os("CA_HARNESS_TEST_BINARY").expect("fresh worker binary"));
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        let cases = crate::harness_cases::freeze_reasoning_cases(&repository).unwrap();
        for case in &cases {
            let temporary = tempfile::tempdir().unwrap();
            let workspace = temporary.path().canonicalize().unwrap().join("trial");
            case.materialize(&workspace).unwrap();
            let broken = verify_coding_workspace(
                &binary,
                &repository,
                &workspace,
                case,
                Duration::from_secs(60),
            )
            .await
            .unwrap();
            assert!(broken.public_passed, "{broken:?}");
            assert!(!broken.hidden_passed, "{broken:?}");
            assert!(broken.failure.is_some());
            for relative in &case.allowed_paths {
                std::fs::copy(
                    repository
                        .join("tests/reasoning_pilot_golden")
                        .join(&case.fixture_id)
                        .join(relative),
                    workspace.join(relative),
                )
                .unwrap();
            }
            let repaired = verify_coding_workspace(
                &binary,
                &repository,
                &workspace,
                case,
                Duration::from_secs(60),
            )
            .await
            .unwrap();
            assert!(
                repaired.public_passed && repaired.hidden_passed,
                "{repaired:?}"
            );
            assert!(repaired.failure.is_none());
            assert_ne!(broken.implementation_digest, repaired.implementation_digest);
        }
    }
}
