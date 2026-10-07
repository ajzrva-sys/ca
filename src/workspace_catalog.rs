use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub(crate) const SCHEMA: &str = "ca.workspace-catalog.v1";
const MAX_CATALOG_BYTES: u64 = 1024 * 1024;
const MAX_WORKSPACES: usize = 128;
const MAX_ROOTS: usize = 32;
const MAX_NAME_BYTES: usize = 64;
const MAX_PATH_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkspaceCatalogEntry {
    pub name: String,
    pub roots: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceCatalogDocument {
    schema: String,
    entries: Vec<WorkspaceCatalogEntry>,
}

impl Default for WorkspaceCatalogDocument {
    fn default() -> Self {
        Self {
            schema: SCHEMA.into(),
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceCatalogStore {
    path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum WorkspaceCatalogError {
    #[error("workspace catalog name must be 1-64 ASCII letters, digits, '-' or '_', starting with a letter or digit")]
    InvalidName,
    #[error("workspace catalog is too large")]
    CatalogTooLarge,
    #[error("workspace catalog contains too many entries")]
    TooManyEntries,
    #[error("workspace '{0}' contains too many roots")]
    TooManyRoots(String),
    #[error("workspace '{0}' has no roots")]
    NoRoots(String),
    #[error("workspace root is missing or is not a directory: {0}")]
    MissingRoot(String),
    #[error("workspace root must be an absolute canonical path without symlink aliases: {0}")]
    NonCanonicalRoot(String),
    #[error("workspace '{workspace}' contains duplicate root: {root}")]
    DuplicateRoot { workspace: String, root: String },
    #[error("workspace catalog contains duplicate name: {0}")]
    DuplicateName(String),
    #[error("workspace catalog entry not found: {0}")]
    NotFound(String),
    #[error("workspace catalog path is not a private regular file")]
    UnsafeCatalogFile,
    #[error("workspace catalog schema is unsupported: {0}")]
    UnsupportedSchema(String),
    #[error("workspace catalog is malformed: {0}")]
    Malformed(String),
    #[error("workspace catalog I/O failed: {0}")]
    Io(String),
    #[error(transparent)]
    Workspace(#[from] crate::workspace::WorkspaceError),
}

impl WorkspaceCatalogStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn list(&self) -> Result<Vec<WorkspaceCatalogEntry>, WorkspaceCatalogError> {
        Ok(self.load_document()?.entries)
    }

    pub(crate) fn save(
        &self,
        name: &str,
        workspace: &crate::workspace::WorkspaceContext,
    ) -> Result<(), WorkspaceCatalogError> {
        let roots = workspace
            .roots()
            .iter()
            .map(|root| root.path.clone())
            .collect::<Vec<_>>();
        self.save_roots(name, &roots)
    }

    pub(crate) fn delete(&self, name: &str) -> Result<(), WorkspaceCatalogError> {
        validate_name(name)?;
        let mut document = self.load_document()?;
        let before = document.entries.len();
        document.entries.retain(|entry| entry.name != name);
        if document.entries.len() == before {
            return Err(WorkspaceCatalogError::NotFound(name.into()));
        }
        self.write_document(&document)
    }

    pub(crate) fn load_workspace(
        &self,
        name: &str,
    ) -> Result<crate::workspace::WorkspaceContext, WorkspaceCatalogError> {
        validate_name(name)?;
        let document = self.load_document()?;
        let entry = document
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| WorkspaceCatalogError::NotFound(name.into()))?;
        context_from_entry(entry)
    }

    pub(crate) fn resolve_selector(
        &self,
        selector: &str,
        launch_cwd: &Path,
    ) -> Result<crate::workspace::WorkspaceContext, WorkspaceCatalogError> {
        let selector = selector.trim();
        if selector.is_empty() {
            return Err(WorkspaceCatalogError::InvalidName);
        }
        let document = self.load_document()?;
        if let Some(entry) = document.entries.iter().find(|entry| entry.name == selector) {
            return context_from_entry(entry);
        }
        let path = Path::new(selector);
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            launch_cwd.join(path)
        };
        crate::workspace::WorkspaceContext::discover(candidate).map_err(Into::into)
    }

    fn save_roots(&self, name: &str, roots: &[PathBuf]) -> Result<(), WorkspaceCatalogError> {
        validate_name(name)?;
        let entry = validated_entry(name, roots)?;
        let mut document = self.load_document()?;
        if let Some(existing) = document
            .entries
            .iter_mut()
            .find(|existing| existing.name == name)
        {
            *existing = entry;
        } else {
            if document.entries.len() >= MAX_WORKSPACES {
                return Err(WorkspaceCatalogError::TooManyEntries);
            }
            document.entries.push(entry);
        }
        document
            .entries
            .sort_by(|left, right| left.name.cmp(&right.name));
        self.write_document(&document)
    }

    fn load_document(&self) -> Result<WorkspaceCatalogDocument, WorkspaceCatalogError> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(WorkspaceCatalogDocument::default());
            }
            Err(error) => return Err(WorkspaceCatalogError::Io(error.to_string())),
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_CATALOG_BYTES
        {
            return Err(WorkspaceCatalogError::UnsafeCatalogFile);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o777 != 0o600 {
                return Err(WorkspaceCatalogError::UnsafeCatalogFile);
            }
        }
        let bytes =
            fs::read(&self.path).map_err(|error| WorkspaceCatalogError::Io(error.to_string()))?;
        let document: WorkspaceCatalogDocument = serde_json::from_slice(&bytes)
            .map_err(|error| WorkspaceCatalogError::Malformed(error.to_string()))?;
        validate_document(document)
    }

    fn write_document(
        &self,
        document: &WorkspaceCatalogDocument,
    ) -> Result<(), WorkspaceCatalogError> {
        let bytes = serde_json::to_vec_pretty(document)
            .map_err(|error| WorkspaceCatalogError::Malformed(error.to_string()))?;
        if bytes.len() as u64 > MAX_CATALOG_BYTES {
            return Err(WorkspaceCatalogError::CatalogTooLarge);
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| WorkspaceCatalogError::Io("catalog has no parent".into()))?;
        fs::create_dir_all(parent).map_err(|error| WorkspaceCatalogError::Io(error.to_string()))?;
        let parent_metadata = fs::symlink_metadata(parent)
            .map_err(|error| WorkspaceCatalogError::Io(error.to_string()))?;
        if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
            return Err(WorkspaceCatalogError::UnsafeCatalogFile);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|error| WorkspaceCatalogError::Io(error.to_string()))?;
        }
        let temporary = parent.join(format!(".catalog-v1.{}.tmp", uuid::Uuid::new_v4().simple()));
        let write_result = (|| {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&temporary)
                .map_err(|error| WorkspaceCatalogError::Io(error.to_string()))?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|error| WorkspaceCatalogError::Io(error.to_string()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(fs::Permissions::from_mode(0o600))
                    .map_err(|error| WorkspaceCatalogError::Io(error.to_string()))?;
            }
            fs::rename(&temporary, &self.path)
                .map_err(|error| WorkspaceCatalogError::Io(error.to_string()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))
                    .map_err(|error| WorkspaceCatalogError::Io(error.to_string()))?;
            }
            if let Ok(directory) = fs::File::open(parent) {
                let _ = directory.sync_all();
            }
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result
    }
}

fn validate_name(name: &str) -> Result<(), WorkspaceCatalogError> {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return Err(WorkspaceCatalogError::InvalidName);
    };
    if name.len() > MAX_NAME_BYTES
        || !first.is_ascii_alphanumeric()
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(WorkspaceCatalogError::InvalidName);
    }
    Ok(())
}

fn validated_entry(
    name: &str,
    roots: &[PathBuf],
) -> Result<WorkspaceCatalogEntry, WorkspaceCatalogError> {
    if roots.is_empty() {
        return Err(WorkspaceCatalogError::NoRoots(name.into()));
    }
    if roots.len() > MAX_ROOTS {
        return Err(WorkspaceCatalogError::TooManyRoots(name.into()));
    }
    let mut selected = BTreeSet::new();
    let mut stored = Vec::with_capacity(roots.len());
    for root in roots {
        let raw = root
            .to_str()
            .filter(|value| value.len() <= MAX_PATH_BYTES)
            .ok_or_else(|| WorkspaceCatalogError::NonCanonicalRoot(root.display().to_string()))?;
        if !root.is_absolute() {
            return Err(WorkspaceCatalogError::NonCanonicalRoot(raw.into()));
        }
        let canonical =
            fs::canonicalize(root).map_err(|_| WorkspaceCatalogError::MissingRoot(raw.into()))?;
        if !canonical.is_dir() {
            return Err(WorkspaceCatalogError::MissingRoot(raw.into()));
        }
        if canonical.as_path() != root.as_path() {
            return Err(WorkspaceCatalogError::NonCanonicalRoot(raw.into()));
        }
        let value = canonical
            .to_str()
            .ok_or_else(|| WorkspaceCatalogError::NonCanonicalRoot(raw.into()))?
            .to_string();
        if !selected.insert(value.clone()) {
            return Err(WorkspaceCatalogError::DuplicateRoot {
                workspace: name.into(),
                root: value,
            });
        }
        stored.push(value);
    }
    Ok(WorkspaceCatalogEntry {
        name: name.into(),
        roots: stored,
    })
}

fn validate_document(
    mut document: WorkspaceCatalogDocument,
) -> Result<WorkspaceCatalogDocument, WorkspaceCatalogError> {
    if document.schema != SCHEMA {
        return Err(WorkspaceCatalogError::UnsupportedSchema(document.schema));
    }
    if document.entries.len() > MAX_WORKSPACES {
        return Err(WorkspaceCatalogError::TooManyEntries);
    }
    let mut names = BTreeSet::new();
    for entry in &document.entries {
        validate_name(&entry.name)?;
        if !names.insert(entry.name.clone()) {
            return Err(WorkspaceCatalogError::DuplicateName(entry.name.clone()));
        }
        let roots = entry.roots.iter().map(PathBuf::from).collect::<Vec<_>>();
        let validated = validated_entry(&entry.name, &roots)?;
        if validated != *entry {
            return Err(WorkspaceCatalogError::NonCanonicalRoot(entry.name.clone()));
        }
    }
    document
        .entries
        .sort_by(|left, right| left.name.cmp(&right.name));
    Ok(document)
}

fn context_from_entry(
    entry: &WorkspaceCatalogEntry,
) -> Result<crate::workspace::WorkspaceContext, WorkspaceCatalogError> {
    let primary = PathBuf::from(
        entry
            .roots
            .first()
            .ok_or_else(|| WorkspaceCatalogError::NoRoots(entry.name.clone()))?,
    );
    let additional = entry.roots[1..]
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    crate::workspace::WorkspaceContext::from_roots(&primary, &additional).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(root: &Path) -> WorkspaceCatalogStore {
        WorkspaceCatalogStore::new(root.join("profile/workspaces/catalog-v1.json"))
    }

    #[test]
    fn catalog_is_private_atomic_bounded_and_round_trips_ordered_roots() {
        let temp = tempfile::tempdir().unwrap();
        let primary = temp.path().join("primary");
        let extra = temp.path().join("extra");
        fs::create_dir_all(&primary).unwrap();
        fs::create_dir_all(&extra).unwrap();
        let primary = fs::canonicalize(primary).unwrap();
        let extra = fs::canonicalize(extra).unwrap();
        let catalog = store(temp.path());
        catalog
            .save_roots("demo", &[primary.clone(), extra.clone()])
            .unwrap();
        assert_eq!(
            catalog.list().unwrap(),
            [WorkspaceCatalogEntry {
                name: "demo".into(),
                roots: vec![
                    primary.to_string_lossy().into_owned(),
                    extra.to_string_lossy().into_owned()
                ]
            }]
        );
        let workspace = catalog.load_workspace("demo").unwrap();
        assert_eq!(workspace.primary().unwrap().path, primary);
        assert_eq!(workspace.roots()[1].path, extra);
        assert!(fs::read_dir(catalog.path().parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(catalog.path()).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn exact_catalog_name_wins_but_explicit_relative_path_forces_path_resolution() {
        let temp = tempfile::tempdir().unwrap();
        let saved = temp.path().join("saved");
        let launch = temp.path().join("launch");
        let ambiguous = launch.join("demo");
        fs::create_dir_all(&saved).unwrap();
        fs::create_dir_all(&ambiguous).unwrap();
        let saved = fs::canonicalize(saved).unwrap();
        let ambiguous = fs::canonicalize(ambiguous).unwrap();
        let launch = fs::canonicalize(launch).unwrap();
        let catalog = store(temp.path());
        catalog
            .save_roots("demo", std::slice::from_ref(&saved))
            .unwrap();
        assert_eq!(
            catalog
                .resolve_selector("demo", &launch)
                .unwrap()
                .primary()
                .unwrap()
                .path,
            saved
        );
        assert_eq!(
            catalog
                .resolve_selector("./demo", &launch)
                .unwrap()
                .primary()
                .unwrap()
                .path,
            ambiguous
        );
    }

    #[test]
    fn malformed_names_duplicates_missing_and_symlink_aliases_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let catalog = store(temp.path());
        assert!(matches!(
            catalog.save_roots("bad/name", std::slice::from_ref(&root)),
            Err(WorkspaceCatalogError::InvalidName)
        ));
        assert!(matches!(
            catalog.save_roots("demo", &[root.clone(), root.clone()]),
            Err(WorkspaceCatalogError::DuplicateRoot { .. })
        ));
        let missing = temp.path().join("missing");
        assert!(matches!(
            catalog.save_roots("demo", &[missing]),
            Err(WorkspaceCatalogError::MissingRoot(_))
        ));
        #[cfg(unix)]
        {
            let alias = temp.path().join("alias");
            std::os::unix::fs::symlink(&root, &alias).unwrap();
            assert!(matches!(
                catalog.save_roots("demo", &[alias]),
                Err(WorkspaceCatalogError::NonCanonicalRoot(_))
            ));
        }
    }

    #[test]
    fn corrupt_or_insecure_catalog_is_rejected_without_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let catalog = store(temp.path());
        fs::create_dir_all(catalog.path().parent().unwrap()).unwrap();
        fs::write(catalog.path(), b"not json").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(catalog.path(), fs::Permissions::from_mode(0o600)).unwrap();
        }
        let before = fs::read(catalog.path()).unwrap();
        assert!(matches!(
            catalog.list(),
            Err(WorkspaceCatalogError::Malformed(_))
        ));
        assert_eq!(fs::read(catalog.path()).unwrap(), before);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(catalog.path(), fs::Permissions::from_mode(0o644)).unwrap();
            assert!(matches!(
                catalog.list(),
                Err(WorkspaceCatalogError::UnsafeCatalogFile)
            ));
        }
    }

    #[test]
    fn entry_and_root_caps_are_enforced() {
        let temp = tempfile::tempdir().unwrap();
        let catalog = store(temp.path());
        let mut roots = Vec::new();
        for index in 0..=MAX_ROOTS {
            let path = temp.path().join(format!("root-{index}"));
            fs::create_dir_all(&path).unwrap();
            roots.push(fs::canonicalize(path).unwrap());
        }
        assert!(matches!(
            catalog.save_roots("too-many", &roots),
            Err(WorkspaceCatalogError::TooManyRoots(_))
        ));
        for index in 0..MAX_WORKSPACES {
            catalog
                .save_roots(&format!("w{index}"), std::slice::from_ref(&roots[0]))
                .unwrap();
        }
        assert!(matches!(
            catalog.save_roots("overflow", std::slice::from_ref(&roots[0])),
            Err(WorkspaceCatalogError::TooManyEntries)
        ));
    }
}
