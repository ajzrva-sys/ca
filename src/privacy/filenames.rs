//! Session-owned, metadata-only filename projection. This is a name boundary,
//! not a source-content anonymizer. All filesystem work belongs on a worker.
#![allow(dead_code)]
use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};

const VERSION: u32 = 2;
const MAX_ENTRIES: usize = 262_144;
const MAX_BYTES: usize = 256 * 1024 * 1024;
// Match the native directory result's existing 5,000-row browsing bound.
const MAX_INITIAL_PATHS: usize = 5_000;
// Automatic previews must not recursively inventory an entire mounted volume.
const MAX_DESCENDANT_ENTRIES: usize = 25_000;
const MAX_DISCOVERY_EXAMPLES: usize = 3;
const ERROR: &str = "Filename aliases could not be validated; local action was blocked";
const LIMIT: &str = "Filename discovery exceeded its bound; no private filenames were sent";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Snapshot {
    version: u32,
    namespace: String,
    root_alias: String,
    #[serde(default)]
    allocation_tag: String,
    next_directory: u64,
    next_file: u64,
    next_name: u64,
    entries: Vec<Entry>,
    names: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    source_spellings: BTreeMap<String, SourceSpelling>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    scopes: Vec<ScopedSnapshot>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    public_names: BTreeSet<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ScopedSnapshot {
    root: PathBuf,
    snapshot: Box<Snapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SourceSpelling {
    original: String,
    target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
}

impl SourceSpelling {
    fn home_path(&self) -> Option<&str> {
        (self.source.is_none() && self.target.is_some())
            .then(|| home_path_suffix(&self.original))
            .flatten()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Entry {
    relative: String,
    alias: String,
    directory: bool,
}

#[derive(Clone)]
pub(crate) struct FilenameAliases {
    workspace: PathBuf,
    /// Local process input, never part of provider text or saved authority.
    home: Option<PathBuf>,
    snapshot: Snapshot,
    /// Current metadata, distinct from historical mappings retained for replay.
    live: BTreeSet<String>,
    forward: BTreeMap<String, String>,
    backward: BTreeMap<String, String>,
    matcher: Option<AhoCorasick>,
    replacements: Vec<String>,
    scopes: BTreeMap<PathBuf, FilenameAliases>,
    authorized_scopes: BTreeSet<PathBuf>,
    /// Process-only basename census; bool records ambiguity among seen names.
    census: BTreeMap<String, bool>,
    coverage: CensusCoverage,
    /// Matcher patterns parallel replacements; empty replacements are allocated only when emitted.
    patterns: Vec<String>,
}

impl FilenameAliases {
    #[cfg(test)]
    pub(crate) fn set_home_for_test(&mut self, home: PathBuf) {
        self.home = Some(home);
    }

    pub(crate) fn new(workspace: PathBuf, namespace: &str) -> Result<Self, String> {
        if !valid_namespace(namespace) {
            return Err(ERROR.into());
        }
        let workspace = std::fs::canonicalize(workspace).map_err(|_| ERROR)?;
        if !workspace.is_dir() || workspace.to_str().is_none() {
            return Err(ERROR.into());
        }
        Self::from_snapshot(
            workspace,
            Snapshot {
                version: VERSION,
                namespace: namespace.into(),
                root_alias: format!("{namespace}__"),
                allocation_tag: String::new(),
                next_directory: 1,
                next_file: 1,
                next_name: 1,
                entries: Vec::new(),
                names: BTreeMap::new(),
                source_spellings: BTreeMap::new(),
                scopes: Vec::new(),
                public_names: BTreeSet::new(),
            },
        )
    }

    pub(crate) fn from_snapshot(workspace: PathBuf, snapshot: Snapshot) -> Result<Self, String> {
        Self::restore_snapshot(workspace, snapshot, true)
    }
    fn restore_snapshot(
        workspace: PathBuf,
        mut snapshot: Snapshot,
        require_live_root: bool,
    ) -> Result<Self, String> {
        if !matches!(snapshot.version, 1 | VERSION)
            || !valid_namespace(&snapshot.namespace)
            || (!snapshot.allocation_tag.is_empty() && !valid_namespace(&snapshot.allocation_tag))
            || !snapshot.root_alias.ends_with("__")
            || !snapshot
                .root_alias
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || snapshot.next_directory == 0
            || snapshot.next_file == 0
            || snapshot.next_name == 0
            || snapshot.entries.len() > MAX_ENTRIES
            || snapshot.names.len() > MAX_ENTRIES
            || snapshot.source_spellings.len() > MAX_ENTRIES
        {
            return Err(ERROR.into());
        }
        let workspace = if require_live_root {
            let canonical = std::fs::canonicalize(workspace).map_err(|_| ERROR)?;
            if !canonical.is_dir() {
                return Err(ERROR.into());
            }
            canonical
        } else {
            if !workspace.is_absolute()
                || workspace
                    .components()
                    .any(|component| matches!(component, Component::ParentDir))
            {
                return Err(ERROR.into());
            }
            workspace
        };
        if workspace.to_str().is_none() {
            return Err(ERROR.into());
        }
        let mut identities = BTreeSet::new();
        let mut aliases = BTreeSet::new();
        let mut bytes = 0_usize;
        for entry in &snapshot.entries {
            valid_relative(&entry.relative)?;
            if !valid_alias_path(&snapshot.root_alias, &entry.alias)
                || !identities.insert(entry.relative.clone())
                || !aliases.insert(entry.alias.clone())
            {
                return Err(ERROR.into());
            }
            bytes = bytes.saturating_add(entry.relative.len() + entry.alias.len());
        }
        for (name, alias) in &snapshot.names {
            if name.is_empty()
                || name.contains('/')
                || name.contains('\0')
                || !alias.ends_with("__")
                || !aliases.insert(alias.clone())
            {
                return Err(ERROR.into());
            }
            bytes = bytes.saturating_add(name.len() + alias.len());
        }
        for (alias, spelling) in &snapshot.source_spellings {
            let source_token = alias.ends_with("__")
                && alias
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
            if !(source_token || valid_alias_path(&snapshot.root_alias, alias))
                || spelling.original.is_empty()
                || !aliases.insert(alias.clone())
            {
                return Err(ERROR.into());
            }
            if let Some(target) = &spelling.target {
                if target != "." || spelling.home_path().is_none() {
                    valid_relative(target)?;
                }
            }
            if let Some(source) = &spelling.source {
                valid_relative(source)?;
            }
            bytes = bytes.saturating_add(alias.len() + spelling.original.len());
        }
        if bytes > MAX_BYTES {
            return Err(LIMIT.into());
        }
        snapshot.version = VERSION;
        let saved_scopes = std::mem::take(&mut snapshot.scopes);
        if saved_scopes.len() > 256 {
            return Err(LIMIT.into());
        }
        let mut scopes = BTreeMap::new();
        for saved in saved_scopes {
            if !saved.root.is_absolute() || !saved.snapshot.scopes.is_empty() {
                return Err(ERROR.into());
            }
            // Saved mappings are historical projection data, never grants.
            let scope = Self::restore_snapshot(saved.root.clone(), *saved.snapshot, false)?;
            if scopes.insert(saved.root, scope).is_some() {
                return Err(ERROR.into());
            }
        }
        let mut this = Self {
            workspace,
            home: std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|home| home.is_absolute()),
            snapshot,
            live: BTreeSet::new(),
            forward: BTreeMap::new(),
            backward: BTreeMap::new(),
            matcher: None,
            replacements: Vec::new(),
            scopes,
            authorized_scopes: BTreeSet::new(),
            census: BTreeMap::new(),
            coverage: CensusCoverage {
                limited: true,
                ..Default::default()
            },
            patterns: Vec::new(),
        };
        this.rebuild()?;
        Ok(this)
    }

    pub(crate) fn shallow_files(&self) -> Vec<(String, String)> {
        self.snapshot
            .entries
            .iter()
            .filter(|entry| !entry.directory && !entry.relative.contains('/'))
            .map(|entry| (entry.relative.clone(), entry.alias.clone()))
            .collect()
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        let mut snapshot = self.snapshot.clone();
        snapshot.scopes = self
            .scopes
            .iter()
            .map(|(root, names)| ScopedSnapshot {
                root: root.clone(),
                snapshot: Box::new(names.snapshot()),
            })
            .collect();
        snapshot
    }

    /// Bind only host-authorized roots. Persisted aliases do not restore access.
    pub(crate) fn authorize_roots(&mut self, roots: &[PathBuf]) -> Result<(), String> {
        let mut active = BTreeSet::new();
        for root in roots {
            let root = root
                .canonicalize()
                .map_err(|_| "Granted folder is unavailable")?;
            if root.starts_with(&self.workspace) {
                continue;
            }
            if !root.is_dir() {
                return Err("Granted root is not a directory".into());
            }
            if !self.scopes.contains_key(&root) {
                let namespace = format!("{}{:x}", self.snapshot.namespace, scope_tag(&root));
                let mut scope = Self::new(root.clone(), &namespace)?;
                scope.home = self.home.clone();
                scope.snapshot.public_names = self.snapshot.public_names.clone();
                self.scopes.insert(root.clone(), scope);
            }
            active.insert(root);
        }
        if self.authorized_scopes == active {
            return Ok(());
        }
        self.authorized_scopes = active;
        self.rebuild()
    }

    pub(crate) fn register_grant_request(&mut self, root: &Path) -> Result<(), String> {
        let root = root
            .canonicalize()
            .map_err(|_| "Requested folder is unavailable")?;
        if root.starts_with(&self.workspace) || self.scopes.contains_key(&root) {
            return Ok(());
        }
        let namespace = format!("{}{:x}", self.snapshot.namespace, scope_tag(&root));
        let scope = Self::new(root.clone(), &namespace)?;
        self.scopes.insert(root, scope);
        self.rebuild()
    }
    pub(crate) fn declare_public_names(&mut self, text: &str) -> Result<(), String> {
        for name in text.split(|ch: char| !(ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.')))
        {
            if name.is_empty() || name.contains("__CA_") || valid_relative(name).is_err() {
                continue;
            }
            if !self.snapshot.names.contains_key(name) && self.project_text(name)? == name {
                if self.snapshot.public_names.len() >= MAX_ENTRIES {
                    return Err(LIMIT.into());
                }
                self.snapshot.public_names.insert(name.into());
            }
        }
        for scope in self.scopes.values_mut() {
            for name in &self.snapshot.public_names {
                if !scope.snapshot.names.contains_key(name) {
                    scope.snapshot.public_names.insert(name.clone());
                }
            }
        }
        Ok(())
    }
    /// Resolving a saved folder for the local grant dialog does not grant access.
    pub(crate) fn resolve_grant_request(&self, reference: &str) -> Result<PathBuf, String> {
        for scope in self.scopes.values() {
            if scope.is_known_path_alias(reference) {
                return scope.resolve_path(reference);
            }
        }
        self.resolve_path(reference)
    }
    pub(crate) fn authorized_path(&self, path: &Path) -> bool {
        path.starts_with(&self.workspace)
            || self
                .authorized_scopes
                .iter()
                .any(|root| path.starts_with(root))
    }

    pub(crate) fn path_alias(&mut self, path: &Path) -> Result<String, String> {
        self.project_path(path.to_str().ok_or(ERROR)?)
    }

    pub(crate) fn matching_files(
        &mut self,
        root: &Path,
        pattern: &str,
    ) -> Result<BTreeSet<PathBuf>, String> {
        let pattern = glob::Pattern::new(pattern).map_err(|_| "Invalid projected filename glob")?;
        if !self.authorized_path(root) {
            return Err("Filename selection needs a current folder grant".into());
        }
        let selected_scope = self
            .authorized_scopes
            .iter()
            .filter(|scope| root.starts_with(scope))
            .max_by_key(|scope| scope.components().count())
            .cloned();
        let scope = if let Some(scope) = &selected_scope {
            self.scopes.get_mut(scope).ok_or(ERROR)?
        } else {
            &mut *self
        };
        let mut stack = vec![root.to_path_buf()];
        let mut visited = 0_usize;
        let mut matches = BTreeSet::new();
        while let Some(path) = stack.pop() {
            visited += 1;
            if visited > MAX_ENTRIES {
                return Err("Filename selection exceeded its entry bound".into());
            }
            let metadata = std::fs::symlink_metadata(&path).map_err(|_| ERROR)?;
            if metadata.is_dir() {
                for entry in std::fs::read_dir(&path).map_err(|_| ERROR)? {
                    stack.push(entry.map_err(|_| ERROR)?.path());
                    if stack.len().saturating_add(visited) > MAX_ENTRIES {
                        return Err("Filename selection exceeded its entry bound".into());
                    }
                }
                continue;
            }
            let native = path.to_str().ok_or(ERROR)?;
            let alias = scope.project_path(native)?;
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or(ERROR)?;
            let descriptive = if scope.disclosed_creation_leaf(name) {
                name.into()
            } else if let Some(alias) = scope.forward.get(name).filter(|alias| !alias.is_empty()) {
                alias.clone()
            } else {
                scope.issue_name(name)?
            };
            if pattern.matches(&alias)
                || pattern.matches(alias.rsplit('/').next().unwrap_or(&alias))
                || pattern.matches(&descriptive)
            {
                matches.insert(path);
            }
        }
        if selected_scope.is_some() {
            self.rebuild()?;
        }
        Ok(matches)
    }

    /// Existing references survive a fork; only future components use its tag.
    pub(crate) fn fork_namespace(&mut self, namespace: &str) -> Result<(), String> {
        if self.snapshot.namespace == namespace {
            return Ok(());
        }
        if !valid_namespace(namespace) {
            return Err(ERROR.into());
        }
        self.snapshot.allocation_tag = namespace.into();
        self.snapshot.namespace = namespace.into();
        for (root, scope) in &mut self.scopes {
            scope.fork_namespace(&format!("{namespace}{:x}", scope_tag(root)))?;
        }
        Ok(())
    }

    /// Retain references emitted by an older task without changing this task's
    /// preferred spellings or granting access to any additional folder.
    pub(crate) fn merge_task_history(&mut self, other: &Self) -> Result<(), String> {
        if self.workspace != other.workspace
            || self.snapshot.root_alias != other.snapshot.root_alias
        {
            return Err(ERROR.into());
        }
        let mut snapshot = self.snapshot.clone();
        for entry in &other.snapshot.entries {
            if self
                .backward
                .get(&entry.alias)
                .is_some_and(|known| known != &entry.relative)
            {
                return Err(ERROR.into());
            }
            if snapshot
                .entries
                .iter()
                .any(|known| known.alias == entry.alias)
            {
                continue;
            }
            if snapshot
                .entries
                .iter()
                .any(|known| known.relative == entry.relative)
            {
                snapshot
                    .source_spellings
                    .entry(entry.alias.clone())
                    .or_insert(SourceSpelling {
                        original: entry.relative.clone(),
                        target: Some(entry.relative.clone()),
                        source: None,
                    });
            } else {
                snapshot.entries.push(entry.clone());
            }
        }
        for (name, alias) in &other.snapshot.names {
            match snapshot.names.get(name) {
                Some(existing) if existing != alias => {
                    snapshot
                        .source_spellings
                        .entry(alias.clone())
                        .or_insert(SourceSpelling {
                            original: name.clone(),
                            target: None,
                            source: None,
                        });
                }
                Some(_) => {}
                None => {
                    snapshot.names.insert(name.clone(), alias.clone());
                }
            }
        }
        for (alias, spelling) in &other.snapshot.source_spellings {
            if snapshot
                .source_spellings
                .get(alias)
                .is_some_and(|known| known != spelling)
            {
                return Err(ERROR.into());
            }
            snapshot
                .source_spellings
                .entry(alias.clone())
                .or_insert_with(|| spelling.clone());
        }
        let validated = Self::restore_snapshot(self.workspace.clone(), snapshot, false)?;
        self.snapshot = validated.snapshot;
        for (root, names) in &other.scopes {
            if let Some(current) = self.scopes.get_mut(root) {
                current.merge_task_history(names)?;
            } else {
                self.scopes.insert(root.clone(), names.clone());
            }
        }
        self.rebuild()
    }

    /// Enumerate every basename, but allocate path references only for shallow
    /// browsing and paths actually projected. Census data never enters a snapshot.
    #[allow(dead_code)]
    pub(crate) fn discover(&mut self) -> Result<bool, String> {
        let census = collect_census(&self.census_roots())?;
        self.apply_census(census)
    }

    pub(crate) fn census_roots(&self) -> Vec<PathBuf> {
        std::iter::once(self.workspace.clone())
            .chain(self.authorized_scopes.iter().cloned())
            .collect()
    }

    pub(crate) fn coverage_note(&self) -> String {
        let mut denied = 0_usize;
        let mut example = None;
        let mut limited = false;
        for names in std::iter::once(self).chain(
            self.authorized_scopes
                .iter()
                .filter_map(|root| self.scopes.get(root)),
        ) {
            denied = denied.saturating_add(names.coverage.denied);
            example = example.or_else(|| names.coverage.examples.first());
            limited |= names.coverage.limited;
        }
        if denied == 0 && !limited {
            return String::new();
        }
        let mut note = "Filename coverage is partial: ".to_owned();
        if denied > 0 {
            note.push_str(&format!("{denied} inaccessible folder(s)"));
        }
        if limited {
            if denied > 0 {
                note.push_str("; ");
            }
            note.push_str("descendant scan capped at 25,000 entries");
        }
        note.push_str(". Unscanned bare names may remain readable.");
        if let Some(path) = example {
            let path = path.to_string_lossy();
            note.push_str(" Example: ");
            note.extend(path.chars().take(48));
            if path.chars().count() > 48 {
                note.push('…');
            }
            note.push('.');
        }
        note
    }

    pub(crate) fn apply_census(&mut self, census: Vec<FilenameCensus>) -> Result<bool, String> {
        if self.census_roots()
            != census
                .iter()
                .map(|entry| entry.root.clone())
                .collect::<Vec<_>>()
        {
            return Err(
                "Filename grants changed during discovery; prepare the request again".into(),
            );
        }
        let issued_names = std::iter::once(&*self)
            .chain(self.scopes.values())
            .flat_map(|scope| {
                scope
                    .snapshot
                    .names
                    .values()
                    .chain(scope.snapshot.source_spellings.keys())
            })
            .collect::<Vec<_>>();
        if census.iter().any(|found| {
            found
                .names
                .keys()
                .filter(|name| name.contains("__CA_"))
                .any(|name| {
                    issued_names
                        .iter()
                        .any(|alias| name.contains(alias.as_str()))
                })
        }) {
            return Err(
                "Filename reference conflicts with a newly discovered name; request was withheld"
                    .into(),
            );
        }
        let mut next = self.clone();
        let before = self.snapshot();
        let census_changed = census.iter().any(|found| {
            let previous = if found.root == self.workspace {
                Some(&*self)
            } else {
                self.scopes.get(&found.root)
            };
            previous.is_none_or(|previous| {
                previous.census != found.names || previous.coverage != found.coverage
            })
        });
        let before_live = self.live.clone();
        for found in census {
            if found.root == next.workspace {
                next.apply_local_census(found)?;
            } else {
                next.scopes
                    .get_mut(&found.root)
                    .ok_or(ERROR)?
                    .apply_local_census(found)?;
            }
        }
        next.rebuild()?;
        let changed = census_changed || next.live != before_live || next.snapshot() != before;
        *self = next;
        Ok(changed)
    }

    fn apply_local_census(&mut self, found: FilenameCensus) -> Result<(), String> {
        self.census = found.names;
        self.coverage = found.coverage;
        if self
            .census
            .keys()
            .any(|name| name.contains(&self.snapshot.root_alias))
        {
            if !self.snapshot.entries.is_empty()
                || self
                    .snapshot
                    .source_spellings
                    .keys()
                    .any(|alias| valid_alias_path(&self.snapshot.root_alias, alias))
            {
                return Err("Filename alias namespace conflicts with a newly discovered name; no private filenames were sent".into());
            }
            let base = self.snapshot.namespace.clone();
            let mut counter = 1_u64;
            loop {
                let candidate = format!("{base}_{counter}__");
                if !self.census.keys().any(|name| name.contains(&candidate)) {
                    self.snapshot.root_alias = candidate;
                    // The unused root namespace changed to avoid a real literal
                    // name. Registration must validate against that new namespace,
                    // not the stale reverse map from before this census.
                    self.rebuild()?;
                    break;
                }
                counter = counter.checked_add(1).ok_or(LIMIT)?;
            }
        }
        if self
            .snapshot
            .source_spellings
            .keys()
            .any(|alias| self.census.keys().any(|name| name.contains(alias)))
        {
            return Err("Source filename reference conflicts with a newly discovered name; request was withheld".into());
        }
        self.live.clear();
        for entry in &self.snapshot.entries {
            match std::fs::symlink_metadata(self.workspace.join(&entry.relative)) {
                Ok(metadata) => {
                    if metadata.is_dir() != entry.directory {
                        return Err(
                            "A registered filename changed kind; local action was blocked".into(),
                        );
                    }
                    self.live.insert(entry.relative.clone());
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error)
                    if error.kind() == std::io::ErrorKind::PermissionDenied
                        && self.coverage.denied > 0 => {}
                Err(_) => return Err(ERROR.into()),
            }
        }
        // Public creation references are issued before their files exist. Once
        // publication has happened, register those exact targets on demand so
        // their original spelling becomes a live read reference as before.
        let created_targets = self
            .snapshot
            .source_spellings
            .values()
            .filter(|spelling| spelling.source.is_none())
            .filter_map(|spelling| spelling.target.clone())
            .filter(|target| target != ".")
            .filter(|target| std::fs::symlink_metadata(self.workspace.join(target)).is_ok())
            .collect::<BTreeSet<_>>();
        for target in created_targets {
            self.register_relative(&target)?;
        }
        // First-level directory references support normal browsing without
        // allocating references for millions of descendants.
        for (relative, _) in found.shallow.into_iter().take(MAX_INITIAL_PATHS) {
            self.register_relative(&relative)?;
        }
        self.rebuild()
    }

    fn validate_native_components(&self, path: &str) -> Result<(), String> {
        validate_native_components(path, self.backward.keys().map(String::as_str))
    }

    fn register_relative(&mut self, relative: &str) -> Result<(), String> {
        valid_relative(relative)?;
        // A partial census has not ruled out literals that resemble already
        // issued references. Validate the whole native path before mutation.
        self.validate_native_components(relative)?;
        let components = Path::new(relative).components().count();
        let mut parent_alias = self.snapshot.root_alias.clone();
        let mut current = PathBuf::new();
        for (index, component) in Path::new(relative).components().enumerate() {
            current.push(component.as_os_str());
            let relative = current.to_str().ok_or(ERROR)?.to_owned();
            if let Some(entry) = self
                .snapshot
                .entries
                .iter()
                .find(|entry| entry.relative == relative)
            {
                if index + 1 < components && !entry.directory {
                    return Err(ERROR.into());
                }
                parent_alias = entry.alias.clone();
                continue;
            }
            if self.snapshot.entries.len() >= MAX_ENTRIES {
                return Err(LIMIT.into());
            }
            let metadata =
                std::fs::symlink_metadata(self.workspace.join(&relative)).map_err(|_| ERROR)?;
            let directory = metadata.is_dir();
            if index + 1 < components && !directory {
                return Err(ERROR.into());
            }
            let counter = if directory {
                &mut self.snapshot.next_directory
            } else {
                &mut self.snapshot.next_file
            };
            let kind = if directory { 'd' } else { 'f' };
            let suffix = if directory { "" } else { extension(&relative) };
            let alias = loop {
                let number = *counter;
                *counter = counter.checked_add(1).ok_or(LIMIT)?;
                let tag = &self.snapshot.allocation_tag;
                let component = if tag.is_empty() {
                    format!("{kind}{number:03}{suffix}")
                } else {
                    format!("{kind}{tag}_{number:03}{suffix}")
                };
                let alias = format!("{parent_alias}/{component}");
                if !self.census.contains_key(&component) && !self.backward.contains_key(&alias) {
                    break alias;
                }
            };
            self.snapshot.entries.push(Entry {
                relative: relative.clone(),
                alias: alias.clone(),
                directory,
            });
            self.live.insert(relative.clone());
            if !self.disclosed_creation_leaf(&relative) {
                self.forward.insert(relative.clone(), alias.clone());
            }
            if !directory || !self.disclosed_creation_leaf(&relative) {
                self.forward.insert(format!("./{relative}"), alias.clone());
            }
            self.forward.insert(
                format!("{}/{relative}", self.workspace.to_str().ok_or(ERROR)?),
                alias.clone(),
            );
            self.backward.insert(alias.clone(), relative.clone());
            let leaf = current
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or(ERROR)?;
            if !self.disclosed_creation_leaf(leaf) {
                let projected = if self.coverage.complete()
                    && !self.census.get(leaf).copied().unwrap_or(false)
                {
                    alias.clone()
                } else {
                    self.snapshot.names.get(leaf).cloned().unwrap_or_default()
                };
                self.forward.insert(leaf.into(), projected);
            }
            self.matcher = None;
            parent_alias = alias;
        }
        Ok(())
    }

    fn register_text_paths(&mut self, text: &str) -> Result<(), String> {
        let issued = self.backward.keys().cloned().collect::<Vec<_>>();
        self.register_home_paths(text, &issued)?;
        self.register_text_paths_with(text, &issued)
    }

    fn register_home_paths(&mut self, text: &str, issued: &[String]) -> Result<(), String> {
        let Some(home) = self.home.clone() else {
            return Ok(());
        };
        let mut bindings = Vec::new();
        for original in text
            .split(|ch: char| ch.is_whitespace() || "\"'`()[]{}=<> ,;".contains(ch))
            .map(|token| token.strip_prefix('@').unwrap_or(token))
            .filter(|token| token.len() <= 4096)
            .collect::<BTreeSet<_>>()
        {
            let Some(suffix) = home_path_suffix(original) else {
                continue;
            };
            let Ok(target) = home.join(suffix).canonicalize() else {
                continue;
            };
            let Some(root) = self
                .authorized_scopes
                .iter()
                .chain(std::iter::once(&self.workspace))
                .filter(|root| target.starts_with(root))
                .max_by_key(|root| root.components().count())
                .cloned()
            else {
                continue;
            };
            validate_native_components(original, issued.iter().map(String::as_str))?;
            validate_native_components(
                target.to_str().ok_or(ERROR)?,
                issued.iter().map(String::as_str),
            )?;
            let relative = target.strip_prefix(&root).map_err(|_| ERROR)?;
            let relative = if relative.as_os_str().is_empty() {
                ".".to_owned()
            } else {
                relative.to_str().ok_or(ERROR)?.to_owned()
            };
            bindings.push((original.to_owned(), root, relative));
        }
        if bindings.is_empty() {
            return Ok(());
        }
        let mut preferred = Vec::new();
        for (original, root, relative) in bindings {
            let names = if root == self.workspace {
                &mut *self
            } else {
                let names = self.scopes.get_mut(&root).ok_or(ERROR)?;
                names.home = Some(home.clone());
                names
            };
            if relative != "." {
                names.register_relative(&relative)?;
            }
            let alias = names.source_spelling(&original, Some(relative), None)?;
            names.forward.insert(original.clone(), alias.clone());
            names.matcher = None;
            preferred.push((original, alias));
        }
        self.rebuild()?;
        // Historical spellings may bind a previously linked target. The live
        // spelling owns this projection, while old references still fail closed.
        self.forward.extend(preferred);
        Ok(())
    }

    fn register_text_paths_with(&mut self, text: &str, issued: &[String]) -> Result<(), String> {
        let candidates = text
            .split(|ch: char| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        '\'' | '"'
                            | '`'
                            | '('
                            | ')'
                            | '['
                            | ']'
                            | '{'
                            | '}'
                            | '='
                            | '<'
                            | '>'
                            | ','
                            | ';'
                    )
            })
            .map(|token| token.strip_prefix('@').unwrap_or(token))
            .filter(|token| !token.is_empty())
            .filter(|token| {
                token.contains('/') || token.contains('.') || self.census.contains_key(*token)
            })
            .collect::<BTreeSet<_>>();
        for candidate in candidates {
            if candidate.starts_with("~/") {
                continue;
            }
            let path = Path::new(candidate);
            let relative = if path.is_absolute() {
                let Some(relative) = path
                    .strip_prefix(&self.workspace)
                    .ok()
                    .and_then(Path::to_str)
                else {
                    continue;
                };
                relative
            } else {
                candidate.strip_prefix("./").unwrap_or(candidate)
            };
            if valid_relative(relative).is_err() {
                continue;
            }
            if self
                .snapshot
                .entries
                .iter()
                .any(|entry| entry.relative == relative)
            {
                continue;
            }
            if std::fs::symlink_metadata(self.workspace.join(relative)).is_ok() {
                validate_native_components(relative, issued.iter().map(String::as_str))?;
                self.register_relative(relative)?;
            }
        }
        let mut scoped_paths_changed = false;
        for scope in self.scopes.values_mut() {
            let before = scope.snapshot.entries.len();
            scope.register_text_paths_with(text, issued)?;
            scoped_paths_changed |= scope.snapshot.entries.len() != before;
        }
        if scoped_paths_changed {
            self.rebuild()?;
        }
        Ok(())
    }

    fn is_registered_path(&self, spelling: &str) -> bool {
        let path = Path::new(spelling);
        let relative = if path.is_absolute() {
            let Ok(relative) = path.strip_prefix(&self.workspace) else {
                return self
                    .scopes
                    .values()
                    .any(|scope| scope.is_registered_path(spelling));
            };
            relative
        } else {
            path
        };
        let mut normalized = PathBuf::new();
        for component in relative.components() {
            match component {
                Component::Normal(value) => normalized.push(value),
                Component::CurDir => {}
                _ => return false,
            }
        }
        normalized.to_str().is_some_and(|relative| {
            self.snapshot
                .entries
                .iter()
                .any(|entry| entry.relative == relative)
        })
    }

    /// Qualified source expressions may be descriptive without naming a live
    /// target. Never manufacture an executable directory-prefix continuation.
    fn register_descriptive_paths(&mut self, text: &str) -> Result<(), String> {
        self.ensure_matcher()?;
        let expressions = text
            .split(|ch: char| ch.is_whitespace() || matches!(ch, '\'' | '"' | '`' | '<' | '>'))
            .map(|word| {
                word.trim_start_matches(['@', '(', '['])
                    .trim_end_matches(['.', ',', ';', ':', ')', ']'])
            })
            .filter(|word| word.len() <= 4096 && word.contains('/') && !word.contains("__CA_"))
            .filter(|word| !self.forward.contains_key(*word))
            // Public creation leaves deliberately lack a raw relative forward
            // key. Their exact registered target still owns its issued form.
            .filter(|word| !self.is_registered_path(word))
            .filter(|word| {
                self.matcher.as_ref().is_some_and(|matcher| {
                    matcher.find_iter(word).any(|found| {
                        filename_match(
                            &self.patterns[found.pattern().as_usize()],
                            word,
                            found.start(),
                            found.end(),
                        )
                    })
                })
            })
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        for expression in expressions {
            let alias = self.source_spelling(&expression, None, None)?;
            self.forward.insert(expression, alias);
            self.matcher = None;
        }
        Ok(())
    }

    /// A new plain leaf was already sent by the model before its file existed.
    /// Keep that public spelling stable without exposing any private parent or
    /// declassifying a basename known before the creation was prepared.
    fn disclosed_creation_leaf(&self, name: &str) -> bool {
        if self
            .snapshot
            .public_names
            .contains(name.rsplit('/').next().unwrap_or(name))
        {
            return true;
        }
        !self.snapshot.names.contains_key(name)
            && self
                .snapshot
                .source_spellings
                .iter()
                .any(|(alias, spelling)| {
                    valid_alias_path(&self.snapshot.root_alias, alias)
                        && spelling.source.is_none()
                        && spelling.target.as_ref().is_some_and(|target| {
                            target == &spelling.original
                                && Path::new(target).file_name().and_then(|leaf| leaf.to_str())
                                    == Some(name)
                        })
                })
    }

    fn rebuild(&mut self) -> Result<(), String> {
        self.forward.clear();
        self.backward.clear();
        let root = self.workspace.to_str().ok_or(ERROR)?.to_owned();
        self.forward
            .insert(root.clone(), self.snapshot.root_alias.clone());
        self.backward
            .insert(self.snapshot.root_alias.clone(), root.clone());
        for entry in &self.snapshot.entries {
            if !self.disclosed_creation_leaf(&entry.relative) {
                self.forward
                    .insert(entry.relative.clone(), entry.alias.clone());
            }
            // A root directory's accepted ./leaf creation argument was also
            // disclosed before discovery; keep that history spelling stable.
            if !entry.directory || !self.disclosed_creation_leaf(&entry.relative) {
                self.forward
                    .insert(format!("./{}", entry.relative), entry.alias.clone());
            }
            self.forward
                .insert(format!("{root}/{}", entry.relative), entry.alias.clone());
            if self
                .backward
                .insert(entry.alias.clone(), entry.relative.clone())
                .is_some()
            {
                return Err(ERROR.into());
            }
        }
        // A unique bare basename can name its file. An ambiguous basename gets
        // a descriptive-only token; only complete path aliases are executable.
        let mut basenames: BTreeMap<&str, Vec<&Entry>> = BTreeMap::new();
        for entry in &self.snapshot.entries {
            let name = entry.relative.rsplit('/').next().ok_or(ERROR)?;
            basenames.entry(name).or_default().push(entry);
        }
        let descriptive_names = self
            .census
            .keys()
            .chain(self.snapshot.names.keys())
            .cloned()
            .chain(basenames.keys().map(|name| (*name).to_owned()))
            .collect::<BTreeSet<_>>();
        for name in &descriptive_names {
            if self.disclosed_creation_leaf(name) {
                continue;
            }
            let issued = self.snapshot.names.get(name);
            if let Some(alias) = issued {
                self.backward.insert(alias.clone(), name.clone());
            }
            if self.coverage.complete() && !self.census.get(name).copied().unwrap_or(false) {
                if let Some(entries) = basenames
                    .get(name.as_str())
                    .filter(|entries| entries.len() == 1)
                {
                    self.forward.insert(name.clone(), entries[0].alias.clone());
                    continue;
                }
            }
            self.forward
                .insert(name.clone(), issued.cloned().unwrap_or_default());
        }
        for (alias, spelling) in &self.snapshot.source_spellings {
            if spelling.home_path().is_some() {
                self.forward
                    .insert(spelling.original.clone(), alias.clone());
            } else if spelling.target.is_none()
                && spelling.source.is_none()
                && spelling.original.contains('/')
            {
                // A descriptive source expression remains private after its
                // components disappear. An actual path mapping keeps priority.
                self.forward
                    .entry(spelling.original.clone())
                    .or_insert_with(|| alias.clone());
            }
            if self
                .backward
                .insert(alias.clone(), spelling.original.clone())
                .is_some()
            {
                return Err(ERROR.into());
            }
        }
        for names in self.scopes.values() {
            for (original, alias) in &names.forward {
                // Relative spellings belong to their own scope. Bare names are
                // descriptive only; typed paths always use complete references.
                if Path::new(original).is_absolute()
                    || !original.contains('/')
                    || home_path_suffix(original).is_some()
                {
                    self.forward
                        .entry(original.clone())
                        .or_insert_with(|| alias.clone());
                }
            }
            for (alias, relative) in &names.backward {
                if names
                    .snapshot
                    .source_spellings
                    .get(alias)
                    .is_some_and(|spelling| spelling.home_path().is_some())
                {
                    self.backward.insert(alias.clone(), relative.clone());
                    continue;
                }
                if names
                    .snapshot
                    .names
                    .values()
                    .any(|name_alias| name_alias == alias)
                {
                    self.backward.insert(alias.clone(), relative.clone());
                    continue;
                }
                let native = if Path::new(relative).is_absolute() {
                    PathBuf::from(relative)
                } else {
                    names.workspace.join(relative)
                };
                self.backward
                    .insert(alias.clone(), native.to_str().ok_or(ERROR)?.into());
            }
        }
        let bytes: usize = self
            .forward
            .iter()
            .map(|(from, to)| from.len() + to.len())
            .sum();
        if bytes > MAX_BYTES {
            return Err(LIMIT.into());
        }
        self.matcher = None;
        Ok(())
    }

    fn ensure_matcher(&mut self) -> Result<(), String> {
        if self.matcher.is_none() {
            self.patterns = self.forward.keys().cloned().collect();
            self.replacements = self.forward.values().cloned().collect();
            self.matcher = Some(
                AhoCorasickBuilder::new()
                    .match_kind(MatchKind::LeftmostLongest)
                    .build(&self.patterns)
                    .map_err(|_| ERROR)?,
            );
        }
        Ok(())
    }

    fn contains_private_spelling(&mut self, text: &str) -> Result<bool, String> {
        self.ensure_matcher()?;
        Ok(self
            .matcher
            .as_ref()
            .ok_or(ERROR)?
            .find_iter(text)
            .any(|found| {
                filename_match(
                    &self.patterns[found.pattern().as_usize()],
                    text,
                    found.start(),
                    found.end(),
                )
            }))
    }

    fn issue_name(&mut self, name: &str) -> Result<String, String> {
        Ok(name.to_owned())
    }

    /// Longest-match, non-recursive projection. Already issued references are
    /// protected, while literal alias-shaped filenames use their own mapping.
    pub(crate) fn project_text(&mut self, text: &str) -> Result<String, String> {
        self.project_text_with_protected(text, &[])
    }

    pub(crate) fn project_text_with_protected(
        &mut self,
        text: &str,
        opaque: &[std::ops::Range<usize>],
    ) -> Result<String, String> {
        self.register_text_paths(text)?;
        // Inputs containing reserved model references are excluded by this
        // registration step and remain subject to reference_ranges below.
        self.register_descriptive_paths(text)?;
        self.ensure_matcher()?;
        let protected = self.reference_ranges(text)?;
        let selectors = crate::skills::qualified_selector_spans(text);
        let matches = self
            .matcher
            .as_ref()
            .ok_or(ERROR)?
            .find_iter(text)
            .filter(|found| {
                filename_match(
                    &self.patterns[found.pattern().as_usize()],
                    text,
                    found.start(),
                    found.end(),
                )
            })
            .map(|found| (found.start(), found.end(), found.pattern().as_usize()))
            .collect::<Vec<_>>();
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        for (start, end, pattern) in matches {
            if protected
                .iter()
                .any(|(left, right)| start < *right && end > *left)
                || opaque
                    .iter()
                    .any(|range| start < range.end && end > range.start)
                || selectors
                    .iter()
                    .any(|(left, right)| start < *right && end > *left)
            {
                continue;
            }
            if self.replacements[pattern].is_empty() {
                let name = self.patterns[pattern].clone();
                self.replacements[pattern] = self.issue_name(&name)?;
            }
            out.push_str(&text[copied..start]);
            out.push_str(&self.replacements[pattern]);
            copied = end;
        }
        out.push_str(&text[copied..]);
        Ok(out)
    }

    pub(crate) fn contains_aliases(&self, text: &str) -> bool {
        let _ = (self, text);
        false
    }

    /// `list_dir` expands a leading `~` before it reads. Projection has to use
    /// that same directory, or every row is a path that does not exist.
    pub(crate) fn expand_tilde(&self, path: &str) -> String {
        let Some(rest) = path.strip_prefix('~') else {
            return path.to_string();
        };
        let Some(home) = &self.home else {
            return path.to_string();
        };
        format!("{}{}", home.display(), rest)
    }

    pub(crate) fn listing_root(&self, path: &str, workspace: &Path) -> Result<PathBuf, String> {
        let path = self.expand_tilde(path);
        let root = if self.contains_aliases(&path) {
            self.resolve_path(&path)?
        } else if Path::new(&path).is_absolute() {
            PathBuf::from(path)
        } else {
            workspace.join(path)
        };
        Ok(root.canonicalize().unwrap_or(root))
    }

    /// A source reference has two identities: its exact authored spelling for
    /// edits, and its workspace target for a model-issued native read.
    pub(crate) fn source_link(
        &mut self,
        source: &str,
        original: &str,
    ) -> Result<Option<String>, String> {
        let issued = self.backward.keys().cloned().collect::<Vec<_>>();
        self.source_link_with(source, original, &issued)
    }

    fn source_link_with(
        &mut self,
        source: &str,
        original: &str,
        issued: &[String],
    ) -> Result<Option<String>, String> {
        if Path::new(source).is_absolute() {
            if let Some(root) = self
                .scopes
                .keys()
                .filter(|root| Path::new(source).starts_with(root))
                .max_by_key(|root| root.components().count())
                .cloned()
            {
                let relative = Path::new(source)
                    .strip_prefix(&root)
                    .map_err(|_| ERROR)?
                    .to_str()
                    .ok_or(ERROR)?
                    .to_owned();
                let result = self
                    .scopes
                    .get_mut(&root)
                    .ok_or(ERROR)?
                    .source_link_with(&relative, original, issued)?;
                self.rebuild()?;
                return Ok(result);
            }
        }
        let value = original.trim();
        let value = if value.len() >= 2
            && matches!(value.as_bytes()[0], b'\'' | b'"' | b'`')
            && value.as_bytes().first() == value.as_bytes().last()
        {
            &value[1..value.len() - 1]
        } else {
            value
        };
        if value.is_empty()
            || value.starts_with(['#', '?'])
            || value.contains(':')
            || value.starts_with("//")
        {
            return Ok(None);
        }
        let value = value.split(['?', '#']).next().unwrap_or(value);
        let decoded = percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .map_err(|_| ERROR)?;
        let candidate = if Path::new(decoded.as_ref()).starts_with(&self.workspace) {
            PathBuf::from(decoded.as_ref())
        } else if let Some(relative) = decoded.strip_prefix('/') {
            self.workspace.join(relative)
        } else {
            self.workspace
                .join(source)
                .parent()
                .ok_or(ERROR)?
                .join(decoded.as_ref())
        };
        // Normalize dot components lexically; native tools retain symlink and
        // real-target permission checks at execution.
        let mut normalized = PathBuf::new();
        for component in candidate.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    normalized.pop();
                }
                _ => normalized.push(component.as_os_str()),
            }
        }
        let Some(relative) = normalized
            .strip_prefix(&self.workspace)
            .ok()
            .and_then(Path::to_str)
        else {
            return Ok(None);
        };
        if std::fs::symlink_metadata(&normalized).is_ok() {
            validate_native_components(relative, issued.iter().map(String::as_str))?;
            self.register_relative(relative)?;
        }
        if !self.live.contains(relative) {
            // Historical reads retain their issued spelling after a rename or
            // move removes the old target. Execution still requires liveness.
            return Ok(self
                .snapshot
                .source_spellings
                .iter()
                .find(|(_, spelling)| {
                    spelling.original == original
                        && spelling.target.as_deref() == Some(relative)
                        && spelling.source.as_deref() == Some(source)
                })
                .map(|(alias, _)| alias.clone()));
        }
        self.source_spelling(original, Some(relative.to_owned()), Some(source))
            .map(Some)
    }

    fn source_spelling(
        &mut self,
        original: &str,
        target: Option<String>,
        source: Option<&str>,
    ) -> Result<String, String> {
        let _ = (self, target, source);
        Ok(original.to_owned())
    }

    /// Remaining filename occurrences in code get exact-spelling references,
    /// rather than losing relative/bare/absolute spelling to canonical paths.
    pub(crate) fn project_source_text(&mut self, text: &str) -> Result<String, String> {
        self.register_text_paths(text)?;
        self.register_descriptive_paths(text)?;
        self.ensure_matcher()?;
        let protected = self.reference_ranges(text)?;
        let selectors = crate::skills::qualified_selector_spans(text);
        let matches = self
            .matcher
            .as_ref()
            .ok_or(ERROR)?
            .find_iter(text)
            .filter(|found| {
                filename_match(
                    &self.patterns[found.pattern().as_usize()],
                    text,
                    found.start(),
                    found.end(),
                )
            })
            .filter(|found| {
                !protected
                    .iter()
                    .any(|(start, end)| found.start() < *end && found.end() > *start)
                    && !selectors
                        .iter()
                        .any(|(start, end)| found.start() < *end && found.end() > *start)
            })
            .map(|found| (found.start(), found.end()))
            .collect::<Vec<_>>();
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        for (start, end) in matches {
            out.push_str(&text[copied..start]);
            out.push_str(&self.source_spelling(&text[start..end], None, None)?);
            copied = end;
        }
        out.push_str(&text[copied..]);
        Ok(out)
    }

    pub(crate) fn restore_source_text(
        &self,
        text: &str,
        source: Option<&str>,
    ) -> Result<String, String> {
        let native_source = source.map(|source| self.workspace.join(source));
        let ranges = self.reference_ranges(text)?;
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        for (start, end) in ranges {
            out.push_str(&text[copied..start]);
            let reference = &text[start..end];
            let owner = self
                .scopes
                .values()
                .find(|scope| scope.snapshot.source_spellings.contains_key(reference))
                .unwrap_or(self);
            let record = owner.snapshot.source_spellings.get(reference);
            let source_record = record.filter(|record| {
                record.source.is_some() || record.target.is_none() || record.home_path().is_some()
            });
            let restored = if let Some(record) = source_record {
                match (&record.source, &record.target, native_source.as_ref()) {
                    (Some(origin), Some(target), Some(destination))
                        if owner.workspace.join(origin).parent() != destination.parent() =>
                    {
                        let original = record.original.trim();
                        let quoted = original.len() >= 2
                            && matches!(original.as_bytes()[0], b'\'' | b'"' | b'`')
                            && original.as_bytes().first() == original.as_bytes().last();
                        let body = if quoted {
                            &original[1..original.len() - 1]
                        } else {
                            original
                        };
                        let destination_root = self
                            .scopes
                            .keys()
                            .chain(std::iter::once(&self.workspace))
                            .filter(|root| destination.starts_with(root))
                            .max_by_key(|root| root.components().count());
                        if body.starts_with('/') && destination_root == Some(&owner.workspace) {
                            record.original.clone()
                        } else {
                            let base = url::Url::from_file_path(destination).map_err(|_| ERROR)?;
                            let path = owner.workspace.join(target);
                            let target = if path.is_dir() {
                                url::Url::from_directory_path(path)
                            } else {
                                url::Url::from_file_path(path)
                            }
                            .map_err(|_| ERROR)?;
                            let relative = source_relative_url(
                                source.ok_or(ERROR)?,
                                base.make_relative(&target).ok_or(ERROR)?,
                            );
                            let suffix = body.find(['?', '#']).map_or("", |start| &body[start..]);
                            if quoted {
                                format!(
                                    "{}{}{}{}",
                                    &original[..1],
                                    relative,
                                    suffix,
                                    &original[original.len() - 1..]
                                )
                            } else {
                                format!("{relative}{suffix}")
                            }
                        }
                    }
                    _ => record.original.clone(),
                }
            } else {
                let source = source.ok_or("A new link requires its target source file")?;
                let base = url::Url::from_file_path(native_source.as_ref().ok_or(ERROR)?)
                    .map_err(|_| ERROR)?;
                let path = self.resolve_path(reference)?;
                let target = if path.is_dir() {
                    url::Url::from_directory_path(path)
                } else {
                    url::Url::from_file_path(path)
                }
                .map_err(|_| ERROR)?;
                let relative =
                    source_relative_url(source, base.make_relative(&target).ok_or(ERROR)?);
                let explicit_current = text[..start].ends_with("./")
                    && (start == 2 || matches!(text.as_bytes()[start - 3], b'\'' | b'"' | b'`'));
                if explicit_current {
                    relative.strip_prefix("./").unwrap_or(&relative).to_owned()
                } else {
                    relative
                }
            };
            out.push_str(&restored);
            copied = end;
        }
        out.push_str(&text[copied..]);
        Ok(out)
    }

    /// Validate outbound content without changing it. A raw registered name
    /// outside an issued reference means a transformation was bypassed.
    #[cfg(test)]
    pub(crate) fn validate_wire_text(&mut self, text: &str) -> Result<(), String> {
        if self.project_text(text)? != text {
            return Err("A filename remains in the provider copy; request was withheld".into());
        }
        Ok(())
    }

    /// Project a typed local path without guessing from a bare basename in
    /// prose. Useful for file listings whose root entries otherwise lack './'.
    pub(crate) fn project_path(&mut self, path: &str) -> Result<String, String> {
        self.validate_native_components(path)?;
        if let Some(root) = self
            .scopes
            .keys()
            .filter(|root| Path::new(path).starts_with(root))
            .max_by_key(|root| root.components().count())
            .cloned()
        {
            let projected = self
                .scopes
                .get_mut(&root)
                .ok_or(ERROR)?
                .project_path(path)?;
            self.rebuild()?;
            return Ok(projected);
        }
        if path == "." || Path::new(path) == self.workspace {
            return Ok(self.snapshot.root_alias.clone());
        }
        let local = Path::new(path);
        let relative = if local.is_absolute() {
            local
                .strip_prefix(&self.workspace)
                .map_err(|_| ERROR)?
                .to_str()
                .ok_or(ERROR)?
        } else {
            path.strip_prefix("./").unwrap_or(path)
        };
        valid_relative(relative)?;
        self.register_relative(relative)?;
        // Historical listing rows still need projection after a supported
        // deletion. Only execution, through resolve_path, requires liveness.
        self.snapshot
            .entries
            .iter()
            .find(|entry| entry.relative == relative)
            .map(|entry| entry.alias.clone())
            .ok_or_else(|| ERROR.into())
    }

    /// Recognize only exact issued path references, including historical ones.
    /// This is projection authority, not permission to execute a removed path.
    pub(crate) fn is_known_path_alias(&self, alias: &str) -> bool {
        self.scopes
            .values()
            .any(|scope| scope.is_known_path_alias(alias))
            || self
                .snapshot
                .source_spellings
                .get(alias)
                .is_some_and(|spelling| spelling.target.is_some())
            || alias == self.snapshot.root_alias
            || (valid_alias_path(&self.snapshot.root_alias, alias)
                && self.backward.contains_key(alias))
    }

    pub(crate) fn resolve_path(&self, alias: &str) -> Result<PathBuf, String> {
        if let Some((root, scope)) = self
            .scopes
            .iter()
            .find(|(_, scope)| scope.is_known_path_alias(alias))
        {
            if !self.authorized_scopes.contains(root) {
                return Err("Folder reference needs a fresh access grant".into());
            }
            return scope.resolve_path(alias);
        }
        if alias == "." || alias == self.snapshot.root_alias {
            return Ok(self.workspace.clone());
        }
        if let Some(spelling) = self.snapshot.source_spellings.get(alias) {
            let target = spelling
                .target
                .as_ref()
                .ok_or("This source reference does not identify a file; use a discovered path")?;
            if let Some(suffix) = spelling.home_path() {
                let current = self
                    .home
                    .as_ref()
                    .ok_or("Home path is unavailable")?
                    .join(suffix)
                    .canonicalize()
                    .map_err(|_| "Home-linked path is no longer available")?;
                let expected = if target == "." {
                    self.workspace.clone()
                } else {
                    self.workspace.join(target)
                };
                if current != expected {
                    return Err("Home-linked path changed; review the current path again".into());
                }
                return Ok(current);
            }
            if !self.live.contains(target) {
                return Err("Source-linked file is no longer available".into());
            }
            return Ok(self.workspace.join(target));
        }
        if !valid_alias_path(&self.snapshot.root_alias, alias) {
            return Err(
                "Use a complete discovered filename alias; local action was blocked".into(),
            );
        }
        let relative = self
            .backward
            .get(alias)
            .ok_or("Unknown filename alias; local action was blocked")?;
        if !self.live.contains(relative) {
            return Err("Filename alias is no longer available; local action was blocked".into());
        }
        // Do not canonicalize here: symlink-target approval remains owned by
        // the actual native tool and its existing workspace/approval checks.
        Ok(self.workspace.join(relative))
    }

    /// A model may choose a new public leaf name under a discovered directory.
    /// Existing files still require their issued reference. Actual publication
    /// retains the native workspace, approval and source-drift checks.
    pub(crate) fn resolve_write_path(&mut self, reference: &str) -> Result<PathBuf, String> {
        if let Some(root) = self
            .scopes
            .iter()
            .find(|(_, scope)| reference.starts_with(&scope.snapshot.root_alias))
            .map(|(root, _)| root.clone())
        {
            if !self.authorized_scopes.contains(&root) {
                return Err("Folder reference needs a fresh access grant".into());
            }
            let path = self
                .scopes
                .get_mut(&root)
                .ok_or(ERROR)?
                .resolve_write_path(reference)?;
            self.rebuild()?;
            return Ok(path);
        }
        if let Some(spelling) = self
            .snapshot
            .source_spellings
            .get(reference)
            .filter(|spelling| spelling.source.is_none() && spelling.home_path().is_none())
        {
            if let Some(target) = &spelling.target {
                let path = self.workspace.join(target);
                if !path.exists() {
                    return Ok(path);
                }
            }
        }
        match self.resolve_path(reference) {
            Ok(path) => return Ok(path),
            Err(error) if self.backward.contains_key(reference) => return Err(error),
            Err(_) => {}
        }
        let mut candidate = reference;
        let mut leaves = Vec::new();
        let directory = loop {
            if candidate == "." || self.is_known_path_alias(candidate) {
                break self.resolve_path(candidate)?;
            }
            let (parent, leaf) = candidate
                .rsplit_once('/')
                .ok_or("Create beneath a discovered directory reference")?;
            valid_relative(leaf)?;
            if leaf.contains('/')
                || leaf.contains("__CA_")
                || self.contains_private_spelling(leaf)?
            {
                return Err(
                    "A new path requires public leaves without protected existing names".into(),
                );
            }
            leaves.push(leaf.to_owned());
            candidate = parent;
        };
        if !directory.is_dir() || leaves.is_empty() {
            return Err("New path parent is unavailable".into());
        }
        let mut path = directory;
        for leaf in leaves.iter().rev() {
            path.push(leaf);
        }
        if std::fs::symlink_metadata(&path).is_ok() {
            return Err("Use the discovered reference to edit an existing file".into());
        }
        let relative = path
            .strip_prefix(&self.workspace)
            .map_err(|_| ERROR)?
            .to_str()
            .ok_or(ERROR)?
            .to_owned();
        let parent = reference.rsplit_once('/').ok_or(ERROR)?.0;
        let parent = if parent == "." {
            self.snapshot.root_alias.as_str()
        } else {
            parent
        };
        let leaf = reference.rsplit_once('/').ok_or(ERROR)?.1;
        let mut prefix = directory_for_creation(&self.workspace, &relative, leaves.len())?;
        let mut alias_prefix = if candidate == "." {
            self.snapshot.root_alias.clone()
        } else {
            candidate.into()
        };
        for leaf in leaves.iter().rev() {
            prefix.push(leaf);
            alias_prefix.push('/');
            alias_prefix.push_str(leaf);
            let original = prefix
                .strip_prefix(&self.workspace)
                .map_err(|_| ERROR)?
                .to_str()
                .ok_or(ERROR)?
                .to_owned();
            self.snapshot
                .source_spellings
                .entry(alias_prefix.clone())
                .or_insert(SourceSpelling {
                    original: original.clone(),
                    target: Some(original),
                    source: None,
                });
        }
        let spelling = SourceSpelling {
            original: relative.clone(),
            target: Some(relative),
            source: None,
        };
        let reference = format!("{parent}/{leaf}");
        self.backward
            .insert(reference.clone(), spelling.original.clone());
        self.snapshot.source_spellings.insert(reference, spelling);
        Ok(path)
    }

    /// For local display and source-reference restoration. Path execution must
    /// call resolve_path instead; this method grants no filesystem authority.
    pub(crate) fn restore_text(&self, text: &str) -> Result<String, String> {
        self.restore_text_with_spans(text).map(|(text, _)| text)
    }

    /// One span per complete filename alias in the restored string.
    pub(crate) fn restore_text_with_spans(
        &self,
        text: &str,
    ) -> Result<(String, Vec<crate::retrieved_content::RetrievedSpan>), String> {
        let ranges = self.reference_ranges(text)?;
        let mut out = String::with_capacity(text.len());
        let mut spans = Vec::new();
        let mut copied = 0;
        for (start, end) in ranges {
            let token = &text[start..end];
            let original = self.backward.get(token).ok_or(ERROR)?;
            out.push_str(&text[copied..start]);
            let span_start = out.len();
            out.push_str(original);
            if out.len() > span_start {
                spans.push(crate::retrieved_content::RetrievedSpan {
                    source: None,
                    start: span_start,
                    end: out.len(),
                });
            }
            copied = end;
        }
        out.push_str(&text[copied..]);
        Ok((out, spans))
    }

    /// URL path slots use the target's root-relative URI spelling, rather than
    /// an absolute native path or a quoted source-code attribute literal.
    pub(crate) fn restore_url_references(&self, text: &str) -> Result<String, String> {
        let ranges = self.reference_ranges(text)?;
        let mut out = String::new();
        let mut copied = 0;
        for (start, end) in ranges {
            let reference = &text[start..end];
            let scope = self
                .scopes
                .values()
                .find(|scope| scope.is_known_path_alias(reference))
                .unwrap_or(self);
            if let Some(spelling) = scope
                .snapshot
                .source_spellings
                .get(reference)
                .filter(|spelling| spelling.target.is_none())
            {
                out.push_str(&text[copied..start]);
                out.push_str(&spelling.original);
                copied = end;
                continue;
            }
            let target = self.resolve_path(reference)?;
            let base = url::Url::from_directory_path(&scope.workspace).map_err(|_| ERROR)?;
            let target = url::Url::from_file_path(target).map_err(|_| ERROR)?;
            let path = base.make_relative(&target).ok_or(ERROR)?;
            out.push_str(&text[copied..start]);
            out.push_str(&path);
            if !text[end..].starts_with(['?', '#']) {
                if let Some(spelling) = scope.snapshot.source_spellings.get(reference) {
                    let original = spelling.original.trim_matches(['\'', '"', '`']);
                    if let Some(suffix) = original.find(['?', '#']) {
                        out.push_str(&original[suffix..]);
                    }
                }
            }
            copied = end;
        }
        out.push_str(&text[copied..]);
        Ok(out)
    }

    fn reference_ranges(&self, text: &str) -> Result<Vec<(usize, usize)>, String> {
        let _ = (self, text);
        Ok(Vec::new())
    }
}

fn directory_for_creation(
    workspace: &Path,
    relative: &str,
    new_components: usize,
) -> Result<PathBuf, String> {
    let mut path = workspace.join(relative);
    for _ in 0..new_components {
        path.pop();
    }
    Ok(path)
}

fn scope_tag(path: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

fn source_relative_url(source: &str, relative: String) -> String {
    if matches!(
        super::projection::source_format(source),
        "javascript" | "typescript" | "tsx"
    ) && !relative.is_empty()
        && !relative.starts_with("./")
        && !relative.starts_with("../")
        && !relative.starts_with('/')
    {
        format!("./{relative}")
    } else {
        relative
    }
}

fn validate_native_components<'a>(
    path: &str,
    issued: impl Iterator<Item = &'a str> + Clone,
) -> Result<(), String> {
    if path.split('/').any(|component| {
        component.contains("__CA_") && issued.clone().any(|alias| component.contains(alias))
    }) {
        return Err(
            "Filename reference conflicts with a newly discovered name; request was withheld"
                .into(),
        );
    }
    Ok(())
}

fn valid_namespace(namespace: &str) -> bool {
    !namespace.is_empty()
        && namespace.len() <= 64
        && namespace
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn home_path_suffix(spelling: &str) -> Option<&str> {
    let suffix = spelling.strip_prefix("~/")?;
    valid_relative(suffix).ok()?;
    Some(suffix)
}

fn valid_relative(relative: &str) -> Result<(), String> {
    if relative.is_empty() || relative.chars().any(char::is_control) || relative.contains('\\') {
        return Err(ERROR.into());
    }
    if Path::new(relative)
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(ERROR.into());
    }
    Ok(())
}

fn valid_alias_path(root: &str, alias: &str) -> bool {
    let Some(relative) = alias
        .strip_prefix(root)
        .and_then(|tail| tail.strip_prefix('/'))
    else {
        return false;
    };
    valid_relative(relative).is_ok()
        && relative.split('/').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        })
}

/// Metadata coverage is process-only; issued references alone enter Snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CensusCoverage {
    denied: usize,
    examples: Vec<PathBuf>,
    limited: bool,
}
impl CensusCoverage {
    fn complete(&self) -> bool {
        self.denied == 0 && !self.limited
    }
    fn denied(&mut self, directory: &Path) {
        self.denied = self.denied.saturating_add(1);
        self.examples.push(directory.into());
        self.examples.sort();
        self.examples.truncate(MAX_DISCOVERY_EXAMPLES);
    }
}

pub(crate) struct FilenameCensus {
    root: PathBuf,
    names: BTreeMap<String, bool>,
    shallow: BTreeMap<String, bool>,
    coverage: CensusCoverage,
}

pub(crate) fn collect_census(roots: &[PathBuf]) -> Result<Vec<FilenameCensus>, String> {
    roots.iter().map(|root| inventory_names(root)).collect()
}

fn remember_name(
    names: &mut HashMap<String, bool>,
    bytes: &mut usize,
    name: &str,
) -> Result<(), String> {
    valid_relative(name)?;
    if let Some(ambiguous) = names.get_mut(name) {
        *ambiguous = true;
    } else {
        *bytes = bytes.saturating_add(name.len());
        if names.len() >= MAX_ENTRIES || *bytes > MAX_BYTES {
            return Err(LIMIT.into());
        }
        names.insert(name.into(), false);
    }
    Ok(())
}

fn inventory_names(root: &Path) -> Result<FilenameCensus, String> {
    inventory_names_with(root, MAX_DESCENDANT_ENTRIES, |path| std::fs::read_dir(path))
}

pub(super) fn inventory_names_with(
    root: &Path,
    descendant_limit: usize,
    read_dir: impl Fn(&Path) -> std::io::Result<std::fs::ReadDir>,
) -> Result<FilenameCensus, String> {
    let mut names = HashMap::new();
    let mut shallow = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    let mut bytes = 0_usize;
    let mut descendants = 0_usize;
    let mut coverage = CensusCoverage::default();
    'walk: while let Some(directory) = stack.pop() {
        if directory != root && descendants >= descendant_limit {
            coverage.limited = true;
            break;
        }
        let entries = match read_dir(&directory) {
            Ok(entries) => entries,
            Err(error)
                if directory != root && error.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                // Its name was already recorded by the readable parent. The
                // inaccessible contents cannot make unrelated work unavailable.
                coverage.denied(&directory);
                continue;
            }
            Err(_) => {
                return Err("Workspace filename discovery failed; request was withheld".into())
            }
        };
        for entry in entries {
            if directory != root {
                if descendants >= descendant_limit {
                    coverage.limited = true;
                    break 'walk;
                }
                descendants += 1;
            }
            let entry = entry.map_err(|_| ERROR)?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or("Non-UTF-8 filename is unsupported; request was withheld")?;
            remember_name(&mut names, &mut bytes, name)?;
            let kind = entry.file_type().map_err(|_| ERROR)?;
            if directory == root {
                shallow.insert(name.into(), kind.is_dir());
            }
            // Complete the root first. Symlink leaves are indexed, never followed.
            if kind.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Ok(FilenameCensus {
        root: root.into(),
        names: names.into_iter().collect(),
        shallow,
        coverage,
    })
}

fn extension(path: &str) -> &str {
    let basename = path.rsplit('/').next().unwrap_or(path);
    let Some(dot) = basename.rfind('.') else {
        return "";
    };
    let suffix = &basename[dot..];
    if matches!(
        suffix,
        ".rs"
            | ".py"
            | ".js"
            | ".ts"
            | ".jsx"
            | ".tsx"
            | ".c"
            | ".h"
            | ".cc"
            | ".cpp"
            | ".hpp"
            | ".go"
            | ".java"
            | ".kt"
            | ".swift"
            | ".sh"
            | ".bash"
            | ".zsh"
            | ".toml"
            | ".json"
            | ".yaml"
            | ".yml"
            | ".md"
            | ".txt"
            | ".html"
            | ".htm"
            | ".xhtml"
            | ".css"
            | ".mjs"
            | ".cjs"
            | ".svg"
            | ".png"
            | ".jpg"
            | ".jpeg"
            | ".gif"
            | ".webp"
            | ".avif"
            | ".ico"
            | ".woff"
            | ".woff2"
            | ".ttf"
            | ".otf"
            | ".sql"
            | ".xml"
            | ".csv"
    ) {
        suffix
    } else {
        ""
    }
}

// A bare filename is a word or path component, not a substring of another
// word. Full paths, dotted names and encoded spellings retain literal matching.
fn filename_match(pattern: &str, text: &str, start: usize, end: usize) -> bool {
    !super::is_word(pattern) || (super::boundary(text, start) && super::boundary(text, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issued_name_is_the_real_name() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("kept.txt"), "a").unwrap();
        let mut aliases = FilenameAliases::new(temp.path().to_path_buf(), "preview").unwrap();
        assert_eq!(aliases.project_text("kept.txt").unwrap(), "kept.txt");
        assert!(!aliases.contains_aliases("kept.txt"));
    }
}
