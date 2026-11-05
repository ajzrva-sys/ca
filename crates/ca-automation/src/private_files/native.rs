use super::{Fingerprint, NativePath};
use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

fn cstr(value: &OsStr) -> Result<CString> {
    Ok(CString::new(value.as_bytes())?)
}
fn checked_fd(fd: i32) -> std::io::Result<File> {
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
fn stat_fp(s: &libc::stat) -> Fingerprint {
    let (modified, changed) = ((s.st_mtime, s.st_mtime_nsec), (s.st_ctime, s.st_ctime_nsec));
    Fingerprint {
        device: s.st_dev as u64,
        inode: s.st_ino,
        size: s.st_size as u64,
        modified,
        changed,
        mode: s.st_mode as u32,
        uid: s.st_uid,
        gid: s.st_gid,
        sha256: None,
    }
}
pub(super) fn supported(fp: &Fingerprint) -> bool {
    matches!(fp.mode & libc::S_IFMT as u32, v if v == libc::S_IFREG as u32 || v == libc::S_IFLNK as u32)
}
pub(super) fn fingerprint(path: &Path, hash: bool) -> Result<Fingerprint> {
    let parent = path.parent().unwrap_or(Path::new("/"));
    let name = path.file_name().unwrap_or(OsStr::new("."));
    let parent = cstr(parent.as_os_str())?;
    let dir = checked_fd(unsafe {
        libc::open(
            parent.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    })?;
    Entry {
        parent: dir,
        name: cstr(name)?,
    }
    .fingerprint(hash)?
    .context("file is unavailable")
}

pub(super) struct Entry {
    parent: File,
    name: CString,
}
impl Entry {
    pub(super) fn open(
        root: &Path,
        expected: &Fingerprint,
        path: &Path,
        create: bool,
    ) -> Result<Self> {
        let relative = path
            .strip_prefix(root)
            .context("entry is outside its bound folder")?;
        let mut components = relative.components().collect::<Vec<_>>();
        let Some(Component::Normal(name)) = components.pop() else {
            anyhow::bail!("invalid entry name")
        };
        let root_name = cstr(root.as_os_str())?;
        let mut dir = checked_fd(unsafe {
            libc::open(
                root_name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        ensure!(
            unsafe { libc::fstat(dir.as_raw_fd(), stat.as_mut_ptr()) } == 0,
            "folder identity cannot be read"
        );
        ensure!(
            stat_fp(&unsafe { stat.assume_init() }).same_node(expected),
            "bound folder identity changed"
        );
        for component in components {
            let Component::Normal(part) = component else {
                anyhow::bail!("invalid entry traversal")
            };
            let part = cstr(part)?;
            let mut fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    part.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0
                && create
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT)
            {
                let result = unsafe { libc::mkdirat(dir.as_raw_fd(), part.as_ptr(), 0o755) };
                ensure!(
                    result == 0
                        || std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST),
                    "destination directory cannot be created"
                );
                dir.sync_all()?;
                fd = unsafe {
                    libc::openat(
                        dir.as_raw_fd(),
                        part.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
            }
            dir = checked_fd(fd)
                .context("entry parent is unavailable or contains a symbolic link")?;
        }
        Ok(Self {
            parent: dir,
            name: cstr(name)?,
        })
    }
    fn stat(&self) -> std::io::Result<Option<libc::stat>> {
        let mut value = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                value.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0
        {
            Ok(Some(unsafe { value.assume_init() }))
        } else {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ENOENT) {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
    fn read_link(&self) -> Result<Vec<u8>> {
        let mut value = vec![0; 65536];
        let n = unsafe {
            libc::readlinkat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
            )
        };
        ensure!(
            n >= 0 && (n as usize) < value.len(),
            "symbolic link target cannot be read"
        );
        value.truncate(n as usize);
        Ok(value)
    }
    fn open_file(&self, write: bool) -> std::io::Result<File> {
        checked_fd(unsafe {
            libc::openat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                (if write {
                    libc::O_WRONLY
                } else {
                    libc::O_RDONLY
                }) | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC,
            )
        })
    }
    pub(super) fn fingerprint(&self, hash: bool) -> Result<Option<Fingerprint>> {
        let Some(stat) = self.stat()? else {
            return Ok(None);
        };
        let mut result = stat_fp(&stat);
        if hash {
            match stat.st_mode & libc::S_IFMT {
                libc::S_IFREG => {
                    let mut file = self.open_file(false)?;
                    let mut current = std::mem::MaybeUninit::<libc::stat>::uninit();
                    ensure!(
                        unsafe { libc::fstat(file.as_raw_fd(), current.as_mut_ptr()) } == 0
                            && stat_fp(&unsafe { current.assume_init() }) == result,
                        "file changed before read"
                    );
                    let mut sha = Sha256::new();
                    let mut buffer = vec![0; 1024 * 1024];
                    loop {
                        let n = file.read(&mut buffer)?;
                        if n == 0 {
                            break;
                        }
                        sha.update(&buffer[..n]);
                    }
                    result.sha256 = Some(format!("{:x}", sha.finalize()));
                }
                libc::S_IFLNK => {
                    result.sha256 = Some(format!("{:x}", Sha256::digest(self.read_link()?)))
                }
                _ => {}
            }
        }
        let after = self.stat()?.context("file disappeared during read")?;
        ensure!(
            stat_fp(&after) == stat_fp(&stat),
            "file changed during read"
        );
        Ok(Some(result))
    }
    pub(super) fn rename_exclusive(&self, to: &Self) -> std::io::Result<()> {
        #[cfg(target_os = "macos")]
        let result = unsafe {
            libc::renameatx_np(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                to.parent.as_raw_fd(),
                to.name.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                to.parent.as_raw_fd(),
                to.name.as_ptr(),
                libc::RENAME_NOREPLACE,
            ) as i32
        };
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        let result = -1;
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }
        to.parent.sync_all()?;
        self.parent.sync_all()
    }
    pub(super) fn unlink(&self) -> Result<()> {
        ensure!(
            unsafe { libc::unlinkat(self.parent.as_raw_fd(), self.name.as_ptr(), 0) } == 0,
            "source removal failed"
        );
        self.parent.sync_all()?;
        Ok(())
    }
    pub(super) fn create_from(&self, from: &Self, _copy: bool) -> Result<Fingerprint> {
        let source = from.stat()?.context("source disappeared")?;
        if source.st_mode & libc::S_IFMT == libc::S_IFLNK {
            let target = CString::new(from.read_link()?)?;
            ensure!(
                unsafe {
                    libc::symlinkat(target.as_ptr(), self.parent.as_raw_fd(), self.name.as_ptr())
                } == 0,
                "exclusive symbolic link creation failed"
            );
        } else {
            let file = checked_fd(unsafe {
                libc::openat(
                    self.parent.as_raw_fd(),
                    self.name.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            })?;
            file.sync_all()?;
        }
        self.parent.sync_all()?;
        self.fingerprint(false)?
            .context("temporary creation failed")
    }
    pub(super) fn copy_from(&self, from: &Self) -> Result<()> {
        let source = from.stat()?.context("source disappeared")?;
        if source.st_mode & libc::S_IFMT == libc::S_IFLNK {
            ensure!(
                self.read_link()? == from.read_link()?,
                "symbolic link changed during copy"
            );
            ensure!(
                unsafe {
                    libc::fchownat(
                        self.parent.as_raw_fd(),
                        self.name.as_ptr(),
                        source.st_uid,
                        source.st_gid,
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                } == 0,
                "symbolic link ownership could not be preserved"
            );
            let times = stat_times(&source);
            ensure!(
                unsafe {
                    libc::utimensat(
                        self.parent.as_raw_fd(),
                        self.name.as_ptr(),
                        times.as_ptr(),
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                } == 0,
                "symbolic link times could not be preserved"
            );
        } else {
            let mut input = from.open_file(false)?;
            let mut output = self.open_file(true)?;
            #[cfg(target_os = "macos")]
            {
                // COPYFILE_ALL copies data, stat, ACLs, xattrs and resource forks
                // between already-open descriptors; it never follows a link.
                unsafe extern "C" {
                    fn fcopyfile(from: i32, to: i32, state: *mut libc::c_void, flags: u32) -> i32;
                }
                ensure!(
                    unsafe {
                        fcopyfile(
                            input.as_raw_fd(),
                            output.as_raw_fd(),
                            std::ptr::null_mut(),
                            15,
                        )
                    } == 0,
                    "native metadata copy failed"
                );
            }
            #[cfg(not(target_os = "macos"))]
            {
                std::io::copy(&mut input, &mut output)?;
                ensure!(
                    unsafe { libc::fchown(output.as_raw_fd(), source.st_uid, source.st_gid) } == 0,
                    "file ownership could not be preserved"
                );
                ensure!(
                    unsafe { libc::fchmod(output.as_raw_fd(), source.st_mode & 0o7777) } == 0,
                    "file mode could not be preserved"
                );
                copy_xattrs(&input, &output)?;
                let times = stat_times(&source);
                ensure!(
                    unsafe { libc::futimens(output.as_raw_fd(), times.as_ptr()) } == 0,
                    "file times could not be preserved"
                );
            }
            // Keep mutable bindings useful on all supported targets.
            let _ = (&mut input, &mut output);
            output.flush()?;
            output.sync_all()?;
        }
        self.parent.sync_all()?;
        Ok(())
    }
}

fn stat_times(s: &libc::stat) -> [libc::timespec; 2] {
    {
        [
            libc::timespec {
                tv_sec: s.st_atime,
                tv_nsec: s.st_atime_nsec,
            },
            libc::timespec {
                tv_sec: s.st_mtime,
                tv_nsec: s.st_mtime_nsec,
            },
        ]
    }
}

#[cfg(target_os = "linux")]
fn copy_xattrs(from: &File, to: &File) -> Result<()> {
    let size = unsafe { libc::flistxattr(from.as_raw_fd(), std::ptr::null_mut(), 0) };
    ensure!(
        size >= 0 && size <= 16 * 1024 * 1024,
        "extended attributes cannot be enumerated"
    );
    let mut names = vec![0u8; size as usize];
    ensure!(
        unsafe { libc::flistxattr(from.as_raw_fd(), names.as_mut_ptr().cast(), names.len()) }
            == size,
        "extended attributes changed"
    );
    for name in names.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let name = CString::new(name)?;
        let size =
            unsafe { libc::fgetxattr(from.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0) };
        ensure!(
            size >= 0 && size <= 16 * 1024 * 1024,
            "extended attribute cannot be read"
        );
        let mut value = vec![0u8; size as usize];
        ensure!(
            unsafe {
                libc::fgetxattr(
                    from.as_raw_fd(),
                    name.as_ptr(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                )
            } == size,
            "extended attribute changed"
        );
        ensure!(
            unsafe {
                libc::fsetxattr(
                    to.as_raw_fd(),
                    name.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                )
            } == 0,
            "extended attribute cannot be preserved"
        );
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn copy_xattrs(_from: &File, _to: &File) -> Result<()> {
    anyhow::bail!("native metadata preservation is unavailable")
}

// Keep native byte roundtrips explicit in this module's compile surface.
const _: fn(&Path) -> NativePath = NativePath::new;
