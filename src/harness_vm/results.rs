//! Import task bytes, never guest-provided commands, paths or success claims.
use super::lease::VmLease;
use crate::harness_cases::FrozenCodingCase;
use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

const SCRIPT: &str = include_str!("results.py");
const MAX_WIRE: u64 = 5 * 1024 * 1024;

fn decode(bytes: &[u8], case: &FrozenCodingCase) -> Result<BTreeMap<String, Vec<u8>>> {
    if bytes.len() as u64 > MAX_WIRE {
        bail!("VM task result exceeds its wire bound");
    }
    let entries: Vec<(String, Vec<u8>)> = serde_json::from_slice(bytes)?;
    let mut files = BTreeMap::new();
    for (name, bytes) in entries {
        if !case.files.contains_key(&name) || files.insert(name, bytes).is_some() {
            bail!("VM task result has an unexpected or repeated path");
        }
    }
    case.validate_result_files(&files)?;
    Ok(files)
}

pub(crate) async fn fetch_result_files(
    lease: &VmLease,
    case: &FrozenCodingCase,
) -> Result<BTreeMap<String, Vec<u8>>> {
    case.validate()?;
    let command = lease.guest_command(&[
        "/usr/bin/sudo",
        "-n",
        "/usr/bin/python3",
        "-I",
        "-c",
        SCRIPT,
    ])?;
    let imported = super::transfer::fetch_stream(
        command,
        lease.remaining().min(Duration::from_secs(30)),
        MAX_WIRE,
    )
    .await?;
    let case = case.clone();
    tokio::task::spawn_blocking(move || decode(&std::fs::read(imported.file.path())?, &case))
        .await?
}

/// Destination is a new directory beneath a host-owned private root. Only the
/// frozen input names are accepted, and no imported byte is executed here.
pub(crate) fn materialize_result(
    case: &FrozenCodingCase,
    files: &BTreeMap<String, Vec<u8>>,
    destination: &Path,
) -> Result<()> {
    case.validate_result_files(files)?;
    std::fs::create_dir(destination)?;
    for (name, bytes) in files {
        let path = destination.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?
            .write_all(bytes)?;
    }
    if case.result_files(destination)? != *files {
        bail!("VM result changed during materialization");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hrs253_vm_results_enforce_frozen_grants_before_materializing() {
        let case =
            crate::harness_cases::freeze_reasoning_cases(Path::new(env!("CARGO_MANIFEST_DIR")))
                .unwrap()
                .remove(0);
        let encode = |files: &BTreeMap<String, Vec<u8>>| {
            serde_json::to_vec(&files.iter().collect::<Vec<_>>()).unwrap()
        };
        let mut actual = case.files.clone();
        actual
            .get_mut(case.allowed_paths.first().unwrap())
            .unwrap()
            .extend_from_slice(b"\n// candidate edit\n");
        let decoded = decode(&encode(&actual), &case).unwrap();
        assert_eq!(decoded, actual);
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("imported");
        materialize_result(&case, &decoded, &destination).unwrap();
        assert_eq!(case.result_files(&destination).unwrap(), decoded);
        assert!(materialize_result(&case, &decoded, &destination).is_err());
        let mut bad = actual.clone();
        bad.insert("../escape".into(), b"bad".to_vec());
        assert!(decode(&encode(&bad), &case).is_err());
        assert!(materialize_result(&case, &bad, &root.path().join("bad")).is_err());
        assert!(!root.path().join("bad").exists());
        let mut bad = actual.clone();
        bad.remove(case.files.keys().next().unwrap());
        assert!(decode(&encode(&bad), &case).is_err());
        let protected = case
            .files
            .keys()
            .find(|k| !case.allowed_paths.contains(*k))
            .unwrap();
        let mut bad = actual.clone();
        bad.insert(protected.clone(), b"changed protected input".to_vec());
        assert!(decode(&encode(&bad), &case).is_err());
        let mut repeated = actual.iter().collect::<Vec<_>>();
        repeated.push(repeated[0]);
        assert!(decode(&serde_json::to_vec(&repeated).unwrap(), &case).is_err());
        let mut huge = actual;
        huge.insert(
            case.allowed_paths.first().unwrap().clone(),
            vec![0; 1024 * 1024 + 1],
        );
        assert!(decode(&encode(&huge), &case).is_err());
    }

    #[tokio::test]
    #[ignore = "requires frozen dedicated macOS VM; imports exact edited task bytes and rejects a guest link"]
    async fn hrs253_vm_real_result_import_checks_frozen_files() {
        use super::super::transfer::{install_tree, package_tree, InputKind};
        use std::time::Instant;
        let binding = std::env::var_os("CA_HARNESS_VM_TEST_BINDING").expect("explicit VM binding");
        let mut frozen: codex_team::self_improvement::FrozenHarnessVm =
            serde_json::from_slice(&std::fs::read(binding).unwrap()).unwrap();
        frozen.validate().unwrap();
        frozen.adapter_digest = env!("CA_HARNESS_ADAPTER_DIGEST").into();
        let case =
            crate::harness_cases::freeze_reasoning_cases(Path::new(env!("CARGO_MANIFEST_DIR")))
                .unwrap()
                .remove(0);
        let owner = tempfile::tempdir().unwrap();
        let workspace = owner.path().join("workspace");
        case.materialize(&workspace).unwrap();
        let package = package_tree(&workspace, Instant::now() + Duration::from_secs(90)).unwrap();
        let lease = VmLease::start(&frozen, Duration::from_secs(90))
            .await
            .unwrap();
        let result: Result<()> = async {
            install_tree(&lease, &package, InputKind::Workspace).await?;
            let name = case.allowed_paths.first().unwrap();
            let packet = serde_json::to_string(name)?;
            let script = "import json,sys; from pathlib import Path; p=Path('/opt/ca-harness/task') / json.loads(sys.argv[1]); p.write_bytes(p.read_bytes()+b'\\n// guest edit\\n')";
            let output = crate::harness_engineering::execute_capture(lease.guest_command(&[
                "/usr/bin/sudo", "-n", "-u", "ca-runner", "/usr/bin/python3", "-I", "-c", script, &packet,
            ])?, lease.remaining()).await?;
            assert!(output.passed, "{}", output.err.1);
            let files = fetch_result_files(&lease, &case).await?;
            let mut expected = case.files.clone();
            expected.get_mut(name).unwrap().extend_from_slice(b"\n// guest edit\n");
            assert_eq!(files, expected);
            materialize_result(&case, &files, &owner.path().join("imported"))?;
            let replace = "import json,sys; from pathlib import Path; p=Path('/opt/ca-harness/task') / json.loads(sys.argv[1]); p.unlink(); p.symlink_to('/etc/passwd')";
            let output = crate::harness_engineering::execute_capture(lease.guest_command(&[
                "/usr/bin/sudo", "-n", "-u", "ca-runner", "/usr/bin/python3", "-I", "-c", replace, &packet,
            ])?, lease.remaining()).await?;
            assert!(output.passed, "{}", output.err.1);
            assert!(fetch_result_files(&lease, &case).await.is_err());
            Ok(())
        }.await;
        let receipt = lease.finish().await.unwrap();
        eprintln!("VM_RESULT_CLEANUP={}", receipt.display());
        result.unwrap();
    }

    #[tokio::test]
    async fn hrs253_vm_guest_result_snapshot_rejects_links_and_races() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("results.py"), SCRIPT).unwrap();
        std::fs::write(
            root.path().join("test_results.py"),
            include_str!("test_results.py"),
        )
        .unwrap();
        let mut command = tokio::process::Command::new("/usr/bin/python3");
        command
            .args(["-I", "-B"])
            .arg(root.path().join("test_results.py"));
        let output = crate::harness_engineering::execute_capture(command, Duration::from_secs(30))
            .await
            .unwrap();
        assert!(output.passed, "{}", output.err.1);
    }
}
