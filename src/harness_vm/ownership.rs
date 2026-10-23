//! A controller keeps this lock until its independent supervisor exits. Closing
//! the caller's lease pipe starts cleanup; it must not admit the next guest.
use anyhow::{bail, Context, Result};
use std::fs::{File, OpenOptions};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

pub(crate) struct VmOwnership(File);

impl VmOwnership {
    pub(crate) fn acquire(home: &Path) -> Result<Self> {
        if home.canonicalize()? != home {
            bail!("VM ownership directory is not canonical");
        }
        let path = home.join(".ca-harness-owner.lock");
        super::validate_parents(&path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(&path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o7077 != 0
        {
            bail!("VM ownership lock must be a private owned regular file");
        }
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(std::io::Error::last_os_error())
                .context("another dedicated VM controller is active or cleaning up");
        }
        // Avoid standard streams and spawn's low-numbered internal descriptors.
        // Duplicates share the flock; release only by closing the last copy.
        let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 64) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error()).context("duplicate VM ownership");
        }
        Ok(Self(unsafe { File::from_raw_fd(fd) }))
    }

    /// Keep `self` alive through spawn. Only the forked supervisor receives the
    /// descriptor; concurrent children retain close-on-exec in the parent.
    pub(super) fn inherit(&self, command: &mut tokio::process::Command) {
        let fd = self.0.as_raw_fd();
        unsafe {
            command.pre_exec(move || {
                if libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::process::Stdio;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, BufReader};

    #[tokio::test]
    async fn hrs253_vm_ownership_survives_parent_release_until_supervisor_exit() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().canonicalize().unwrap();
        let owner = VmOwnership::acquire(&home).unwrap();
        assert!(VmOwnership::acquire(&home).is_err());
        let mut command = tokio::process::Command::new(crate::harness_vm::HOST_PYTHON);
        command
            .args([
                "-I",
                "-B",
                "-c",
                r#"
import os, select, subprocess, sys
fd = int(sys.argv[1])
os.fstat(fd)
# The supervisor's children must not retain ownership after it exits.
probe = 'import os,sys; os.fstat(int(sys.argv[1]))'
child = subprocess.run([sys.executable, '-I', '-c', probe, str(fd)],
                       stderr=subprocess.DEVNULL, timeout=3)
assert child.returncode != 0
print('ready', flush=True)
assert select.select([sys.stdin], [], [], 10)[0]
assert os.read(sys.stdin.fileno(), 1) == b''
"#,
            ])
            .arg(owner.0.as_raw_fd().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        owner.inherit(&mut command);
        let mut child = command.spawn().unwrap();
        let input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
        let ready = tokio::time::timeout(Duration::from_secs(5), output.next_line())
            .await
            .unwrap()
            .unwrap();
        if ready.as_deref() != Some("ready") {
            child.kill().await.ok();
            child.wait().await.ok();
            panic!("supervisor did not inherit VM ownership");
        }
        drop(owner);
        let remained_locked = VmOwnership::acquire(&home).is_err();
        drop(input);
        let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(
            remained_locked,
            "parent release admitted overlapping VM work"
        );
        assert!(status.success());
        assert!(VmOwnership::acquire(&home).is_ok());
    }

    #[test]
    fn hrs253_vm_ownership_rejects_linked_or_shared_locks() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().canonicalize().unwrap();
        let path = home.join(".ca-harness-owner.lock");
        drop(VmOwnership::acquire(&home).unwrap());
        let saved = home.join("saved-lock");
        std::fs::rename(&path, &saved).unwrap();
        symlink(&saved, &path).unwrap();
        assert!(VmOwnership::acquire(&home).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::hard_link(&saved, &path).unwrap();
        assert!(VmOwnership::acquire(&home).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::rename(&saved, &path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(VmOwnership::acquire(&home).is_err());
    }
}
