//! An independent supervisor owns the VM. Dropping the owner closes its lease
//! pipe, including when the owner is killed before Rust destructors can run.
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::FrozenHarnessVm;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;

const SUPERVISOR: &str = include_str!("supervisor.py");

pub(crate) struct VmLease {
    frozen: FrozenHarnessVm,
    name: String,
    receipt_directory: PathBuf,
    child: tokio::process::Child,
    input: Option<tokio::process::ChildStdin>,
    deadline: Instant,
    _ownership: super::ownership::VmOwnership,
}

impl Drop for VmLease {
    fn drop(&mut self) {
        // Do not kill the supervisor: it must survive owner cancellation and
        // finish whole-VM cleanup. It also has its own fixed hard deadline.
        self.input.take();
    }
}

fn events(root: &Path) -> Result<Vec<serde_json::Value>> {
    let path = root.join("events.jsonl");
    if std::fs::metadata(&path)?.len() > 32 * 1024 {
        bail!("VM supervisor receipt exceeded its bound");
    }
    let bytes = std::fs::read(path)?;
    if bytes.len() > 32 * 1024 {
        bail!("VM supervisor receipt exceeded its bound");
    }
    // Only complete lines are observable while the supervisor is writing.
    let complete = bytes.iter().rposition(|b| *b == b'\n');
    let Some(end) = complete else {
        return Ok(Vec::new());
    };
    let rows = bytes[..end]
        .split(|b| *b == b'\n')
        .map(serde_json::from_slice)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if rows.len() > 2 {
        bail!("VM supervisor emitted unexpected events");
    }
    Ok(rows)
}

impl VmLease {
    pub(crate) async fn start(frozen: &FrozenHarnessVm, allowance: Duration) -> Result<Self> {
        if allowance.as_secs() == 0 || allowance > Duration::from_secs(7200) {
            bail!("invalid VM lease allowance");
        }
        let deadline = Instant::now() + allowance;
        frozen.validate()?;
        let ownership = super::ownership::VmOwnership::acquire(&frozen.profile.home)?;
        let checked = frozen.clone();
        tokio::task::spawn_blocking(move || super::verify(&checked, deadline)).await??;
        let directory = tempfile::Builder::new()
            .prefix("ca-harness-vm-lease-")
            .tempdir()?;
        let out = std::fs::File::create(directory.path().join("events.jsonl"))?;
        let err = std::fs::File::create(directory.path().join("supervisor.log"))?;
        let mut command = tokio::process::Command::new("/usr/bin/python3");
        command
            .args(["-I", "-B", "-c", SUPERVISOR])
            .env_clear()
            .env("HOME", directory.path())
            .env("TMPDIR", directory.path())
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .current_dir(directory.path())
            .stdin(Stdio::piped())
            .stdout(out)
            .stderr(err)
            .kill_on_drop(false)
            .process_group(0);
        ownership.inherit(&mut command);
        let mut child = command.spawn().context("start dedicated VM supervisor")?;
        // Both retain the shared flock: the parent through final verification,
        // the supervisor through cleanup after parent death or cancellation.
        // Never explicitly LOCK_UN, which would release both copies.
        let input = child.stdin.take().context("VM lease input missing")?;
        // Retain the tiny receipt even if the caller disappears. No secret or
        // candidate output is written here. The supervisor never depends on a
        // temporary directory that its dying owner can remove.
        let receipt_directory = directory.keep();
        let mut lease = Self {
            frozen: frozen.clone(),
            name: String::new(),
            receipt_directory,
            child,
            input: Some(input),
            deadline,
            _ownership: ownership,
        };
        let seconds = deadline.saturating_duration_since(Instant::now()).as_secs();
        if seconds == 0 {
            bail!("VM lease budget exhausted before startup");
        }
        let mut config = serde_json::to_vec(&serde_json::json!({
            "profile": frozen.profile, "files": frozen.files, "seconds": seconds,
        }))?;
        if config.len() > 65536 {
            bail!("VM lease configuration exceeded its bound");
        }
        config.push(b'\n');
        tokio::time::timeout(
            Duration::from_secs(5),
            lease.input.as_mut().unwrap().write_all(&config),
        )
        .await??;
        let startup_deadline = deadline.min(Instant::now() + Duration::from_secs(250));
        loop {
            let rows = events(&lease.receipt_directory)?;
            if let Some(first) = rows.first() {
                if first.get("ready").and_then(|v| v.as_bool()) != Some(true) {
                    bail!(
                        "VM supervisor failed before startup; receipt: {}",
                        lease.receipt_directory.display()
                    );
                }
                let name = first
                    .get("name")
                    .and_then(|v| v.as_str())
                    .context("VM supervisor name missing")?;
                let suffix = name
                    .strip_prefix("ca-harness-run-")
                    .context("invalid disposable VM name")?;
                if suffix.len() != 32
                    || !suffix
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    bail!("invalid disposable VM name");
                }
                lease.name = name.to_owned();
                if rows.len() != 1 || lease.child.try_wait()?.is_some() {
                    bail!("VM supervisor exited during startup");
                }
                lease.attest_guest().await?;
                return Ok(lease);
            }
            if lease.child.try_wait()?.is_some() {
                bail!("VM supervisor ended without readiness");
            }
            if Instant::now() >= startup_deadline {
                bail!("VM supervisor startup deadline exhausted");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn receipts(&self) -> &Path {
        &self.receipt_directory
    }
    pub(crate) fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    async fn attest_guest(&self) -> Result<()> {
        let deadline = self.deadline.min(Instant::now() + Duration::from_secs(120));
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("VM guest control service did not become ready");
            }
            let command = self.guest_command(&[
                "/usr/bin/python3", "-I", "-c",
                "import hashlib; print(hashlib.sha256(open('/opt/ca-harness/tart-guest-agent-host-only','rb').read()).hexdigest())",
            ])?;
            let result = crate::harness_engineering::execute_capture(
                command,
                remaining.min(Duration::from_secs(15)),
            )
            .await;
            if let Ok(output) = result {
                if output.passed {
                    if output.out.1.trim() != self.frozen.profile.guest_agent_sha256 {
                        bail!("VM guest control agent differs from its frozen identity");
                    }
                    return Ok(());
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// Only host-owned fixed operations may call this. Provider tool requests
    /// execute through the guest worker, never through the administrative RPC.
    pub(crate) fn guest_command(&self, arguments: &[&str]) -> Result<tokio::process::Command> {
        self.guest_command_mode(arguments, false)
    }

    pub(crate) fn guest_command_with_input(
        &self,
        arguments: &[&str],
    ) -> Result<tokio::process::Command> {
        self.guest_command_mode(arguments, true)
    }

    fn guest_command_mode(
        &self,
        arguments: &[&str],
        input: bool,
    ) -> Result<tokio::process::Command> {
        if self.remaining().is_zero() {
            bail!("VM lease deadline exhausted");
        }
        let mut command = tokio::process::Command::new(&self.frozen.profile.tart);
        command.arg("exec");
        if input {
            command.arg("-i");
        }
        command
            .arg(&self.name)
            .args(arguments)
            .env_clear()
            .env("TART_HOME", &self.frozen.profile.home)
            .env("HOME", &self.receipt_directory)
            .env("TMPDIR", &self.receipt_directory)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .current_dir(&self.receipt_directory)
            .kill_on_drop(true)
            .process_group(crate::jobs::owned_child_process_group());
        Ok(command)
    }

    pub(crate) async fn finish(mut self) -> Result<PathBuf> {
        self.input.take();
        let status = tokio::time::timeout(Duration::from_secs(100), self.child.wait()).await??;
        let rows = events(&self.receipt_directory)?;
        let final_row = rows.last().context("VM cleanup receipt missing")?;
        if !status.success()
            || rows.len() != 2
            || final_row.get("stopped").and_then(|v| v.as_bool()) != Some(true)
            || final_row.get("name").and_then(|v| v.as_str()) != Some(self.name.as_str())
            || final_row.get("error") != Some(&serde_json::Value::Null)
        {
            bail!(
                "VM cleanup failed; receipt: {}",
                self.receipt_directory.display()
            );
        }
        let frozen = self.frozen.clone();
        tokio::task::spawn_blocking(move || {
            super::verify(&frozen, Instant::now() + Duration::from_secs(30))
        })
        .await??;
        Ok(self.receipt_directory.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mock_profile(root: &Path) -> codex_team::self_improvement::HarnessVmProfile {
        let home = root.canonicalize().unwrap();
        let template = home.join("vms/base");
        std::fs::create_dir_all(&template).unwrap();
        for name in ["config.json", "disk.img", "nvram.bin"] {
            std::fs::write(template.join(name), b"frozen mock input").unwrap();
        }
        let tart = home.join("tart");
        let mock = include_str!("test_supervisor.py")
            .split("MOCK = '''")
            .nth(1)
            .unwrap()
            .split("'''")
            .next()
            .unwrap();
        std::fs::write(&tart, mock).unwrap();
        std::fs::set_permissions(&tart, std::fs::Permissions::from_mode(0o700)).unwrap();
        codex_team::self_improvement::HarnessVmProfile {
            tart,
            home,
            template: "base".into(),
            guest_agent_sha256: "a".repeat(64),
        }
    }

    #[tokio::test]
    async fn hrs253_vm_rust_lease_rejects_wrong_guest_agent_before_admission() {
        let root = tempfile::tempdir().unwrap();
        let mut profile = mock_profile(root.path());
        profile.guest_agent_sha256 = "b".repeat(64);
        let frozen = super::super::freeze(&profile, 30).unwrap();
        let attempt = VmLease::start(&frozen, Duration::from_secs(30)).await;
        let rejected = attempt.is_err();
        drop(attempt);
        tokio::time::timeout(Duration::from_secs(10), async {
            while std::fs::read_dir(profile.home.join("vms")).unwrap().count() != 1 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(rejected, "guest agent drift was admitted");
    }

    #[tokio::test]
    async fn hrs253_vm_rust_lease_finishes_and_drop_stops_its_vm() {
        let root = tempfile::tempdir().unwrap();
        let profile = mock_profile(root.path());
        let frozen = super::super::freeze(&profile, 30).unwrap();
        let lease = VmLease::start(&frozen, Duration::from_secs(30))
            .await
            .unwrap();
        let name = lease.name().to_owned();
        assert!(profile.home.join("vms").join(&name).exists());
        assert!(super::super::ownership::VmOwnership::acquire(&profile.home).is_err());
        let receipts = lease.finish().await.unwrap();
        assert_eq!(events(&receipts).unwrap().len(), 2);
        assert!(!profile.home.join("vms").join(name).exists());
        assert!(super::super::ownership::VmOwnership::acquire(&profile.home).is_ok());

        let lease = VmLease::start(&frozen, Duration::from_secs(30))
            .await
            .unwrap();
        let name = lease.name().to_owned();
        let receipts = lease.receipts().to_owned();
        drop(lease);
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let rows = events(&receipts).unwrap();
                if rows.len() == 2 {
                    assert_eq!(rows[1]["stopped"], true);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(!profile.home.join("vms").join(name).exists());
        assert!(profile.home.join("vms/base").exists());
    }

    #[tokio::test]
    async fn hrs253_vm_supervisor_owns_cancellation_and_failed_cleanup() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("supervisor.py"), SUPERVISOR).unwrap();
        std::fs::write(
            directory.path().join("test_supervisor.py"),
            include_str!("test_supervisor.py"),
        )
        .unwrap();
        let mut command = tokio::process::Command::new("/usr/bin/python3");
        command
            .args(["-I", "-B"])
            .arg(directory.path().join("test_supervisor.py"));
        let output = crate::harness_engineering::execute_capture(command, Duration::from_secs(60))
            .await
            .unwrap();
        assert!(output.passed, "{}", output.err.1);
    }

    #[tokio::test]
    #[ignore = "boots and removes one disposable VM from an explicitly supplied image binding"]
    async fn hrs253_vm_rust_lease_controls_real_disposable_guest() {
        let binding = std::env::var_os("CA_HARNESS_VM_TEST_BINDING")
            .expect("explicit previously hashed VM binding required");
        let mut frozen: FrozenHarnessVm =
            serde_json::from_slice(&std::fs::read(binding).unwrap()).unwrap();
        frozen.validate().unwrap();
        // This is a new controller fixture, not a saved experiment plan. Reuse
        // the already-hashed image inventory only if its exact metadata still
        // verifies, while binding this fixture to the controller under test.
        frozen.adapter_digest = env!("CA_HARNESS_ADAPTER_DIGEST").into();
        let lease = VmLease::start(&frozen, Duration::from_secs(240))
            .await
            .unwrap();
        assert!(super::super::ownership::VmOwnership::acquire(&frozen.profile.home).is_err());
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let command = lease.guest_command(&["/usr/bin/true"]).unwrap();
            let result =
                crate::harness_engineering::execute_capture(command, Duration::from_secs(15)).await;
            if result.is_ok_and(|output| output.passed) {
                break;
            }
            assert!(Instant::now() < deadline, "guest RPC did not become ready");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let command = lease.guest_command(&["/usr/bin/python3", "-I", "-c",
            "import hashlib; print(hashlib.sha256(open('/opt/ca-harness/tart-guest-agent-host-only','rb').read()).hexdigest())"
        ]).unwrap();
        let output = crate::harness_engineering::execute_capture(command, Duration::from_secs(15))
            .await
            .unwrap();
        assert!(output.passed, "{}", output.err.1);
        assert_eq!(output.out.1.trim(), frozen.profile.guest_agent_sha256);
        let receipt = lease.finish().await.unwrap();
        assert!(super::super::ownership::VmOwnership::acquire(&frozen.profile.home).is_ok());
        eprintln!("RUST_VM_LEASE_RECEIPT={}", receipt.display());
    }
}
