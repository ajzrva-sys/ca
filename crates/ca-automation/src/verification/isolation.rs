//! Required deterministic-verification isolation.
//!
//! Learned evidence recipes never execute in the governed working tree.  This
//! module creates a private source projection, exposes only dependency caches
//! (not Cargo config/credentials), scrubs the environment, and launches the
//! typed command under macOS Seatbelt with network and original-repository
//! access denied. Unsupported hosts fail closed.

use anyhow::{anyhow, bail, Context, Result};
use std::collections::BTreeSet;
use std::ffi::{CString, OsStr};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
#[cfg(test)]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cancellation::CancellationToken;
use crate::paths::Paths;
use crate::verification::recipe::VerificationCommand;

const MAX_PROJECTED_FILES: usize = 20_000;
const MAX_PROJECTED_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PROJECTED_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const MAX_GIT_LIST_BYTES: usize = 8 * 1024 * 1024;
static SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(1);
#[cfg(test)]
static TEST_ISOLATION_READY: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub(crate) struct IsolationReadiness {
    pub(crate) ready: bool,
    pub(crate) backend: String,
    pub(crate) summary: String,
}

#[derive(Debug)]
pub(crate) struct PreparedIsolatedCommand {
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<String>,
    pub(crate) cwd: PathBuf,
    pub(crate) env: Vec<(String, String)>,
    _scratch: Arc<ScratchDirectory>,
    _system_tls_config: Option<File>,
}

#[derive(Debug)]
pub(crate) struct IsolationSession {
    scratch: Arc<ScratchDirectory>,
    denied_source_root: PathBuf,
    source_root: PathBuf,
    target: PathBuf,
    home: PathBuf,
    cargo_home: PathBuf,
    tmp: PathBuf,
    cache_reads: Vec<PathBuf>,
    sandboxed: bool,
}

#[derive(Debug)]
struct ScratchDirectory {
    path: PathBuf,
    base: PathBuf,
    device: u64,
    inode: u64,
    _owner: File,
}

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        let expected = format!("codex-team-verification-{}-", std::process::id());
        if self
            .path
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with(&expected))
            && self.path.parent() == Some(self.base.as_path())
            && fs::symlink_metadata(&self.path).is_ok_and(|metadata| {
                metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && metadata.uid() == unsafe { libc::geteuid() }
                    && metadata.dev() == self.device
                    && metadata.ino() == self.inode
            })
        {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub(crate) fn require_isolation_backend() -> Result<IsolationReadiness> {
    #[cfg(debug_assertions)]
    if test_isolation_double_active() {
        return Ok(IsolationReadiness {
            ready: true,
            backend: "debug-integration-preflight-double".to_string(),
            summary: "debug-only isolated-command test double".to_string(),
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        bail!("required verification isolation is currently supported only on macOS");
    }
    #[cfg(target_os = "macos")]
    {
        let sandbox = Path::new("/usr/bin/sandbox-exec");
        require_regular_executable(sandbox)?;
        let scratch = create_scratch()?;
        let profile = isolation_profile(
            &scratch.path,
            &scratch.path,
            Path::new("/usr/bin"),
            &[],
            Path::new("/nonexistent/codex-team-verification-source"),
        );
        let output = Command::new(sandbox)
            .env_clear()
            .args(["-p", &profile, "/usr/bin/true"])
            .output()
            .context("run verification isolation self-test")?;
        if !output.status.success() {
            let diagnostic = String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(4096)
                .collect::<String>();
            bail!(
                "required verification isolation self-test failed ({:?}); learned recipe approval and execution remain disabled: {}",
                output.status.code(),
                diagnostic.trim()
            );
        }
        Ok(IsolationReadiness {
            ready: true,
            backend: "macos-seatbelt-source-projection-v1".to_string(),
            summary: "private source projection, scrubbed environment, offline dependencies, original repository denied, network denied".to_string(),
        })
    }
}

#[cfg(test)]
pub(crate) fn enable_test_isolation_backend() {
    TEST_ISOLATION_READY.store(true, Ordering::Relaxed);
}

#[cfg(debug_assertions)]
fn test_isolation_double_active() -> bool {
    #[cfg(test)]
    if TEST_ISOLATION_READY.load(Ordering::Relaxed) {
        return true;
    }
    std::env::var_os("CODEX_TEAM_TEST_ISOLATION_READY").as_deref() == Some(OsStr::new("1"))
}

impl IsolationSession {
    pub(crate) fn new(paths: &Paths) -> Result<Self> {
        require_isolation_backend()?;
        let scratch = Arc::new(create_scratch()?);
        let workspace = scratch.path.join("workspace");
        let target = scratch.path.join("target");
        let home = scratch.path.join("home");
        let cargo_home = scratch.path.join("cargo-home");
        let tmp = scratch.path.join("tmp");
        for directory in [&target, &home, &cargo_home, &tmp] {
            create_private_directory(directory)?;
        }
        #[cfg(debug_assertions)]
        let sandboxed = !test_isolation_double_active();
        #[cfg(not(debug_assertions))]
        let sandboxed = true;
        let source_root = if sandboxed {
            create_private_directory(&workspace)?;
            project_repository(paths, &workspace)?;
            reject_git_dependencies(&workspace)?;
            workspace
        } else {
            paths.root.clone()
        };
        let mut cache_reads = Vec::new();
        prepare_sanitized_cargo_home(&cargo_home, &mut cache_reads)?;
        Ok(Self {
            scratch,
            denied_source_root: paths.root.clone(),
            source_root,
            target,
            home,
            cargo_home,
            tmp,
            cache_reads,
            sandboxed,
        })
    }

    pub(crate) fn rust_metadata_digest(&self, package: &str) -> Result<String> {
        let crate_name = package.replace('-', "_");
        let root = self.target.join("debug/deps");
        let mut paths = std::fs::read_dir(&root)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(OsStr::to_str)
                    .is_some_and(|name| {
                        name.starts_with(&format!("lib{crate_name}-")) && name.ends_with(".rmeta")
                    })
            })
            .collect::<Vec<_>>();
        paths.sort();
        anyhow::ensure!(!paths.is_empty(), "compiled Rust metadata is unavailable");
        artifact_digest(&root, &paths, false)
    }

    pub(crate) fn rust_public_api_digest(&self, package: &str) -> Result<String> {
        let root = self.target.join("doc").join(package.replace('-', "_"));
        let mut paths = walkdir::WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|entry| {
                entry.file_type().is_file() && entry.path().extension() == Some(OsStr::new("html"))
            })
            .map(|entry| entry.into_path())
            .collect::<Vec<_>>();
        paths.sort();
        anyhow::ensure!(
            !paths.is_empty(),
            "compiled public Rust documentation is unavailable"
        );
        artifact_digest(&root, &paths, true)
    }

    pub(crate) fn prepare(
        &self,
        command: &VerificationCommand,
        cancellation: &CancellationToken,
    ) -> Result<PreparedIsolatedCommand> {
        let cwd = match command.cwd.as_deref() {
            Some(relative) => contained_projection_path(&self.source_root, relative)?,
            None => self.source_root.clone(),
        };
        if !cwd.is_dir() {
            bail!("isolated verification working directory does not exist");
        }
        let actual_program = if let Some(relative) = command.program.strip_prefix("target/") {
            contained_projection_path(&self.target, relative)?
        } else if self.sandboxed {
            resolve_trusted_program(&command.program)?
        } else {
            #[cfg(debug_assertions)]
            {
                resolve_test_program(&command.program)?
            }
            #[cfg(not(debug_assertions))]
            unreachable!()
        };
        if actual_program.exists() {
            require_regular_executable(&actual_program)?;
        } else if !command.program.starts_with("target/") {
            bail!("isolated verification executable is unavailable");
        }
        let developer_paths =
            if self.sandboxed && matches!(command.program.as_str(), "cargo" | "swift") {
                Some(crate::verification::library_reads::trusted_developer_paths(
                    cancellation,
                )?)
            } else {
                None
            };
        let path_value = trusted_path_value(
            &actual_program,
            developer_paths.as_ref().map(|(_, _, bin)| bin.as_path()),
        );
        let mut env = vec![
            ("HOME".to_string(), self.home.display().to_string()),
            ("TMPDIR".to_string(), self.tmp.display().to_string()),
            (
                "CARGO_HOME".to_string(),
                self.cargo_home.display().to_string(),
            ),
            (
                "CARGO_TARGET_DIR".to_string(),
                self.target.display().to_string(),
            ),
            ("CARGO_NET_OFFLINE".to_string(), "true".to_string()),
            (
                "RUSTUP_HOME".to_string(),
                self.home.join("rustup").display().to_string(),
            ),
            ("PATH".to_string(), path_value),
            ("LANG".to_string(), "C.UTF-8".to_string()),
            ("LC_ALL".to_string(), "C.UTF-8".to_string()),
        ];
        if let Some((developer, sdk, _)) = developer_paths {
            env.push(("DEVELOPER_DIR".into(), developer.display().to_string()));
            env.push(("SDKROOT".into(), sdk.display().to_string()));
        }
        let system_tls_config = if self.sandboxed && command.program == "cargo" {
            Some(read_public_system_tls_config()?)
        } else {
            None
        };
        let system_tls_reads = if system_tls_config.is_some() {
            vec![
                PathBuf::from("/private/etc/ssl/openssl.cnf"),
                PathBuf::from("/etc/ssl/openssl.cnf"),
            ]
        } else {
            Vec::new()
        };
        let (program, args) = if self.sandboxed {
            let toolchain_root = toolchain_root_for(&actual_program);
            let library_reads = if command.program.starts_with("target/") {
                Vec::new()
            } else {
                crate::verification::library_reads::trusted_library_reads(
                    &actual_program,
                    cancellation,
                )?
            };
            let profile = isolation_profile_with_libraries(
                &self.scratch.path,
                &self.source_root,
                &toolchain_root,
                &self.cache_reads,
                &self.denied_source_root,
                &library_reads,
                &system_tls_reads,
            );
            let mut args = vec!["-p".to_string(), profile];
            args.push(actual_program.display().to_string());
            args.extend(command.args.clone());
            (PathBuf::from("/usr/bin/sandbox-exec"), args)
        } else {
            (actual_program, command.args.clone())
        };
        Ok(PreparedIsolatedCommand {
            program,
            args,
            cwd,
            env,
            _scratch: self.scratch.clone(),
            _system_tls_config: system_tls_config,
        })
    }
}

fn validate_public_tls_config(bytes: &[u8]) -> Result<()> {
    let text = std::str::from_utf8(bytes).context("public OS TLS configuration is not UTF-8")?;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if line.starts_with('.') {
            bail!("public OS TLS configuration contains an external lookup directive");
        }
        let key = line
            .split_once('=')
            .map(|(key, _)| key.trim().to_ascii_lowercase());
        if key.as_deref().is_some_and(|key| {
            matches!(
                key,
                "openssl_conf" | "dynamic_path" | "so_path" | "oid_file" | "include"
            ) || key.starts_with("include")
                || key.contains("module")
                || key.contains("provider")
                || key.contains("engine")
        }) {
            bail!("public OS TLS configuration contains a module, provider or external lookup");
        }
    }
    Ok(())
}

fn read_public_system_tls_config() -> Result<File> {
    const MAX_PUBLIC_TLS_BYTES: u64 = 64 * 1024;
    let path = Path::new("/private/etc/ssl/openssl.cnf");
    if path.canonicalize()? != path {
        bail!("public OS TLS configuration is not its fixed canonical path");
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let before = file.metadata()?;
    if !before.is_file()
        || before.uid() != 0
        || before.mode() & 0o022 != 0
        || before.nlink() != 1
        || before.len() > MAX_PUBLIC_TLS_BYTES
    {
        bail!("public OS TLS configuration is not an immutable root-owned regular file");
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_PUBLIC_TLS_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PUBLIC_TLS_BYTES {
        bail!("public OS TLS configuration exceeded its read bound");
    }
    validate_public_tls_config(&bytes)?;
    let after = file.metadata()?;
    if before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
    {
        bail!("public OS TLS configuration changed during validation");
    }
    Ok(file)
}

#[cfg(debug_assertions)]
fn resolve_test_program(program: &str) -> Result<PathBuf> {
    if let Ok(program) = resolve_trusted_program(program) {
        return Ok(program);
    }
    let path = std::env::var_os("PATH").context("test isolation PATH is unavailable")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(program);
        if !candidate.exists() {
            continue;
        }
        let canonical = fs::canonicalize(candidate)?;
        require_regular_executable(&canonical)?;
        return Ok(canonical);
    }
    bail!("isolated verification executable is unavailable")
}

fn create_scratch() -> Result<ScratchDirectory> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let base = if let Some(store) = crate::runtime::current_run_store() {
        let context = store.context();
        let base = context
            .state_root
            .join("runs")
            .join(&context.run_id)
            .join("verification-scratch");
        crate::runtime::private_dir(&base)?;
        base.canonicalize()?
    } else {
        fs::canonicalize("/tmp").context("canonicalize fixed verification scratch root")?
    };
    let path = base.join(format!(
        "codex-team-verification-{}-{nonce}-{sequence}",
        std::process::id()
    ));
    fs::create_dir(&path).context("create private verification scratch")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    // Retain the directory inode so replacement cleanup cannot be confused by
    // an inode being recycled after the original path is removed.
    let owner = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)?;
    let metadata = owner.metadata()?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
    {
        bail!("verification scratch ownership changed");
    }
    Ok(ScratchDirectory {
        path,
        base,
        device: metadata.dev(),
        inode: metadata.ino(),
        _owner: owner,
    })
}

fn create_private_directory(path: &Path) -> Result<()> {
    fs::create_dir(path)
        .with_context(|| format!("create isolated directory {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn project_repository(paths: &Paths, workspace: &Path) -> Result<()> {
    let git = Path::new("/usr/bin/git");
    require_regular_executable(git)?;
    let output = Command::new(git)
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "credential.helper=",
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(&paths.root)
        .output()
        .context("enumerate verification source projection")?;
    if !output.status.success() || output.stdout.len() > MAX_GIT_LIST_BYTES {
        bail!("verification source projection enumeration failed or exceeded limits");
    }
    let mut names = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .map(|name| {
            std::str::from_utf8(name)
                .map(str::to_string)
                .map_err(|_| anyhow!("verification source path is not UTF-8"))
        })
        .collect::<Result<Vec<_>>>()?;
    names.sort();
    names.dedup();
    if names.len() > MAX_PROJECTED_FILES {
        bail!("verification source projection exceeds file-count limit");
    }
    let mut total = 0u64;
    for relative in names {
        validate_relative_projection_path(&relative)?;
        if is_private_or_protected_path(&relative) {
            continue;
        }
        let source = read_no_follow_regular_source(&paths.root, &relative)?;
        total = total.saturating_add(source.bytes.len() as u64);
        if total > MAX_PROJECTED_TOTAL_BYTES {
            bail!("verification source projection exceeds aggregate limit");
        }
        let destination = workspace.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        copy_regular_file(&source.bytes, &destination, source.mode)?;
    }
    Ok(())
}

fn copy_regular_file(bytes: &[u8], destination: &Path, mode: u32) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(mode & 0o777);
    let mut file = options.open(destination)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

struct ProjectedSource {
    bytes: Vec<u8>,
    mode: u32,
}

pub(crate) fn read_contained_single_link_file(
    root: &Path,
    relative: &str,
    max_bytes: u64,
) -> Result<Vec<u8>> {
    Ok(read_contained_single_link_file_with_mode(root, relative, max_bytes)?.0)
}

pub(crate) fn read_contained_single_link_file_with_mode(
    root: &Path,
    relative: &str,
    max_bytes: u64,
) -> Result<(Vec<u8>, u32)> {
    validate_relative_projection_path(relative)?;
    let source = read_no_follow_regular_source(root, relative)?;
    if source.bytes.len() as u64 > max_bytes {
        bail!("contained file exceeds its read limit");
    }
    Ok((source.bytes, source.mode))
}

fn read_no_follow_regular_source(root: &Path, relative: &str) -> Result<ProjectedSource> {
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)
        .context("open verification source root without following links")?;
    let root_device = directory.metadata()?.dev();
    let components = Path::new(relative).components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            bail!("verification projection path is not a safe relative path");
        };
        let name = CString::new(name.as_bytes())
            .map_err(|_| anyhow!("verification projection path contains a NUL byte"))?;
        let final_component = index + 1 == components.len();
        let flags = if final_component {
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK
        } else {
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC
        };
        let descriptor = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error())
                .context("open verification source component without following links");
        }
        let mut opened = unsafe { File::from_raw_fd(descriptor) };
        let before = opened.metadata()?;
        if before.dev() != root_device {
            bail!("verification projection rejects filesystem mount crossings");
        }
        if final_component {
            if !before.is_file() || before.nlink() != 1 {
                bail!("verification projection accepts single-link regular files only");
            }
            if before.len() > MAX_PROJECTED_FILE_BYTES {
                bail!("verification source file exceeds per-file limit");
            }
            let mut bytes = Vec::with_capacity(before.len() as usize);
            (&mut opened)
                .take(MAX_PROJECTED_FILE_BYTES + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_PROJECTED_FILE_BYTES {
                bail!("verification source file exceeded its bound while reading");
            }
            let after = opened.metadata()?;
            if before.dev() != after.dev()
                || before.ino() != after.ino()
                || before.len() != after.len()
                || before.mtime() != after.mtime()
                || before.mtime_nsec() != after.mtime_nsec()
                || before.ctime() != after.ctime()
                || before.ctime_nsec() != after.ctime_nsec()
            {
                bail!("verification source changed while projection was created");
            }
            return Ok(ProjectedSource {
                bytes,
                mode: before.mode(),
            });
        }
        if !before.is_dir() {
            bail!("verification projection path ancestor is not a directory");
        }
        directory = opened;
    }
    bail!("verification projection path has no file component")
}

fn validate_relative_projection_path(relative: &str) -> Result<()> {
    if relative.is_empty() || relative.len() > 4096 || Path::new(relative).is_absolute() {
        bail!("verification projection path is invalid");
    }
    let mut depth = 0usize;
    for component in Path::new(relative).components() {
        match component {
            Component::Normal(_) => depth += 1,
            _ => bail!("verification projection path escapes the repository"),
        }
    }
    if depth == 0 || depth > 64 {
        bail!("verification projection path depth is invalid");
    }
    Ok(())
}

fn is_private_or_protected_path(relative: &str) -> bool {
    let first = relative
        .split('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        first.as_str(),
        ".git" | ".orchestrator" | "oracles" | "reference"
    )
}

fn contained_projection_path(root: &Path, relative: &str) -> Result<PathBuf> {
    validate_relative_projection_path(relative)?;
    Ok(root.join(relative))
}

fn reject_git_dependencies(workspace: &Path) -> Result<()> {
    let lock = workspace.join("Cargo.lock");
    if lock.is_file() {
        let text = fs::read_to_string(&lock)?;
        if text
            .lines()
            .any(|line| line.trim_start().starts_with("source = \"git+"))
        {
            bail!("isolated verification v1 rejects Cargo git dependencies");
        }
    }
    Ok(())
}

fn prepare_sanitized_cargo_home(cargo_home: &Path, reads: &mut Vec<PathBuf>) -> Result<()> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Ok(());
    };
    let source = home.join(".cargo").join("registry");
    let metadata = match fs::symlink_metadata(&source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("inspect Cargo registry cache"),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("Cargo registry cache must be a real directory");
    }
    let canonical = fs::canonicalize(&source)?;
    let destination = cargo_home.join("registry");
    std::os::unix::fs::symlink(&canonical, &destination)?;
    reads.push(canonical);
    Ok(())
}

fn resolve_trusted_program(program: &str) -> Result<PathBuf> {
    if program.contains('/') {
        bail!(
            "isolated verification executable must be a trusted tool name or target-relative path"
        );
    }
    for directory in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        let candidate = Path::new(directory).join(program);
        if candidate.exists() {
            let canonical = fs::canonicalize(candidate)?;
            require_regular_executable(&canonical)?;
            return Ok(canonical);
        }
    }
    bail!("isolated verification executable is unavailable")
}

fn require_regular_executable(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect executable {}", path.display()))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.permissions().mode() & 0o111 == 0
    {
        bail!("required isolation executable is not a regular executable");
    }
    Ok(())
}

fn toolchain_root_for(program: &Path) -> PathBuf {
    program
        .ancestors()
        .find(|ancestor| ancestor.file_name() == Some(OsStr::new("Cellar")))
        .and_then(|cellar| {
            program
                .strip_prefix(cellar)
                .ok()
                .and_then(|relative| relative.components().next())
                .and_then(|component| match component {
                    Component::Normal(name) => Some(cellar.join(name)),
                    _ => None,
                })
        })
        .unwrap_or_else(|| {
            program
                .parent()
                .unwrap_or(Path::new("/usr/bin"))
                .to_path_buf()
        })
}

fn trusted_path_value(program: &Path, physical_compiler_bin: Option<&Path>) -> String {
    let mut directories = Vec::new();
    if let Some(bin) = physical_compiler_bin {
        // /usr/bin/cc is an xcrun shim, which may try an installation fallback
        // or write a global cache. Run the validated installed compiler itself.
        directories.push(bin.display().to_string());
    }
    if let Some(parent) = program.parent() {
        directories.push(parent.display().to_string());
    }
    directories.push("/usr/bin".to_string());
    directories.push("/bin".to_string());
    directories.dedup();
    directories.join(":")
}

fn isolation_profile(
    scratch: &Path,
    workspace: &Path,
    toolchain_root: &Path,
    cache_reads: &[PathBuf],
    denied_source_root: &Path,
) -> String {
    isolation_profile_with_libraries(
        scratch,
        workspace,
        toolchain_root,
        cache_reads,
        denied_source_root,
        &[],
        &[],
    )
}

fn isolation_profile_with_libraries(
    scratch: &Path,
    workspace: &Path,
    toolchain_root: &Path,
    cache_reads: &[PathBuf],
    denied_source_root: &Path,
    library_reads: &[PathBuf],
    system_tls_reads: &[PathBuf],
) -> String {
    let mut reads = vec![
        PathBuf::from("/usr"),
        PathBuf::from("/bin"),
        PathBuf::from("/System"),
        PathBuf::from("/Library/Developer"),
        PathBuf::from("/Applications/Xcode.app"),
        scratch.to_path_buf(),
        workspace.to_path_buf(),
        toolchain_root.to_path_buf(),
    ];
    reads.extend_from_slice(cache_reads);
    reads.sort();
    reads.dedup();
    let read_rules = reads
        .iter()
        .filter(|path| path.exists())
        .map(|path| format!("(subpath \"{}\")", sbpl_escape(path)))
        .collect::<Vec<_>>()
        .join(" ");
    let read_metadata_rules = reads
        .iter()
        .flat_map(|path| path.ancestors().skip(1))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|path| format!("(literal \"{}\")", sbpl_escape(path)))
        .collect::<Vec<_>>()
        .join(" ");
    let library_rules = library_reads
        .iter()
        .map(|path| format!("(subpath \"{}\")", sbpl_escape(path)))
        .collect::<Vec<_>>()
        .join(" ");
    let library_metadata_rules = library_reads
        .iter()
        .flat_map(|path| path.ancestors().skip(1))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|path| format!("(literal \"{}\")", sbpl_escape(path)))
        .collect::<Vec<_>>()
        .join(" ");
    let library_permissions = if library_rules.is_empty() {
        String::new()
    } else {
        format!("(allow file-read* {library_rules})\n(allow file-map-executable {library_rules})\n(allow file-read-metadata {library_metadata_rules})\n")
    };
    let scratch_metadata_rules = scratch
        .ancestors()
        .skip(1)
        .map(|path| format!("(literal \"{}\")", sbpl_escape(path)))
        .collect::<Vec<_>>()
        .join(" ");
    let tls_rules = system_tls_reads
        .iter()
        .map(|path| format!("(literal \"{}\")", sbpl_escape(path)))
        .collect::<Vec<_>>()
        .join(" ");
    let tls_metadata_rules = system_tls_reads
        .iter()
        .flat_map(|path| path.ancestors().skip(1))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|path| format!("(literal \"{}\")", sbpl_escape(path)))
        .collect::<Vec<_>>()
        .join(" ");
    let tls_permissions = if tls_rules.is_empty() {
        String::new()
    } else {
        format!("(allow file-read* {tls_rules})\n(allow file-read-metadata {tls_metadata_rules})\n")
    };
    let mut denied_reads = vec![
        denied_source_root.to_path_buf(),
        PathBuf::from("/etc"),
        PathBuf::from("/private/etc"),
        PathBuf::from("/Volumes"),
        PathBuf::from("/Network"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        denied_reads.push(PathBuf::from(home));
    }
    denied_reads.sort();
    denied_reads.dedup();
    let denied_read_rules = denied_reads
        .iter()
        .map(|path| {
            let mut exceptions = Vec::new();
            if scratch.starts_with(path) {
                exceptions.push(format!(
                    "(require-not (subpath \"{}\"))",
                    sbpl_escape(scratch)
                ));
            }
            for cache in cache_reads {
                if cache.starts_with(path) {
                    exceptions.push(format!(
                        "(require-not (subpath \"{}\"))",
                        sbpl_escape(cache)
                    ));
                }
            }
            for config in system_tls_reads {
                if config.starts_with(path) {
                    exceptions.push(format!(
                        "(require-not (literal \"{}\"))",
                        sbpl_escape(config)
                    ));
                }
            }
            if exceptions.is_empty() {
                format!("(subpath \"{}\")", sbpl_escape(path))
            } else {
                format!(
                    "(require-all (subpath \"{}\") {})",
                    sbpl_escape(path),
                    exceptions.join(" ")
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "(version 1)\n(deny default)\n(allow process*)\n(allow sysctl-read)\n(allow mach-lookup)\n(allow file-read-data)\n(deny file-read-data {denied_read_rules})\n(allow file-read* {read_rules})\n(allow file-read-metadata {read_metadata_rules})\n{library_permissions}{tls_permissions}(allow file-read-metadata {scratch_metadata_rules})\n(allow file-write* (subpath \"{}\"))\n(deny network*)\n",
        sbpl_escape(scratch)
    )
}

fn sbpl_escape(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn artifact_digest(root: &Path, paths: &[PathBuf], include_paths: bool) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    let mut total = 0_u64;
    for path in paths {
        let metadata = fs::symlink_metadata(path)?;
        anyhow::ensure!(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.uid() == unsafe { libc::geteuid() }
                && path.canonicalize()? == *path,
            "Rust artifact must be an owned, contained regular file"
        );
        total = total.saturating_add(metadata.len());
        anyhow::ensure!(
            paths.len() <= 10_000
                && metadata.len() <= 16 * 1024 * 1024
                && total <= 128 * 1024 * 1024,
            "Rust artifacts exceed the digest bound"
        );
        if include_paths {
            hash.update(path.strip_prefix(root)?.as_os_str().as_encoded_bytes());
            hash.update([0]);
        }
        // Cargo creates hard links between its own artifact directories. These
        // are digest inputs only, opened without following a replaced leaf.
        let mut options = fs::OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let mut file = options.open(path)?;
        let opened = file.metadata()?;
        anyhow::ensure!(
            metadata.dev() == opened.dev() && metadata.ino() == opened.ino(),
            "Rust artifact changed before hashing"
        );
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        let after = file.metadata()?;
        anyhow::ensure!(
            bytes.len() as u64 == opened.len()
                && opened.len() == after.len()
                && opened.mtime() == after.mtime()
                && opened.mtime_nsec() == after.mtime_nsec(),
            "Rust artifact changed while hashing"
        );
        hash.update(bytes);
        hash.update([0]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_paths_reject_escapes_and_private_roots() {
        assert!(validate_relative_projection_path("src/lib.rs").is_ok());
        assert!(validate_relative_projection_path("../secret").is_err());
        assert!(validate_relative_projection_path("/tmp/secret").is_err());
        assert!(is_private_or_protected_path(".GiT/config"));
        assert!(is_private_or_protected_path("ORACLES/x"));
        assert!(!is_private_or_protected_path("src/lib.rs"));
    }

    #[test]
    fn generated_profile_denies_network_and_writes_only_scratch() {
        let scratch = Path::new("/tmp/codex-team-verification-test");
        let profile = isolation_profile(
            scratch,
            &scratch.join("workspace"),
            Path::new("/usr/bin"),
            &[],
            Path::new("/Users/example/project"),
        );
        assert!(profile.contains("(deny network*)"));
        assert!(profile.contains("(allow file-read-data)"));
        assert!(profile.contains("(deny file-read-data "));
        assert!(profile.contains("(subpath \"/Users/example/project\")"));
        assert!(
            profile.contains("(allow file-write* (subpath \"/tmp/codex-team-verification-test\"))")
        );
        assert!(!profile.contains(".cargo/credentials"));
    }

    #[test]
    fn public_tls_configuration_rejects_external_lookup_and_module_loading() {
        assert!(validate_public_tls_config(
            b"# public configuration\n[req]\ndefault_bits = 2048\n"
        )
        .is_ok());
        for directive in [
            ".include /private/config",
            "openssl_conf = runtime",
            "dynamic_path = /private/plugin.dylib",
            "provider = plugin",
            "engines = plugins",
            "module = plugin",
            "oid_file = /private/lookup",
        ] {
            assert!(
                validate_public_tls_config(directive.as_bytes()).is_err(),
                "{directive}"
            );
        }
    }

    #[test]
    fn physical_compiler_precedes_system_shims_without_importing_caller_path() {
        assert_eq!(
            trusted_path_value(
                Path::new("/opt/homebrew/Cellar/rust/1/bin/cargo"),
                Some(Path::new("/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin")),
            ),
            "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin:/opt/homebrew/Cellar/rust/1/bin:/usr/bin:/bin"
        );
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn actual_native_cargo_compiles_in_projection_with_private_sources_and_writes_denied(
    ) -> Result<()> {
        let root = tempfile::tempdir()?;
        let root = root.path().canonicalize()?;
        let repository = root.join("repository");
        fs::create_dir_all(repository.join("src"))?;
        fs::write(repository.join("Cargo.toml"), "[package]\nname = \"physical_verification_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n")?;
        fs::write(repository.join("Cargo.lock"), "version = 4\n\n[[package]]\nname = \"physical_verification_fixture\"\nversion = \"0.1.0\"\n")?;
        let paths = Paths::new(&repository)?;
        let private = repository.join(".orchestrator/private-canary");
        fs::create_dir_all(private.parent().context("private fixture parent")?)?;
        fs::write(&private, b"private source canary")?;
        let escaped_write = repository.join("outside-write-canary");
        fs::write(repository.join("src/lib.rs"), format!(
            "#[test]\nfn physical_boundary() {{ assert!(std::fs::read({:?}).is_err()); assert!(std::fs::read(\"/etc/passwd\").is_err()); assert!(std::fs::write({:?}, b\"unapproved\").is_err()); std::fs::write(\"verified-marker\", b\"passed\").unwrap(); }}\n",
            private.display().to_string(), escaped_write.display().to_string(),
        ))?;
        for args in [
            vec!["init", "--quiet"],
            vec!["add", "Cargo.toml", "Cargo.lock", "src/lib.rs"],
        ] {
            let output = crate::git_metadata::command_no_filters(&repository)?
                .args(args)
                .output()?;
            anyhow::ensure!(
                output.status.success(),
                "initialize physical verification fixture"
            );
        }
        let context = crate::runtime::ExecutionContext {
            schema: crate::runtime::EXECUTION_SCHEMA.into(),
            run_id: "actual-cargo-verification".into(),
            profile: "default".into(),
            state_root: root.join("state"),
            lease_root: root.join("leases"),
            definition_digest: "a".repeat(64),
            resource_binding_digest: "b".repeat(64),
            policy_digest: "c".repeat(64),
            resources: Vec::new(),
            runtime_seconds: 60,
            output_bytes: 8192,
        };
        let store = crate::runtime::RunStore::open(&context)?;
        crate::runtime::EXECUTION_STORE
            .scope(store, async {
                let mut session = IsolationSession::new(&paths)?;
                // Other unit fixtures can globally enable the debug double. This
                // regression always launches real Seatbelt and a real compiler.
                if !session.sandboxed {
                    let workspace = session.scratch.path.join("workspace");
                    create_private_directory(&workspace)?;
                    project_repository(&paths, &workspace)?;
                    reject_git_dependencies(&workspace)?;
                    session.source_root = workspace;
                    session.sandboxed = true;
                }
                let mut command = VerificationCommand::new(
                    "cargo".into(),
                    vec!["test".into(), "--offline".into(), "--locked".into()],
                    "isolation.actual-native-cargo",
                );
                command.timeout_seconds = Some(60);
                let result =
                    crate::verification::runner::run_isolated_deterministic_command_in_session(
                        &paths, &command, &session,
                    )?;
                assert_eq!(result.exit_code, 0, "{}", result.stderr_tail);
                assert_eq!(
                    fs::read(session.source_root.join("verified-marker"))?,
                    b"passed"
                );
                assert_eq!(fs::read(&private)?, b"private source canary");
                assert!(!escaped_write.exists());
                Ok::<_, anyhow::Error>(())
            })
            .await
    }

    #[cfg(unix)]
    #[test]
    fn projection_reader_rejects_symlinks_and_hardlinks() -> Result<()> {
        let root = create_scratch()?;
        fs::create_dir(root.path.join("src"))?;
        fs::write(root.path.join("src/safe.rs"), "fn safe() {}\n")?;
        let safe = read_no_follow_regular_source(&root.path, "src/safe.rs")?;
        assert_eq!(safe.bytes, b"fn safe() {}\n");

        let outside = root.path.join("outside.txt");
        fs::write(&outside, "canary")?;
        std::os::unix::fs::symlink(&outside, root.path.join("src/link.rs"))?;
        assert!(read_no_follow_regular_source(&root.path, "src/link.rs").is_err());

        fs::hard_link(&outside, root.path.join("src/hard.rs"))?;
        assert!(read_no_follow_regular_source(&root.path, "src/hard.rs").is_err());
        assert_eq!(fs::read_to_string(outside)?, "canary");
        Ok(())
    }

    #[tokio::test]
    async fn native_scratch_stays_in_the_admitted_run_and_cleans_up_its_owned_directory(
    ) -> Result<()> {
        let root = tempfile::tempdir()?;
        let root = root.path().canonicalize()?;
        let context = crate::runtime::ExecutionContext {
            schema: crate::runtime::EXECUTION_SCHEMA.into(),
            run_id: "verification-scratch-test".into(),
            profile: "default".into(),
            state_root: root.join("state"),
            lease_root: root.join("leases"),
            definition_digest: "a".repeat(64),
            resource_binding_digest: "b".repeat(64),
            policy_digest: "c".repeat(64),
            resources: Vec::new(),
            runtime_seconds: 10,
            output_bytes: 8192,
        };
        let store = crate::runtime::RunStore::open(&context)?;
        crate::runtime::EXECUTION_STORE
            .scope(store, async {
                let scratch = create_scratch()?;
                let expected = context
                    .state_root
                    .join("runs")
                    .join(&context.run_id)
                    .join("verification-scratch");
                assert_eq!(scratch.path.parent(), Some(expected.as_path()));
                assert_eq!(fs::metadata(&scratch.path)?.mode() & 0o777, 0o700);
                let path = scratch.path.clone();
                fs::write(path.join("evidence"), b"owned verification")?;
                drop(scratch);
                assert!(!path.exists());
                assert!(expected.is_dir());
                Ok::<_, anyhow::Error>(())
            })
            .await
    }

    #[test]
    fn scratch_cleanup_does_not_remove_a_replacement_directory() -> Result<()> {
        let scratch = create_scratch()?;
        let path = scratch.path.clone();
        fs::remove_dir(&path)?;
        fs::create_dir(&path)?;
        fs::write(path.join("retained"), b"different owner instance")?;
        drop(scratch);
        assert_eq!(
            fs::read(path.join("retained"))?,
            b"different owner instance"
        );
        fs::remove_dir_all(path)?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn admitted_scratch_under_a_denied_parent_keeps_sibling_data_denied() -> Result<()> {
        let root = tempfile::tempdir()?;
        let root = root.path().canonicalize()?;
        let scratch = root.join("admitted-scratch");
        create_private_directory(&scratch)?;
        let admitted = scratch.join("evidence");
        let denied = root.join("private-sibling");
        fs::write(&admitted, b"verification evidence")?;
        fs::write(&denied, b"private fixture")?;
        let profile = isolation_profile(&scratch, &scratch, Path::new("/bin"), &[], &root);
        let read = |path: &Path| {
            Command::new("/usr/bin/sandbox-exec")
                .env_clear()
                .args(["-p", &profile, "/bin/cat"])
                .arg(path)
                .output()
        };
        let evidence = read(&admitted)?;
        assert!(evidence.status.success());
        assert_eq!(evidence.stdout, b"verification evidence");
        assert!(!read(&denied)?.status.success());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn one_isolation_session_preserves_chain_artifacts() -> Result<()> {
        enable_test_isolation_backend();
        let root = create_scratch()?;
        let paths = Paths::new(&root.path)?;
        let session = IsolationSession::new(&paths)?;
        let command = VerificationCommand::new(
            "target/release/fixture".to_string(),
            vec!["--help".to_string()],
            "isolation.test",
        );
        let first = session.prepare(&command, &CancellationToken::default())?;
        assert!(!first.program.exists());
        fs::create_dir_all(first.program.parent().context("fixture binary parent")?)?;
        fs::write(&first.program, "#!/bin/sh\nexit 0\n")?;
        fs::set_permissions(&first.program, fs::Permissions::from_mode(0o700))?;
        let second = session.prepare(&command, &CancellationToken::default())?;
        assert_eq!(first.program, second.program);
        require_regular_executable(&second.program)?;
        Ok(())
    }
}
