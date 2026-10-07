//! Fixed executable probes use a fresh guest per invocation, including recovery.
use super::lease::VmLease;
use crate::harness_engineering::{artifact_digest, execute_capture, CommandOutput};
use anyhow::{bail, Result};
use codex_team::self_improvement::FrozenHarnessVm;
use std::path::Path;
use std::time::{Duration, Instant};

const SCRIPT: &str = include_str!("probe.py");

pub(crate) async fn run(
    frozen: &FrozenHarnessVm,
    binary: &Path,
    flags: &[&str],
    allowance: Duration,
) -> Result<Vec<CommandOutput>> {
    if allowance.is_zero()
        || allowance > Duration::from_secs(7200)
        || !matches!(
            flags,
            ["--version", "--help"] | ["--internal-harness-build-identity"]
        )
    {
        bail!("invalid fixed VM executable probe");
    }
    let deadline = Instant::now() + allowance;
    let path = binary.to_owned();
    let digest = tokio::task::spawn_blocking(move || artifact_digest(&path)).await??;
    let mut profile = crate::sandbox::sandbox_profile(&crate::sandbox::SandboxPolicy {
        roots: vec!["/Users/ca-runner/harness-probe".into()],
        read_only_roots: vec!["/opt/homebrew".into()],
        temporary_directory: "/Users/ca-runner/harness-probe/tmp".into(),
        network_allowed: false,
        hide_other_processes: true,
    })?;
    profile.push_str("(allow file-read* (literal \"/opt/ca-harness/worker\"))\n");
    let mut outputs = Vec::new();
    for flag in flags {
        let lease =
            VmLease::start(frozen, deadline.saturating_duration_since(Instant::now())).await?;
        let result = async {
            super::transfer::install_worker(&lease, binary, &digest).await?;
            let command = lease.guest_command(&[
                "/usr/bin/sudo",
                "-n",
                "/usr/bin/python3",
                "-I",
                "-c",
                SCRIPT,
                flag,
                &profile,
            ])?;
            execute_capture(command, lease.remaining().min(Duration::from_secs(15))).await
        }
        .await;
        let cleanup = lease.finish().await;
        let output = result?;
        cleanup?;
        if Instant::now() >= deadline {
            bail!("VM executable probe allowance exhausted during cleanup");
        }
        if !output.passed || output.out.1.is_empty() {
            bail!("VM executable probe failed: {flag}");
        }
        outputs.push(output);
    }
    Ok(outputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires dedicated VM; proves fixed executable probes cannot run on host and bind exact bytes"]
    async fn hrs253_vm_executable_probes_run_in_fresh_unprivileged_guests() {
        let binding = std::env::var_os("CA_HARNESS_VM_TEST_BINDING").expect("VM binding");
        let mut frozen: FrozenHarnessVm =
            serde_json::from_slice(&std::fs::read(binding).unwrap()).unwrap();
        frozen.validate().unwrap();
        frozen.adapter_digest = env!("CA_HARNESS_ADAPTER_DIGEST").into();
        let owner = tempfile::tempdir().unwrap();
        let binary = owner.path().join("probe");
        std::fs::write(&binary, "#!/usr/bin/python3\nimport os,sys\nassert os.geteuid()==550\nassert os.environ['HOME']=='/Users/ca-runner/harness-probe/home'\nassert not os.path.exists('/Users/ca-runner/harness-probe/home/prior')\nopen('/Users/ca-runner/harness-probe/home/prior','w').close()\nprint('a'*64 if sys.argv[1]=='--internal-harness-build-identity' else 'fixture '+sys.argv[1])\n").unwrap();
        let outputs = run(
            &frozen,
            &binary,
            &["--version", "--help"],
            Duration::from_secs(120),
        )
        .await
        .unwrap();
        assert_eq!(outputs[0].out.1.trim(), "fixture --version");
        assert_eq!(outputs[1].out.1.trim(), "fixture --help");
        let identity = run(
            &frozen,
            &binary,
            &[crate::harness_activation::IDENTITY_FLAG],
            Duration::from_secs(90),
        )
        .await
        .unwrap();
        assert_eq!(identity[0].out.1.trim(), "a".repeat(64));
        assert!(
            run(&frozen, &binary, &["--execute"], Duration::from_secs(90))
                .await
                .is_err()
        );
    }
}
