//! Required deterministic-verification isolation.
//!
//! Learned evidence recipes never execute in the governed working tree.  This
//! module creates a private source projection, exposes only dependency caches
//! (not Cargo config/credentials), scrubs the environment, and launches the
//! typed command under macOS Seatbelt or the authenticated FreeBSD jail service
//! with network and original-repository access denied. Unsupported hosts fail closed.

use anyhow::{anyhow, bail, Context, Result};
use std::collections::{BTreeSet, VecDeque};
use std::ffi::{CString, OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
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
    #[cfg(not(any(target_os = "macos", target_os = "freebsd")))]
    {
        bail!(
            "required verification isolation requires macOS Seatbelt or the FreeBSD jail service"
        );
    }
    #[cfg(target_os = "freebsd")]
    {
        crate::freebsd_sandbox::require_backend()?;
        Ok(IsolationReadiness {
            ready: true,
            backend: "freebsd-jail-source-projection-v1".to_string(),
            summary: "private source projection, scrubbed environment, offline dependencies, original repository denied, network denied".to_string(),
        })
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
        let python_tool =
            if self.sandboxed && matches!(command.program.as_str(), "python" | "pytest") {
                Some(resolve_python_tool(
                    &command.program,
                    std::env::var_os("PATH").as_deref(),
                    &self.denied_source_root,
                    std::env::var_os("HOME").as_deref().map(Path::new),
                )?)
            } else {
                None
            };
        let actual_program = if let Some(tool) = &python_tool {
            tool.interpreter.clone()
        } else if let Some(relative) = command.program.strip_prefix("target/") {
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
            require_regular_executable(&fs::canonicalize(&actual_program)?)?;
        } else if !command.program.starts_with("target/") {
            bail!("isolated verification executable is unavailable");
        }
        let developer_paths = if self.sandboxed
            && cfg!(target_os = "macos")
            && matches!(command.program.as_str(), "cargo" | "swift")
        {
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
        if python_tool.is_some() {
            // The selected environment is a read-only dependency, never a
            // destination for generated bytecode or verification state.
            env.push(("PYTHONDONTWRITEBYTECODE".into(), "1".into()));
            if std::env::var_os("PYTEST_DISABLE_PLUGIN_AUTOLOAD").as_deref()
                == Some(OsStr::new("1"))
            {
                env.push(("PYTEST_DISABLE_PLUGIN_AUTOLOAD".into(), "1".into()));
            }
        }
        if let Some((developer, sdk, _)) = developer_paths {
            env.push(("DEVELOPER_DIR".into(), developer.display().to_string()));
            env.push(("SDKROOT".into(), sdk.display().to_string()));
        }
        let system_tls_config =
            if self.sandboxed && cfg!(target_os = "macos") && command.program == "cargo" {
                Some(read_public_system_tls_config()?)
            } else {
                None
            };
        #[cfg(not(target_os = "freebsd"))]
        let system_tls_reads = if system_tls_config.is_some() {
            vec![
                PathBuf::from("/private/etc/ssl/openssl.cnf"),
                PathBuf::from("/etc/ssl/openssl.cnf"),
            ]
        } else {
            Vec::new()
        };
        let (program, args) = if self.sandboxed {
            #[cfg(target_os = "freebsd")]
            {
                let mut reads = crate::freebsd_sandbox::system_reads();
                reads.extend_from_slice(&self.cache_reads);
                if let Some(tool) = &python_tool {
                    reads.extend_from_slice(&tool.reads);
                }
                let mut args = Vec::new();
                if let Some(script) = python_tool.as_ref().and_then(|tool| tool.script.as_ref()) {
                    args.push(script.display().to_string());
                }
                args.extend_from_slice(&command.args);
                crate::freebsd_sandbox::prepare_process(
                    &actual_program,
                    &args,
                    std::slice::from_ref(&self.scratch.path),
                    &reads,
                    std::slice::from_ref(&self.denied_source_root),
                    false,
                )?
            }
            #[cfg(not(target_os = "freebsd"))]
            {
                let toolchain_root = toolchain_root_for(&actual_program);
                let mut library_reads =
                    if command.program.starts_with("target/") || python_tool.is_some() {
                        Vec::new()
                    } else {
                        crate::verification::library_reads::trusted_library_reads(
                            &actual_program,
                            cancellation,
                        )?
                    };
                let mut dependency_reads = self.cache_reads.clone();
                if let Some(tool) = &python_tool {
                    dependency_reads.extend_from_slice(&tool.reads);
                    library_reads.extend_from_slice(&tool.reads);
                }
                let profile = isolation_profile_with_libraries(
                    &self.scratch.path,
                    &self.source_root,
                    &toolchain_root,
                    &dependency_reads,
                    &self.denied_source_root,
                    &library_reads,
                    &system_tls_reads,
                );
                #[cfg(target_os = "macos")]
                let profile = {
                    let mut profile = profile;
                    if let Some(tool) = &python_tool {
                        for path in python_runtime_metadata_paths(&tool.interpreter)? {
                            profile.push_str(&format!(
                                "(allow file-read-metadata (literal \"{}\"))\n",
                                sbpl_escape(&path)
                            ));
                        }
                    }
                    profile
                };
                let mut args = vec!["-p".to_string(), profile];
                args.push(actual_program.display().to_string());
                if let Some(script) = python_tool.as_ref().and_then(|tool| tool.script.as_ref()) {
                    args.push(script.display().to_string());
                }
                args.extend(command.args.clone());
                (PathBuf::from("/usr/bin/sandbox-exec"), args)
            }
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

fn projection_git_paths(paths: &Paths, arguments: &[&str]) -> Result<Vec<String>> {
    let git = Path::new(crate::git_metadata::EXECUTABLE);
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
        ])
        .args(arguments)
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
    for relative in &names {
        validate_relative_projection_path(relative)?;
    }
    Ok(names)
}

fn projected_source_paths(paths: &Paths) -> Result<Vec<String>> {
    Ok(projection_git_paths(
        paths,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?
    .into_iter()
    .filter(|relative| !is_private_or_protected_path(relative))
    .collect())
}

/// Require every final candidate to be represented by the isolated source
/// projection. Git-ignored/private inputs cannot receive evidence from a
/// projection that omits their bytes. A staged deletion is represented by
/// absence, despite no longer appearing in the index's file list.
pub(crate) fn require_projected_candidate_paths(
    paths: &Paths,
    candidates: &[String],
) -> Result<()> {
    let revision = crate::revision::repository_revision(paths)?;
    let projected = projected_source_paths(paths)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let staged_deleted = projection_git_paths(
        paths,
        &[
            "diff",
            "--cached",
            "--name-only",
            "--diff-filter=D",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
        ],
    )?
    .into_iter()
    .collect::<BTreeSet<_>>();
    for relative in candidates {
        validate_relative_projection_path(relative)?;
        if is_private_or_protected_path(relative) {
            bail!("final candidate path {relative} is excluded from the verification source projection");
        }
        if projected.contains(relative) {
            continue;
        }
        if staged_deleted.contains(relative)
            && fs::symlink_metadata(paths.root.join(relative))
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        bail!("final candidate path {relative} is excluded from the verification source projection (it may be Git-ignored)");
    }
    if crate::revision::repository_revision(paths)? != revision {
        bail!("repository changed while candidate projection membership was validated");
    }
    Ok(())
}

fn project_repository(paths: &Paths, workspace: &Path) -> Result<()> {
    let revision = crate::revision::repository_revision(paths)?;
    let names = projected_source_paths(paths)?;
    let deleted = projection_git_paths(paths, &["ls-files", "-z", "--deleted"])?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut total = 0u64;
    let mut links = Vec::new();
    for relative in names {
        let target = match read_projected_symlink(&paths.root, &relative) {
            Ok(target) => target,
            Err(error)
                if deleted.contains(&relative)
                    && error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        };
        if let Some(target) = target {
            total = total.saturating_add(target.as_os_str().as_bytes().len() as u64);
            if total > MAX_PROJECTED_TOTAL_BYTES {
                bail!("verification source projection exceeds aggregate limit");
            }
            links.push((relative, target));
            continue;
        }
        let source = match read_no_follow_regular_source(&paths.root, &relative) {
            Ok(source) => source,
            Err(error)
                if deleted.contains(&relative)
                    && error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        };
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
    // Install links only after all regular bytes have been copied, so an
    // unchecked link can never influence a projection destination write.
    for (relative, target) in &links {
        let destination = workspace.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        std::os::unix::fs::symlink(target, destination)?;
    }
    validate_projected_symlinks(workspace, &links)?;
    if crate::revision::repository_revision(paths)? != revision {
        bail!("repository changed while the verification source projection was created");
    }
    Ok(())
}

fn read_projected_symlink(root: &Path, relative: &str) -> Result<Option<PathBuf>> {
    validate_relative_projection_path(relative)?;
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)?;
    let root_device = directory.metadata()?.dev();
    let components = Path::new(relative).components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            bail!("verification projection path is not a safe relative path");
        };
        let name = CString::new(name.as_bytes())?;
        if index + 1 != components.len() {
            let descriptor = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if descriptor < 0 {
                return Err(std::io::Error::last_os_error())
                    .context("open source link ancestor without following links");
            }
            directory = unsafe { File::from_raw_fd(descriptor) };
            if directory.metadata()?.dev() != root_device {
                bail!("verification projection rejects filesystem mount crossings");
            }
            continue;
        }
        let inspect = || -> Result<libc::stat> {
            let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
            if unsafe {
                libc::fstatat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    metadata.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error())
                    .context("inspect source link without following it");
            }
            Ok(unsafe { metadata.assume_init() })
        };
        let before = inspect()?;
        if before.st_dev != root_device as libc::dev_t {
            bail!("verification projection rejects filesystem mount crossings");
        }
        if before.st_mode & libc::S_IFMT != libc::S_IFLNK {
            return Ok(None);
        }
        if before.st_nlink != 1 || before.st_size < 0 || before.st_size > 4096 {
            bail!("verification source link exceeds its bounds");
        }
        let mut bytes = vec![0u8; 4097];
        let length = unsafe {
            libc::readlinkat(
                directory.as_raw_fd(),
                name.as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
            )
        };
        if length < 0 {
            return Err(std::io::Error::last_os_error())
                .context("read source link without following it");
        }
        if length == 0 || length > 4096 {
            bail!("verification source link exceeds its read bound");
        }
        let after = inspect()?;
        if before.st_dev != after.st_dev
            || before.st_ino != after.st_ino
            || before.st_mode != after.st_mode
            || before.st_nlink != after.st_nlink
            || before.st_size != after.st_size
            || before.st_mtime != after.st_mtime
            || before.st_mtime_nsec != after.st_mtime_nsec
            || before.st_ctime != after.st_ctime
            || before.st_ctime_nsec != after.st_ctime_nsec
        {
            bail!("verification source link changed while projection was created");
        }
        bytes.truncate(length as usize);
        let target = PathBuf::from(OsString::from_vec(bytes));
        if target.is_absolute() {
            bail!("verification source links must be relative to the repository");
        }
        return Ok(Some(target));
    }
    bail!("verification projection path has no link component")
}

fn validate_projected_symlinks(workspace: &Path, links: &[(String, PathBuf)]) -> Result<()> {
    // Resolve only the completed private projection. No original source target
    // is opened, and ignored/private/unrepresented targets cannot be accepted.
    for (relative, _) in links {
        let mut pending = Path::new(relative)
            .components()
            .map(|component| component.as_os_str().to_os_string())
            .collect::<VecDeque<_>>();
        let mut resolved = PathBuf::new();
        let mut followed = 0usize;
        let mut steps = 0usize;
        while let Some(component) = pending.pop_front() {
            steps += 1;
            if steps > 256 || pending.len() > 256 {
                bail!("verification source link resolution exceeds its bound");
            }
            if component == OsStr::new(".") {
                continue;
            }
            if component == OsStr::new("..") {
                if !resolved.pop() {
                    bail!("verification source link escapes the repository");
                }
                continue;
            }
            let next = resolved.join(&component);
            let metadata = fs::symlink_metadata(workspace.join(&next))
                .context("verification source link target is absent from the projection")?;
            if metadata.file_type().is_symlink() {
                followed += 1;
                if followed > 40 {
                    bail!("verification source links are cyclic or exceed the chain limit");
                }
                let target = fs::read_link(workspace.join(&next))?;
                if target.is_absolute() || target.as_os_str().as_bytes().len() > 4096 {
                    bail!("verification source links must remain relative and bounded");
                }
                let components = target
                    .components()
                    .map(|part| part.as_os_str().to_os_string())
                    .collect::<Vec<_>>();
                for part in components.into_iter().rev() {
                    pending.push_front(part);
                }
            } else {
                if !metadata.is_dir() && (!metadata.is_file() || metadata.nlink() != 1) {
                    bail!("verification source link target is not a projected directory or regular file");
                }
                if !pending.is_empty() && !metadata.is_dir() {
                    bail!("verification source link traverses a non-directory");
                }
                resolved = next;
            }
        }
        if resolved.as_os_str().is_empty() {
            bail!("verification source links cannot expose the projection root");
        }
        // Check the actual spelling too (for example, a regular-file target
        // with a trailing slash is not a valid directory traversal).
        fs::metadata(workspace.join(relative))
            .context("verification source link does not resolve in the projection")?;
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

#[derive(Debug)]
struct PythonTool {
    interpreter: PathBuf,
    script: Option<PathBuf>,
    reads: Vec<PathBuf>,
}

fn resolve_python_tool(
    program: &str,
    path: Option<&OsStr>,
    source_root: &Path,
    home: Option<&Path>,
) -> Result<PythonTool> {
    anyhow::ensure!(
        matches!(program, "python" | "pytest"),
        "unsupported Python tool"
    );
    let mut directories = path
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .filter(|directory| directory.is_absolute())
        .collect::<Vec<_>>();
    directories.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ]);
    let candidate = directories
        .into_iter()
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
        .context("isolated verification executable is unavailable")?;
    let candidate = candidate
        .parent()
        .context("Python tool directory")?
        .canonicalize()?
        .join(program);
    let physical = candidate.canonicalize()?;
    require_regular_executable(&physical)?;
    let (interpreter, script) = if program == "pytest" {
        let bytes = read_runtime_metadata(&physical, 4096)?;
        let first_line = bytes
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap_or_default();
        let declared = std::str::from_utf8(first_line)?
            .strip_prefix("#!")
            .context("pytest entry point must declare its Python interpreter")?
            .trim();
        let interpreter = PathBuf::from(declared);
        anyhow::ensure!(
            interpreter.is_absolute()
                && !declared.chars().any(char::is_whitespace)
                && python_executable_name(&interpreter),
            "pytest entry point must declare one absolute Python interpreter"
        );
        (interpreter, Some(physical))
    } else {
        (candidate, None)
    };
    let bin = interpreter
        .parent()
        .context("Python interpreter directory")?
        .canonicalize()?;
    anyhow::ensure!(
        bin.file_name() == Some(OsStr::new("bin")),
        "Python interpreter must be installed in a bin directory"
    );
    let declared_interpreter =
        bin.join(interpreter.file_name().context("Python interpreter name")?);
    let physical_interpreter = declared_interpreter.canonicalize()?;
    require_regular_executable(&physical_interpreter)?;
    anyhow::ensure!(
        python_executable_name(&physical_interpreter),
        "Python interpreter has an unsupported executable name"
    );
    let base = physical_interpreter
        .parent()
        .and_then(Path::parent)
        .context("Python installation root")?
        .to_path_buf();
    // A runtime root is an actual Python installation, not an arbitrary parent
    // named by a script. Its standard library remains inside that installation.
    let library = base.join("lib");
    anyhow::ensure!(
        fs::read_dir(&library)?
            .filter_map(|entry| entry.ok())
            .any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("python3."))
                    && entry.path().join("encodings/__init__.py").is_file()
            }),
        "Python installation standard library is unavailable"
    );
    let mut reads = vec![base.clone()];
    let environment = bin.parent().context("Python environment root")?;
    let is_environment = environment.join("pyvenv.cfg").is_file();
    if is_environment {
        let config = read_runtime_metadata(&environment.join("pyvenv.cfg"), 16 * 1024)?;
        let home = std::str::from_utf8(&config)?
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once('=')?;
                (key.trim() == "home").then(|| PathBuf::from(value.trim()))
            })
            .context("Python environment does not declare its base installation")?;
        anyhow::ensure!(
            home.is_absolute()
                && home.canonicalize()?
                    == physical_interpreter.parent().context("Python base bin")?,
            "Python environment base installation differs from its interpreter"
        );
        reads.push(environment.to_path_buf());
    }
    if let Some(script) = &script {
        anyhow::ensure!(
            script.parent() == Some(bin.as_path())
                || (!is_environment && script.starts_with(&base)),
            "pytest entry point is outside its declared Python environment"
        );
    }
    let interpreter = if is_environment {
        declared_interpreter
    } else {
        physical_interpreter
    };
    reads.sort();
    reads.dedup();
    for root in &reads {
        anyhow::ensure!(
            root.parent().is_some()
                && !source_root.starts_with(root)
                && !home.is_some_and(|home| home.starts_with(root))
                && !root.strip_prefix(source_root).ok().is_some_and(|relative| {
                    is_private_or_protected_path(&relative.to_string_lossy())
                }),
            "Python runtime root would expose a protected source or home directory"
        );
    }
    Ok(PythonTool {
        interpreter,
        script,
        reads,
    })
}

fn python_executable_name(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| {
            name == "python"
                || name == "python3"
                || name.strip_prefix("python3.").is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
                })
        })
}

#[cfg(target_os = "macos")]
fn python_runtime_metadata_paths(interpreter: &Path) -> Result<Vec<PathBuf>> {
    anyhow::ensure!(
        interpreter.is_absolute(),
        "Python interpreter must be absolute"
    );
    let mut pending = interpreter
        .components()
        .map(|part| part.as_os_str().to_os_string())
        .collect::<std::collections::VecDeque<_>>();
    let mut resolved = PathBuf::new();
    let mut paths = BTreeSet::new();
    let mut links = 0;
    let mut components = 0;
    while let Some(part) = pending.pop_front() {
        components += 1;
        anyhow::ensure!(
            components <= 512,
            "Python runtime metadata path exceeds its bound"
        );
        if part == OsStr::new("/") {
            resolved = PathBuf::from("/");
        } else if part == OsStr::new(".") {
            continue;
        } else if part == OsStr::new("..") {
            resolved.pop();
            continue;
        } else {
            resolved.push(part);
        }
        paths.insert(resolved.clone());
        if fs::symlink_metadata(&resolved)?.file_type().is_symlink() {
            links += 1;
            anyhow::ensure!(
                links <= 32,
                "Python runtime symlink chain exceeds its bound"
            );
            let target = fs::read_link(&resolved)?;
            resolved.pop();
            let mut redirected = target
                .components()
                .map(|part| part.as_os_str().to_os_string())
                .collect::<std::collections::VecDeque<_>>();
            redirected.append(&mut pending);
            pending = redirected;
        }
    }
    anyhow::ensure!(
        resolved == interpreter.canonicalize()?,
        "Python runtime symlink identity changed"
    );
    Ok(paths.into_iter().collect())
}

fn read_runtime_metadata(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let before = file.metadata()?;
    anyhow::ensure!(
        before.is_file() && before.len() <= limit,
        "Python runtime metadata exceeds its bound"
    );
    let mut bytes = Vec::new();
    (&mut file).take(limit + 1).read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    anyhow::ensure!(
        bytes.len() as u64 <= limit
            && before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.len() == after.len()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec(),
        "Python runtime metadata changed during validation"
    );
    Ok(bytes)
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

#[cfg(not(target_os = "freebsd"))]
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

#[cfg(any(not(target_os = "freebsd"), test))]
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

#[cfg(any(not(target_os = "freebsd"), test))]
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
        "(version 1)\n(deny default)\n(allow process*)\n(allow sysctl-read)\n(allow mach-lookup)\n(allow file-read-data)\n(deny file-read-data {denied_read_rules})\n(allow file-read* {read_rules})\n(allow file-read-metadata {read_metadata_rules})\n{library_permissions}{tls_permissions}(allow file-read-metadata {scratch_metadata_rules})\n(allow file-write* (subpath \"{}\"))\n(allow file-write-data (literal \"/dev/null\"))\n(deny network*)\n",
        sbpl_escape(scratch)
    )
}

#[cfg(any(not(target_os = "freebsd"), test))]
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
    fn projection_preserves_untracked_final_bytes_and_tracked_deletions() -> Result<()> {
        let repository = tempfile::tempdir()?;
        let destination = tempfile::tempdir()?;
        fs::write(repository.path().join("kept.rs"), "old source\n")?;
        fs::write(repository.path().join("deleted.rs"), "obsolete source\n")?;
        fs::write(repository.path().join(".gitignore"), "ignored.rs\n")?;
        for arguments in [
            vec!["init", "--quiet"],
            vec!["config", "user.email", "test@example.invalid"],
            vec!["config", "user.name", "Projection Test"],
            vec!["add", "kept.rs", "deleted.rs", ".gitignore"],
            vec!["commit", "--quiet", "-m", "initial"],
        ] {
            let output = crate::git_metadata::command_no_filters(repository.path())?
                .args(arguments)
                .output()?;
            anyhow::ensure!(output.status.success(), "initialize projection fixture");
        }
        fs::write(repository.path().join("kept.rs"), "final source\n")?;
        fs::write(
            repository.path().join("new.rs"),
            "actual untracked final bytes\n",
        )?;
        fs::write(
            repository.path().join("ignored.rs"),
            "must not claim tested\n",
        )?;
        fs::remove_file(repository.path().join("deleted.rs"))?;
        let paths = Paths::new(repository.path())?;
        require_projected_candidate_paths(&paths, &["new.rs".into(), "deleted.rs".into()])?;
        assert!(require_projected_candidate_paths(&paths, &["ignored.rs".into()]).is_err());
        assert!(
            require_projected_candidate_paths(&paths, &[".orchestrator/config.toml".into()])
                .is_err()
        );
        project_repository(&paths, destination.path())?;
        assert_eq!(
            fs::read(destination.path().join("new.rs"))?,
            b"actual untracked final bytes\n"
        );
        assert_eq!(
            fs::read(destination.path().join("kept.rs"))?,
            b"final source\n"
        );
        assert!(!destination.path().join("deleted.rs").exists());
        assert!(!destination.path().join("ignored.rs").exists());
        let output = crate::git_metadata::command_no_filters(repository.path())?
            .args(["rm", "--cached", "deleted.rs"])
            .output()?;
        anyhow::ensure!(output.status.success(), "stage tracked deletion");
        require_projected_candidate_paths(&paths, &["deleted.rs".into()])?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn projection_preserves_internal_relative_symlinks() -> Result<()> {
        let repository = tempfile::tempdir()?;
        let destination = tempfile::tempdir()?;
        fs::create_dir(repository.path().join("docs"))?;
        fs::write(
            repository.path().join("README.md"),
            b"project documentation\n",
        )?;
        std::os::unix::fs::symlink("../README.md", repository.path().join("docs/index.md"))?;
        std::os::unix::fs::symlink("index.md", repository.path().join("docs/alias.md"))?;
        std::os::unix::fs::symlink("docs", repository.path().join("manual"))?;
        for args in [
            vec!["init", "--quiet"],
            vec!["config", "user.email", "test@example.invalid"],
            vec!["config", "user.name", "Projection Test"],
            vec!["add", "."],
            vec!["commit", "--quiet", "-m", "internal links"],
        ] {
            let output = crate::git_metadata::command_no_filters(repository.path())?
                .args(args)
                .output()?;
            anyhow::ensure!(output.status.success(), "initialize linked repository");
        }
        let paths = Paths::new(repository.path())?;
        project_repository(&paths, destination.path())?;
        assert_eq!(
            fs::read_link(destination.path().join("docs/index.md"))?,
            Path::new("../README.md")
        );
        assert_eq!(
            fs::read_link(destination.path().join("docs/alias.md"))?,
            Path::new("index.md")
        );
        assert_eq!(
            fs::read_link(destination.path().join("manual"))?,
            Path::new("docs")
        );
        assert_eq!(
            fs::read(destination.path().join("manual/alias.md"))?,
            b"project documentation\n"
        );
        // Exact link text and source bytes are retained, while ordinary strict
        // file reads continue refusing symlinks.
        assert!(read_no_follow_regular_source(repository.path(), "docs/index.md").is_err());
        assert_eq!(
            fs::read(repository.path().join("README.md"))?,
            b"project documentation\n"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn projection_rejects_unsafe_or_unrepresented_symlinks() -> Result<()> {
        for (name, target) in [
            ("absolute", "/etc/passwd"),
            ("escaping", "../outside"),
            ("cycle", "cycle"),
            ("private", ".orchestrator/private"),
            ("ignored", "ignored.txt"),
            ("dangling", "missing.txt"),
        ] {
            let repository = tempfile::tempdir()?;
            let destination = tempfile::tempdir()?;
            fs::write(repository.path().join(".gitignore"), "ignored.txt\n")?;
            fs::write(repository.path().join("ignored.txt"), "unprojected")?;
            fs::create_dir(repository.path().join(".orchestrator"))?;
            fs::write(repository.path().join(".orchestrator/private"), "private")?;
            std::os::unix::fs::symlink(target, repository.path().join(name))?;
            for args in [
                vec!["init", "--quiet"],
                vec!["config", "user.email", "test@example.invalid"],
                vec!["config", "user.name", "Projection Test"],
                vec!["add", ".gitignore", name],
                vec!["commit", "--quiet", "-m", "unsafe link"],
            ] {
                let output = crate::git_metadata::command_no_filters(repository.path())?
                    .args(args)
                    .output()?;
                anyhow::ensure!(
                    output.status.success(),
                    "initialize unsafe linked repository"
                );
            }
            assert!(
                project_repository(&Paths::new(repository.path())?, destination.path()).is_err(),
                "{name}"
            );
        }
        Ok(())
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
            "mod added;\n#[test]\nfn physical_boundary() {{ assert_eq!(added::value(), 42); assert!(std::fs::read({:?}).is_err()); assert!(std::fs::read(\"/etc/passwd\").is_err()); assert!(std::fs::write({:?}, b\"unapproved\").is_err()); std::fs::write(\"verified-marker\", b\"passed\").unwrap(); }}\n",
            private.display().to_string(), escaped_write.display().to_string(),
        ))?;
        fs::write(repository.join("src/deleted.rs"), "obsolete source\n")?;
        for args in [
            vec!["init", "--quiet"],
            vec![
                "add",
                "Cargo.toml",
                "Cargo.lock",
                "src/lib.rs",
                "src/deleted.rs",
            ],
        ] {
            let output = crate::git_metadata::command_no_filters(&repository)?
                .args(args)
                .output()?;
            anyhow::ensure!(
                output.status.success(),
                "initialize physical verification fixture"
            );
        }
        fs::remove_file(repository.join("src/deleted.rs"))?;
        fs::write(
            repository.join("src/added.rs"),
            "pub fn value() -> i32 { 42 }\n",
        )?;
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

    fn python_environment_fixture(root: &Path) -> Result<(PathBuf, PathBuf)> {
        let base = root.join("python-installation");
        let environment = root.join("selected-venv");
        fs::create_dir_all(base.join("bin"))?;
        fs::create_dir_all(base.join("lib/python3.11/encodings"))?;
        fs::create_dir_all(environment.join("bin"))?;
        fs::write(
            base.join("lib/python3.11/encodings/__init__.py"),
            b"# stdlib fixture\n",
        )?;
        let physical = base.join("bin/python3.11");
        fs::copy("/bin/cat", &physical)?;
        fs::set_permissions(&physical, fs::Permissions::from_mode(0o755))?;
        std::os::unix::fs::symlink(&physical, environment.join("bin/python"))?;
        fs::write(
            environment.join("pyvenv.cfg"),
            format!(
                "home = {}\ninclude-system-site-packages = false\n",
                base.join("bin").display()
            ),
        )?;
        let pytest = environment.join("bin/pytest");
        fs::write(
            &pytest,
            format!(
                "#!{}\n# pytest entry point fixture\n",
                environment.join("bin/python").display()
            ),
        )?;
        fs::set_permissions(&pytest, fs::Permissions::from_mode(0o755))?;
        Ok((base, environment))
    }

    #[test]
    fn runtime_python_path_preserves_virtualenv_and_resolves_pytest_interpreter() -> Result<()> {
        let root = tempfile::tempdir()?;
        let root = root.path().canonicalize()?;
        let (base, environment) = python_environment_fixture(&root)?;
        let path =
            std::env::join_paths([Path::new("relative-untrusted"), &environment.join("bin")])?;
        for name in ["python", "pytest"] {
            let tool = resolve_python_tool(
                name,
                Some(&path),
                &root.join("source"),
                Some(&root.join("home")),
            )?;
            assert_eq!(tool.interpreter, environment.join("bin/python"));
            let expected_script = (name == "pytest").then(|| environment.join("bin/pytest"));
            assert_eq!(tool.script, expected_script);
            assert_eq!(tool.reads, [base.clone(), environment.clone()]);
            assert!(!tool.reads.contains(&root));
        }
        Ok(())
    }

    #[test]
    fn runtime_python_rejects_mismatched_base_and_broad_or_private_roots() -> Result<()> {
        let root = tempfile::tempdir()?;
        let root = root.path().canonicalize()?;
        let (base, environment) = python_environment_fixture(&root)?;
        let path = std::env::join_paths([environment.join("bin")])?;
        let resolve = |source: &Path, home: Option<&Path>| {
            resolve_python_tool("pytest", Some(&path), source, home)
        };
        assert!(resolve(&root.join("source"), Some(&base)).is_err());
        assert!(resolve(&base, None).is_err());
        // A selected environment may be inside the source tree, but protected
        // operational directories never become dependency read exceptions.
        let private = root.join(".orchestrator/selected-venv");
        fs::create_dir_all(private.parent().context("private runtime parent")?)?;
        fs::rename(&environment, &private)?;
        let private_path = std::env::join_paths([private.join("bin")])?;
        fs::write(
            private.join("bin/pytest"),
            format!("#!{}\n", private.join("bin/python").display()),
        )?;
        assert!(resolve_python_tool("pytest", Some(&private_path), &root, None).is_err());
        fs::rename(&private, &environment)?;
        fs::write(
            environment.join("bin/pytest"),
            format!("#!{}\n", environment.join("bin/python").display()),
        )?;
        fs::write(
            environment.join("pyvenv.cfg"),
            format!("home = {}\n", root.display()),
        )?;
        assert!(resolve(&root.join("source"), None).is_err());
        assert!(resolve_python_tool("sh", Some(&path), &root.join("source"), None).is_err());
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_python_runtime_grants_keep_sibling_private_files_and_writes_denied() -> Result<()> {
        let root = tempfile::tempdir()?;
        let root = root.path().canonicalize()?;
        let (base, environment) = python_environment_fixture(&root)?;
        let alias = root.join("python-current");
        std::os::unix::fs::symlink("python-installation", &alias)?;
        fs::remove_file(environment.join("bin/python"))?;
        std::os::unix::fs::symlink(alias.join("bin/python3.11"), environment.join("bin/python"))?;
        let signed = Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(base.join("bin/python3.11"))
            .output()?;
        assert!(signed.status.success(), "{signed:?}");
        let scratch = root.join("scratch");
        create_private_directory(&scratch)?;
        let private = root.join("private-host-file");
        fs::write(&private, b"private host fixture")?;
        let mut profile = isolation_profile_with_libraries(
            &scratch,
            &scratch,
            Path::new("/bin"),
            &[base.clone(), environment.clone()],
            &root,
            &[base.clone(), environment.clone()],
            &[],
        );
        let launch = |profile: &str| {
            Command::new("/usr/bin/sandbox-exec")
                .env_clear()
                .args(["-p", profile])
                .arg(environment.join("bin/python"))
                .arg(environment.join("pyvenv.cfg"))
                .output()
        };
        assert!(!launch(&profile)?.status.success());
        let metadata = python_runtime_metadata_paths(&environment.join("bin/python"))?;
        assert!(metadata.contains(&alias));
        assert!(!metadata.contains(&private));
        for path in metadata {
            profile.push_str(&format!(
                "(allow file-read-metadata (literal \"{}\"))\n",
                sbpl_escape(&path)
            ));
        }
        let launched = launch(&profile)?;
        assert!(launched.status.success(), "{launched:?}");
        let run = |program: &str, args: &[&OsStr]| {
            Command::new("/usr/bin/sandbox-exec")
                .env_clear()
                .args(["-p", &profile, program])
                .args(args)
                .output()
        };
        for allowed in [
            environment.join("pyvenv.cfg"),
            base.join("lib/python3.11/encodings/__init__.py"),
        ] {
            assert!(run("/bin/cat", &[allowed.as_os_str()])?.status.success());
        }
        assert!(!run("/bin/cat", &[private.as_os_str()])?.status.success());
        assert!(!run(
            "/usr/bin/touch",
            &[environment.join("forbidden-write").as_os_str()]
        )?
        .status
        .success());
        assert!(!environment.join("forbidden-write").exists());
        assert!(run(
            "/bin/sh",
            &[OsStr::new("-c"), OsStr::new("printf sink >/dev/null")]
        )?
        .status
        .success());
        assert!(profile.contains("(deny network*)"));
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn python_runtime_metadata_rejects_symlink_cycles() -> Result<()> {
        let root = tempfile::tempdir()?;
        std::os::unix::fs::symlink("second", root.path().join("first"))?;
        std::os::unix::fs::symlink("first", root.path().join("second"))?;
        assert!(python_runtime_metadata_paths(&root.path().join("first"))
            .unwrap_err()
            .to_string()
            .contains("symlink chain exceeds its bound"));
        Ok(())
    }
}
