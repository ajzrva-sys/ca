//! Fixed engineering gates execute as ca-runner in fresh disposable guests.
use super::lease::VmLease;
use super::transfer::{self, Artifact, ImportedArtifact, InputKind};
use crate::harness_engineering::{artifact_digest, execute_capture, spec_digest, GateReceipt};
use anyhow::{bail, Context, Result};
use codex_team::release_readiness::{fixed_release_gates, FixedGate};
use codex_team::self_improvement::{canonical_digest, FrozenHarnessVm};
use std::collections::BTreeMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::time::{Duration, Instant};

const SCRIPT: &str = include_str!("gate.py");

fn binary_gate(gate: &FixedGate) -> Option<Artifact> {
    match gate.program {
        "target/release/ca" => Some(Artifact::Ca),
        "target/release/codex-team" => Some(Artifact::CodexTeam),
        _ => None,
    }
}
fn builds_artifacts(gate: &FixedGate) -> bool {
    matches!(gate.id, "release_default" | "release_all_features")
}

fn validate_gate(gate: &FixedGate, seconds: u64) -> Result<()> {
    let expected = fixed_release_gates()
        .iter()
        .find(|g| g.id == gate.id)
        .context("unknown VM engineering gate")?;
    if spec_digest(expected)? != spec_digest(gate)?
        || seconds == 0
        || seconds > gate.timeout_seconds
    {
        bail!("VM engineering gate differs from its fixed specification or allowance");
    }
    Ok(())
}

/// Publish only into the already-audited private checkout, without executing
/// imported bytes. Protected parents and same-filesystem staging prevent links
/// or interrupted copies from redirecting the publication.
fn publish(workspace: &Path, name: Artifact, imported: ImportedArtifact) -> Result<String> {
    let workspace = workspace.canonicalize()?;
    let mut parent = workspace;
    for component in ["target", "release"] {
        parent.push(component);
        match std::fs::create_dir(&parent) {
            Ok(()) => std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700))?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
        let metadata = std::fs::symlink_metadata(&parent)?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o022 != 0
        {
            bail!("VM artifact destination directory is not protected");
        }
    }
    let destination = parent.join(name.name());
    match std::fs::symlink_metadata(&destination) {
        Ok(m) if !m.is_file() || m.nlink() != 1 => {
            bail!("VM artifact destination is not a private regular file")
        }
        Ok(_) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    let mut staged = tempfile::NamedTempFile::new_in(&parent)?;
    let copied = std::io::copy(&mut imported.file.reopen()?, staged.as_file_mut())?;
    if copied != imported.bytes || artifact_digest(staged.path())? != imported.sha256 {
        bail!("VM artifact changed before publication");
    }
    staged
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o700))?;
    staged.as_file().sync_all()?;
    staged.persist(destination)?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(imported.sha256)
}

pub(crate) async fn run_gate(
    frozen: &FrozenHarnessVm,
    workspace: &Path,
    gate: &FixedGate,
    seconds: u64,
) -> Result<GateReceipt> {
    validate_gate(gate, seconds)?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let source = workspace.to_owned();
    let package =
        tokio::task::spawn_blocking(move || transfer::package_tree(&source, deadline)).await??;
    let lease = VmLease::start(frozen, deadline.saturating_duration_since(Instant::now())).await?;
    let operation = async {
        transfer::install_tree(&lease, &package, InputKind::Source).await?;
        let mut artifact_digests = BTreeMap::new();
        if let Some(name) = binary_gate(gate) {
            let path = workspace.join("target/release").join(name.name());
            let observed = path.clone();
            let digest = tokio::task::spawn_blocking(move || artifact_digest(&observed)).await??;
            transfer::install_artifact(&lease, &path, &digest, name).await?;
            artifact_digests.insert(name.name().to_owned(), digest);
        }
        let packet = serde_json::to_string(
            &serde_json::json!({"program":gate.program,"arguments":gate.args}),
        )?;
        let command = lease.guest_command(&[
            "/usr/bin/sudo",
            "-n",
            "/usr/bin/python3",
            "-I",
            "-c",
            SCRIPT,
            &packet,
        ])?;
        let output = execute_capture(command, lease.remaining()).await?;
        let output_digest = canonical_digest(&(&output.out.0, &output.err.0))?;
        let diagnostic = if output.passed {
            None
        } else {
            Some(crate::harness_engineering::failure_diagnostic(&output))
        };
        let mut imports = Vec::new();
        if output.passed && builds_artifacts(gate) {
            for name in [Artifact::Ca, Artifact::CodexTeam] {
                imports.push((name, transfer::fetch_artifact(&lease, name).await?));
            }
        }
        Ok::<_, anyhow::Error>((
            GateReceipt {
                spec_digest: spec_digest(gate)?,
                passed: output.passed,
                output_digest,
                diagnostic,
                artifact_digests,
            },
            imports,
        ))
    }
    .await;
    // Cleanup must be confirmed before accepting any gate or publishing output.
    // On cancellation Drop still closes the independent supervisor's lease.
    let cleanup = lease.finish().await;
    let (mut receipt, imports) = operation?;
    cleanup?;
    for (name, imported) in imports {
        let destination = workspace.to_owned();
        let digest =
            tokio::task::spawn_blocking(move || publish(&destination, name, imported)).await??;
        receipt.artifact_digests.insert(name.name().into(), digest);
    }
    if Instant::now() >= deadline {
        bail!("VM engineering gate exhausted its total allowance");
    }
    Ok(receipt)
}

pub(crate) fn validate_receipts(
    vm: bool,
    receipts: &BTreeMap<String, GateReceipt>,
    admitted: Option<&str>,
) -> Result<()> {
    if !vm {
        return Ok(());
    }
    if receipts
        .keys()
        .any(|id| !fixed_release_gates().iter().any(|gate| gate.id == id))
    {
        bail!("unknown VM gate receipt");
    }
    let mut artifacts = BTreeMap::new();
    let mut missing = false;
    let mut failed = false;
    for gate in fixed_release_gates() {
        let Some(receipt) = receipts.get(gate.id) else {
            missing = true;
            continue;
        };
        if missing || failed {
            bail!("VM gate receipts are not a fixed-matrix prefix");
        }
        failed = !receipt.passed;
        if receipt.artifact_digests.values().any(|value| {
            value.len() != 64
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }) {
            bail!("invalid VM artifact receipt digest");
        }
        if builds_artifacts(gate) && receipt.passed {
            if receipt.artifact_digests.len() != 2
                || !receipt.artifact_digests.contains_key("ca")
                || !receipt.artifact_digests.contains_key("codex-team")
            {
                bail!("VM release artifact receipt missing");
            }
            artifacts = receipt.artifact_digests.clone();
        } else if let Some(name) = binary_gate(gate) {
            if receipt.artifact_digests.len() != 1
                || artifacts.get(name.name()) != receipt.artifact_digests.get(name.name())
            {
                bail!("VM smoke gate did not use the released artifact");
            }
        } else if !receipt.artifact_digests.is_empty() {
            bail!("unexpected VM gate artifact receipt");
        }
    }
    if let Some(digest) = admitted {
        if missing || failed {
            bail!("VM artifact admitted before the complete engineering matrix");
        }
        if artifacts.get("ca").map(String::as_str) != Some(digest) {
            bail!("VM admitted artifact differs from its engineering receipts");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::Write;

    fn receipt(gate: &FixedGate) -> GateReceipt {
        let mut artifact_digests = BTreeMap::new();
        if builds_artifacts(gate) {
            artifact_digests.insert("ca".into(), "a".repeat(64));
            artifact_digests.insert("codex-team".into(), "b".repeat(64));
        } else if let Some(name) = binary_gate(gate) {
            artifact_digests.insert(
                name.name().into(),
                if name.name() == "ca" { "a" } else { "b" }.repeat(64),
            );
        }
        GateReceipt {
            spec_digest: spec_digest(gate).unwrap(),
            passed: true,
            output_digest: "c".repeat(64),
            diagnostic: None,
            artifact_digests,
        }
    }

    #[test]
    fn hrs253_vm_engineering_receipts_bind_smokes_to_release_artifacts() {
        let receipts: BTreeMap<_, _> = fixed_release_gates()
            .iter()
            .map(|g| (g.id.into(), receipt(g)))
            .collect();
        validate_receipts(true, &receipts, Some(&"a".repeat(64))).unwrap();
        assert!(validate_receipts(true, &receipts, Some(&"f".repeat(64))).is_err());
        let mut altered = receipts.clone();
        altered
            .get_mut("ca_help")
            .unwrap()
            .artifact_digests
            .insert("ca".into(), "f".repeat(64));
        assert!(validate_receipts(true, &altered, None).is_err());
        let mut altered = receipts.clone();
        altered.remove("format");
        assert!(validate_receipts(true, &altered, None).is_err());
        let mut altered = receipts.clone();
        altered.get_mut("format").unwrap().passed = false;
        assert!(validate_receipts(true, &altered, None).is_err());
        let mut altered = receipts;
        altered
            .get_mut("release_all_features")
            .unwrap()
            .artifact_digests
            .clear();
        assert!(validate_receipts(true, &altered, None).is_err());
        let legacy = receipt(&fixed_release_gates()[0]);
        let json = serde_json::to_value(&legacy).unwrap();
        assert!(json.get("artifact_digests").is_none());
        let restored: GateReceipt = serde_json::from_value(json).unwrap();
        assert!(restored.artifact_digests.is_empty());
    }

    fn imported(bytes: &[u8]) -> ImportedArtifact {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(bytes).unwrap();
        ImportedArtifact {
            file,
            sha256: format!("{:x}", Sha256::digest(bytes)),
            bytes: bytes.len() as u64,
        }
    }

    #[test]
    fn hrs253_vm_artifact_publication_rejects_links_and_changed_bytes() {
        let root = tempfile::tempdir().unwrap();
        let digest = publish(root.path(), Artifact::Ca, imported(b"opaque bytes")).unwrap();
        assert_eq!(digest, format!("{:x}", Sha256::digest(b"opaque bytes")));
        assert_eq!(
            std::fs::read(root.path().join("target/release/ca")).unwrap(),
            b"opaque bytes"
        );
        let mut changed = imported(b"replacement");
        changed.sha256 = "0".repeat(64);
        assert!(publish(root.path(), Artifact::Ca, changed).is_err());
        assert_eq!(
            std::fs::read(root.path().join("target/release/ca")).unwrap(),
            b"opaque bytes"
        );
        let other = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), other.path().join("target")).unwrap();
        assert!(publish(other.path(), Artifact::Ca, imported(b"bad")).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        let fake = FixedGate {
            program: "/bin/sh",
            ..fixed_release_gates()[0]
        };
        assert!(validate_gate(&fake, 30).is_err());
    }

    fn frozen_fixture() -> FrozenHarnessVm {
        let path =
            std::env::var_os("CA_HARNESS_VM_TEST_BINDING").expect("explicit VM binding required");
        let mut frozen: FrozenHarnessVm =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        frozen.validate().unwrap();
        frozen.adapter_digest = env!("CA_HARNESS_ADAPTER_DIGEST").into();
        frozen
    }

    #[tokio::test]
    #[ignore = "runs the complete fixed engineering matrix in fresh macOS VMs using a tiny project"]
    async fn hrs253_vm_real_fixed_matrix_compiles_and_smokes_only_in_guest() {
        let frozen = frozen_fixture();
        let root = tempfile::Builder::new()
            .prefix("ca-vm-matrix-")
            .tempdir()
            .unwrap()
            .keep();
        let repository = root.join("repository");
        crate::harness_engineering::tests::source(&repository);
        let program = r#"fn main() {
    unsafe extern "C" { fn geteuid() -> u32; }
    assert_ne!(unsafe { geteuid() }, 0);
    assert!(std::env::var("HOME").unwrap().starts_with("/Users/ca-runner/harness-"));
    let source = "/opt/ca-harness/project/src/bin/ca.rs";
    assert!(std::fs::OpenOptions::new().write(true).open(source).is_err());
    println!("VM-only fixture");
}
#[test]
fn guest_account_and_source_protection() { main(); }
"#;
        for name in ["ca", "codex-team"] {
            let path = repository.join(format!("src/bin/{name}.rs"));
            std::fs::write(&path, program).unwrap();
            assert!(std::process::Command::new("/opt/homebrew/bin/rustfmt")
                .arg(path)
                .status()
                .unwrap()
                .success());
        }
        assert!(std::process::Command::new("/usr/bin/git")
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-am",
                "VM-only fixture"
            ])
            .current_dir(&repository)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .success());
        let mut receipts = BTreeMap::new();
        for gate in fixed_release_gates() {
            eprintln!("VM_GATE_STARTED={}", gate.id);
            let result = crate::harness_engineering::run_gate_selected(
                &repository,
                gate,
                gate.timeout_seconds.min(180),
                Some(&frozen),
            )
            .await
            .unwrap();
            eprintln!(
                "VM_GATE_FINISHED={} passed={} diagnostic={:?}",
                gate.id, result.passed, result.diagnostic
            );
            let passed = result.passed;
            receipts.insert(gate.id.to_owned(), result);
            std::fs::write(
                root.join("gates.json"),
                serde_json::to_vec_pretty(&receipts).unwrap(),
            )
            .unwrap();
            assert!(passed, "VM gate failed: {}", gate.id);
            validate_receipts(true, &receipts, None).unwrap();
        }
        let digest = artifact_digest(&repository.join("target/release/ca")).unwrap();
        validate_receipts(true, &receipts, Some(&digest)).unwrap();
        eprintln!("VM_MATRIX_RECEIPTS={}", root.join("gates.json").display());
    }

    #[tokio::test]
    #[ignore = "runs one selected fixed gate on the current ca snapshot in a fresh VM"]
    async fn hrs253_vm_real_ca_fixed_gate() {
        let frozen = frozen_fixture();
        let id = std::env::var("CA_HARNESS_ENGINEERING_GATE").unwrap_or_else(|_| "format".into());
        let gate = fixed_release_gates().iter().find(|g| g.id == id).unwrap();
        let root = tempfile::Builder::new()
            .prefix("ca-vm-real-gate-")
            .tempdir()
            .unwrap()
            .keep();
        let vault =
            codex_team::self_improvement::LearningVault::open_at(root.join("vault")).unwrap();
        let (snapshot, owner) = crate::harness_snapshot::capture(
            &vault,
            Path::new(env!("CARGO_MANIFEST_DIR")),
            env!("CA_SOURCE_DIGEST"),
        )
        .unwrap();
        let _retained_snapshot = owner.keep();
        let receipt = crate::harness_engineering::run_gate_selected(
            &snapshot.repository,
            gate,
            gate.timeout_seconds,
            Some(&frozen),
        )
        .await
        .unwrap();
        std::fs::write(
            root.join("gate.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
        eprintln!(
            "VM_CA_GATE_RECEIPT={} passed={} diagnostic={:?}",
            root.join("gate.json").display(),
            receipt.passed,
            receipt.diagnostic
        );
        assert!(receipt.passed);
    }
}
