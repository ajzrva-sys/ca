//! Dedicated VM inputs are frozen before generation. No live profile is consulted
//! after freezing, and VM plans must never fall back to the host sandbox.
pub(crate) mod engineering;
#[allow(dead_code)] // Worker routes will consume the remaining lease APIs.
pub(crate) mod lease;
pub(crate) mod ownership;
pub(crate) mod probes;
pub(crate) mod results;
pub(crate) mod transfer;
pub(crate) mod verifier;
pub(crate) mod worker;

use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{
    FrozenHarnessVm, HarnessPlan, HarnessVmFile, HarnessVmProfile, HARNESS_VM_FILES,
    HARNESS_VM_SCHEMA,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read, Seek};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(target_os = "freebsd")]
pub(crate) const HOST_PYTHON: &str = "/usr/local/bin/python3";
#[cfg(not(target_os = "freebsd"))]
pub(crate) const HOST_PYTHON: &str = "/usr/bin/python3";

fn file_path(profile: &HarnessVmProfile, name: &str) -> PathBuf {
    if name == "tart" {
        profile.tart.clone()
    } else {
        profile.home.join("vms").join(&profile.template).join(name)
    }
}
fn maximum(name: &str) -> u64 {
    if name == "disk.img" {
        512 * 1024 * 1024 * 1024
    } else {
        256 * 1024 * 1024
    }
}
fn open_input(path: &Path, name: &str) -> Result<File> {
    if path.canonicalize()? != path {
        bail!("VM input path is not canonical");
    }
    validate_parents(path)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let m = file.metadata()?;
    if !m.is_file()
        || m.nlink() != 1
        || m.len() == 0
        || m.len() > maximum(name)
        || m.mode() & 0o7022 != 0
        || (m.uid() != unsafe { libc::geteuid() } && m.uid() != 0)
        || (name == "tart" && m.mode() & 0o100 == 0)
    {
        bail!("VM input is not a bounded, protected regular file");
    }
    Ok(file)
}
fn validate_parents(path: &Path) -> Result<()> {
    for parent in path.ancestors().skip(1) {
        let metadata = std::fs::symlink_metadata(parent)?;
        let trusted_owner = metadata.uid() == 0 || metadata.uid() == unsafe { libc::geteuid() };
        // A trusted sticky temporary directory protects the owned entry below it.
        if !metadata.is_dir()
            || !trusted_owner
            || (metadata.mode() & 0o022 != 0 && metadata.mode() & 0o1000 == 0)
        {
            bail!("VM input directory can be replaced by another account");
        }
    }
    Ok(())
}
fn identity(m: &Metadata, sha256: String) -> HarnessVmFile {
    HarnessVmFile {
        sha256,
        bytes: m.len(),
        device: m.dev(),
        inode: m.ino(),
        uid: m.uid(),
        mode: m.mode() & 0o777,
        modified_seconds: m.mtime(),
        modified_nanoseconds: m.mtime_nsec(),
        changed_seconds: m.ctime(),
        changed_nanoseconds: m.ctime_nsec(),
    }
}
fn hash_input(path: &Path, name: &str, deadline: Instant) -> Result<HarnessVmFile> {
    let mut file = open_input(path, name)?;
    let before = identity(&file.metadata()?, String::new());
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        crate::harness_idle::preparation_boundary()?;
        if Instant::now() >= deadline {
            bail!("VM freezing exceeded the remaining preparation budget");
        }
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes = bytes.saturating_add(count as u64);
        if bytes > before.bytes {
            bail!("VM input grew while freezing");
        }
        digest.update(&buffer[..count]);
    }
    if bytes != before.bytes
        || identity(&file.metadata()?, String::new()) != before
        || identity(&open_input(path, name)?.metadata()?, String::new()) != before
    {
        bail!("VM input changed while freezing");
    }
    Ok(HarnessVmFile {
        sha256: format!("{:x}", digest.finalize()),
        ..before
    })
}

struct Child(std::process::Child, bool, bool);
impl Drop for Child {
    fn drop(&mut self) {
        if self.1 {
            if self.2 {
                unsafe {
                    libc::kill(-(self.0.id() as i32), libc::SIGKILL);
                }
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VmState {
    name: String,
    source: String,
    running: bool,
    state: String,
}
fn stopped_listing(bytes: &[u8], name: &str) -> Result<()> {
    let rows: Vec<VmState> = serde_json::from_slice(bytes).context("invalid Tart VM inventory")?;
    if rows
        .iter()
        .any(|row| row.source == "local" && row.name.starts_with("ca-harness-run-"))
    {
        // Even "stopped" can precede a surviving Tart runner starting the VM
        // after supervisor death. Successful cleanup deletes the owned clone.
        bail!("a disposable harness VM remains; its cleanup must finish before admission");
    }
    let mut matches = rows
        .iter()
        .filter(|row| row.name == name && row.source == "local");
    let row = matches.next().context("dedicated VM template is missing")?;
    if matches.next().is_some() || row.running || row.state != "stopped" {
        bail!("dedicated VM template must be stopped before freezing or cloning");
    }
    Ok(())
}
fn require_stopped(profile: &HarnessVmProfile, preparation_deadline: Instant) -> Result<()> {
    crate::harness_idle::preparation_boundary()?;
    if Instant::now() >= preparation_deadline {
        bail!("VM preparation budget exhausted");
    }
    let home = tempfile::tempdir()?;
    let mut output = tempfile::tempfile()?;
    let mut child = Child(
        Command::new(&profile.tart)
            .args(["list", "--source", "local", "--format", "json"])
            .env_clear()
            .env("HOME", home.path())
            .env("TMPDIR", home.path())
            .env("TART_HOME", &profile.home)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .current_dir(home.path())
            .stdin(Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(Stdio::null())
            .process_group(crate::jobs::owned_child_process_group())
            .spawn()?,
        true,
        !crate::jobs::worker_contained(),
    );
    let deadline = (Instant::now() + Duration::from_secs(30)).min(preparation_deadline);
    loop {
        crate::harness_idle::preparation_boundary()?;
        if output.metadata()?.len() > 1024 * 1024 {
            bail!("VM inventory exceeded its bound");
        }
        if let Some(status) = child.0.try_wait()? {
            child.1 = false;
            if !status.success() {
                bail!("could not inspect dedicated VM template state");
            }
            break;
        }
        if Instant::now() >= deadline {
            bail!("dedicated VM template inspection timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    output.rewind()?;
    let mut bytes = Vec::new();
    output.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        bail!("VM inventory exceeded its bound");
    }
    stopped_listing(&bytes, &profile.template)
}

pub(crate) fn freeze(profile: &HarnessVmProfile, maximum_seconds: u64) -> Result<FrozenHarnessVm> {
    if maximum_seconds == 0 || maximum_seconds > 7200 {
        bail!("invalid VM preparation allowance");
    }
    let deadline = Instant::now() + Duration::from_secs(maximum_seconds);
    profile.validate()?;
    let profile = HarnessVmProfile {
        tart: profile.tart.canonicalize()?,
        home: profile.home.canonicalize()?,
        ..profile.clone()
    };
    let mut files = BTreeMap::new();
    files.insert("tart".into(), hash_input(&profile.tart, "tart", deadline)?);
    require_stopped(&profile, deadline)?;
    for name in HARNESS_VM_FILES.into_iter().filter(|name| *name != "tart") {
        files.insert(
            name.into(),
            hash_input(&file_path(&profile, name), name, deadline)?,
        );
    }
    let frozen = FrozenHarnessVm {
        schema: HARNESS_VM_SCHEMA.into(),
        profile,
        adapter_digest: env!("CA_HARNESS_ADAPTER_DIGEST").into(),
        files,
    };
    verify(&frozen, deadline)?;
    Ok(frozen)
}

/// Hashes were read at freeze time. The guest cannot access these host files;
/// any subsequent replacement, write, chmod or restored mtime changes the pinned
/// inode/ctime tuple. Check before and after cloning, with the template stopped.
pub(crate) fn verify(frozen: &FrozenHarnessVm, deadline: Instant) -> Result<()> {
    frozen.validate()?;
    if frozen.adapter_digest != env!("CA_HARNESS_ADAPTER_DIGEST") {
        bail!("frozen VM host adapter build changed");
    }
    for name in HARNESS_VM_FILES {
        crate::harness_idle::preparation_boundary()?;
        let saved = &frozen.files[name];
        let file = open_input(&file_path(&frozen.profile, name), name)?;
        if identity(&file.metadata()?, saved.sha256.clone()) != *saved {
            bail!("frozen VM input changed: {name}");
        }
    }
    require_stopped(&frozen.profile, deadline)
}

/// Admission uses the immutable plan, never a mutable runtime profile. Every
/// execution path subsequently owns and rechecks its fresh guest lease.
pub(crate) async fn validate_backend(plan: &HarnessPlan) -> Result<()> {
    if let Some(frozen) = &plan.vm {
        let frozen = frozen.clone();
        tokio::task::spawn_blocking(move || {
            verify(&frozen, Instant::now() + Duration::from_secs(30))
        })
        .await??;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn hrs253_vm_admission_rejects_running_or_uncertain_leftovers() {
        for (running, state, rejected) in [
            (true, "running", true),
            (true, "stopped", true),
            (false, "starting", true),
            (false, "stopped", true),
        ] {
            let inventory = serde_json::to_vec(&serde_json::json!([
                {"Name": "base", "Source": "local", "Running": false, "State": "stopped"},
                {"Name": "ca-harness-run-orphan", "Source": "local", "Running": running, "State": state}
            ])).unwrap();
            assert_eq!(stopped_listing(&inventory, "base").is_err(), rejected);
        }
    }
    fn profile(root: &Path) -> HarnessVmProfile {
        let template = root.join("vms/base");
        std::fs::create_dir_all(&template).unwrap();
        for name in ["config.json", "disk.img", "nvram.bin"] {
            std::fs::write(template.join(name), b"frozen input").unwrap();
        }
        let tart = root.join("tart");
        std::fs::write(&tart,b"#!/bin/sh\nprintf '%s' '[{\"Name\":\"base\",\"Source\":\"local\",\"Running\":false,\"State\":\"stopped\"}]'\n").unwrap();
        std::fs::set_permissions(&tart, std::fs::Permissions::from_mode(0o700)).unwrap();
        HarnessVmProfile {
            tart,
            home: root.into(),
            template: "base".into(),
            guest_agent_sha256: "a".repeat(64),
        }
    }
    #[test]
    fn hrs253_vm_freeze_rejects_same_length_edits_with_restored_mtime() {
        let root = tempfile::tempdir().unwrap();
        let profile = profile(&root.path().canonicalize().unwrap());
        let frozen = freeze(&profile, 30).unwrap();
        verify(&frozen, Instant::now() + Duration::from_secs(30)).unwrap();
        let path = file_path(&frozen.profile, "disk.img");
        let original = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(2));
        std::fs::write(&path, b"changed data").unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(original))
            .unwrap();
        assert!(verify(&frozen, Instant::now() + Duration::from_secs(30)).is_err());
    }
    #[test]
    fn hrs253_vm_freeze_rejects_links_permissions_and_running_templates() {
        let root = tempfile::tempdir().unwrap();
        let profile = profile(&root.path().canonicalize().unwrap());
        let file = file_path(&profile, "nvram.bin");
        let saved = file.with_extension("saved");
        std::fs::rename(&file, &saved).unwrap();
        std::os::unix::fs::symlink(&saved, &file).unwrap();
        assert!(freeze(&profile, 30).is_err());
        std::fs::remove_file(&file).unwrap();
        std::fs::hard_link(&saved, &file).unwrap();
        assert!(freeze(&profile, 30).is_err());
        std::fs::remove_file(&file).unwrap();
        std::fs::rename(&saved, &file).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(freeze(&profile, 30).is_err());
        assert!(stopped_listing(
            br#"[{"Name":"base","Source":"local","Running":true,"State":"running"}]"#,
            "base"
        )
        .is_err());
        assert!(stopped_listing(
            br#"[{"Name":"base","Source":"local","Running":false}]"#,
            "base"
        )
        .is_err());
        assert!(stopped_listing(b"[]", "base").is_err());
    }
    #[tokio::test]
    async fn hrs253_vm_binding_admits_only_the_unchanged_selected_backend() {
        let root = tempfile::tempdir().unwrap();
        let profile = profile(&root.path().canonicalize().unwrap());
        let frozen = freeze(&profile, 30).unwrap();
        let (mut plan, _, _) = crate::harness_generation::tests::inputs();
        validate_backend(&plan).await.unwrap();
        plan.vm = Some(frozen.clone());
        plan.seal().unwrap();
        validate_backend(&plan).await.unwrap();
        let mut changed = frozen;
        changed.adapter_digest = if changed.adapter_digest == "a".repeat(64) {
            "b".repeat(64)
        } else {
            "a".repeat(64)
        };
        assert!(verify(&changed, Instant::now() + Duration::from_secs(30)).is_err());
        plan.vm = Some(changed);
        plan.seal().unwrap();
        assert!(validate_backend(&plan).await.is_err());
        plan.vm = Some(freeze(&profile, 30).unwrap());
        plan.seal().unwrap();
        std::fs::write(file_path(&profile, "config.json"), b"changed template").unwrap();
        assert!(
            validate_backend(&plan).await.is_err(),
            "invalid VM must not fall back to host"
        );
    }
    #[test]
    fn hrs253_vm_freeze_rejects_replaceable_input_directories() {
        let root = tempfile::tempdir().unwrap();
        let profile = profile(&root.path().canonicalize().unwrap());
        std::fs::set_permissions(
            profile.home.join("vms/base"),
            std::fs::Permissions::from_mode(0o777),
        )
        .unwrap();
        assert!(freeze(&profile, 30).is_err());
    }
    #[test]
    #[ignore = "hashes the explicit prepared macOS VM image without booting or changing it"]
    fn hrs253_vm_freezes_real_prepared_template() {
        let profile_file =
            std::env::var_os("CA_HARNESS_VM_TEST_PROFILE").expect("explicit VM profile required");
        let profile: HarnessVmProfile =
            serde_json::from_slice(&std::fs::read(profile_file).unwrap()).unwrap();
        eprintln!("Freezing prepared VM template {}", profile.template);
        let started = Instant::now();
        let frozen = freeze(&profile, 600).unwrap();
        verify(&frozen, Instant::now() + Duration::from_secs(30)).unwrap();
        let root = tempfile::Builder::new()
            .prefix("ca-hrs253-vm-binding-")
            .tempdir()
            .unwrap()
            .keep();
        std::fs::write(
            root.join("frozen-vm.json"),
            serde_json::to_vec_pretty(&frozen).unwrap(),
        )
        .unwrap();
        eprintln!(
            "VM_BINDING_RECEIPT={} elapsed_seconds={:.3}",
            root.join("frozen-vm.json").display(),
            started.elapsed().as_secs_f64()
        );
    }
}
