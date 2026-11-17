//! Host admission for child tools and processes.
//!
//! An [`AdmittedExecutionGrant`] is created by the host at spawn. Model
//! arguments, restored sessions, and worker messages do not create one.
//! Release 1 enforces path, command, and tool limits inside the host process.
//! On macOS, an absent `child_execution` resolves to Isolated. Linux and
//! FreeBSD stay in-process. An explicit Isolated request fails closed off macOS.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

pub const GRANT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChildExecutionMode {
    InProcess,
    Isolated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedChildExecution {
    InProcess,
    Isolated,
}

/// Absent stays on the existing in-process path until this platform has a
/// recorded native qualification. Explicit isolated fails closed before that.
pub fn resolve_child_execution(
    configured: Option<ChildExecutionMode>,
) -> Result<ResolvedChildExecution, &'static str> {
    // The macOS release artifact passed worker confinement. Other platforms
    // stay in-process even when this flag is set.
    resolve_child_execution_qualified(configured, true)
}

/// `qualified` is true only after the macOS release artifact has passed the
/// worker confinement test. Linux and FreeBSD never take the new default.
pub fn resolve_child_execution_qualified(
    configured: Option<ChildExecutionMode>,
    qualified: bool,
) -> Result<ResolvedChildExecution, &'static str> {
    match configured {
        Some(ChildExecutionMode::InProcess) => Ok(ResolvedChildExecution::InProcess),
        None => {
            if qualified && cfg!(target_os = "macos") {
                Ok(ResolvedChildExecution::Isolated)
            } else {
                Ok(ResolvedChildExecution::InProcess)
            }
        }
        Some(ChildExecutionMode::Isolated) => {
            if cfg!(target_os = "macos") {
                Ok(ResolvedChildExecution::Isolated)
            } else {
                Err("isolated child execution is unavailable on this platform")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    ExactFile,
    DirectoryTree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathAccess {
    Read,
    /// Native tool mutation of an admitted exact file or a file inside a
    /// writable directory or output directory.
    Write,
    /// Shell and formatter commands may read an exact file and must not write it.
    ShellWrite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathScope {
    pub path: PathBuf,
    pub kind: ScopeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedExecutionGrant {
    pub version: u32,
    pub id: Uuid,
    pub owner_run: Uuid,
    pub parent_lineage: Vec<Uuid>,
    pub policy_generation: u64,
    pub digest: String,
    pub tools: BTreeSet<String>,
    pub commands: BTreeSet<String>,
    pub read_scopes: Vec<PathScope>,
    pub write_scopes: Vec<PathScope>,
    pub output_directories: Vec<PathBuf>,
    pub runtime_read_scopes: Vec<PathBuf>,
    pub destinations: BTreeSet<String>,
    pub credential_operations: BTreeSet<String>,
    pub protected_host_paths: Vec<PathBuf>,
    pub execution_mode: ResolvedChildExecution,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GrantError {
    #[error("path is outside the admitted grant")]
    OutsideScope,
    #[error("path scope is relative or contains a parent component")]
    UnstablePath,
    #[error("symlink scope cannot be admitted")]
    SymlinkScope,
    #[error("scope cannot be rebased into the worktree")]
    NotRebasable,
    #[error("execution grant generation is stale")]
    StaleGeneration,
    #[error("requested tool is outside the parent grant")]
    ToolWidening,
    #[error("requested command is outside the parent grant")]
    CommandWidening,
    #[error("requested path is outside the parent grant")]
    PathWidening,
    #[error("destination and credential references are not admitted")]
    ReferenceWidening,
    #[error("admitted path is unavailable")]
    Unavailable,
}

pub struct LegacyGrantView<'a> {
    pub tools: &'a [String],
    pub path_scopes: &'a [String],
    pub command_scopes: &'a [String],
}

impl AdmittedExecutionGrant {
    pub fn from_legacy(
        view: LegacyGrantView<'_>,
        owner_run: Uuid,
        policy_generation: u64,
        parent_lineage: Vec<Uuid>,
    ) -> Result<Self, GrantError> {
        let mut read_scopes = Vec::new();
        let mut write_scopes = Vec::new();
        for raw in view.path_scopes {
            let scope = classify_scope(Path::new(raw))?;
            read_scopes.push(scope.clone());
            write_scopes.push(scope);
        }
        let mut grant = Self {
            version: GRANT_VERSION,
            id: Uuid::new_v4(),
            owner_run,
            parent_lineage,
            policy_generation,
            digest: String::new(),
            tools: view.tools.iter().cloned().collect(),
            commands: view.command_scopes.iter().cloned().collect(),
            read_scopes,
            write_scopes,
            output_directories: Vec::new(),
            runtime_read_scopes: Vec::new(),
            destinations: BTreeSet::new(),
            credential_operations: BTreeSet::new(),
            protected_host_paths: Vec::new(),
            execution_mode: ResolvedChildExecution::InProcess,
        };
        grant.digest = grant.compute_digest();
        Ok(grant)
    }

    pub fn ensure_current(&self, live_generation: u64) -> Result<(), GrantError> {
        if self.policy_generation != live_generation {
            Err(GrantError::StaleGeneration)
        } else {
            Ok(())
        }
    }

    pub fn authorize_tool(&self, tool: &str) -> Result<(), GrantError> {
        if self.tools.contains(tool) {
            Ok(())
        } else {
            Err(GrantError::ToolWidening)
        }
    }

    pub fn authorize_command(&self, command: &str) -> Result<(), GrantError> {
        if self.commands.contains(command) {
            Ok(())
        } else {
            Err(GrantError::CommandWidening)
        }
    }

    /// Release 1 has no destination broker. Any child network request is denied.
    pub fn authorize_network(&self) -> Result<(), GrantError> {
        if self.destinations.is_empty() {
            Err(GrantError::ReferenceWidening)
        } else {
            Ok(())
        }
    }

    pub fn authorize_path(&self, candidate: &Path, access: PathAccess) -> Result<(), GrantError> {
        let candidate = absolute_lexical(candidate)?;
        let scopes: &[PathScope] = match access {
            PathAccess::Read => &self.read_scopes,
            PathAccess::Write | PathAccess::ShellWrite => &self.write_scopes,
        };
        if let Some(scope) = scopes
            .iter()
            .find(|scope| lexical_cover(scope, &candidate, access))
        {
            return anchor_within(scope, &candidate, access);
        }
        if matches!(access, PathAccess::Write | PathAccess::ShellWrite) {
            let parent = candidate.parent().ok_or(GrantError::OutsideScope)?;
            let parent = canonical_existing(parent)?;
            if self
                .output_directories
                .iter()
                .any(|dir| parent == *dir || parent.starts_with(dir))
                && !hostile_symlink(&parent, &candidate)?
            {
                return Ok(());
            }
        }
        Err(GrantError::OutsideScope)
    }

    pub fn intersect_narrowing(
        &self,
        tools: Option<&[String]>,
        commands: Option<&[String]>,
        read_paths: Option<&[String]>,
        output_directories: Option<&[String]>,
        destinations: Option<&[String]>,
        credential_operations: Option<&[String]>,
    ) -> Result<Self, GrantError> {
        if destinations.is_some_and(|items| !items.is_empty())
            || credential_operations.is_some_and(|items| !items.is_empty())
        {
            return Err(GrantError::ReferenceWidening);
        }
        let tools = match tools {
            None => self.tools.clone(),
            Some(requested) => {
                if requested.iter().any(|tool| !self.tools.contains(tool)) {
                    return Err(GrantError::ToolWidening);
                }
                requested.iter().cloned().collect()
            }
        };
        let commands = match commands {
            None => self.commands.clone(),
            Some(requested) => {
                if requested
                    .iter()
                    .any(|command| !self.commands.contains(command))
                {
                    return Err(GrantError::CommandWidening);
                }
                requested.iter().cloned().collect()
            }
        };
        let read_scopes = match read_paths {
            None => self.read_scopes.clone(),
            Some(paths) => narrow_paths(&self.read_scopes, paths)?,
        };
        let write_scopes = match read_paths {
            None => self.write_scopes.clone(),
            Some(paths) => narrow_paths(&self.write_scopes, paths)?,
        };
        let output_directories = match output_directories {
            None => self.output_directories.clone(),
            Some(paths) => {
                let mut admitted = Vec::new();
                for raw in paths {
                    let path = canonical_existing(Path::new(raw))?;
                    let covered = self
                        .output_directories
                        .iter()
                        .any(|dir| path.starts_with(dir))
                        || self.write_scopes.iter().any(|scope| {
                            scope.kind == ScopeKind::DirectoryTree && path.starts_with(&scope.path)
                        });
                    if !covered {
                        return Err(GrantError::PathWidening);
                    }
                    admitted.push(path);
                }
                admitted
            }
        };
        let mut grant = Self {
            version: self.version,
            id: Uuid::new_v4(),
            owner_run: self.owner_run,
            parent_lineage: {
                let mut lineage = self.parent_lineage.clone();
                lineage.push(self.id);
                lineage
            },
            policy_generation: self.policy_generation,
            digest: String::new(),
            tools,
            commands,
            read_scopes,
            write_scopes,
            output_directories,
            runtime_read_scopes: self.runtime_read_scopes.clone(),
            destinations: BTreeSet::new(),
            credential_operations: BTreeSet::new(),
            protected_host_paths: self.protected_host_paths.clone(),
            execution_mode: self.execution_mode,
        };
        grant.digest = grant.compute_digest();
        Ok(grant)
    }

    fn compute_digest(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(GRANT_VERSION.to_le_bytes());
        hasher.update(self.id.as_bytes());
        hasher.update(self.policy_generation.to_le_bytes());
        for tool in &self.tools {
            hasher.update(tool.as_bytes());
            hasher.update([0]);
        }
        for command in &self.commands {
            hasher.update(command.as_bytes());
            hasher.update([0]);
        }
        for scope in self.read_scopes.iter().chain(&self.write_scopes) {
            hasher.update(scope.path.to_string_lossy().as_bytes());
            hasher.update([scope.kind as u8]);
        }
        for destination in &self.destinations {
            hasher.update(destination.as_bytes());
            hasher.update([0]);
        }
        format!("{:x}", hasher.finalize())
    }

    /// Host admission only. Model arguments cannot add a destination.
    pub fn host_bind_destination(&mut self, reference: &str) {
        self.destinations.insert(reference.to_string());
        self.digest = self.compute_digest();
    }
}

pub fn rebase_scope_strings(
    scopes: &[String],
    source_root: &Path,
    worktree_root: &Path,
) -> Result<Vec<String>, GrantError> {
    let source = canonical_existing(source_root)?;
    let worktree = canonical_existing(worktree_root)?;
    let mut rebased = Vec::with_capacity(scopes.len());
    for raw in scopes {
        let scope = classify_scope(Path::new(raw))?;
        let mapped = if scope.path == source {
            worktree.clone()
        } else if scope.path.starts_with(&source) {
            let relative = scope
                .path
                .strip_prefix(&source)
                .map_err(|_| GrantError::NotRebasable)?;
            worktree.join(relative)
        } else {
            return Err(GrantError::NotRebasable);
        };
        rebased.push(mapped.to_string_lossy().into_owned());
    }
    Ok(rebased)
}

fn narrow_paths(parent: &[PathScope], requested: &[String]) -> Result<Vec<PathScope>, GrantError> {
    let mut scopes = Vec::new();
    for raw in requested {
        let scope = classify_scope(Path::new(raw))?;
        let covered = parent.iter().any(|parent| path_within(parent, &scope.path));
        if !covered {
            return Err(GrantError::PathWidening);
        }
        scopes.push(scope);
    }
    Ok(scopes)
}

fn path_within(parent: &PathScope, candidate: &Path) -> bool {
    match parent.kind {
        ScopeKind::ExactFile => candidate == parent.path,
        ScopeKind::DirectoryTree => candidate == parent.path || candidate.starts_with(&parent.path),
    }
}

fn classify_scope(path: &Path) -> Result<PathScope, GrantError> {
    let path = absolute_lexical(path)?;
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let canonical = std::fs::canonicalize(&path).map_err(|_| GrantError::SymlinkScope)?;
            let target =
                std::fs::symlink_metadata(&canonical).map_err(|_| GrantError::SymlinkScope)?;
            if !target.is_dir() {
                return Err(GrantError::SymlinkScope);
            }
            Ok(PathScope {
                path: canonical,
                kind: ScopeKind::DirectoryTree,
            })
        }
        Ok(metadata) if metadata.is_dir() => Ok(PathScope {
            path: canonical_existing(&path)?,
            kind: ScopeKind::DirectoryTree,
        }),
        Ok(_) => Ok(PathScope {
            path: canonical_existing(&path).unwrap_or(path),
            kind: ScopeKind::ExactFile,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(PathScope {
            path,
            kind: ScopeKind::ExactFile,
        }),
        Err(_) => Err(GrantError::Unavailable),
    }
}

fn absolute_lexical(path: &Path) -> Result<PathBuf, GrantError> {
    if !path.is_absolute() {
        return Err(GrantError::UnstablePath);
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => clean.push(component),
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) => return Err(GrantError::UnstablePath),
        }
    }
    Ok(clean)
}

fn canonical_existing(path: &Path) -> Result<PathBuf, GrantError> {
    std::fs::canonicalize(path).map_err(|_| GrantError::Unavailable)
}

fn align_to_scope(scope: &Path, candidate: &Path) -> PathBuf {
    if candidate.starts_with(scope) {
        return candidate.to_path_buf();
    }
    if let Some((parent, name)) = candidate.parent().zip(candidate.file_name()) {
        if let Ok(parent) = std::fs::canonicalize(parent) {
            let aligned = parent.join(name);
            if aligned == scope || aligned.starts_with(scope) {
                return aligned;
            }
        }
    }
    std::fs::canonicalize(candidate).unwrap_or_else(|_| candidate.to_path_buf())
}

fn lexical_cover(scope: &PathScope, candidate: &Path, access: PathAccess) -> bool {
    let candidate = align_to_scope(&scope.path, candidate);
    let scope_path = &scope.path;
    match scope.kind {
        ScopeKind::ExactFile => candidate == *scope_path && access != PathAccess::ShellWrite,
        ScopeKind::DirectoryTree => {
            if candidate == *scope_path {
                !matches!(access, PathAccess::Write | PathAccess::ShellWrite)
            } else {
                candidate.starts_with(scope_path)
            }
        }
    }
}

/// Symlinks above the admitted scope are operating-system aliases such as
/// `/tmp` -> `/private/tmp`. A symlink at or inside the scope is an escape.
fn hostile_symlink(scope: &Path, candidate: &Path) -> Result<bool, GrantError> {
    let mut prefix = PathBuf::new();
    let mut inside = false;
    for component in candidate.components() {
        prefix.push(component);
        if matches!(component, Component::RootDir) {
            continue;
        }
        let metadata = match std::fs::symlink_metadata(&prefix) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(GrantError::Unavailable),
        };
        if metadata.file_type().is_symlink() {
            if inside {
                return Ok(true);
            }
            let canonical = std::fs::canonicalize(&prefix).map_err(|_| GrantError::SymlinkScope)?;
            if canonical == scope || scope.starts_with(&canonical) {
                inside = canonical == scope;
                continue;
            }
            return Ok(true);
        }
        if let Ok(canonical) = std::fs::canonicalize(&prefix) {
            if canonical == scope || canonical.starts_with(scope) {
                inside = true;
            }
        }
    }
    Ok(false)
}

fn anchor_within(
    scope: &PathScope,
    candidate: &Path,
    access: PathAccess,
) -> Result<(), GrantError> {
    if hostile_symlink(&scope.path, candidate)? {
        return Err(GrantError::SymlinkScope);
    }
    let resolved = align_to_scope(&scope.path, candidate);
    if !lexical_cover(scope, &resolved, access) && resolved != candidate {
        return Err(GrantError::OutsideScope);
    }
    #[cfg(unix)]
    {
        if scope.kind == ScopeKind::DirectoryTree && scope.path.is_dir() {
            let relative = resolved
                .strip_prefix(&scope.path)
                .or_else(|_| candidate.strip_prefix(&scope.path))
                .map_err(|_| GrantError::OutsideScope)?;
            open_relative_nofollow(&scope.path, relative, access)?;
        } else if scope.kind == ScopeKind::ExactFile && resolved.exists() {
            open_exact_nofollow(&resolved)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn open_exact_nofollow(path: &Path) -> Result<(), GrantError> {
    use std::os::unix::ffi::OsStrExt;
    let text = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| GrantError::UnstablePath)?;
    let fd = unsafe {
        libc::open(
            text.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(GrantError::SymlinkScope);
    }
    unsafe { libc::close(fd) };
    Ok(())
}

#[cfg(unix)]
fn open_relative_nofollow(
    root: &Path,
    relative: &Path,
    access: PathAccess,
) -> Result<(), GrantError> {
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::AsRawFd;
    let root_text = std::ffi::CString::new(root.as_os_str().as_bytes())
        .map_err(|_| GrantError::UnstablePath)?;
    let root_fd = unsafe {
        libc::open(
            root_text.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if root_fd < 0 {
        return Err(GrantError::Unavailable);
    }
    let mut current = unsafe { std::fs::File::from_raw_fd(root_fd) };
    let parts: Vec<_> = relative.components().collect();
    if parts.is_empty() {
        if matches!(access, PathAccess::Write | PathAccess::ShellWrite) {
            return Err(GrantError::OutsideScope);
        }
        return Ok(());
    }
    for (index, component) in parts.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(GrantError::UnstablePath);
        };
        let name = std::ffi::CString::new(name.as_bytes()).map_err(|_| GrantError::UnstablePath)?;
        let last = index + 1 == parts.len();
        let flags = if last {
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK
        } else {
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW
        };
        let fd = unsafe { libc::openat(current.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            if last
                && error.kind() == std::io::ErrorKind::NotFound
                && matches!(access, PathAccess::Write)
            {
                return Ok(());
            }
            return Err(GrantError::SymlinkScope);
        }
        current = unsafe { std::fs::File::from_raw_fd(fd) };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant_for(root: &Path, file: &Path) -> AdmittedExecutionGrant {
        AdmittedExecutionGrant::from_legacy(
            LegacyGrantView {
                tools: &[
                    "read_file".into(),
                    "write_file".into(),
                    "run_command".into(),
                ],
                path_scopes: &[file.to_string_lossy().into_owned()],
                command_scopes: &["printf scoped".into()],
            },
            Uuid::nil(),
            7,
            Vec::new(),
        )
        .unwrap_or_else(|_| {
            let _ = root;
            panic!("grant");
        })
    }

    #[test]
    fn explicit_isolated_fails_until_native_qualification() {
        #[cfg(target_os = "macos")]
        assert_eq!(
            resolve_child_execution(None).unwrap(),
            ResolvedChildExecution::Isolated
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            resolve_child_execution(None).unwrap(),
            ResolvedChildExecution::InProcess
        );
        assert_eq!(
            resolve_child_execution(Some(ChildExecutionMode::InProcess)).unwrap(),
            ResolvedChildExecution::InProcess
        );
        #[cfg(target_os = "macos")]
        assert_eq!(
            resolve_child_execution(Some(ChildExecutionMode::Isolated)).unwrap(),
            ResolvedChildExecution::Isolated
        );
        #[cfg(not(target_os = "macos"))]
        assert!(resolve_child_execution(Some(ChildExecutionMode::Isolated)).is_err());
        assert_eq!(
            resolve_child_execution_qualified(None, false).unwrap(),
            ResolvedChildExecution::InProcess
        );
        #[cfg(target_os = "macos")]
        assert_eq!(
            resolve_child_execution_qualified(None, true).unwrap(),
            ResolvedChildExecution::Isolated
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            resolve_child_execution_qualified(None, true).unwrap(),
            ResolvedChildExecution::InProcess
        );
        let manifest = include_str!("../Cargo.toml");
        assert!(manifest.contains("e9796cca01eb863a9a83e96c18ae6120aa50beb1"));
        assert!(manifest.contains("=0.81.0"));
        assert!(manifest.contains("default-features = false"));
        assert!(manifest.contains("target_os = \"linux\""));
    }

    #[test]
    fn exact_file_grant_reads_that_file_and_rejects_siblings_and_prefixes() {
        let temp = tempfile::tempdir().unwrap();
        let allowed = temp.path().join("allowed.txt");
        let sibling = temp.path().join("denied.txt");
        let prefixed = temp.path().join("allowed.txt2");
        std::fs::write(&allowed, "ok").unwrap();
        std::fs::write(&sibling, "no").unwrap();
        std::fs::write(&prefixed, "no").unwrap();
        let grant = grant_for(temp.path(), &allowed);
        assert!(grant.authorize_path(&allowed, PathAccess::Read).is_ok());
        assert_eq!(
            grant.authorize_path(&sibling, PathAccess::Read),
            Err(GrantError::OutsideScope)
        );
        assert_eq!(
            grant.authorize_path(&prefixed, PathAccess::Read),
            Err(GrantError::OutsideScope)
        );
        assert!(grant.authorize_path(&allowed, PathAccess::Write).is_ok());
        assert_eq!(
            grant.authorize_path(&allowed, PathAccess::ShellWrite),
            Err(GrantError::OutsideScope)
        );
        assert!(grant.authorize_network().is_err());
        assert!(grant.ensure_current(7).is_ok());
        assert_eq!(grant.ensure_current(8), Err(GrantError::StaleGeneration));
    }

    #[test]
    fn directory_grant_rejects_symlink_escape_and_parent_components() {
        let temp = tempfile::tempdir().unwrap();
        let assigned = temp.path().join("assigned");
        let outside = temp.path().join("outside");
        std::fs::create_dir(&assigned).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, assigned.join("linked")).unwrap();
        let grant = AdmittedExecutionGrant::from_legacy(
            LegacyGrantView {
                tools: &["read_file".into()],
                path_scopes: &[assigned.to_string_lossy().into_owned()],
                command_scopes: &[],
            },
            Uuid::nil(),
            1,
            Vec::new(),
        )
        .unwrap();
        let child = assigned.join("plain.txt");
        std::fs::write(&child, "ok").unwrap();
        assert!(grant.authorize_path(&child, PathAccess::Read).is_ok());
        #[cfg(unix)]
        {
            let escaped = assigned.join("linked").join("secret.txt");
            assert!(grant.authorize_path(&escaped, PathAccess::Read).is_err());
        }
        let dotted = assigned.join("..").join("outside").join("secret.txt");
        assert_eq!(
            grant.authorize_path(&dotted, PathAccess::Read),
            Err(GrantError::UnstablePath)
        );
    }

    #[test]
    fn worktree_rebase_keeps_one_file_and_rejects_another_root() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let other = temp.path().join("other");
        let worktree = temp.path().join("worktree");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&other).unwrap();
        std::fs::create_dir(&worktree).unwrap();
        let file = source.join("only.txt");
        std::fs::write(&file, "one").unwrap();
        std::fs::write(other.join("else.txt"), "two").unwrap();
        let rebased =
            rebase_scope_strings(&[file.to_string_lossy().into_owned()], &source, &worktree)
                .unwrap();
        let rebased_path = PathBuf::from(&rebased[0]);
        assert_eq!(
            rebased_path.file_name().and_then(|name| name.to_str()),
            Some("only.txt")
        );
        assert_eq!(
            rebased_path.parent().unwrap().canonicalize().unwrap(),
            worktree.canonicalize().unwrap()
        );
        let foreign = rebase_scope_strings(
            &[other.join("else.txt").to_string_lossy().into_owned()],
            &source,
            &worktree,
        );
        assert_eq!(foreign, Err(GrantError::NotRebasable));
        let whole =
            rebase_scope_strings(&[source.to_string_lossy().into_owned()], &source, &worktree)
                .unwrap();
        assert_eq!(
            PathBuf::from(&whole[0]).canonicalize().unwrap(),
            worktree.canonicalize().unwrap()
        );
    }

    #[test]
    fn narrowing_rejects_tool_command_and_reference_widening() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("a.txt");
        std::fs::write(&file, "a").unwrap();
        let grant = grant_for(temp.path(), &file);
        assert!(grant
            .intersect_narrowing(
                Some(&["read_file".into()]),
                Some(&[]),
                None,
                None,
                None,
                None
            )
            .unwrap()
            .tools
            .contains("read_file"));
        assert_eq!(
            grant.intersect_narrowing(
                Some(&["delete_everything".into()]),
                None,
                None,
                None,
                None,
                None
            ),
            Err(GrantError::ToolWidening)
        );
        assert_eq!(
            grant.intersect_narrowing(None, None, None, None, Some(&["example".into()]), None),
            Err(GrantError::ReferenceWidening)
        );
    }
}
