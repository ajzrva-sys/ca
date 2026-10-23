//! Native installation of ordinary local build artifacts.

use sha2::{Digest, Sha256};
use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(crate) struct InstallRequest {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub backup: bool,
    pub verify: bool,
    pub self_update: bool,
}

pub(crate) fn install(request: InstallRequest) -> Result<String, String> {
    install_with(request, &NativePlatform)
}

trait Platform {
    fn sign(&self, path: &Path) -> Result<(), String>;
    fn signature(&self, path: &Path) -> Result<(), String>;
    fn readback(&self, path: &Path) -> Result<(String, String), String>;
}

struct NativePlatform;

impl Platform for NativePlatform {
    fn sign(&self, path: &Path) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        run_checked(
            Command::new("/usr/bin/codesign")
                .args(["--force", "--sign", "-"])
                .arg(path),
            Duration::from_secs(10),
        )?;
        #[cfg(not(target_os = "macos"))]
        let _ = path;
        Ok(())
    }

    fn signature(&self, path: &Path) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        run_checked(
            Command::new("/usr/bin/codesign").arg("-vvv").arg(path),
            Duration::from_secs(10),
        )?;
        #[cfg(not(target_os = "macos"))]
        let _ = path;
        Ok(())
    }

    fn readback(&self, path: &Path) -> Result<(String, String), String> {
        let version = run_checked(Command::new(path).arg("--version"), Duration::from_secs(5))?;
        let help = run_checked(Command::new(path).arg("--help"), Duration::from_secs(5))?;
        if !version.trim().starts_with("ca ") || help.trim().is_empty() {
            return Err("candidate did not report a ca version and help".into());
        }
        Ok((version, help))
    }
}

fn install_with(request: InstallRequest, platform: &impl Platform) -> Result<String, String> {
    let destination = &request.destination;
    if !destination.is_absolute() {
        return Err("Destination must be an absolute path".into());
    }
    let parent = destination
        .parent()
        .filter(|parent| parent.is_dir())
        .ok_or("Destination directory does not exist")?;
    let original = match std::fs::symlink_metadata(destination) {
        Ok(metadata) => {
            require_regular(&metadata)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.uid() != unsafe { libc::geteuid() } {
                    return Err("Destination is owned by another user; choose a user-owned install directory".into());
                }
            }
            Some(metadata)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Cannot inspect destination: {error}")),
    };
    let mut stage_builder = tempfile::Builder::new();
    stage_builder.prefix(".ca-install-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        stage_builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let stage = stage_builder
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot stage beside destination: {error}"))?;
    let candidate = stage.path().join("ca");
    let (source_hash, _) = copy_stable(&request.source, &candidate)?;
    set_executable(&candidate)?;
    platform.sign(&candidate)?;
    set_executable(&candidate)?;
    platform.signature(&candidate)?;
    File::open(&candidate)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("Cannot sync signed candidate: {error}"))?;
    let signed_hash = hash_file(&candidate)?;
    let expected_readback = if request.self_update {
        Some(platform.readback(&candidate)?)
    } else {
        None
    };

    // Even backup=false retains a private transaction copy until verification.
    // Replacing the running ca always retains a durable recovery copy.
    let mut recovery = None;
    if let Some(original_metadata) = &original {
        let old_hash = hash_file(destination)?;
        let recovery_path = if request.backup || request.self_update {
            let name = destination
                .file_name()
                .ok_or("Destination has no filename")?;
            parent.join(format!(
                "{}.recovery-{}-{old_hash}",
                name.to_string_lossy(),
                chrono::Utc::now().format("%Y%m%dT%H%M%S%.9fZ")
            ))
        } else {
            stage.path().join("previous")
        };
        let (copied_hash, copied_metadata) = copy_stable(destination, &recovery_path)?;
        if old_hash != copied_hash || !same_identity(original_metadata, &copied_metadata) {
            let _ = std::fs::remove_file(&recovery_path);
            return Err("Destination changed while preserving recovery copy".into());
        }
        set_binary_metadata(&recovery_path, Some(original_metadata))
            .map_err(|error| format!("Cannot preserve recovery metadata: {error}"))?;
        File::open(&recovery_path)
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("Cannot sync recovery copy: {error}"))?;
        recovery = Some((recovery_path, old_hash));
    }
    if let Some((_, old_hash)) = &recovery {
        if hash_file(destination)? != *old_hash {
            return Err("Destination changed before atomic replacement".into());
        }
    } else if std::fs::symlink_metadata(destination).is_ok() {
        return Err("Destination appeared before atomic replacement".into());
    }
    std::fs::rename(&candidate, destination)
        .map_err(|error| format!("Atomic installation failed: {error}"))?;
    let verification: Result<(), String> = (|| {
        sync_directory(parent)?;
        verify_mode_owner(destination)?;
        if hash_file(destination)? != signed_hash {
            return Err("Installed SHA-256 does not match the signed candidate".into());
        }
        platform.signature(destination)?;
        if let Some(expected) = &expected_readback {
            if platform.readback(destination)? != *expected {
                return Err("Installed version/help differs from the staged ca".into());
            }
        }
        Ok(())
    })();
    if let Err(error) = verification {
        let restored: Result<(), String> = (|| {
            if let Some((path, old_hash)) = &recovery {
                let rollback = stage.path().join("rollback");
                let (copied_hash, _) = copy_stable(path, &rollback)?;
                if copied_hash != *old_hash {
                    return Err("Recovery SHA-256 changed".into());
                }
                set_binary_metadata(&rollback, original.as_ref())
                    .map_err(|error| format!("Cannot restore executable metadata: {error}"))?;
                std::fs::rename(rollback, destination)
                    .map_err(|error| format!("Cannot restore previous executable: {error}"))?;
                if hash_file(destination)? != *old_hash {
                    return Err("Restored executable SHA-256 mismatch".into());
                }
                platform.signature(destination)?;
            } else {
                std::fs::remove_file(destination)
                    .map_err(|error| format!("Cannot remove failed new install: {error}"))?;
            }
            sync_directory(parent)
        })();
        return match restored {
            Ok(()) => Err(format!(
                "Post-install verification failed: {error}; previous state restored"
            )),
            Err(rollback) => {
                let recovery_path = recovery
                    .as_ref()
                    .map_or_else(|| "none".into(), |(path, _)| path.display().to_string());
                if recovery
                    .as_ref()
                    .is_some_and(|(path, _)| path.starts_with(stage.path()))
                {
                    let _retained = stage.keep();
                }
                Err(format!(
                    "Post-install verification failed: {error}; ROLLBACK FAILED: {rollback}; recovery: {recovery_path}"
                ))
            }
        };
    }
    let recovery_note = if request.backup || request.self_update {
        recovery.as_ref().map_or_else(String::new, |(path, hash)| {
            format!("\nRecovery: {} (SHA-256: {hash})", path.display())
        })
    } else {
        String::new()
    };
    Ok(format!(
        "Installed {} -> {}\nSource SHA-256: {source_hash}\nInstalled signed SHA-256: {signed_hash}{recovery_note}{}",
        request.source.display(), destination.display(),
        if request.verify { "" } else { "\nAtomic integrity verification remains mandatory" }
    ))
}

fn require_regular(metadata: &Metadata) -> Result<(), String> {
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Binary paths must be regular files, not symlinks".into());
    }
    Ok(())
}

fn open_regular(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("Cannot open {}: {error}", path.display()))?;
    require_regular(&file.metadata().map_err(|error| error.to_string())?)?;
    Ok(file)
}

fn same_identity(before: &Metadata, after: &Metadata) -> bool {
    let same = before.len() == after.len() && before.modified().ok() == after.modified().ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        same && before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec()
    }
    #[cfg(not(unix))]
    same
}

fn digest_reader(reader: &mut impl Read) -> Result<String, String> {
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn hash_file(path: &Path) -> Result<String, String> {
    digest_reader(&mut open_regular(path)?)
}

fn copy_stable(source: &Path, destination: &Path) -> Result<(String, Metadata), String> {
    let mut source_file = open_regular(source)?;
    let before = source_file.metadata().map_err(|error| error.to_string())?;
    let expected_hash = digest_reader(&mut source_file)?;
    source_file
        .seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options
        .open(destination)
        .map_err(|error| format!("Cannot create staged copy: {error}"))?;
    std::io::copy(&mut source_file, &mut output).map_err(|error| error.to_string())?;
    output
        .flush()
        .and_then(|()| output.sync_all())
        .map_err(|error| error.to_string())?;
    let after = source_file.metadata().map_err(|error| error.to_string())?;
    let path_metadata = std::fs::symlink_metadata(source).map_err(|error| error.to_string())?;
    if !same_identity(&before, &after)
        || !same_identity(&before, &path_metadata)
        || hash_file(destination)? != expected_hash
    {
        return Err("Binary changed while staging; installation was not committed".into());
    }
    Ok((expected_hash, before))
}

fn set_executable(path: &Path) -> Result<(), String> {
    set_binary_metadata(path, None)?;
    verify_mode_owner(path)
}

fn set_binary_metadata(path: &Path, previous: Option<&Metadata>) -> Result<(), String> {
    let file = open_regular(path)?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let uid = previous.map_or_else(|| unsafe { libc::geteuid() }, MetadataExt::uid);
        let gid = previous.map_or_else(|| unsafe { libc::getegid() }, MetadataExt::gid);
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if (metadata.uid() != uid || metadata.gid() != gid)
            && unsafe { libc::fchown(file.as_raw_fd(), uid, gid) } != 0
        {
            return Err(format!(
                "Cannot set binary owner/group: {}",
                std::io::Error::last_os_error()
            ));
        }
        let permissions = previous.map_or_else(
            || std::fs::Permissions::from_mode(0o755),
            Metadata::permissions,
        );
        let expected_mode = permissions.mode() & 0o7777;
        file.set_permissions(permissions)
            .map_err(|error| format!("Cannot set executable mode: {error}"))?;
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if metadata.uid() != uid
            || metadata.gid() != gid
            || metadata.mode() & 0o7777 != expected_mode
        {
            return Err("Binary owner/group/mode did not match the requested metadata".into());
        }
    }
    #[cfg(not(unix))]
    if let Some(previous) = previous {
        file.set_permissions(previous.permissions())
            .map_err(|error| format!("Cannot restore executable permissions: {error}"))?;
    }
    Ok(())
}

fn verify_mode_owner(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    require_regular(&metadata)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o7777 != 0o755
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.gid() != unsafe { libc::getegid() }
        {
            return Err(
                "Installed executable must be owned by the current user/group with mode 0755"
                    .into(),
            );
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("Cannot sync install directory: {error}"))
}

#[cfg(unix)]
fn verification_group_exists(pid: u32) -> Result<bool, String> {
    let pid = i32::try_from(pid).map_err(|_| "Binary verification process id overflowed")?;
    if unsafe { libc::kill(-pid, 0) } == 0 {
        return Ok(true);
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err("Cannot inspect binary verification process group".into()),
    }
}

fn cleanup_verification_group(pid: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        if !verification_group_exists(pid)? {
            return Ok(());
        }
        let group = -i32::try_from(pid).map_err(|_| "Binary verification process id overflowed")?;
        for (signal, grace) in [
            (libc::SIGTERM, Duration::from_millis(100)),
            (libc::SIGKILL, Duration::from_secs(1)),
        ] {
            unsafe { libc::kill(group, signal) };
            let deadline = Instant::now() + grace;
            loop {
                if !verification_group_exists(pid)? {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        Err("Binary verification process group cleanup did not complete".into())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Ok(())
    }
}

fn run_checked(command: &mut Command, timeout: Duration) -> Result<String, String> {
    // A regular temporary output file cannot block readback on inherited pipes.
    let mut output = tempfile::tempfile().map_err(|error| error.to_string())?;
    command
        .stdout(output.try_clone().map_err(|error| error.to_string())?)
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Cannot launch binary verification: {error}"))?;
    let deadline = Instant::now() + timeout;
    let process_result = loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break Ok(()),
            Ok(Some(_)) => break Err("Binary signing or verification command failed".to_string()),
            Err(error) => break Err(format!("Cannot observe binary verification: {error}")),
            Ok(None) if Instant::now() >= deadline => {
                break Err("Binary verification timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    if process_result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let cleanup = cleanup_verification_group(child.id());
    match (process_result, cleanup) {
        (Err(error), Err(cleanup)) => return Err(format!("{error}; {cleanup}")),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => return Err(error),
        (Ok(()), Ok(())) => {}
    }
    output
        .seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    output
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 65536 {
        return Err("Binary verification output exceeded 64 KiB".into());
    }
    String::from_utf8(bytes).map_err(|_| "Binary verification output was not UTF-8".into())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct FixturePlatform {
        signatures: AtomicUsize,
        readbacks: AtomicUsize,
        fail_sign: bool,
        fail_installed_signature: bool,
        block_rollback: bool,
        mismatch_installed_readback: bool,
    }

    impl Platform for FixturePlatform {
        fn sign(&self, path: &Path) -> Result<(), String> {
            if self.fail_sign {
                return Err("fixture signing failure".into());
            }
            OpenOptions::new()
                .append(true)
                .open(path)
                .unwrap()
                .write_all(b"-signed")
                .unwrap();
            Ok(())
        }

        fn signature(&self, path: &Path) -> Result<(), String> {
            let call = self.signatures.fetch_add(1, Ordering::SeqCst);
            if self.block_rollback && call == 1 {
                std::fs::remove_file(path).unwrap();
                std::fs::create_dir(path).unwrap();
                return Err("fixture post-install failure with blocked rollback".into());
            }
            if self.fail_installed_signature && call == 1 {
                return Err("fixture installed signature failure".into());
            }
            Ok(())
        }

        fn readback(&self, _path: &Path) -> Result<(String, String), String> {
            let call = self.readbacks.fetch_add(1, Ordering::SeqCst);
            let version = if self.mismatch_installed_readback && call == 1 {
                "ca wrong-version"
            } else {
                "ca 9.8.7"
            };
            Ok((version.into(), "Usage: ca".into()))
        }
    }

    fn fixture_file(path: &Path, bytes: &[u8]) {
        std::fs::write(path, bytes).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn request(root: &Path) -> InstallRequest {
        fixture_file(&root.join("build"), b"new-build");
        fixture_file(&root.join("installed"), b"previous-build");
        InstallRequest {
            source: root.join("build"),
            destination: root.join("installed"),
            backup: true,
            verify: true,
            self_update: false,
        }
    }

    fn recoveries(root: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".recovery-")
            })
            .collect()
    }

    #[test]
    fn ordinary_0755_build_signs_stage_and_names_recovery_with_previous_hash() {
        let root = tempfile::tempdir().unwrap();
        let request = request(root.path());
        let old_hash = hash_file(&request.destination).unwrap();
        let source_hash = hash_file(&request.source).unwrap();
        let platform = FixturePlatform::default();
        let receipt = install_with(request, &platform).unwrap();
        assert_eq!(
            std::fs::read(root.path().join("build")).unwrap(),
            b"new-build"
        );
        assert_eq!(
            std::fs::read(root.path().join("installed")).unwrap(),
            b"new-build-signed"
        );
        let backups = recoveries(root.path());
        assert_eq!(backups.len(), 1);
        assert!(backups[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(&old_hash));
        assert_eq!(hash_file(&backups[0]).unwrap(), old_hash);
        assert!(receipt.contains(&format!("Source SHA-256: {source_hash}")));
        assert!(receipt.contains(&hash_file(&root.path().join("installed")).unwrap()));
        verify_mode_owner(&root.path().join("installed")).unwrap();
        assert_eq!(platform.signatures.load(Ordering::SeqCst), 2);
        assert!(!root.path().read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".ca-install-")));
    }

    #[test]
    fn failed_installed_signature_restores_original_bytes_and_mode() {
        use std::os::unix::fs::MetadataExt;

        let root = tempfile::tempdir().unwrap();
        let request = request(root.path());
        let original = std::fs::metadata(&request.destination).unwrap();
        let platform = FixturePlatform {
            fail_installed_signature: true,
            ..Default::default()
        };
        let error = install_with(request, &platform).unwrap_err();
        assert!(error.contains("previous state restored"), "{error}");
        assert_eq!(
            std::fs::read(root.path().join("installed")).unwrap(),
            b"previous-build"
        );
        let restored = std::fs::metadata(root.path().join("installed")).unwrap();
        assert_eq!(restored.uid(), original.uid());
        assert_eq!(restored.gid(), original.gid());
        assert_eq!(restored.mode() & 0o7777, original.mode() & 0o7777);
        assert_eq!(recoveries(root.path()).len(), 1);
        assert_eq!(platform.signatures.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn self_update_forces_durable_recovery_and_full_readback_when_flags_false() {
        let root = tempfile::tempdir().unwrap();
        let mut request = request(root.path());
        request.backup = false;
        request.verify = false;
        request.self_update = true;
        let platform = FixturePlatform {
            mismatch_installed_readback: true,
            ..Default::default()
        };
        let error = install_with(request, &platform).unwrap_err();
        assert!(error.contains("version/help"), "{error}");
        assert!(error.contains("previous state restored"), "{error}");
        assert_eq!(
            std::fs::read(root.path().join("installed")).unwrap(),
            b"previous-build"
        );
        assert_eq!(recoveries(root.path()).len(), 1);
        assert_eq!(platform.readbacks.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn failed_rollback_preserves_the_reported_private_recovery_when_backup_is_false() {
        let root = tempfile::tempdir().unwrap();
        let mut request = request(root.path());
        request.backup = false;
        let old_hash = hash_file(&request.destination).unwrap();
        let error = install_with(
            request,
            &FixturePlatform {
                block_rollback: true,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(error.contains("ROLLBACK FAILED"), "{error}");
        let recovery = PathBuf::from(error.split_once("recovery: ").unwrap().1);
        assert!(recovery.starts_with(root.path()));
        assert!(recovery.parent().unwrap().is_dir());
        assert_eq!(std::fs::read(&recovery).unwrap(), b"previous-build");
        assert_eq!(hash_file(&recovery).unwrap(), old_hash);
        assert!(recoveries(root.path()).is_empty());
    }

    #[test]
    fn signing_failure_leaves_destination_untouched_without_spurious_backup() {
        let root = tempfile::tempdir().unwrap();
        let request = request(root.path());
        let error = install_with(
            request,
            &FixturePlatform {
                fail_sign: true,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(error.contains("signing failure"));
        assert_eq!(
            std::fs::read(root.path().join("installed")).unwrap(),
            b"previous-build"
        );
        assert!(recoveries(root.path()).is_empty());
    }

    #[test]
    fn source_and_destination_symlinks_are_rejected_without_replacement() {
        for source_symlink in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let mut request = request(root.path());
            let link = root.path().join("link");
            let target = if source_symlink {
                &request.source
            } else {
                &request.destination
            };
            std::os::unix::fs::symlink(target, &link).unwrap();
            if source_symlink {
                request.source = link;
            } else {
                request.destination = link;
            }
            assert!(install_with(request, &FixturePlatform::default()).is_err());
            assert_eq!(
                std::fs::read(root.path().join("installed")).unwrap(),
                b"previous-build"
            );
        }
    }

    #[test]
    fn verification_process_timeout_is_bounded_and_reaped() {
        let began = Instant::now();
        let error = run_checked(
            Command::new("/bin/sh").args(["-c", "sleep 30"]),
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(began.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn verification_cleans_descendants_after_successful_and_failed_parent_exit() {
        for exit_code in ["0", "1"] {
            let root = tempfile::tempdir().unwrap();
            let pids = root.path().join("pids");
            let began = Instant::now();
            let result = run_checked(
                Command::new("/bin/sh")
                    .args([
                        "-c",
                        "sleep 30 & printf '%s %s\\n' \"$$\" \"$!\" > \"$1\"; exit \"$2\"",
                        "fixture",
                    ])
                    .arg(&pids)
                    .arg(exit_code),
                Duration::from_secs(2),
            );
            let pids = std::fs::read_to_string(pids).unwrap();
            let mut pids = pids
                .split_whitespace()
                .map(|pid| pid.parse::<i32>().unwrap());
            let parent = pids.next().unwrap();
            let descendant = pids.next().unwrap();
            let descendant_alive = unsafe { libc::kill(descendant, 0) } == 0;
            if descendant_alive {
                unsafe { libc::kill(descendant, libc::SIGKILL) };
            }
            assert!(
                !descendant_alive,
                "verifier descendant survived parent exit {exit_code}"
            );
            assert!(!verification_group_exists(parent as u32).unwrap());
            assert_eq!(result.is_ok(), exit_code == "0", "{result:?}");
            if let Err(error) = result {
                assert!(error.contains("command failed"), "{error}");
            }
            assert!(began.elapsed() < Duration::from_secs(2));
        }
    }

    #[test]
    fn native_install_signs_ordinary_build_without_modifying_source() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("build");
        std::fs::copy("/bin/echo", &source).unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o755)).unwrap();
        let original_hash = hash_file(&source).unwrap();
        let destination = root.path().join("installed");
        install(InstallRequest {
            source: source.clone(),
            destination: destination.clone(),
            backup: true,
            verify: true,
            self_update: false,
        })
        .unwrap();
        assert_eq!(hash_file(&source).unwrap(), original_hash);
        verify_mode_owner(&destination).unwrap();
        NativePlatform.signature(&destination).unwrap();
        let output = Command::new(&destination)
            .arg("native-install-ok")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            "native-install-ok"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_install_normalizes_inherited_group_in_private_stage_and_preserves_rollback_metadata()
    {
        use std::os::unix::fs::MetadataExt;

        struct InspectNativePlatform {
            signatures: AtomicUsize,
            fail_installed_signature: bool,
        }

        impl Platform for InspectNativePlatform {
            fn sign(&self, path: &Path) -> Result<(), String> {
                let stage = std::fs::metadata(path.parent().unwrap()).unwrap();
                assert_eq!(stage.mode() & 0o7777, 0o700);
                assert_eq!(stage.uid(), unsafe { libc::geteuid() });
                verify_mode_owner(path)?;
                NativePlatform.sign(path)
            }

            fn signature(&self, path: &Path) -> Result<(), String> {
                if self.signatures.fetch_add(1, Ordering::SeqCst) == 1
                    && self.fail_installed_signature
                {
                    return Err("fixture failure after native installation".into());
                }
                NativePlatform.signature(path)
            }

            fn readback(&self, path: &Path) -> Result<(String, String), String> {
                NativePlatform.readback(path)
            }
        }

        for fail_installed_signature in [false, true] {
            let root = tempfile::Builder::new()
                .prefix("ca-install-inherited-group-")
                .tempdir_in("/private/var/tmp")
                .unwrap();
            let source = root.path().join("build");
            let destination = root.path().join("installed");
            std::fs::copy("/bin/echo", &source).unwrap();
            std::fs::copy("/bin/echo", &destination).unwrap();
            std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o750)).unwrap();
            let source_hash = hash_file(&source).unwrap();
            let source_metadata = std::fs::metadata(&source).unwrap();
            let old_metadata = std::fs::metadata(&destination).unwrap();
            assert_eq!(
                source_metadata.gid(),
                std::fs::metadata(root.path()).unwrap().gid(),
                "fixture must preserve macOS parent-group inheritance"
            );
            let result = install_with(
                InstallRequest {
                    source: source.clone(),
                    destination: destination.clone(),
                    backup: true,
                    verify: true,
                    self_update: false,
                },
                &InspectNativePlatform {
                    signatures: AtomicUsize::new(0),
                    fail_installed_signature,
                },
            );
            let installed = std::fs::metadata(&destination).unwrap();
            if fail_installed_signature {
                assert!(result.unwrap_err().contains("previous state restored"));
                assert_eq!(installed.uid(), old_metadata.uid());
                assert_eq!(installed.gid(), old_metadata.gid());
                assert_eq!(installed.mode() & 0o7777, old_metadata.mode() & 0o7777);
                assert_eq!(hash_file(&destination).unwrap(), source_hash);
            } else {
                result.unwrap();
                verify_mode_owner(&destination).unwrap();
            }
            NativePlatform.signature(&destination).unwrap();
            assert_eq!(hash_file(&source).unwrap(), source_hash);
            let unchanged_source = std::fs::metadata(&source).unwrap();
            assert_eq!(unchanged_source.uid(), source_metadata.uid());
            assert_eq!(unchanged_source.gid(), source_metadata.gid());
            assert_eq!(unchanged_source.mode(), source_metadata.mode());
        }
    }
}
