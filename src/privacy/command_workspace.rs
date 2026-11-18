//! Owner-scoped writable command views. No hard links and no implicit publish.
use super::Context;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(crate) struct Workspaces {
    generations: BTreeMap<String, Generation>,
    inplace: Vec<PathBuf>,
}
struct Generation {
    roots: Vec<(PathBuf, PathBuf)>,
    digest: String,
}
pub(crate) struct PreparedCommand {
    pub command: String,
    pub cwd: String,
    pub roots: Vec<PathBuf>,
    pub generation: String,
    pub artifact_directory: String,
}
impl Workspaces {
    pub fn roots(&self) -> Vec<PathBuf> {
        self.generations
            .values()
            .flat_map(|generation| generation.roots.iter().map(|(_, view)| view.clone()))
            .collect()
    }
}

impl Context {
    /// Only an in-process, host-created mapping establishes a command-copy role.
    /// No command output or saved observation can create this authority.
    pub(crate) fn command_copy_binding(&self, path: &Path) -> Option<(String, String, PathBuf)> {
        if path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return None;
        }
        let manager = self
            .command_workspaces
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        manager.generations.values().find_map(|generation| {
            generation.roots.iter().find_map(|(source, view)| {
                let relative = path.strip_prefix(view).ok()?;
                Some((
                    generation.digest.clone(),
                    super::digest(view.to_string_lossy().as_bytes()),
                    source.join(relative),
                ))
            })
        })
    }

    pub(crate) fn command_artifact_roots(&self) -> Vec<PathBuf> {
        self.command_workspaces
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .roots()
    }

    /// Called on a worker after the original native command has been admitted.
    pub(crate) fn prepare_command_workspace(
        &self,
        command: &str,
        cwd: &str,
        roots: &[PathBuf],
    ) -> Result<PreparedCommand, String> {
        let mut manager = self
            .command_workspaces
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let base = self
            .scope
            .join("command-workspaces")
            .join(self.session_id.to_string());
        // An explicitly selected artifact directory keeps the generation that
        // owns it, including while newer source generations exist.
        let artifact_key = manager.generations.iter().find_map(|(key, generation)| {
            generation
                .roots
                .iter()
                .any(|(_, view)| Path::new(cwd).starts_with(view))
                .then(|| key.clone())
        });
        let key = if let Some(key) = artifact_key {
            key
        } else {
            let mut sources = roots
                .iter()
                .filter_map(|root| root.canonicalize().ok())
                .collect::<Vec<_>>();
            sources.sort();
            sources.dedup();
            // Generated views are already writable independent copies. Never clone
            // them recursively, or clone a broad ancestor of ca's private store.
            sources.retain(|root| !root.starts_with(&base) && root != Path::new("/"));
            let mut pinned = Vec::<PathBuf>::new();
            for source in sources {
                if !pinned.iter().any(|parent| source.starts_with(parent)) {
                    pinned.push(source);
                }
            }
            if !pinned.iter().any(|root| Path::new(cwd).starts_with(root)) {
                return Err("Command directory needs a current project or folder grant".into());
            }
            let excluded = [self.private_metadata_root(), base.clone()];
            if manager.inplace == pinned {
                drop(manager);
                self.refresh_authorized_roots()?;
                return Ok(PreparedCommand {
                    command: command.to_string(),
                    cwd: cwd.to_string(),
                    roots: pinned,
                    generation: "inplace".into(),
                    artifact_directory: self
                        .project_filename_path(Path::new(cwd))
                        .unwrap_or_default(),
                });
            }
            let (identity, capped) = inventory(&pinned, &excluded, FILE_BUDGET)?;
            let digest = super::digest(
                &serde_json::to_vec(&identity)
                    .map_err(|_| "Command snapshot cannot be identified")?,
            );
            if capped {
                manager.inplace = pinned.clone();
                drop(manager);
                self.refresh_authorized_roots()?;
                return Ok(PreparedCommand {
                    command: command.to_string(),
                    cwd: cwd.to_string(),
                    roots: pinned,
                    generation: digest,
                    artifact_directory: self
                        .project_filename_path(Path::new(cwd))
                        .unwrap_or_default(),
                });
            }
            if !manager.generations.contains_key(&digest) {
                ca_automation::runtime::private_dir(&base)
                    .map_err(|_| "Command workspace storage is unavailable")?;
                let mut stable = None;
                for _ in 0..2 {
                    let (identity, capped) = inventory(&pinned, &excluded, FILE_BUDGET)?;
                    if capped {
                        continue;
                    }
                    let digest = super::digest(
                        &serde_json::to_vec(&identity)
                            .map_err(|_| "Command snapshot cannot be identified")?,
                    );
                    let staging = tempfile::Builder::new()
                        .prefix("generation-")
                        .tempdir_in(&base)
                        .map_err(|_| "Command workspace cannot be staged")?;
                    let mut mapped = Vec::new();
                    for (index, source) in pinned.iter().enumerate() {
                        let view = staging.path().join(format!("root-{index}"));
                        copy_tree(source, &view, &excluded)?;
                        mapped.push((source.clone(), view));
                    }
                    rebase_links(&mapped)?;
                    if inventory(&pinned, &excluded, FILE_BUDGET)?.0 != identity {
                        continue;
                    }
                    let directory = staging.keep();
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(
                            &directory,
                            std::fs::Permissions::from_mode(0o700),
                        )
                        .map_err(|_| "Command workspace permissions cannot be secured")?;
                    }
                    let generation = Generation {
                        roots: mapped,
                        digest: digest.clone(),
                    };
                    ca_automation::runtime::atomic_write(&directory.join("generation.json"), &serde_json::json!({"schema":"ca.command-workspace.v1","source_digest":digest,"session_id":self.session_id}))
                    .map_err(|_| "Command workspace receipt cannot be saved")?;
                    manager.generations.insert(digest.clone(), generation);
                    stable = Some(digest);
                    break;
                }
                if stable.is_none() {
                    return Err(
                        "Project changed while preparing the command workspace; retry the command"
                            .into(),
                    );
                }
            }
            // The first identity may have drifted while staging. Rebind to the
            // independently checked identity; no command has executed yet.
            let current = inventory(&pinned, &excluded, FILE_BUDGET)?.0;
            super::digest(
                &serde_json::to_vec(&current)
                    .map_err(|_| "Command snapshot cannot be identified")?,
            )
        };
        let generation = manager
            .generations
            .get(&key)
            .ok_or("Project changed before command dispatch; retry the command")?;
        let map = |path: &Path| -> Result<PathBuf, String> {
            if generation
                .roots
                .iter()
                .any(|(_, view)| path.starts_with(view))
            {
                return Ok(path.to_path_buf());
            }
            let (source, view) = generation
                .roots
                .iter()
                .filter(|(source, _)| path.starts_with(source))
                .max_by_key(|(source, _)| source.components().count())
                .ok_or("Command path is outside granted roots")?;
            Ok(view.join(
                path.strip_prefix(source)
                    .map_err(|_| "Command path cannot be mapped")?,
            ))
        };
        let cwd = map(Path::new(cwd))?;
        // Quoted native absolute arguments were generated by shell::restore.
        // Reparse them as literals when rebasing; never replace command syntax.
        let command = super::shell::rebase_paths(command, |path| map(path))?;
        let mut prepared = PreparedCommand {
            command,
            cwd: cwd.to_str().ok_or("Unsupported command path")?.into(),
            roots: generation
                .roots
                .iter()
                .map(|(_, view)| view.clone())
                .collect(),
            generation: generation.digest.clone(),
            artifact_directory: String::new(),
        };
        drop(manager);
        self.refresh_authorized_roots()?;
        prepared.artifact_directory = self.project_filename_path(Path::new(&prepared.cwd))?;
        Ok(prepared)
    }
}

type Inventory = Vec<(String, u64, u128, u32, String)>;
const FILE_BUDGET: usize = 262_144;
const CONTENT_FILE_CAP: u64 = 1024 * 1024;
const CONTENT_TOTAL_CAP: u64 = 64 * 1024 * 1024;

fn included_source(entry: &walkdir::DirEntry, excluded: &[PathBuf]) -> bool {
    !excluded.iter().any(|path| entry.path().starts_with(path))
        && (entry.depth() == 0 || !crate::command_mutations::skipped_directory(entry.file_name()))
}

fn walk_is_unreadable(error: &walkdir::Error) -> bool {
    let Some(io) = error.io_error() else {
        return false;
    };
    matches!(
        io.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound
    ) || matches!(io.raw_os_error(), Some(1 | 13))
}

fn walk_failure(action: &str, error: &walkdir::Error) -> String {
    let path = error
        .path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "the workspace root".to_string());
    format!(
        "Command source cannot be {action} at {path}: {error}. Grant read access, or keep the workspace off volume metadata such as .fseventsd, then retry the command."
    )
}

fn inventory(
    roots: &[PathBuf],
    excluded: &[PathBuf],
    file_limit: usize,
) -> Result<(Inventory, bool), String> {
    let mut inventory = Vec::new();
    let mut bytes_read = 0_u64;
    let mut capped = false;
    'roots: for root in roots {
        for entry in walkdir::WalkDir::new(root)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| included_source(entry, excluded))
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if walk_is_unreadable(&error) => continue,
                Err(error) => return Err(walk_failure("inventoried", &error)),
            };
            if inventory.len() >= file_limit {
                capped = true;
                break 'roots;
            }
            let metadata = std::fs::symlink_metadata(entry.path())
                .map_err(|_| "Command source changed during discovery")?;
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |time| time.as_nanos());
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::MetadataExt;
                metadata.mode()
            };
            #[cfg(not(unix))]
            let mode = u32::from(metadata.permissions().readonly());
            let mut hasher = Sha256::new();
            if metadata.file_type().is_symlink() {
                let target = std::fs::read_link(entry.path())
                    .map_err(|_| "Command link target is unavailable")?;
                #[cfg(unix)]
                {
                    use std::os::unix::ffi::OsStrExt;
                    hasher.update(target.as_os_str().as_bytes());
                }
                #[cfg(not(unix))]
                hasher.update(target.to_string_lossy().as_bytes());
            } else if metadata.is_file()
                && (metadata.len() > CONTENT_FILE_CAP || bytes_read >= CONTENT_TOTAL_CAP)
            {
                capped = true;
                hasher.update(b"fingerprint");
                hasher.update(metadata.len().to_le_bytes());
            } else if metadata.is_file() {
                let mut options = std::fs::OpenOptions::new();
                options.read(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
                }
                let mut file = options
                    .open(entry.path())
                    .map_err(|_| "Command source identity cannot be read")?;
                let mut buffer = [0_u8; 65536];
                loop {
                    let count = file
                        .read(&mut buffer)
                        .map_err(|_| "Command source changed while hashing")?;
                    if count == 0 {
                        break;
                    }
                    let room = CONTENT_TOTAL_CAP.saturating_sub(bytes_read);
                    if room == 0 {
                        capped = true;
                        break;
                    }
                    let count = count.min(room as usize);
                    bytes_read += count as u64;
                    hasher.update(&buffer[..count]);
                    if bytes_read >= CONTENT_TOTAL_CAP {
                        capped = true;
                        break;
                    }
                }
            }
            let content = format!("{:x}", hasher.finalize());
            inventory.push((
                entry
                    .path()
                    .to_str()
                    .ok_or("Unsupported command filename encoding")?
                    .into(),
                metadata.len(),
                modified,
                mode,
                content,
            ));
        }
    }
    Ok((inventory, capped))
}
fn copy_tree(source: &Path, destination: &Path, excluded: &[PathBuf]) -> Result<(), String> {
    for entry in walkdir::WalkDir::new(source)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| included_source(entry, excluded))
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if walk_is_unreadable(&error) => continue,
            Err(error) => return Err(walk_failure("copied", &error)),
        };
        let target = destination.join(
            entry
                .path()
                .strip_prefix(source)
                .map_err(|_| "Command source escaped its root")?,
        );
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target).map_err(|_| "Command directory cannot be created")?;
        } else if entry.file_type().is_symlink() {
            #[cfg(unix)]
            {
                let link = std::fs::read_link(entry.path())
                    .map_err(|_| "Command symlink cannot be inspected")?;
                std::os::unix::fs::symlink(link, &target)
                    .map_err(|_| "Command symlink cannot be copied")?;
            }
            #[cfg(not(unix))]
            return Err("Command symlinks are unsupported on this platform".into());
        } else if entry.file_type().is_file() {
            clone_or_copy(entry.path(), &target)?;
        } else {
            return Err("Command snapshot contains an unsupported special file".into());
        }
    }
    Ok(())
}
fn rebase_links(roots: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    for (source, view) in roots {
        for entry in walkdir::WalkDir::new(view).follow_links(false) {
            let entry = entry.map_err(|_| "Cloned links cannot be inspected")?;
            if !entry.file_type().is_symlink() {
                continue;
            }
            let link =
                std::fs::read_link(entry.path()).map_err(|_| "Cloned link is unavailable")?;
            let original = source.join(
                entry
                    .path()
                    .strip_prefix(view)
                    .map_err(|_| "Cloned link escaped its root")?,
            );
            let native = if link.is_absolute() {
                link.clone()
            } else {
                original
                    .parent()
                    .ok_or("Cloned link has no parent")?
                    .join(&link)
            };
            let target = match native.canonicalize() {
                Ok(target) => target,
                Err(_) => continue,
            };
            if let Some((root, destination)) = roots
                .iter()
                .filter(|(root, _)| target.starts_with(root))
                .max_by_key(|(root, _)| root.components().count())
            {
                let rebased = destination.join(
                    target
                        .strip_prefix(root)
                        .map_err(|_| "Cloned link target escaped its root")?,
                );
                std::fs::remove_file(entry.path()).map_err(|_| "Cloned link cannot be rebased")?;
                #[cfg(unix)]
                std::os::unix::fs::symlink(rebased, entry.path())
                    .map_err(|_| "Cloned link cannot be rebased")?;
            }
        }
    }
    Ok(())
}

fn clone_or_copy(source: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        unsafe extern "C" {
            fn clonefile(
                source: *const std::ffi::c_char,
                destination: *const std::ffi::c_char,
                flags: i32,
            ) -> i32;
        }
        let source = std::ffi::CString::new(source.as_os_str().as_bytes())
            .map_err(|_| "Invalid command source path")?;
        let target = std::ffi::CString::new(destination.as_os_str().as_bytes())
            .map_err(|_| "Invalid command destination path")?;
        // CLONE_NOFOLLOW. Every file gets a separate inode; no hard links.
        if unsafe { clonefile(source.as_ptr(), target.as_ptr(), 1) } == 0 {
            return Ok(());
        }
    }
    std::fs::copy(source, destination)
        .map(|_| ())
        .map_err(|_| "Command file cannot be independently copied".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_command_clone_isolates_files_and_preserves_links() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let view = temp.path().join("view");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("Private.html"), "original").unwrap();
        copy_tree(&source, &view, &[]).unwrap();
        std::fs::write(view.join("Private.html"), "changed").unwrap();
        assert_eq!(
            std::fs::read_to_string(source.join("Private.html")).unwrap(),
            "original"
        );
        std::fs::rename(view.join("Private.html"), view.join("new.html")).unwrap();
        assert!(source.join("Private.html").exists());
    }

    #[test]
    fn volume_metadata_is_left_out_of_the_command_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("keep.txt"), "visible").unwrap();
        let events = source.join(".fseventsd");
        std::fs::create_dir(&events).unwrap();
        std::fs::write(events.join("note.txt"), "event").unwrap();
        let (found, capped) = inventory(std::slice::from_ref(&source), &[], FILE_BUDGET).unwrap();
        assert!(!capped);
        assert!(found.iter().any(|(path, ..)| path.ends_with("keep.txt")));
        assert!(found.iter().all(|(path, ..)| !path.contains(".fseventsd")));
        let view = temp.path().join("view");
        copy_tree(&source, &view, &[]).unwrap();
        assert_eq!(
            std::fs::read_to_string(view.join("keep.txt")).unwrap(),
            "visible"
        );
        assert!(!view.join(".fseventsd").exists());
    }

    #[cfg(unix)]
    #[test]
    fn unlistable_directory_does_not_fail_the_command_snapshot() {
        use std::os::unix::fs::PermissionsExt;
        struct Restore(PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("keep.txt"), "visible").unwrap();
        let sealed = source.join("sealed");
        std::fs::create_dir(&sealed).unwrap();
        std::fs::write(sealed.join("hidden.txt"), "secret").unwrap();
        let _restore = Restore(sealed.clone());
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o000)).unwrap();
        let (found, _) = inventory(std::slice::from_ref(&source), &[], FILE_BUDGET).unwrap();
        assert!(found.iter().any(|(path, ..)| path.ends_with("keep.txt")));
        assert!(found.iter().all(|(path, ..)| !path.ends_with("hidden.txt")));
    }

    #[test]
    fn oversized_workspace_stays_runnable() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("keep.txt"), "visible").unwrap();
        let large = source.join("large.bin");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(CONTENT_FILE_CAP + 1)
            .unwrap();
        let (found, capped) = inventory(std::slice::from_ref(&source), &[], FILE_BUDGET).unwrap();
        assert!(capped);
        assert!(found.iter().any(|(path, ..)| path.ends_with("keep.txt")));
        assert!(found.iter().any(|(path, ..)| path.ends_with("large.bin")));
    }

    #[test]
    fn file_budget_runs_the_command_in_place() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("a.txt"), "a").unwrap();
        std::fs::write(source.join("b.txt"), "b").unwrap();
        let (found, capped) = inventory(std::slice::from_ref(&source), &[], 2).unwrap();
        assert!(capped);
        assert!(found.len() <= 2);
        assert!(!found.is_empty());
    }
}
