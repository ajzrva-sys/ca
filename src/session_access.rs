//! One nonblocking writer identity for every local session participant.
//!
//! Lock files are stable: removing a participating file would allow two
//! processes to lock different inodes for the same session.
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SessionAccessCoordinator {
    scope: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AccessKey {
    scope: PathBuf,
    session_id: Uuid,
}

struct LeaseInner {
    key: AccessKey,
    #[cfg(unix)]
    owner_pid: u32,
    _file: File,
}

impl Drop for LeaseInner {
    fn drop(&mut self) {
        #[cfg(unix)]
        if self.owner_pid == std::process::id() {
            use std::os::fd::AsRawFd;
            // A pre-exec child may still hold a copied descriptor. Session
            // authority ends at the last original-process capability, so do
            // not depend on the child's eventual CLOEXEC close to release it.
            let _ = unsafe { libc::flock(self._file.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

/// Clones share one writer ownership. Acquisition is deliberately not reentrant;
/// callers already holding ownership pass this capability to owned operations.
#[derive(Clone)]
pub struct SessionWriterLease {
    inner: Arc<LeaseInner>,
}

impl std::fmt::Debug for SessionWriterLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionWriterLease")
            .field("session_id", &self.inner.key.session_id)
            .finish_non_exhaustive()
    }
}

fn owners() -> &'static Mutex<HashMap<AccessKey, Weak<LeaseInner>>> {
    static OWNERS: OnceLock<Mutex<HashMap<AccessKey, Weak<LeaseInner>>>> = OnceLock::new();
    OWNERS.get_or_init(|| Mutex::new(HashMap::new()))
}

impl SessionAccessCoordinator {
    pub fn new(scope: impl Into<PathBuf>) -> Self {
        Self {
            scope: scope.into(),
        }
    }

    /// Default active/archive directories share their profile parent. Custom
    /// stores can instead pass an explicit common scope to `new`.
    pub fn for_session_root(root: &Path) -> Self {
        let scope = if matches!(
            root.file_name().and_then(|n| n.to_str()),
            Some("sessions" | "archive")
        ) {
            root.parent().unwrap_or(root)
        } else {
            root
        };
        Self::new(scope)
    }

    pub fn scope(&self) -> &Path {
        &self.scope
    }

    pub fn try_writer(&self, session_id: Uuid) -> io::Result<SessionWriterLease> {
        std::fs::create_dir_all(&self.scope)?;
        let scope = std::fs::canonicalize(&self.scope)?;
        let key = AccessKey { scope, session_id };
        let mut owners = owners()
            .lock()
            .map_err(|_| io::Error::other("session ownership coordinator is unavailable"))?;
        owners.retain(|_, owner| owner.strong_count() > 0);
        if owners.get(&key).and_then(Weak::upgrade).is_some() {
            return Err(busy());
        }
        let dir = key.scope.join(".session-access");
        std::fs::create_dir_all(&dir)?;
        let metadata = std::fs::symlink_metadata(&dir)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session lock storage is not a directory",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let path = dir.join(format!("{session_id}.lock"));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let file = options.open(&path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session lock is not a regular file",
            ));
        }
        lock_file(&file)?;
        let inner = Arc::new(LeaseInner {
            key: key.clone(),
            #[cfg(unix)]
            owner_pid: std::process::id(),
            _file: file,
        });
        owners.insert(key, Arc::downgrade(&inner));
        Ok(SessionWriterLease { inner })
    }

    pub fn validate(&self, lease: &SessionWriterLease, session_id: Uuid) -> io::Result<()> {
        if lease.inner.key.session_id != session_id
            || std::fs::canonicalize(&self.scope)? != lease.inner.key.scope
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "session writer ownership has a different storage or session identity",
            ));
        }
        Ok(())
    }
}

fn busy() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "session is busy in another local writer",
    )
}

#[cfg(unix)]
fn lock_file(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    // flock belongs to this open file description. The last original-process
    // capability explicitly releases it; process death also closes the file.
    // CLOEXEC prevents passing session authority to an executed child.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::WouldBlock {
        Err(busy())
    } else {
        Err(error)
    }
}

#[cfg(not(unix))]
fn lock_file(_file: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "session writer coordination is not supported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_is_nonreentrant_and_last_clone_releases_stable_file() {
        let root = tempfile::tempdir().unwrap();
        let access = SessionAccessCoordinator::new(root.path());
        let id = Uuid::new_v4();
        let lease = access.try_writer(id).unwrap();
        let clone = lease.clone();
        assert_eq!(
            access.try_writer(id).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(lease);
        assert_eq!(
            access.try_writer(id).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(clone);
        let path = root
            .path()
            .join(".session-access")
            .join(format!("{id}.lock"));
        assert!(path.is_file());
        access.try_writer(id).unwrap();
        assert!(path.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn last_clone_releases_writer_even_if_an_inherited_descriptor_remains_open() {
        use std::os::fd::{AsRawFd, FromRawFd};

        let root = tempfile::tempdir().unwrap();
        let access = SessionAccessCoordinator::new(root.path());
        let id = Uuid::new_v4();
        let lease = access.try_writer(id).unwrap();
        let clone = lease.clone();
        // Model a descriptor copied by fork before CLOEXEC closes it in exec.
        let inherited_fd =
            unsafe { libc::fcntl(lease.inner._file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
        assert!(inherited_fd >= 0);
        let inherited = unsafe { File::from_raw_fd(inherited_fd) };

        drop(lease);
        assert_eq!(
            access.try_writer(id).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(clone);
        let next = access
            .try_writer(id)
            .expect("the last writer capability releases ownership");
        assert_eq!(
            access.try_writer(id).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(inherited);
        assert_eq!(
            access.try_writer(id).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(next);
        assert!(access.try_writer(id).is_ok());
    }

    #[test]
    fn lease_is_bound_to_canonical_scope_and_session() {
        let root = tempfile::tempdir().unwrap();
        let access = SessionAccessCoordinator::new(root.path());
        let id = Uuid::new_v4();
        let lease = access.try_writer(id).unwrap();
        SessionAccessCoordinator::new(root.path().join("."))
            .validate(&lease, id)
            .unwrap();
        assert!(access.validate(&lease, Uuid::new_v4()).is_err());
        let other = tempfile::tempdir().unwrap();
        assert!(SessionAccessCoordinator::new(other.path())
            .validate(&lease, id)
            .is_err());
    }

    #[test]
    fn active_and_archive_roots_share_the_profile_scope() {
        let root = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let lease = SessionAccessCoordinator::for_session_root(&root.path().join("sessions"))
            .try_writer(id)
            .unwrap();
        assert_eq!(
            SessionAccessCoordinator::for_session_root(&root.path().join("archive"))
                .try_writer(id)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(lease);
    }

    #[test]
    fn child_writer_probe() {
        let Some(root) = std::env::var_os("CA_SESSION_ACCESS_TEST_ROOT") else {
            return;
        };
        let id = Uuid::parse_str(&std::env::var("CA_SESSION_ACCESS_TEST_ID").unwrap()).unwrap();
        let access = SessionAccessCoordinator::new(PathBuf::from(root));
        assert_eq!(
            access.try_writer(id).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn another_process_observes_writer_ownership() {
        let root = tempfile::tempdir().unwrap();
        let access = SessionAccessCoordinator::new(root.path());
        let id = Uuid::new_v4();
        let _lease = access.try_writer(id).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["session_access::tests::child_writer_probe", "--exact"])
            .env("CA_SESSION_ACCESS_TEST_ROOT", root.path())
            .env("CA_SESSION_ACCESS_TEST_ID", id.to_string())
            .status()
            .unwrap();
        assert!(status.success());
    }
}
