//! A fresh VM verifies imported task bytes with the pinned baseline executable.
use super::lease::VmLease;
use super::transfer::{install_tree, install_worker, package_tree, InputKind};
use crate::harness_cases::FrozenCodingCase;
use crate::harness_verifier::{run_verifier_command, VerifiedCodingResult};
use anyhow::{bail, Result};
use codex_team::self_improvement::FrozenHarnessVm;
use std::path::Path;
use std::time::{Duration, Instant};

const SCRIPT: &str = include_str!("verifier.py");

/// Caller pins the trusted baseline and binds its hash before this call. This
/// operation never reuses a worker VM, so detached worker children cannot
/// observe verification or forge its completion frame.
pub(crate) async fn verify_workspace(
    frozen: &FrozenHarnessVm,
    baseline: &Path,
    expected_binary: &str,
    workspace: &Path,
    case: &FrozenCodingCase,
    allowance: Duration,
) -> Result<VerifiedCodingResult> {
    if allowance.is_zero() || allowance > Duration::from_secs(7200) {
        bail!("invalid VM verification allowance");
    }
    let deadline = Instant::now() + allowance;
    let (root, input) = (workspace.to_owned(), case.clone());
    let (package, expected) = tokio::task::spawn_blocking(move || -> Result<_> {
        let digest = input.check_edits(&root)?;
        let package = package_tree(&root, deadline)?;
        if input.check_edits(&root)? != digest {
            bail!("verification input changed during packaging");
        }
        Ok((package, digest))
    })
    .await??;
    let lease = VmLease::start(frozen, deadline.saturating_duration_since(Instant::now())).await?;
    let operation = async {
        install_tree(&lease, &package, InputKind::Workspace).await?;
        install_worker(&lease, baseline, expected_binary).await?;
        let command = lease.guest_command_with_input(&[
            "/usr/bin/sudo",
            "-n",
            "/usr/bin/python3",
            "-I",
            "-c",
            SCRIPT,
        ])?;
        run_verifier_command(
            command,
            Path::new("/opt/ca-harness/verification"),
            Path::new("/opt/ca-harness/task"),
            case,
            &expected,
            lease.remaining(),
        )
        .await
    }
    .await;
    let cleanup = lease.finish().await;
    let result = operation?;
    cleanup?;
    if Instant::now() >= deadline {
        bail!("VM verification allowance exhausted during cleanup");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires matching fresh ca and frozen VM; verifies all six buggy and repaired fixtures in separate guests"]
    async fn hrs253_vm_baseline_verifier_distinguishes_bugs_and_repairs() {
        let binding = std::env::var_os("CA_HARNESS_VM_TEST_BINDING").expect("explicit VM binding");
        let mut frozen: FrozenHarnessVm =
            serde_json::from_slice(&std::fs::read(binding).unwrap()).unwrap();
        frozen.validate().unwrap();
        frozen.adapter_digest = env!("CA_HARNESS_ADAPTER_DIGEST").into();
        let binary = std::path::PathBuf::from(
            std::env::var_os("CA_HARNESS_TEST_BINARY").expect("matching baseline binary"),
        );
        let digest = crate::harness_engineering::artifact_digest(&binary).unwrap();
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        for case in crate::harness_cases::freeze_reasoning_cases(repository).unwrap() {
            for repaired in [false, true] {
                let owner = tempfile::tempdir().unwrap();
                let workspace = owner.path().join("task");
                case.materialize(&workspace).unwrap();
                if repaired {
                    for name in &case.allowed_paths {
                        std::fs::copy(
                            repository
                                .join("tests/reasoning_pilot_golden")
                                .join(&case.fixture_id)
                                .join(name),
                            workspace.join(name),
                        )
                        .unwrap();
                    }
                }
                let result = verify_workspace(
                    &frozen,
                    &binary,
                    &digest,
                    &workspace,
                    &case,
                    Duration::from_secs(120),
                )
                .await
                .unwrap();
                assert!(
                    result.public_passed,
                    "{} repaired={repaired}: {result:?}",
                    case.fixture_id
                );
                assert_eq!(
                    result.hidden_passed, repaired,
                    "{}: {result:?}",
                    case.fixture_id
                );
                eprintln!("VM_VERIFIED={} repaired={repaired}", case.fixture_id);
            }
        }
    }
}
