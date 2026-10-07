//! Trusted content baselines for ignored protected evidence trees.
//!
//! The aggregate manifest is deliberately small and is trusted only when read
//! from `HEAD`. Tree content is opened without following links and large files
//! are streamed into domain-separated SHA-256 leaf and tree digests.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::ffi::{CString, OsStr};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Component, Path};
use std::process::Stdio;
use walkdir::WalkDir;

#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

use crate::fs_util::write_text_atomic;
use crate::paths::Paths;

pub(crate) const BASELINE_PATH: &str = ".codex-team-protected-trees.json";
const MANIFEST_SCHEMA: &str = "codex-team.protected_trees_baseline.v1";
const ALGORITHM: &str = "sha256-merkle-v2-ds-store-excluded";
const PREVIEW_SCHEMA: &str = "codex-team.protected_tree_baseline_preview.v1";
const APPLY_SCHEMA: &str = "codex-team.protected_tree_baseline_apply.v1";
const PROTECTED_ROOTS: [&str; 2] = ["oracles", "reference"];
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_ENTRIES: u64 = 100_000;
const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 8 * 1024;
const MAX_PATH_DEPTH: usize = 128;
const MAX_SYMLINK_BYTES: usize = 4 * 1024;
const STREAM_BUFFER_BYTES: usize = 128 * 1024;
const MAX_GIT_OUTPUT_BYTES: usize = 128 * 1024;
const EXCLUDED_FINDER_METADATA: &str = ".DS_Store";

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProtectedTreeBaselineV1 {
    schema: String,
    algorithm: String,
    approved_from_revision: String,
    roots: Vec<ProtectedTreeDigestV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProtectedTreeDigestV1 {
    path: String,
    sha256: String,
    entries: u64,
    regular_files: u64,
    directories: u64,
    symlinks: u64,
    bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProtectedTreeBaselinePreviewV1 {
    pub(crate) schema: String,
    pub(crate) repository_revision: String,
    pub(crate) manifest_digest: String,
    pub(crate) manifest: ProtectedTreeBaselineV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProtectedTreeBaselineApplyV1 {
    pub(crate) schema: String,
    pub(crate) status: String,
    pub(crate) repository_revision: String,
    pub(crate) manifest_digest: String,
    pub(crate) path: String,
}

#[derive(Debug, Default)]
struct ScanCounts {
    entries: u64,
    regular_files: u64,
    directories: u64,
    symlinks: u64,
    bytes: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct StatIdentity {
    device: u64,
    inode: u64,
    mode: u32,
    links: u64,
    size: u64,
    modified_seconds: i64,
    changed_seconds: i64,
}

pub(crate) fn preview(paths: &Paths) -> Result<ProtectedTreeBaselinePreviewV1> {
    let first = build_manifest(paths)?;
    let second = build_manifest(paths)?;
    if first != second {
        bail!("protected trees changed while the baseline preview was created");
    }
    let manifest_digest = manifest_digest(&first)?;
    Ok(ProtectedTreeBaselinePreviewV1 {
        schema: PREVIEW_SCHEMA.to_string(),
        repository_revision: first.approved_from_revision.clone(),
        manifest_digest,
        manifest: first,
    })
}

pub(crate) fn apply(
    paths: &Paths,
    expected_revision: &str,
    accept_digest: &str,
) -> Result<ProtectedTreeBaselineApplyV1> {
    validate_object_id(expected_revision)?;
    validate_sha256(accept_digest)?;
    let initial_revision = git_head(paths)?;
    if initial_revision != expected_revision {
        bail!("protected-tree baseline approval is stale");
    }
    ensure_manifest_worktree_safe(paths)?;

    let candidate = preview(paths)?;
    if candidate.repository_revision != expected_revision {
        bail!("protected-tree baseline approval is stale");
    }
    if candidate.manifest_digest != accept_digest {
        bail!("protected-tree baseline digest does not match the approved preview");
    }
    if git_head(paths)? != expected_revision {
        bail!("protected-tree baseline approval became stale before apply");
    }

    let target = paths.root.join(BASELINE_PATH);
    let previous = read_optional_regular_no_follow(&target, MAX_MANIFEST_BYTES)?;
    let mut serialized = serde_json::to_string_pretty(&candidate.manifest)?;
    serialized.push('\n');
    write_text_atomic(&target, &serialized)?;

    if git_head(paths)? != expected_revision {
        restore_manifest(&target, previous.as_deref())?;
        bail!("repository revision changed during protected-tree baseline apply");
    }

    Ok(ProtectedTreeBaselineApplyV1 {
        schema: APPLY_SCHEMA.to_string(),
        status: "applied".to_string(),
        repository_revision: expected_revision.to_string(),
        manifest_digest: accept_digest.to_string(),
        path: BASELINE_PATH.to_string(),
    })
}

/// Return bounded, root-level verification findings. Empty means the committed
/// manifest covers every ignored protected root and every recorded tree still
/// matches.
pub(crate) fn verify(
    paths: &Paths,
    present_roots: &[String],
    ignored_roots: &[String],
) -> Result<Vec<String>> {
    let committed = read_head_manifest(paths)?;
    let Some(bytes) = committed else {
        return Ok(ignored_roots
            .iter()
            .map(|root| format!("{root}/ (committed content baseline is missing)"))
            .collect());
    };
    let manifest = match parse_manifest(&bytes) {
        Ok(manifest) => manifest,
        Err(_) => {
            return Ok(vec![format!(
                "{BASELINE_PATH} (committed content baseline is invalid)"
            )]);
        }
    };

    let present = present_roots
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let recorded = manifest
        .roots
        .iter()
        .map(|root| root.path.as_str())
        .collect::<BTreeSet<_>>();
    let mut findings = Vec::new();
    for root in ignored_roots {
        if !recorded.contains(root.as_str()) {
            findings.push(format!("{root}/ (ignored protected root is not baselined)"));
        }
    }
    for baseline in &manifest.roots {
        if !present.contains(baseline.path.as_str()) {
            findings.push(format!(
                "{}/ (baselined protected root is missing)",
                baseline.path
            ));
            continue;
        }
        match scan_tree(&paths.root, &baseline.path) {
            Ok(current) if current == *baseline => {}
            Ok(_) => findings.push(format!(
                "{}/ (committed content baseline does not match)",
                baseline.path
            )),
            Err(_) => findings.push(format!("{}/ (content scan failed)", baseline.path)),
        }
    }
    findings.sort();
    findings.dedup();
    Ok(findings)
}

fn build_manifest(paths: &Paths) -> Result<ProtectedTreeBaselineV1> {
    let approved_from_revision = git_head(paths)?;
    let mut roots = Vec::new();
    for root in PROTECTED_ROOTS {
        match fs::symlink_metadata(paths.root.join(root)) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                roots.push(scan_tree(&paths.root, root)?);
            }
            Ok(_) => bail!("protected root {root} is not a real directory"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => bail!("protected root {root} could not be inspected"),
        }
    }
    if roots.is_empty() {
        bail!("repository has no protected trees to baseline");
    }
    roots.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(ProtectedTreeBaselineV1 {
        schema: MANIFEST_SCHEMA.to_string(),
        algorithm: ALGORITHM.to_string(),
        approved_from_revision,
        roots,
    })
}

fn scan_tree(repo: &Path, logical_root: &str) -> Result<ProtectedTreeDigestV1> {
    #[cfg(not(unix))]
    {
        let _ = (repo, logical_root);
        bail!("protected-tree content baselines require a Unix host");
    }
    #[cfg(unix)]
    {
        scan_tree_unix(repo, logical_root)
    }
}

#[cfg(unix)]
fn scan_tree_unix(repo: &Path, logical_root: &str) -> Result<ProtectedTreeDigestV1> {
    if !PROTECTED_ROOTS.contains(&logical_root) {
        bail!("protected-tree baseline root is not allowed");
    }
    let root_path = repo.join(logical_root);
    let root_file = open_directory_no_follow(&root_path)?;
    let root_metadata = root_file
        .metadata()
        .context("inspect protected-tree root descriptor")?;
    if !root_metadata.is_dir() {
        bail!("protected-tree root is not a directory");
    }
    let root_device = root_metadata.dev();

    let mut tree_hasher = Sha256::new();
    hash_field(&mut tree_hasher, b"codex-team.protected-tree.tree.v1");
    hash_field(&mut tree_hasher, logical_root.as_bytes());
    let mut counts = ScanCounts::default();
    let walker = WalkDir::new(&root_path)
        .min_depth(1)
        .follow_links(false)
        .sort_by(|left, right| {
            left.file_name()
                .as_bytes()
                .cmp(right.file_name().as_bytes())
        })
        .into_iter()
        // Finder rewrites these host metadata files without changing oracle
        // evidence. Exclude the exact component before counts or hashes; all
        // similarly named entries remain protected.
        .filter_entry(|entry| entry.file_name() != OsStr::new(EXCLUDED_FINDER_METADATA));
    for entry in walker {
        let entry = entry.map_err(|_| anyhow!("protected-tree traversal failed"))?;
        let relative = entry
            .path()
            .strip_prefix(&root_path)
            .map_err(|_| anyhow!("protected-tree traversal escaped its root"))?;
        validate_relative_path(relative)?;
        counts.entries = counts
            .entries
            .checked_add(1)
            .ok_or_else(|| anyhow!("protected-tree entry count overflow"))?;
        if counts.entries > MAX_ENTRIES {
            bail!("protected-tree entry limit exceeded");
        }
        let leaf = hash_entry(&root_file, root_device, relative, &mut counts)?;
        tree_hasher.update(leaf);
    }

    Ok(ProtectedTreeDigestV1 {
        path: logical_root.to_string(),
        sha256: format!("{:x}", tree_hasher.finalize()),
        entries: counts.entries,
        regular_files: counts.regular_files,
        directories: counts.directories,
        symlinks: counts.symlinks,
        bytes: counts.bytes,
    })
}

#[cfg(unix)]
fn hash_entry(
    root: &File,
    root_device: u64,
    relative: &Path,
    counts: &mut ScanCounts,
) -> Result<[u8; 32]> {
    let (parent, name) = open_parent(root, root_device, relative)?;
    let before = stat_at(parent.as_raw_fd(), &name)?;
    if before.device != root_device {
        bail!("protected-tree mount crossing is not allowed");
    }
    let path_bytes = relative.as_os_str().as_bytes();
    let mut leaf = Sha256::new();
    hash_field(&mut leaf, b"codex-team.protected-tree.leaf.v1");
    hash_field(&mut leaf, path_bytes);

    match before.mode & u32::from(libc::S_IFMT) {
        value if value == u32::from(libc::S_IFDIR) => {
            let directory = open_at(
                parent.as_raw_fd(),
                &name,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )?;
            let opened = directory.metadata()?;
            ensure_opened_identity(&before, &opened, root_device)?;
            hash_field(&mut leaf, b"directory");
            counts.directories = checked_increment(counts.directories)?;
        }
        value if value == u32::from(libc::S_IFREG) => {
            if before.links != 1 {
                bail!("protected-tree hardlinked regular files are not allowed");
            }
            if before.size > MAX_FILE_BYTES {
                bail!("protected-tree file size limit exceeded");
            }
            counts.bytes = counts
                .bytes
                .checked_add(before.size)
                .ok_or_else(|| anyhow!("protected-tree byte count overflow"))?;
            if counts.bytes > MAX_TOTAL_BYTES {
                bail!("protected-tree aggregate byte limit exceeded");
            }
            let mut file = open_at(
                parent.as_raw_fd(),
                &name,
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )?;
            let opened_before = file.metadata()?;
            ensure_opened_identity(&before, &opened_before, root_device)?;
            let content_digest = stream_digest(&mut file, before.size)?;
            let opened_after = file.metadata()?;
            if !same_metadata(&opened_before, &opened_after) {
                bail!("protected-tree file changed while it was hashed");
            }
            let path_after = stat_at(parent.as_raw_fd(), &name)?;
            if before != path_after {
                bail!("protected-tree file changed while it was hashed");
            }
            hash_field(&mut leaf, b"regular");
            hash_field(&mut leaf, &[u8::from(before.mode & 0o111 != 0)]);
            hash_field(&mut leaf, &before.size.to_be_bytes());
            hash_field(&mut leaf, &content_digest);
            counts.regular_files = checked_increment(counts.regular_files)?;
        }
        value if value == u32::from(libc::S_IFLNK) => {
            let target = read_link_at(parent.as_raw_fd(), &name)?;
            let after = stat_at(parent.as_raw_fd(), &name)?;
            if before != after {
                bail!("protected-tree symlink changed while it was hashed");
            }
            hash_field(&mut leaf, b"symlink");
            hash_field(&mut leaf, &target);
            counts.symlinks = checked_increment(counts.symlinks)?;
        }
        _ => bail!("protected-tree special files are not allowed"),
    }
    Ok(leaf.finalize().into())
}

#[cfg(unix)]
fn open_directory_no_follow(path: &Path) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .context("open protected-tree directory without following links")
}

#[cfg(unix)]
fn open_parent(root: &File, root_device: u64, relative: &Path) -> Result<(File, CString)> {
    let mut directory = root.try_clone()?;
    let components = relative.components().collect::<Vec<_>>();
    let (final_component, ancestors) = components
        .split_last()
        .ok_or_else(|| anyhow!("protected-tree entry path is empty"))?;
    for component in ancestors {
        let Component::Normal(name) = component else {
            bail!("protected-tree path contains an unsafe component");
        };
        let name = cstring(name.as_bytes())?;
        let opened = open_at(
            directory.as_raw_fd(),
            &name,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )?;
        let metadata = opened.metadata()?;
        if !metadata.is_dir() || metadata.dev() != root_device {
            bail!("protected-tree ancestor is unsafe");
        }
        directory = opened;
    }
    let Component::Normal(name) = final_component else {
        bail!("protected-tree path contains an unsafe final component");
    };
    Ok((directory, cstring(name.as_bytes())?))
}

#[cfg(unix)]
fn open_at(directory: i32, name: &CString, flags: i32) -> Result<File> {
    let descriptor = unsafe { libc::openat(directory, name.as_ptr(), flags) };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error())
            .context("open protected-tree entry without following links");
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(unix)]
fn stat_at(directory: i32, name: &CString) -> Result<StatIdentity> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            directory,
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).context("inspect protected-tree entry");
    }
    let stat = unsafe { stat.assume_init() };
    if stat.st_size < 0 {
        bail!("protected-tree entry has an invalid size");
    }
    Ok(StatIdentity {
        device: stat.st_dev as u64,
        inode: stat.st_ino,
        mode: stat.st_mode as u32,
        links: stat.st_nlink as u64,
        size: stat.st_size as u64,
        modified_seconds: stat.st_mtime,
        changed_seconds: stat.st_ctime,
    })
}

#[cfg(unix)]
fn ensure_opened_identity(
    before: &StatIdentity,
    opened: &fs::Metadata,
    root_device: u64,
) -> Result<()> {
    if opened.dev() != root_device
        || opened.dev() != before.device
        || opened.ino() != before.inode
        || opened.mode() != before.mode
        || opened.nlink() != before.links
        || opened.len() != before.size
        || opened.mtime() != before.modified_seconds
        || opened.ctime() != before.changed_seconds
    {
        bail!("protected-tree entry changed before it was opened");
    }
    Ok(())
}

#[cfg(unix)]
fn same_metadata(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.mode() == right.mode()
        && left.nlink() == right.nlink()
        && left.len() == right.len()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
}

#[cfg(unix)]
fn stream_digest(file: &mut File, expected: u64) -> Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = vec![0u8; STREAM_BUFFER_BYTES];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| anyhow!("protected-tree file size overflow"))?;
        if total > expected || total > MAX_FILE_BYTES {
            bail!("protected-tree file changed size while it was hashed");
        }
        hasher.update(&buffer[..count]);
    }
    if total != expected {
        bail!("protected-tree file changed size while it was hashed");
    }
    Ok(hasher.finalize().into())
}

#[cfg(unix)]
fn read_link_at(directory: i32, name: &CString) -> Result<Vec<u8>> {
    let mut target = vec![0u8; MAX_SYMLINK_BYTES + 1];
    let count = unsafe {
        libc::readlinkat(
            directory,
            name.as_ptr(),
            target.as_mut_ptr().cast(),
            target.len(),
        )
    };
    if count < 0 {
        return Err(std::io::Error::last_os_error()).context("read protected-tree symlink");
    }
    let count = count as usize;
    if count > MAX_SYMLINK_BYTES {
        bail!("protected-tree symlink target exceeds its limit");
    }
    target.truncate(count);
    Ok(target)
}

#[cfg(unix)]
fn cstring(bytes: &[u8]) -> Result<CString> {
    CString::new(bytes).map_err(|_| anyhow!("protected-tree path contains a NUL byte"))
}

fn validate_relative_path(path: &Path) -> Result<()> {
    #[cfg(unix)]
    if path.as_os_str().as_bytes().len() > MAX_PATH_BYTES {
        bail!("protected-tree path exceeds its byte limit");
    }
    #[cfg(not(unix))]
    if path.as_os_str().to_string_lossy().len() > MAX_PATH_BYTES {
        bail!("protected-tree path exceeds its byte limit");
    }
    let mut depth = 0usize;
    for component in path.components() {
        match component {
            Component::Normal(_) => depth += 1,
            _ => bail!("protected-tree path escapes its root"),
        }
    }
    if depth == 0 || depth > MAX_PATH_DEPTH {
        bail!("protected-tree path depth is invalid");
    }
    Ok(())
}

fn checked_increment(value: u64) -> Result<u64> {
    value
        .checked_add(1)
        .ok_or_else(|| anyhow!("protected-tree count overflow"))
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn manifest_digest(manifest: &ProtectedTreeBaselineV1) -> Result<String> {
    let canonical = serde_json::to_vec(manifest)?;
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, b"codex-team.protected-tree.manifest-digest.v1");
    hash_field(&mut hasher, &canonical);
    Ok(format!("{:x}", hasher.finalize()))
}

fn parse_manifest(bytes: &[u8]) -> Result<ProtectedTreeBaselineV1> {
    if bytes.is_empty() || bytes.len() > MAX_MANIFEST_BYTES {
        bail!("protected-tree baseline manifest size is invalid");
    }
    let manifest: ProtectedTreeBaselineV1 = serde_json::from_slice(bytes)?;
    if manifest.schema != MANIFEST_SCHEMA || manifest.algorithm != ALGORITHM {
        bail!("protected-tree baseline manifest version is unsupported");
    }
    validate_object_id(&manifest.approved_from_revision)?;
    if manifest.roots.is_empty() || manifest.roots.len() > PROTECTED_ROOTS.len() {
        bail!("protected-tree baseline root count is invalid");
    }
    let mut previous = None;
    for root in &manifest.roots {
        if !PROTECTED_ROOTS.contains(&root.path.as_str()) {
            bail!("protected-tree baseline contains an unsupported root");
        }
        if previous.is_some_and(|value: &str| value >= root.path.as_str()) {
            bail!("protected-tree baseline roots are not uniquely sorted");
        }
        previous = Some(root.path.as_str());
        validate_sha256(&root.sha256)?;
        let typed = root
            .regular_files
            .checked_add(root.directories)
            .and_then(|value| value.checked_add(root.symlinks))
            .ok_or_else(|| anyhow!("protected-tree baseline count overflow"))?;
        if root.entries != typed || root.entries > MAX_ENTRIES || root.bytes > MAX_TOTAL_BYTES {
            bail!("protected-tree baseline counts are invalid");
        }
    }
    Ok(manifest)
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("expected a lowercase SHA-256 digest");
    }
    Ok(())
}

fn validate_object_id(value: &str) -> Result<()> {
    if !matches!(value.len(), 40 | 64)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("expected a lowercase full Git object ID");
    }
    Ok(())
}

fn git_head(paths: &Paths) -> Result<String> {
    let output = git_output(paths, &["rev-parse", "--verify", "HEAD"])?;
    let revision = std::str::from_utf8(&output)?.trim().to_ascii_lowercase();
    validate_object_id(&revision)?;
    Ok(revision)
}

fn read_head_manifest(paths: &Paths) -> Result<Option<Vec<u8>>> {
    let output = crate::git_metadata::command_no_filters(&paths.root)?
        .args(["show", &format!("HEAD:{BASELINE_PATH}")])
        .current_dir(&paths.root)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .context("read committed protected-tree baseline")?;
    if !output.status.success() {
        return Ok(None);
    }
    if output.stdout.len() > MAX_MANIFEST_BYTES {
        bail!("committed protected-tree baseline exceeds its size limit");
    }
    Ok(Some(output.stdout))
}

fn git_output(paths: &Paths, args: &[&str]) -> Result<Vec<u8>> {
    let output = crate::git_metadata::query(&paths.root, args)?
        .current_dir(&paths.root)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .context("run protected-tree Git inspection")?;
    if !output.status.success() {
        bail!("protected-tree Git inspection failed");
    }
    if output.stdout.len() > MAX_GIT_OUTPUT_BYTES {
        bail!("protected-tree Git output exceeded its limit");
    }
    Ok(output.stdout)
}

fn ensure_manifest_worktree_safe(paths: &Paths) -> Result<()> {
    let target = paths.root.join(BASELINE_PATH);
    let committed = read_head_manifest(paths)?;
    let working = read_optional_regular_no_follow(&target, MAX_MANIFEST_BYTES)?;
    match (committed.as_deref(), working.as_deref()) {
        (None, None) => {}
        (Some(expected), Some(actual)) if expected == actual => {}
        (None, Some(_)) => bail!("untracked protected-tree baseline already exists"),
        (Some(_), None) => bail!("committed protected-tree baseline is missing from the worktree"),
        (Some(_), Some(_)) => bail!("protected-tree baseline has uncommitted changes"),
    }
    let status = git_output(
        paths,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            BASELINE_PATH,
        ],
    )?;
    if !status.is_empty() {
        bail!("protected-tree baseline path is not clean before apply");
    }
    Ok(())
}

fn read_optional_regular_no_follow(path: &Path, max_bytes: usize) -> Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("inspect protected-tree baseline path"),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("protected-tree baseline path is not a regular file");
    }
    if metadata.len() > max_bytes as u64 {
        bail!("protected-tree baseline file exceeds its size limit");
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options.open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() != metadata.len() {
        bail!("protected-tree baseline changed while it was opened");
    }
    let mut bytes = Vec::with_capacity(opened.len() as usize);
    std::io::Read::by_ref(&mut file)
        .take((max_bytes + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        bail!("protected-tree baseline exceeded its size limit while reading");
    }
    Ok(Some(bytes))
}

fn restore_manifest(path: &Path, previous: Option<&[u8]>) -> Result<()> {
    match previous {
        Some(bytes) => {
            let text = std::str::from_utf8(bytes)
                .context("previous protected-tree baseline was not UTF-8")?;
            write_text_atomic(path, text)
        }
        None => match fs::remove_file(path) {
            Ok(()) => sync_parent(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("remove stale protected-tree baseline"),
        },
    }
}

fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(
            path.parent()
                .ok_or_else(|| anyhow!("protected-tree baseline has no parent"))?,
        )?
        .sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture(name: &str) -> Result<(std::path::PathBuf, Paths)> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("codex-team-protected-{name}-{nonce}"));
        fs::create_dir_all(root.join("oracles/empty"))?;
        fs::create_dir_all(root.join("reference"))?;
        fs::write(root.join("oracles/data.bin"), [0_u8, 1, 2, 255])?;
        fs::write(root.join("reference/note.txt"), "evidence\n")?;
        fs::write(root.join(".gitignore"), "/oracles/\n/reference/\n")?;
        run_git(&root, &["init", "-q"])?;
        run_git(&root, &["config", "user.name", "Codex Team Test"])?;
        run_git(
            &root,
            &["config", "user.email", "codex-team@example.invalid"],
        )?;
        run_git(&root, &["add", ".gitignore"])?;
        run_git(&root, &["commit", "-q", "-m", "fixture"])?;
        let paths = Paths::new(&root)?;
        Ok((root, paths))
    }

    fn run_git(root: &Path, args: &[&str]) -> Result<()> {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            bail!("fixture Git command failed");
        }
        Ok(())
    }

    fn commit_baseline(root: &Path, paths: &Paths) -> Result<ProtectedTreeBaselinePreviewV1> {
        let preview = preview(paths)?;
        apply(
            paths,
            &preview.repository_revision,
            &preview.manifest_digest,
        )?;
        run_git(root, &["add", BASELINE_PATH])?;
        run_git(root, &["commit", "-q", "-m", "baseline"])?;
        Ok(preview)
    }

    #[test]
    fn preview_is_stable_and_apply_is_revision_and_digest_bound() -> Result<()> {
        let (root, paths) = fixture("preview")?;
        let preview = preview(&paths)?;
        assert_eq!(preview.schema, PREVIEW_SCHEMA);
        assert_eq!(preview.manifest.roots.len(), 2);
        assert!(apply(&paths, &preview.repository_revision, &"0".repeat(64)).is_err());
        assert!(!root.join(BASELINE_PATH).exists());
        let applied = apply(
            &paths,
            &preview.repository_revision,
            &preview.manifest_digest,
        )?;
        assert_eq!(applied.status, "applied");
        assert!(root.join(BASELINE_PATH).is_file());
        assert!(apply(
            &paths,
            &preview.repository_revision,
            &preview.manifest_digest
        )
        .is_err());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn committed_baseline_detects_content_shape_mode_and_link_drift() -> Result<()> {
        let (root, paths) = fixture("drift")?;
        commit_baseline(&root, &paths)?;
        let present = vec!["oracles".to_string(), "reference".to_string()];
        assert!(verify(&paths, &present, &present)?.is_empty());

        fs::write(root.join("reference/note.txt"), "changed\n")?;
        assert!(verify(&paths, &present, &present)?
            .iter()
            .any(|finding| finding.starts_with("reference/")));
        fs::write(root.join("reference/note.txt"), "evidence\n")?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::{symlink, PermissionsExt};
            let path = root.join("reference/note.txt");
            let mut permissions = fs::metadata(&path)?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions)?;
            assert!(!verify(&paths, &present, &present)?.is_empty());
            let mut permissions = fs::metadata(&path)?.permissions();
            permissions.set_mode(0o644);
            fs::set_permissions(&path, permissions)?;
            symlink("note.txt", root.join("reference/link"))?;
            assert!(!verify(&paths, &present, &present)?.is_empty());
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn finder_metadata_is_absent_from_baselines_and_cannot_create_drift() -> Result<()> {
        let (root, paths) = fixture("finder-metadata")?;
        fs::write(root.join("oracles/.DS_Store"), b"initial root metadata")?;
        fs::write(
            root.join("oracles/empty/.DS_Store"),
            b"initial nested metadata",
        )?;
        fs::write(root.join("reference/.DS_Store"), b"reference metadata")?;
        fs::write(root.join("reference/.DS_Store.keep"), b"protected")?;

        let preview = commit_baseline(&root, &paths)?;
        assert_eq!(preview.manifest.algorithm, ALGORITHM);
        let oracles = preview
            .manifest
            .roots
            .iter()
            .find(|entry| entry.path == "oracles")
            .unwrap();
        let reference = preview
            .manifest
            .roots
            .iter()
            .find(|entry| entry.path == "reference")
            .unwrap();
        assert_eq!(oracles.entries, 2);
        assert_eq!(reference.entries, 2);

        fs::write(root.join("oracles/.DS_Store"), b"changed root metadata")?;
        fs::remove_file(root.join("oracles/empty/.DS_Store"))?;
        fs::write(
            root.join("reference/.DS_Store"),
            b"changed reference metadata",
        )?;
        let present = vec!["oracles".to_string(), "reference".to_string()];
        assert!(verify(&paths, &present, &present)?.is_empty());

        fs::write(root.join("reference/.DS_Store.keep"), b"drift")?;
        assert!(verify(&paths, &present, &present)?
            .iter()
            .any(|finding| finding.starts_with("reference/")));
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn scanner_hashes_symlinks_without_following_and_rejects_hardlinks_and_special_files(
    ) -> Result<()> {
        use std::os::unix::fs::symlink;
        let (root, paths) = fixture("entry-types")?;
        let outside = root.join("outside.txt");
        fs::write(&outside, "outside")?;
        symlink(&outside, root.join("oracles/link"))?;
        assert_eq!(preview(&paths)?.manifest.roots.len(), 2);

        fs::hard_link(&outside, root.join("oracles/hard"))?;
        assert!(preview(&paths).is_err());
        fs::remove_file(root.join("oracles/hard"))?;

        let fifo = root.join("oracles/fifo");
        let fifo_c = CString::new(fifo.as_os_str().as_bytes())?;
        let result = unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) };
        if result != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        assert!(preview(&paths).is_err());
        assert_eq!(fs::read_to_string(outside)?, "outside");
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn uncommitted_or_invalid_manifest_never_grants_trust() -> Result<()> {
        let (root, paths) = fixture("uncommitted")?;
        let present = vec!["oracles".to_string(), "reference".to_string()];
        assert!(verify(&paths, &present, &present)?
            .iter()
            .all(|finding| finding.contains("baseline is missing")));

        let preview = preview(&paths)?;
        apply(
            &paths,
            &preview.repository_revision,
            &preview.manifest_digest,
        )?;
        assert!(verify(&paths, &present, &present)?
            .iter()
            .all(|finding| finding.contains("baseline is missing")));
        run_git(&root, &["add", BASELINE_PATH])?;
        run_git(&root, &["commit", "-q", "-m", "baseline"])?;
        fs::write(root.join(BASELINE_PATH), "{}\n")?;
        assert!(verify(&paths, &present, &present)?.is_empty());
        assert!(
            !crate::verification::ca::run_builtin_check(
                &paths,
                crate::verification::ca::BuiltinCheck::ProtectedTrees,
            )?
            .passed
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn manifest_parser_rejects_unknown_fields_counts_and_roots() -> Result<()> {
        let (root, paths) = fixture("manifest")?;
        let preview = preview(&paths)?;
        let mut value = serde_json::to_value(&preview.manifest)?;
        value["unknown"] = serde_json::json!(true);
        assert!(parse_manifest(&serde_json::to_vec(&value)?).is_err());

        let mut value = serde_json::to_value(&preview.manifest)?;
        value["roots"][0]["entries"] = serde_json::json!(999_999);
        assert!(parse_manifest(&serde_json::to_vec(&value)?).is_err());

        let mut value = serde_json::to_value(&preview.manifest)?;
        value["roots"][0]["path"] = serde_json::json!("private");
        assert!(parse_manifest(&serde_json::to_vec(&value)?).is_err());
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
