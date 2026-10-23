//! Fixed trusted-tool Mach-O dependency metadata for physical isolation.
use crate::cancellation::CancellationToken;
use anyhow::Result;
#[cfg(not(target_os = "freebsd"))]
use std::path::Path;
use std::path::PathBuf;

#[cfg(not(target_os = "freebsd"))]
pub(super) fn trusted_library_reads(
    executable: &Path,
    cancellation: &CancellationToken,
) -> Result<Vec<PathBuf>> {
    #[cfg(target_os = "macos")]
    {
        macos::collect(executable, cancellation)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (executable, cancellation);
        anyhow::bail!("trusted library dependency discovery requires macOS")
    }
}

pub(super) fn trusted_developer_paths(
    cancellation: &CancellationToken,
) -> Result<(PathBuf, PathBuf, PathBuf)> {
    #[cfg(target_os = "macos")]
    {
        macos::developer_paths(cancellation)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = cancellation;
        anyhow::bail!("trusted SDK discovery requires macOS")
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use anyhow::{bail, Context};
    use std::collections::{BTreeSet, VecDeque};
    use std::fs;
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    use std::path::Component;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const MAX_OBJECTS: usize = 128;
    const MAX_RPATHS: usize = 64;
    const MAX_METADATA_BYTES: u64 = 256 * 1024;
    const MAX_ERROR_BYTES: u64 = 16 * 1024;
    const MAX_DISCOVERY_TIME: Duration = Duration::from_secs(20);

    fn system_path(path: &Path) -> bool {
        [
            "/usr",
            "/System",
            "/Library/Developer",
            "/Applications/Xcode.app",
        ]
        .iter()
        .any(|root| path.starts_with(root))
            && !path.components().any(|c| matches!(c, Component::ParentDir))
    }

    fn trusted_install_path(path: &Path) -> bool {
        [
            "/opt/homebrew/Cellar",
            "/usr/local/Cellar",
            "/usr/local/lib",
        ]
        .iter()
        .any(|root| path.starts_with(root))
            || system_path(path)
    }

    fn regular_library(path: &Path) -> Result<PathBuf> {
        let canonical = path
            .canonicalize()
            .context("resolve trusted executable library dependency")?;
        if !trusted_install_path(&canonical) {
            bail!("executable library dependency is outside trusted installed roots");
        }
        let metadata = fs::symlink_metadata(&canonical)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("executable library dependency is not a canonical regular file");
        }
        Ok(canonical)
    }

    fn declared_directory(path: &Path) -> Result<PathBuf> {
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                Component::RootDir => normalized.push("/"),
                Component::Normal(value) => normalized.push(value),
                Component::CurDir => {}
                Component::ParentDir if normalized.pop() => {}
                _ => bail!("trusted library directory escapes its declared root"),
            }
        }
        if !trusted_install_path(&normalized)
            && !["/opt/homebrew/opt", "/usr/local/opt"]
                .iter()
                .any(|root| normalized.starts_with(root))
        {
            bail!("declared library directory is outside trusted installed roots");
        }
        let canonical = normalized.canonicalize()?;
        if !trusted_install_path(&canonical) || !canonical.is_dir() {
            bail!("declared library directory is not a trusted installed directory");
        }
        Ok(normalized)
    }

    fn expand_loader_path(value: &str, loader: &Path, executable: &Path) -> Result<PathBuf> {
        if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
            bail!("executable library path is outside its bound");
        }
        if let Some(relative) = value.strip_prefix("@loader_path/") {
            Ok(loader
                .parent()
                .context("library loader parent")?
                .join(relative))
        } else if let Some(relative) = value.strip_prefix("@executable_path/") {
            Ok(executable
                .parent()
                .context("trusted executable parent")?
                .join(relative))
        } else if Path::new(value).is_absolute() {
            Ok(value.into())
        } else {
            bail!("unsupported executable library path")
        }
    }

    #[derive(Default)]
    struct Metadata {
        dependencies: Vec<String>,
        rpaths: Vec<String>,
    }

    fn parse_metadata(output: &str) -> Result<Metadata> {
        let mut result = Metadata::default();
        let mut command = "";
        for line in output.lines().map(str::trim) {
            if line.starts_with("Load command ") {
                command = "";
            } else if let Some(value) = line.strip_prefix("cmd ") {
                command = value;
            } else if command == "LC_RPATH" {
                if let Some(value) = line.strip_prefix("path ") {
                    let (path, _) = value
                        .rsplit_once(" (offset ")
                        .context("Mach-O rpath metadata")?;
                    result.rpaths.push(path.into());
                }
            } else if matches!(
                command,
                "LC_LOAD_DYLIB"
                    | "LC_LOAD_WEAK_DYLIB"
                    | "LC_REEXPORT_DYLIB"
                    | "LC_LOAD_UPWARD_DYLIB"
            ) {
                if let Some(value) = line.strip_prefix("name ") {
                    let (path, _) = value
                        .rsplit_once(" (offset ")
                        .context("Mach-O library metadata")?;
                    result.dependencies.push(path.into());
                }
            }
            if result.dependencies.len() > MAX_OBJECTS || result.rpaths.len() > MAX_RPATHS {
                bail!("trusted executable library metadata exceeds its count bound");
            }
        }
        Ok(result)
    }

    fn cancelled(cancellation: &CancellationToken) -> bool {
        cancellation.is_cancelled()
            || crate::runtime::current_run_store()
                .is_some_and(|store| store.cancel_requested().unwrap_or(true))
    }

    fn metadata_command(
        program: &str,
        args: &[&std::ffi::OsStr],
        deadline: Instant,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>> {
        if cancelled(cancellation) || Instant::now() >= deadline {
            bail!("trusted executable library discovery was cancelled or timed out");
        }
        let mut child = Command::new(program)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("inspect fixed trusted executable library metadata")?;
        let stdout = child.stdout.take().context("library metadata stdout")?;
        let stderr = child.stderr.take().context("library metadata stderr")?;
        let read = |reader: Box<dyn Read + Send>, limit: u64| {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                reader.take(limit + 1).read_to_end(&mut bytes)?;
                Ok::<_, std::io::Error>(bytes)
            })
        };
        let stdout = read(Box::new(stdout), MAX_METADATA_BYTES);
        let stderr = read(Box::new(stderr), MAX_ERROR_BYTES);
        let local_deadline = deadline.min(Instant::now() + Duration::from_secs(5));
        let (status, interrupted) = loop {
            match child.try_wait() {
                Ok(Some(status)) => break (Ok(status), false),
                Ok(None) if !cancelled(cancellation) && Instant::now() < local_deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(None) => {
                    let _ = child.kill();
                    break (child.wait(), true);
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break (Err(error), false);
                }
            }
        };
        let output = stdout
            .join()
            .map_err(|_| anyhow::anyhow!("library metadata reader stopped"))??;
        let errors = stderr
            .join()
            .map_err(|_| anyhow::anyhow!("library metadata error reader stopped"))??;
        if interrupted
            || !status?.success()
            || output.len() as u64 > MAX_METADATA_BYTES
            || errors.len() as u64 > MAX_ERROR_BYTES
        {
            bail!("trusted executable library metadata is unavailable or exceeded its bound");
        }
        Ok(output)
    }

    fn inspect(
        path: &Path,
        deadline: Instant,
        cancellation: &CancellationToken,
    ) -> Result<Metadata> {
        let before = fs::symlink_metadata(path)?;
        let output = metadata_command(
            "/usr/bin/otool",
            &[std::ffi::OsStr::new("-l"), path.as_os_str()],
            deadline,
            cancellation,
        )?;
        let after = fs::symlink_metadata(path)?;
        if before.dev() != after.dev()
            || before.ino() != after.ino()
            || before.len() != after.len()
            || before.mtime() != after.mtime()
            || before.mtime_nsec() != after.mtime_nsec()
        {
            bail!("trusted executable changed during library metadata discovery");
        }
        parse_metadata(std::str::from_utf8(&output).context("Mach-O metadata is not UTF-8")?)
    }

    pub(super) fn developer_paths(
        cancellation: &CancellationToken,
    ) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let output = metadata_command(
            "/usr/bin/xcode-select",
            &[std::ffi::OsStr::new("-p")],
            Instant::now() + Duration::from_secs(5),
            cancellation,
        )?;
        if output.len() > 4096 {
            bail!("trusted SDK path exceeded its bound");
        }
        let declared = PathBuf::from(std::str::from_utf8(&output)?.trim());
        let developer = declared.canonicalize()?;
        if !["/Library/Developer", "/Applications/Xcode.app"]
            .iter()
            .any(|root| developer.starts_with(root))
        {
            bail!("configured SDK is outside fixed trusted Developer roots");
        }
        let meta = fs::symlink_metadata(&developer)?;
        if !meta.is_dir() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
            bail!("configured SDK installation is not protected");
        }
        let sdk = [
            developer.join("SDKs/MacOSX.sdk"),
            developer.join("Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk"),
        ]
        .into_iter()
        .find(|path| path.is_dir())
        .context("trusted macOS SDK is unavailable")?
        .canonicalize()?;
        let meta = fs::symlink_metadata(&sdk)?;
        if !sdk.starts_with(&developer)
            || !meta.is_dir()
            || meta.uid() != 0
            || meta.mode() & 0o022 != 0
        {
            bail!("configured SDK path is not protected");
        }
        let tool_bin = [
            developer.join("Toolchains/XcodeDefault.xctoolchain/usr/bin"),
            developer.join("usr/bin"),
        ]
        .into_iter()
        .find(|path| path.join("cc").is_file() && path.join("clang").is_file())
        .context("trusted physical compiler is unavailable")?
        .canonicalize()?;
        let meta = fs::symlink_metadata(&tool_bin)?;
        if !tool_bin.starts_with(&developer)
            || !meta.is_dir()
            || meta.uid() != 0
            || meta.mode() & 0o022 != 0
        {
            bail!("trusted physical compiler directory is not protected");
        }
        for name in ["cc", "clang"] {
            let executable = tool_bin.join(name).canonicalize()?;
            let meta = fs::symlink_metadata(&executable)?;
            if !executable.starts_with(&tool_bin)
                || !meta.is_file()
                || meta.uid() != 0
                || meta.mode() & 0o022 != 0
                || meta.mode() & 0o111 == 0
            {
                bail!("trusted physical compiler is not a protected regular executable");
            }
        }
        Ok((developer, sdk, tool_bin))
    }

    pub(super) fn collect(
        executable: &Path,
        cancellation: &CancellationToken,
    ) -> Result<Vec<PathBuf>> {
        let executable = executable.canonicalize()?;
        let deadline = Instant::now() + MAX_DISCOVERY_TIME;
        let mut pending = VecDeque::from([executable.clone()]);
        if executable.file_name().is_some_and(|name| name == "cargo") {
            let bin = executable.parent().context("trusted Cargo bin directory")?;
            // Cargo starts these fixed compiler companions, whose dylibs need
            // the same physical sandbox. They remain inside this one bounded
            // metadata closure and are never taken from repository config.
            for name in ["rustc", "rustdoc"] {
                pending.push_back(regular_library(&bin.join(name))?);
            }
        }
        let mut inspected = BTreeSet::new();
        let mut libraries = BTreeSet::new();
        let mut read_directories = BTreeSet::new();
        let mut rpaths = BTreeSet::new();
        while let Some(loader) = pending.pop_front() {
            if !inspected.insert(loader.clone()) {
                continue;
            }
            if inspected.len() > MAX_OBJECTS {
                bail!("trusted executable library closure exceeds its count bound");
            }
            let metadata = inspect(&loader, deadline, cancellation)?;
            for path in metadata.rpaths {
                let path = expand_loader_path(&path, &loader, &executable)?;
                if !path.exists() {
                    continue;
                }
                let path = path.canonicalize()?;
                if !trusted_install_path(&path) || !path.is_dir() {
                    bail!("trusted executable rpath is outside installed library roots");
                }
                rpaths.insert(path);
                if rpaths.len() > MAX_RPATHS {
                    bail!("trusted executable rpath count exceeds its bound");
                }
            }
            for declared in metadata.dependencies {
                let candidate = if let Some(relative) = declared.strip_prefix("@rpath/") {
                    if relative.is_empty()
                        || relative.len() > 4096
                        || relative.chars().any(char::is_control)
                    {
                        bail!("trusted executable rpath dependency is invalid");
                    }
                    rpaths
                        .iter()
                        .map(|root| root.join(relative))
                        .find(|path| path.is_file())
                        .context("trusted executable rpath dependency is unavailable")?
                } else {
                    expand_loader_path(&declared, &loader, &executable)?
                };
                // System shared-cache libraries need not exist as filesystem
                // files. Their established read scope already covers them.
                if system_path(&candidate) {
                    continue;
                }
                let library = regular_library(&candidate)?;
                // dyld probes intermediate versioned symlink basenames as well
                // as the final regular library. Restrict traversal/read/map to
                // the declared and canonical library directories only.
                read_directories.insert(declared_directory(
                    candidate.parent().context("declared library directory")?,
                )?);
                read_directories.insert(
                    library
                        .parent()
                        .context("canonical library directory")?
                        .to_path_buf(),
                );
                if libraries.insert(library.clone()) {
                    pending.push_back(library);
                }
            }
        }
        Ok(read_directories.into_iter().collect())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn metadata_resolution_rejects_private_and_unsupported_paths() {
            assert!(!trusted_install_path(Path::new(
                "/Users/example/.ssh/library.dylib"
            )));
            assert!(expand_loader_path(
                "@unknown/lib.dylib",
                Path::new("/usr/bin/tool"),
                Path::new("/usr/bin/tool")
            )
            .is_err());
            let metadata = parse_metadata("Load command 0\ncmd LC_LOAD_DYLIB\nname @loader_path/../lib/needed.dylib (offset 24)\nLoad command 1\ncmd LC_RPATH\npath @executable_path/../lib (offset 12)\n").unwrap();
            assert_eq!(metadata.dependencies, ["@loader_path/../lib/needed.dylib"]);
            assert_eq!(metadata.rpaths, ["@executable_path/../lib"]);
        }
    }
}
