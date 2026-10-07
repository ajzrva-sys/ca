//! Bounded immutable input packages and named, non-executing artifact imports.
use super::lease::VmLease;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const SCRIPT: &str = include_str!("transfer.py");
const MAX_FILE: u64 = 16 * 1024 * 1024;
const MAX_TOTAL: u64 = 512 * 1024 * 1024;
const MAX_ARCHIVE: u64 = 640 * 1024 * 1024;

pub(crate) struct InputPackage {
    file: tempfile::NamedTempFile,
    sha256: String,
    files: usize,
    bytes: u64,
}

pub(crate) struct ImportedArtifact {
    pub(crate) file: tempfile::NamedTempFile,
    pub(crate) sha256: String,
    pub(crate) bytes: u64,
}

#[derive(Clone, Copy)]
pub(crate) enum InputKind {
    Source,
    Workspace,
}
impl InputKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Workspace => "workspace",
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) enum Artifact {
    Ca,
    CodexTeam,
}
impl Artifact {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Ca => "ca",
            Self::CodexTeam => "codex-team",
        }
    }
}

fn boundary(deadline: Instant) -> Result<()> {
    crate::harness_idle::preparation_boundary()?;
    if Instant::now() >= deadline {
        bail!("VM transfer preparation deadline exhausted");
    }
    Ok(())
}

fn inventory(root: &Path, deadline: Instant) -> Result<BTreeMap<PathBuf, (u64, u32)>> {
    fn walk(
        root: &Path,
        relative: &Path,
        files: &mut BTreeMap<PathBuf, (u64, u32)>,
        entries: &mut usize,
        deadline: Instant,
    ) -> Result<()> {
        boundary(deadline)?;
        *entries += 1;
        if *entries > 50_000 || relative.components().count() > 64 {
            bail!("VM source inventory exceeds its bound");
        }
        let path = root.join(relative);
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            for entry in std::fs::read_dir(&path)? {
                let name = entry?.file_name();
                if relative.as_os_str().is_empty()
                    && matches!(
                        name.to_str(),
                        Some("target" | "node_modules" | "oracles" | "reference")
                    )
                {
                    continue;
                }
                walk(root, &relative.join(name), files, entries, deadline)?;
            }
        } else if metadata.is_file() && metadata.nlink() == 1 && metadata.len() <= MAX_FILE {
            let text = relative.to_str().context("VM source path is not UTF-8")?;
            if text.len() > 4096 || text.contains('\\') {
                bail!("unsupported VM source path");
            }
            files.insert(relative.into(), (metadata.len(), metadata.mode() & 0o100));
        } else {
            bail!("VM source contains a link, special or oversized file");
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    walk(root, Path::new(""), &mut files, &mut 0, deadline)?;
    if files.is_empty() || files.values().map(|v| v.0).sum::<u64>() > MAX_TOTAL {
        bail!("VM source byte inventory exceeds its bound or is empty");
    }
    Ok(files)
}

fn digest_file(file: &mut File, maximum: u64, deadline: Instant) -> Result<String> {
    file.rewind()?;
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        boundary(deadline)?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > maximum {
            bail!("VM package exceeds its byte bound");
        }
        digest.update(&buffer[..count]);
    }
    file.rewind()?;
    Ok(format!("{:x}", digest.finalize()))
}

/// Call only for the already audited private checkout, or a frozen task workspace.
/// The original user checkout and its runtime files are not transfer roots.
pub(crate) fn package_tree(root: &Path, deadline: Instant) -> Result<InputPackage> {
    let root = root.canonicalize()?;
    let files = inventory(&root, deadline)?;
    let mut file = tempfile::NamedTempFile::new()?;
    let mut observed = BTreeMap::new();
    let mut total = 0u64;
    {
        let mut archive = tar::Builder::new(file.as_file_mut());
        for (path, (size, executable)) in &files {
            boundary(deadline)?;
            let bytes =
                crate::workspace_materialization::read_bounded_regular_file(&root, path, MAX_FILE)?;
            if bytes.len() as u64 != *size {
                bail!("VM source changed while packaging");
            }
            let mut header = tar::Header::new_gnu();
            header.set_size(*size);
            header.set_mode(if *executable == 0 { 0o644 } else { 0o755 });
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            archive.append_data(&mut header, path, bytes.as_slice())?;
            if archive.get_ref().metadata()?.len() > MAX_ARCHIVE {
                bail!("VM archive exceeds its byte bound");
            }
            observed.insert(path.clone(), format!("{:x}", Sha256::digest(&bytes)));
            total += *size;
        }
        archive.finish()?;
    }
    if inventory(&root, deadline)? != files {
        bail!("VM source inventory changed while packaging");
    }
    for (path, expected) in observed {
        boundary(deadline)?;
        let bytes =
            crate::workspace_materialization::read_bounded_regular_file(&root, &path, MAX_FILE)?;
        if format!("{:x}", Sha256::digest(bytes)) != expected {
            bail!("VM source bytes changed while packaging");
        }
    }
    let sha256 = digest_file(file.as_file_mut(), MAX_ARCHIVE, deadline)?;
    Ok(InputPackage {
        file,
        sha256,
        files: files.len(),
        bytes: total,
    })
}

pub(crate) async fn install_tree(
    lease: &VmLease,
    package: &InputPackage,
    kind: InputKind,
) -> Result<()> {
    let command = lease.guest_command_with_input(&[
        "/usr/bin/sudo",
        "-n",
        "/usr/bin/python3",
        "-I",
        "-c",
        SCRIPT,
        kind.name(),
        &package.sha256,
    ])?;
    let output = crate::harness_engineering::execute_capture_with_input(
        command,
        lease.remaining().min(Duration::from_secs(120)),
        Stdio::from(package.file.reopen()?),
    )
    .await?;
    if !output.passed {
        bail!("VM source installation failed: {}", output.err.1);
    }
    let receipt: serde_json::Value = serde_json::from_str(output.out.1.trim())?;
    if receipt["sha256"].as_str() != Some(&package.sha256)
        || receipt["files"].as_u64() != Some(package.files as u64)
        || receipt["bytes"].as_u64() != Some(package.bytes)
    {
        bail!("VM source installation receipt changed");
    }
    Ok(())
}

pub(crate) async fn install_worker(lease: &VmLease, path: &Path, expected: &str) -> Result<()> {
    install_binary(lease, path, expected, None).await
}

pub(crate) async fn install_artifact(
    lease: &VmLease,
    path: &Path,
    expected: &str,
    artifact: Artifact,
) -> Result<()> {
    install_binary(lease, path, expected, Some(artifact)).await
}

async fn install_binary(
    lease: &VmLease,
    path: &Path,
    expected: &str,
    artifact: Option<Artifact>,
) -> Result<()> {
    let path = path.to_owned();
    let expected = expected.to_owned();
    let deadline = Instant::now() + lease.remaining();
    let (file, digest) = tokio::task::spawn_blocking(move || -> Result<_> {
        let parent = path.parent().context("worker parent missing")?;
        let name = path.file_name().context("worker name missing")?;
        let bytes = crate::workspace_materialization::read_bounded_regular_file(
            parent,
            Path::new(name),
            MAX_TOTAL,
        )?;
        boundary(deadline)?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        if digest != expected {
            bail!("VM worker changed before transfer");
        }
        let mut file = tempfile::NamedTempFile::new()?;
        file.write_all(&bytes)?;
        Ok((file, digest))
    })
    .await??;
    let mut arguments = vec![
        "/usr/bin/sudo",
        "-n",
        "/usr/bin/python3",
        "-I",
        "-c",
        SCRIPT,
    ];
    if let Some(artifact) = artifact {
        arguments.extend(["install-artifact", artifact.name(), &digest]);
    } else {
        arguments.extend(["worker", &digest]);
    }
    let command = lease.guest_command_with_input(&arguments)?;
    let output = crate::harness_engineering::execute_capture_with_input(
        command,
        lease.remaining().min(Duration::from_secs(120)),
        Stdio::from(file.reopen()?),
    )
    .await?;
    let receipt: serde_json::Value = serde_json::from_str(output.out.1.trim())
        .context("VM worker installation receipt missing")?;
    if !output.passed || receipt["sha256"].as_str() != Some(&digest) {
        bail!("VM worker installation failed");
    }
    Ok(())
}

struct Group(Option<i32>);
impl Drop for Group {
    fn drop(&mut self) {
        if let Some(group) = self.0 {
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
        }
    }
}

pub(crate) async fn fetch_artifact(
    lease: &VmLease,
    artifact: Artifact,
) -> Result<ImportedArtifact> {
    let command = lease.guest_command(&[
        "/usr/bin/sudo",
        "-n",
        "/usr/bin/python3",
        "-I",
        "-c",
        SCRIPT,
        "artifact",
        artifact.name(),
    ])?;
    fetch_stream(
        command,
        lease.remaining().min(Duration::from_secs(120)),
        MAX_TOTAL,
    )
    .await
}

/// Only fixed host-owned export commands may use this bounded byte transport.
pub(crate) async fn fetch_stream(
    mut command: tokio::process::Command,
    deadline: Duration,
    maximum: u64,
) -> Result<ImportedArtifact> {
    if deadline.is_zero()
        || deadline > Duration::from_secs(7200)
        || maximum == 0
        || maximum > MAX_TOTAL
    {
        bail!("invalid VM export bound");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(crate::jobs::owned_child_process_group());
    let mut child = command.spawn()?;
    let pid = i32::try_from(child.id().context("VM export process missing")?)?;
    let group = Group((!crate::jobs::worker_contained()).then_some(pid));
    let mut output = child.stdout.take().context("VM export stream missing")?;
    let file = tempfile::NamedTempFile::new()?;
    let mut writer = tokio::fs::File::from_std(file.reopen()?);
    let result = tokio::time::timeout(deadline, async {
        let mut digest = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let count = output.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            bytes += count as u64;
            if bytes > maximum {
                bail!("VM export exceeds its byte bound");
            }
            digest.update(&buffer[..count]);
            writer.write_all(&buffer[..count]).await?;
        }
        if !child.wait().await?.success() || bytes == 0 {
            bail!("VM artifact export failed");
        }
        writer.flush().await?;
        Ok::<_, anyhow::Error>((format!("{:x}", digest.finalize()), bytes))
    })
    .await;
    drop(group);
    if result.is_err() || result.as_ref().is_ok_and(|r| r.is_err()) {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    let (sha256, bytes) = result.context("VM artifact export timed out")??;
    Ok(ImportedArtifact {
        file,
        sha256,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn hrs253_vm_packages_exact_files_and_rejects_links() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::create_dir(root.path().join("target")).unwrap();
        std::fs::write(root.path().join("src/main.rs"), b"fn main() {}\n").unwrap();
        std::fs::write(root.path().join("target/excluded"), b"not input").unwrap();
        std::fs::set_permissions(
            root.path().join("src/main.rs"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let package = package_tree(root.path(), Instant::now() + Duration::from_secs(30)).unwrap();
        assert_eq!(package.files, 1);
        assert_eq!(package.bytes, 13);
        let mut archive = tar::Archive::new(package.file.reopen().unwrap());
        let mut entries = archive.entries().unwrap();
        let entry = entries.next().unwrap().unwrap();
        assert_eq!(entry.path().unwrap(), Path::new("src/main.rs"));
        assert_eq!(entry.header().mode().unwrap(), 0o755);
        assert!(entries.next().is_none());
        assert!(package_tree(root.path(), Instant::now()).is_err());
        symlink("src/main.rs", root.path().join("alias")).unwrap();
        assert!(package_tree(root.path(), Instant::now() + Duration::from_secs(30)).is_err());
        std::fs::remove_file(root.path().join("alias")).unwrap();
        std::fs::hard_link(root.path().join("src/main.rs"), root.path().join("hard")).unwrap();
        assert!(package_tree(root.path(), Instant::now() + Duration::from_secs(30)).is_err());
    }

    #[tokio::test]
    async fn hrs253_vm_guest_transfer_rejects_hostile_archives() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("transfer.py"), SCRIPT).unwrap();
        std::fs::write(
            root.path().join("test_transfer.py"),
            include_str!("test_transfer.py"),
        )
        .unwrap();
        let mut command = tokio::process::Command::new("/usr/bin/python3");
        command
            .args(["-I", "-B"])
            .arg(root.path().join("test_transfer.py"));
        let output = crate::harness_engineering::execute_capture(command, Duration::from_secs(30))
            .await
            .unwrap();
        assert!(output.passed, "{}", output.err.1);
    }

    #[tokio::test]
    #[ignore = "transfers inputs and artifacts through one explicitly selected disposable VM"]
    async fn hrs253_vm_real_transfer_protects_inputs_and_imports_exact_artifact() {
        let binding = std::env::var_os("CA_HARNESS_VM_TEST_BINDING")
            .expect("explicit previously hashed VM binding required");
        let mut frozen: codex_team::self_improvement::FrozenHarnessVm =
            serde_json::from_slice(&std::fs::read(binding).unwrap()).unwrap();
        frozen.validate().unwrap();
        frozen.adapter_digest = env!("CA_HARNESS_ADAPTER_DIGEST").into();
        let lease = VmLease::start(&frozen, Duration::from_secs(240))
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("input.txt"), b"frozen bytes\n").unwrap();
        let package = package_tree(root.path(), Instant::now() + Duration::from_secs(30)).unwrap();
        install_tree(&lease, &package, InputKind::Source)
            .await
            .unwrap();
        install_tree(&lease, &package, InputKind::Workspace)
            .await
            .unwrap();
        let worker = root.path().join("worker");
        std::fs::write(&worker, b"test worker bytes").unwrap();
        let worker_digest = format!("{:x}", Sha256::digest(b"test worker bytes"));
        install_worker(&lease, &worker, &worker_digest)
            .await
            .unwrap();
        let script = r#"
from pathlib import Path
import os
source=Path('/opt/ca-harness/project/input.txt')
assert source.read_bytes()==b'frozen bytes\n'
try:
 source.write_bytes(b'wrong')
 raise AssertionError('runner could modify source')
except PermissionError: pass
try:
 os.chmod(source,0o777)
 raise AssertionError('runner could chmod source')
except PermissionError: pass
try:
 Path('/opt/ca-harness/worker').write_bytes(b'wrong')
 raise AssertionError('runner could modify worker')
except PermissionError: pass
Path('/opt/ca-harness/task/input.txt').write_bytes(b'changed task')
target=Path('/opt/ca-harness/project/target/release')
target.mkdir()
(target/'ca').write_bytes(b'exact candidate artifact\x00\xff')
print('protected inputs and writable outputs verified')
"#;
        let command = lease
            .guest_command(&[
                "/usr/bin/sudo",
                "-n",
                "-u",
                "ca-runner",
                "/usr/bin/python3",
                "-I",
                "-c",
                script,
            ])
            .unwrap();
        let output = crate::harness_engineering::execute_capture(command, Duration::from_secs(20))
            .await
            .unwrap();
        assert!(output.passed, "{}", output.err.1);
        let artifact = fetch_artifact(&lease, Artifact::Ca).await.unwrap();
        let expected = b"exact candidate artifact\x00\xff";
        assert_eq!(std::fs::read(artifact.file.path()).unwrap(), expected);
        assert_eq!(artifact.bytes, expected.len() as u64);
        assert_eq!(artifact.sha256, format!("{:x}", Sha256::digest(expected)));
        // A candidate cannot turn a named artifact export into a read of a
        // different guest file by replacing the artifact with a symlink.
        let command = lease.guest_command(&["/usr/bin/sudo", "-n", "-u", "ca-runner", "/usr/bin/python3", "-I", "-c",
            "from pathlib import Path; p=Path('/opt/ca-harness/project/target/release/ca'); p.unlink(); p.symlink_to('/etc/passwd')"
        ]).unwrap();
        assert!(
            crate::harness_engineering::execute_capture(command, Duration::from_secs(15))
                .await
                .unwrap()
                .passed
        );
        assert!(fetch_artifact(&lease, Artifact::Ca).await.is_err());
        let receipt = lease.finish().await.unwrap();
        eprintln!("VM_TRANSFER_RECEIPT={}", receipt.display());
    }
}
