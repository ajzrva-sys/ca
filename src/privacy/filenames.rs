//! Session-owned, metadata-only filename projection. This is a name boundary,
//! not a source-content anonymizer. All filesystem work belongs on a worker.
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
                root_alias: format!("__CA_PATH_{namespace}__"),
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
            || !snapshot.root_alias.starts_with("__CA_PATH_")
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
                || !alias.starts_with("__CA_NAME_")
                || !alias.ends_with("__")
                || !aliases.insert(alias.clone())
            {
                return Err(ERROR.into());
            }
            bytes = bytes.saturating_add(name.len() + alias.len());
        }
        for (alias, spelling) in &snapshot.source_spellings {
            let source_token = alias.starts_with("__CA_NAME_")
                && alias.ends_with("__")
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
            let base = format!("__CA_PATH_{}", self.snapshot.namespace);
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
        if !self.census.contains_key(name) {
            if let Some(root) = self
                .scopes
                .iter()
                .find(|(_, scope)| scope.census.contains_key(name))
                .map(|(root, _)| root.clone())
            {
                let alias = self.scopes.get_mut(&root).ok_or(ERROR)?.issue_name(name)?;
                self.forward.insert(name.into(), alias.clone());
                self.backward.insert(alias.clone(), name.into());
                return Ok(alias);
            }
        }
        if let Some(alias) = self.snapshot.names.get(name) {
            return Ok(alias.clone());
        }
        if self.snapshot.names.len() >= MAX_ENTRIES {
            return Err(LIMIT.into());
        }
        let alias = loop {
            let number = self.snapshot.next_name;
            self.snapshot.next_name = number.checked_add(1).ok_or(LIMIT)?;
            let alias = format!("__CA_NAME_{}_{number}__", self.snapshot.namespace);
            if !self.backward.contains_key(&alias)
                && !self
                    .census
                    .keys()
                    .chain(self.scopes.values().flat_map(|scope| scope.census.keys()))
                    .filter(|name| name.contains("__CA_"))
                    .any(|name| name.contains(&alias))
            {
                break alias;
            }
        };
        self.snapshot.names.insert(name.into(), alias.clone());
        self.forward.insert(name.into(), alias.clone());
        self.backward.insert(alias.clone(), name.into());
        Ok(alias)
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
        text.contains("__CA_PATH_") || text.contains("__CA_NAME_")
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
        let spelling = SourceSpelling {
            original: original.into(),
            target,
            source: source.map(str::to_owned),
        };
        let home_path = spelling.home_path().is_some();
        if let Some((alias, _)) = self
            .snapshot
            .source_spellings
            .iter()
            .find(|(alias, value)| {
                **value == spelling
                    && (!home_path || valid_alias_path(&self.snapshot.root_alias, alias))
            })
        {
            return Ok(alias.clone());
        }
        if self.snapshot.source_spellings.len() >= MAX_ENTRIES {
            return Err(LIMIT.into());
        }
        let alias = if home_path {
            let target = spelling.target.as_deref().ok_or(ERROR)?;
            let directory = std::fs::metadata(self.workspace.join(target))
                .map_err(|_| ERROR)?
                .is_dir();
            let kind = if directory { 'd' } else { 'f' };
            let suffix = if directory { "" } else { extension(target) };
            let counter = if directory {
                &mut self.snapshot.next_directory
            } else {
                &mut self.snapshot.next_file
            };
            loop {
                let number = *counter;
                *counter = counter.checked_add(1).ok_or(LIMIT)?;
                let tag = &self.snapshot.allocation_tag;
                let component = if tag.is_empty() {
                    format!("{kind}{number:03}{suffix}")
                } else {
                    format!("{kind}{tag}_{number:03}{suffix}")
                };
                let alias = format!("{}/{component}", self.snapshot.root_alias);
                if !self.census.contains_key(&component)
                    && !self.backward.contains_key(&alias)
                    && !self.forward.keys().any(|name| name.contains(&alias))
                {
                    break alias;
                }
            }
        } else {
            let mut counter = self.snapshot.source_spellings.len() + 1;
            loop {
                let alias = format!("__CA_NAME_{}_source_{counter}__", self.snapshot.namespace);
                if !self.backward.contains_key(&alias)
                    && !self.forward.keys().any(|name| name.contains(&alias))
                {
                    break alias;
                }
                counter += 1;
            }
        };
        self.backward.insert(alias.clone(), original.into());
        self.snapshot
            .source_spellings
            .insert(alias.clone(), spelling);
        Ok(alias)
    }

    /// Remaining filename occurrences in code get exact-spelling references,
    /// rather than losing relative/bare/absolute spelling to canonical paths.
    pub(crate) fn project_source_text(&mut self, text: &str) -> Result<String, String> {
        self.register_text_paths(text)?;
        self.register_descriptive_paths(text)?;
        self.ensure_matcher()?;
        let protected = self.reference_ranges(text)?;
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
        let mut ranges = Vec::new();
        let mut offset = 0;
        while offset < text.len() {
            let tail = &text[offset..];
            let starts = [tail.find("__CA_PATH_"), tail.find("__CA_NAME_")];
            let Some(relative_start) = starts.into_iter().flatten().min() else {
                break;
            };
            let start = offset + relative_start;
            // A discovered literal name can contain the reserved-looking
            // spelling. It must be projected as original data, not mistaken
            // for an invented reference. Allocation avoids exact collisions.
            if !self
                .backward
                .keys()
                .any(|token| text[start..].starts_with(token.as_str()))
            {
                if let Some(original) = self
                    .forward
                    .keys()
                    .filter(|original| text[start..].starts_with(original.as_str()))
                    .max_by_key(|original| original.len())
                {
                    offset = start + original.len();
                    continue;
                }
            }
            let matched = self
                .backward
                .keys()
                .filter(|token| text[start..].starts_with(token.as_str()))
                .max_by_key(|token| token.len())
                .ok_or("Unknown filename reference; request was withheld")?;
            let end = start + matched.len();
            // A registered directory is not a valid prefix of an invented path.
            if text[end..].starts_with('/')
                && text[end + 1..]
                    .chars()
                    .next()
                    .is_some_and(|c| !c.is_whitespace() && !"\"'`,;)]}".contains(c))
            {
                return Err(
                    "Unknown or traversing filename reference; local action was blocked".into(),
                );
            }
            ranges.push((start, end));
            offset = end;
        }
        Ok(ranges)
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

    fn fixture() -> (tempfile::TempDir, FilenameAliases) {
        let temp = tempfile::tempdir().unwrap();
        let aliases = FilenameAliases::new(temp.path().into(), "test").unwrap();
        (temp, aliases)
    }

    #[cfg(unix)]
    fn home_fixture() -> (tempfile::TempDir, PathBuf, FilenameAliases) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let home = root.join("home");
        let workspace = root.join("workspace");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&workspace).unwrap();
        std::os::unix::fs::symlink(&workspace, home.join("projects")).unwrap();
        let mut names = FilenameAliases::new(workspace, "home_test").unwrap();
        names.home = Some(home.clone());
        (temp, home, names)
    }

    #[cfg(unix)]
    #[test]
    fn home_path_root_and_nested_references_restore_and_resume() {
        let (_temp, home, mut names) = home_fixture();
        std::fs::create_dir(names.workspace.join("nested")).unwrap();
        std::fs::write(names.workspace.join("nested/private.txt"), "private").unwrap();
        names.discover().unwrap();
        let root = names.project_text("~/projects").unwrap();
        let nested = names.project_text("~/projects/nested/private.txt").unwrap();
        assert!(root.starts_with(&format!("{}/d", names.snapshot.root_alias)));
        assert!(nested.starts_with(&format!("{}/f", names.snapshot.root_alias)));
        assert!(nested.ends_with(".txt"));
        assert!(names.is_known_path_alias(&root));
        assert_eq!(names.resolve_path(&root).unwrap(), names.workspace);
        assert_eq!(
            names.resolve_path(&nested).unwrap(),
            names.workspace.join("nested/private.txt")
        );
        let original =
            "testing filter. list '~/projects' then read `~/projects/nested/private.txt`";
        let projected = names.project_text(original).unwrap();
        assert!(!projected.contains("~/"));
        assert_eq!(names.restore_text(&projected).unwrap(), original);
        assert_eq!(
            names.restore_source_text(&root, None).unwrap(),
            "~/projects"
        );
        let snapshot =
            serde_json::from_slice(&serde_json::to_vec(&names.snapshot()).unwrap()).unwrap();
        let mut resumed =
            FilenameAliases::from_snapshot(names.workspace.clone(), snapshot).unwrap();
        resumed.home = Some(home);
        resumed.discover().unwrap();
        resumed.fork_namespace("child").unwrap();
        assert_eq!(resumed.project_text("~/projects").unwrap(), root);
        assert_eq!(resumed.resolve_path(&root).unwrap(), names.workspace);
        assert_eq!(resumed.restore_text(&projected).unwrap(), original);
        assert_eq!(
            resumed.resolve_path(&nested).unwrap(),
            names.workspace.join("nested/private.txt")
        );
        assert!(
            resumed.resolve_path("~/projects").is_err(),
            "raw model paths gain no authority"
        );
    }

    #[cfg(unix)]
    #[test]
    fn home_path_prefers_typed_reference_and_retains_legacy_name_history() {
        let (_temp, home, mut names) = home_fixture();
        names.discover().unwrap();
        let legacy = "__CA_NAME_home_test_source_1__";
        names.snapshot.source_spellings.insert(
            legacy.into(),
            SourceSpelling {
                original: "~/projects".into(),
                target: Some(".".into()),
                source: None,
            },
        );
        names.rebuild().unwrap();
        let mut resumed =
            FilenameAliases::from_snapshot(names.workspace.clone(), names.snapshot()).unwrap();
        resumed.home = Some(home.clone());
        let typed = resumed.project_text("~/projects").unwrap();
        assert!(valid_alias_path(&resumed.snapshot.root_alias, &typed));
        assert_ne!(typed, legacy);
        for reference in [legacy, typed.as_str()] {
            assert_eq!(resumed.restore_text(reference).unwrap(), "~/projects");
            assert_eq!(resumed.resolve_path(reference).unwrap(), resumed.workspace);
        }
        let mut inherited =
            FilenameAliases::from_snapshot(resumed.workspace.clone(), resumed.snapshot()).unwrap();
        inherited.home = Some(home);
        inherited.fork_namespace("next_task").unwrap();
        assert_eq!(inherited.project_text("~/projects").unwrap(), typed);
        assert_eq!(inherited.restore_text(legacy).unwrap(), "~/projects");
        // A root-bound spelling is issued authority even with no child entries.
        // Its namespace cannot be renamed to accommodate a new literal name.
        let before = inherited.snapshot();
        let literal = inherited.workspace.join(&inherited.snapshot.root_alias);
        std::fs::write(&literal, "late literal namespace").unwrap();
        assert!(inherited.discover().is_err());
        assert_eq!(inherited.snapshot(), before);
    }

    #[cfg(unix)]
    #[test]
    fn home_path_scope_binding_requires_current_grant() {
        let (temp, home, mut names) = home_fixture();
        let external = temp.path().canonicalize().unwrap().join("external");
        std::fs::create_dir(&external).unwrap();
        std::fs::write(external.join("private.txt"), "private").unwrap();
        std::os::unix::fs::symlink(&external, home.join("outside")).unwrap();
        names.discover().unwrap();
        assert_eq!(names.project_text("~/outside").unwrap(), "~/outside");
        assert!(names.authorized_scopes.is_empty());
        names
            .authorize_roots(std::slice::from_ref(&external))
            .unwrap();
        names.discover().unwrap();
        let root = names.project_text("~/outside").unwrap();
        let file = names.project_text("~/outside/private.txt").unwrap();
        assert_eq!(names.resolve_path(&root).unwrap(), external);
        assert_eq!(names.restore_text(&file).unwrap(), "~/outside/private.txt");
        let mut resumed =
            FilenameAliases::from_snapshot(names.workspace.clone(), names.snapshot()).unwrap();
        resumed.home = Some(home.clone());
        for scope in resumed.scopes.values_mut() {
            scope.home = Some(home.clone());
        }
        assert!(
            resumed.resolve_path(&root).is_err(),
            "snapshot grants no access"
        );
        resumed
            .authorize_roots(std::slice::from_ref(&external))
            .unwrap();
        resumed.discover().unwrap();
        assert_eq!(
            resumed.resolve_path(&file).unwrap(),
            external.join("private.txt")
        );
        resumed.authorize_roots(&[]).unwrap();
        assert!(resumed.resolve_path(&root).is_err());
        assert!(resumed.resolve_write_path(&file).is_err());
        assert_eq!(
            resumed.restore_text(&file).unwrap(),
            "~/outside/private.txt"
        );
    }

    #[cfg(unix)]
    #[test]
    fn home_path_stale_links_and_traversal_never_gain_execution() {
        let (temp, home, mut names) = home_fixture();
        std::fs::create_dir(names.workspace.join("nested")).unwrap();
        std::fs::write(names.workspace.join("nested/private.txt"), "private").unwrap();
        names.discover().unwrap();
        let root = names.project_text("~/projects").unwrap();
        let file = names.project_text("~/projects/nested/private.txt").unwrap();
        let outside = temp.path().canonicalize().unwrap().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::remove_file(home.join("projects")).unwrap();
        std::os::unix::fs::symlink(&outside, home.join("projects")).unwrap();
        assert!(names.resolve_path(&root).is_err());
        assert!(names.resolve_path(&file).is_err());
        assert!(names.resolve_write_path(&file).is_err());
        assert_eq!(names.restore_text(&root).unwrap(), "~/projects");
        assert!(names.authorized_scopes.is_empty());
        std::fs::remove_file(home.join("projects")).unwrap();
        assert!(names.resolve_path(&root).is_err());
        std::os::unix::fs::symlink(&names.workspace, home.join("projects")).unwrap();
        assert_eq!(names.resolve_path(&root).unwrap(), names.workspace);
        std::fs::remove_file(names.workspace.join("nested/private.txt")).unwrap();
        assert!(
            names.resolve_write_path(&file).is_err(),
            "a stale home path cannot recreate its target"
        );
        for spelling in [
            "~/../workspace",
            "~/projects/../outside",
            "~other/projects",
            "~//outside",
        ] {
            let projected = names.project_text(spelling).unwrap();
            assert!(names.resolve_path(&projected).is_err(), "{spelling}");
        }
        assert!(names.resolve_path("__CA_NAME_invented_source_1__").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn home_path_unscanned_literal_reference_collisions_are_atomic() {
        let (_temp, _home, mut names) = home_fixture();
        std::fs::create_dir(names.workspace.join("deep")).unwrap();
        std::fs::write(names.workspace.join("deep/private.txt"), "private").unwrap();
        names.discover().unwrap();
        let issued = names.project_text("private.txt").unwrap();
        for literal in [issued, names.snapshot.root_alias.clone()] {
            std::fs::write(names.workspace.join("deep").join(&literal), "collision").unwrap();
            let before = names.snapshot();
            assert!(names
                .project_text(&format!("~/projects/deep/{literal}"))
                .is_err());
            assert_eq!(names.snapshot(), before);
            std::fs::remove_file(names.workspace.join("deep").join(&literal)).unwrap();
        }
    }

    #[test]
    fn filename_words_do_not_reclassify_other_words_or_published_copy_leaves() {
        let (root, mut names) = fixture();
        let prose = "stdout outer output without about timeout_secs build target";
        names.declare_public_names(prose).unwrap();
        let copy = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(copy.path().join("target/debug/build/out")).unwrap();
        names.authorize_roots(&[copy.path().into()]).unwrap();
        names.discover().unwrap();
        assert_eq!(names.project_text(prose).unwrap(), prose);
        let code = "function output() { return stdout + outer + timeout_secs; }";
        assert_eq!(names.project_source_text(code).unwrap(), code);
        let ordinary_path = "folder/stdout/notes";
        assert_eq!(names.project_text(ordinary_path).unwrap(), ordinary_path);
        assert_eq!(
            names.project_source_text(ordinary_path).unwrap(),
            ordinary_path
        );
        let private_path = names.project_text("folder/out/notes").unwrap();
        assert_ne!(private_path, "folder/out/notes");
        assert!(
            names.resolve_path(&private_path).is_err(),
            "descriptive paths grant no executable target"
        );
        for relative in ["target", "target/debug/build", "target/debug/build/out"] {
            let native = copy.path().join(relative);
            let alias = names.project_path(native.to_str().unwrap()).unwrap();
            assert!(alias.starts_with("__CA_PATH_"));
            assert_eq!(names.resolve_path(&alias).unwrap(), native);
        }
        assert_ne!(names.project_text("out").unwrap(), "out");
        assert!(!names.project_text("/out/").unwrap().contains("/out/"));
        std::fs::write(root.path().join("PrivateLeaf317.rs"), "private").unwrap();
        names.discover().unwrap();
        assert!(!names
            .project_text("prefixPrivateLeaf317.rsSuffix")
            .unwrap()
            .contains("PrivateLeaf317.rs"));
    }

    #[test]
    fn filename_census_repeated_metadata_exceeds_old_entry_count_without_snapshot_growth() {
        let mut names = HashMap::new();
        let mut bytes = 0;
        for _ in 0..MAX_ENTRIES + 1 {
            remember_name(&mut names, &mut bytes, "reused-private.rs").unwrap();
        }
        assert_eq!(names.len(), 1);
        assert_eq!(bytes, "reused-private.rs".len());
        assert!(names["reused-private.rs"]);
        assert!(remember_name(&mut names, &mut bytes, "unsupported\\name").is_err());
    }

    #[test]
    fn filename_aliases_symbolic_authored_paths_are_descriptive_and_never_executable() {
        let (root, mut names) = fixture();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::create_dir(root.path().join("tests")).unwrap();
        std::fs::write(root.path().join("src/config.rs"), "source").unwrap();
        std::fs::create_dir_all(root.path().join("templates/.agent")).unwrap();
        std::fs::write(
            root.path().join("templates/.agent/PLANS.md"),
            "unrelated plan",
        )
        .unwrap();
        names.discover().unwrap();
        let original =
            "See `tests/*.ca`, `src/{config,session,context,approval,undo,markdown,test}.rs` and `.agent/PLANS.md`.";
        let projected = names.project_text(original).unwrap();
        names.validate_wire_text(&projected).unwrap();
        assert_eq!(names.restore_text(&projected).unwrap(), original);
        assert!(
            !projected.contains("tests/")
                && !projected.contains("src/")
                && !projected.contains(".agent/")
        );
        let references = names.reference_ranges(&projected).unwrap();
        assert_eq!(references.len(), 3);
        for (start, end) in references {
            let reference = &projected[start..end];
            assert!(reference.starts_with("__CA_NAME_"));
            assert!(names.resolve_path(reference).is_err());
            assert!(names.resolve_write_path(reference).is_err());
        }
        let directory = names.project_path("tests").unwrap();
        assert!(names.project_text(&format!("{directory}/*.ca")).is_err());
        assert!(names
            .project_text(&format!("{directory}/../outside.rs"))
            .is_err());
        std::fs::remove_dir_all(root.path().join("templates/.agent")).unwrap();
        names.discover().unwrap();
        assert_eq!(names.project_text(original).unwrap(), projected);
        assert_eq!(names.restore_text(&projected).unwrap(), original);
        let mut loaded =
            FilenameAliases::from_snapshot(root.path().into(), names.snapshot()).unwrap();
        loaded.discover().unwrap();
        assert_eq!(loaded.project_text(original).unwrap(), projected);
        assert_eq!(loaded.restore_text(&projected).unwrap(), original);
        for (start, end) in loaded.reference_ranges(&projected).unwrap() {
            assert!(loaded.resolve_path(&projected[start..end]).is_err());
            assert!(loaded.resolve_write_path(&projected[start..end]).is_err());
        }
    }

    #[test]
    fn filename_census_masks_unvisited_names_and_preserves_deferred_links_globs_resume_deletion() {
        let (root, mut names) = fixture();
        std::fs::create_dir_all(root.path().join("target/.hidden/deep")).unwrap();
        let file = root
            .path()
            .join("target/.hidden/deep/PrivateDeferred317.rs");
        std::fs::write(&file, "original source").unwrap();
        names.discover().unwrap();
        assert_eq!(
            names.snapshot.entries.len(),
            1,
            "only shallow browsing is allocated"
        );
        assert!(
            names.snapshot.names.is_empty(),
            "the global census is not persisted"
        );
        let prose = names
            .project_text("Discuss PrivateDeferred317.rs before visiting it")
            .unwrap();
        assert!(!prose.contains("PrivateDeferred317.rs"));
        assert_eq!(names.snapshot.names.len(), 1);
        let descriptive = names.snapshot.names["PrivateDeferred317.rs"].clone();
        let alias = names
            .project_path("target/.hidden/deep/PrivateDeferred317.rs")
            .unwrap();
        assert_eq!(names.resolve_path(&alias).unwrap(), file);
        assert_eq!(names.resolve_write_path(&alias).unwrap(), file);
        let link = names
            .source_link("index.html", "target/.hidden/deep/PrivateDeferred317.rs")
            .unwrap()
            .unwrap();
        assert_eq!(names.resolve_path(&link).unwrap(), file);
        assert!(names
            .matching_files(&root.path().join("target"), "*.rs")
            .unwrap()
            .contains(&file));
        let snapshot = names.snapshot();
        assert!(serde_json::to_vec(&snapshot).unwrap().len() < 8 * 1024 * 1024);
        let mut loaded = FilenameAliases::from_snapshot(root.path().into(), snapshot).unwrap();
        loaded.discover().unwrap();
        assert_eq!(loaded.resolve_path(&alias).unwrap(), file);
        assert_eq!(
            loaded.restore_text(&descriptive).unwrap(),
            "PrivateDeferred317.rs"
        );
        std::fs::remove_file(&file).unwrap();
        loaded.discover().unwrap();
        assert!(loaded.resolve_path(&alias).is_err());
        assert!(loaded.resolve_path(&link).is_err());
        assert_eq!(
            loaded
                .project_path("target/.hidden/deep/PrivateDeferred317.rs")
                .unwrap(),
            alias
        );
    }

    #[test]
    fn filename_census_scoped_lazy_names_do_not_collide_with_literal_foreign_aliases() {
        let (root, mut names) = fixture();
        std::fs::create_dir(root.path().join("deep")).unwrap();
        std::fs::write(root.path().join("deep/RootPrivate317.rs"), "x").unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::create_dir(other.path().join("deep")).unwrap();
        std::fs::write(other.path().join("__CA_NAME_test_1__"), "literal").unwrap();
        std::fs::write(other.path().join("deep/ForeignPrivate317.rs"), "x").unwrap();
        names
            .authorize_roots(&[other.path().to_path_buf()])
            .unwrap();
        names.discover().unwrap();
        let root_alias = names.project_text("RootPrivate317.rs").unwrap();
        assert_ne!(root_alias, "__CA_NAME_test_1__");
        let foreign = names.project_text("ForeignPrivate317.rs").unwrap();
        assert!(!foreign.contains("ForeignPrivate317.rs"));
        assert_ne!(foreign, root_alias);
        assert_eq!(
            names.restore_text(&foreign).unwrap(),
            "ForeignPrivate317.rs"
        );
        let mut loaded =
            FilenameAliases::from_snapshot(root.path().into(), names.snapshot()).unwrap();
        loaded
            .authorize_roots(&[other.path().to_path_buf()])
            .unwrap();
        loaded.discover().unwrap();
        assert_eq!(
            loaded.restore_text(&foreign).unwrap(),
            "ForeignPrivate317.rs"
        );
    }

    #[test]
    fn filename_census_new_literal_issued_name_collision_is_transactionally_withheld() {
        let (root, mut names) = fixture();
        std::fs::create_dir_all(root.path().join("deep")).unwrap();
        std::fs::write(root.path().join("deep/PrivateBasename317.rs"), "x").unwrap();
        names.discover().unwrap();
        let issued = names.project_text("PrivateBasename317.rs").unwrap();
        assert!(issued.starts_with("__CA_NAME_"));
        let before = names.snapshot();
        std::fs::write(root.path().join(&issued), "literal conflicting name").unwrap();
        assert!(names.discover().is_err());
        assert_eq!(names.snapshot(), before);
    }

    #[test]
    fn whole_site_source_spelling_snapshot_fork_and_target_identity() {
        let (root, mut names) = fixture();
        std::fs::create_dir(root.path().join("pages")).unwrap();
        std::fs::create_dir(root.path().join("assets")).unwrap();
        std::fs::write(root.path().join("assets/private logo.svg"), "<svg/>").unwrap();
        names.discover().unwrap();
        let spelling = "../assets/private%20logo.svg?size=2#mark";
        let token = names
            .source_link("pages/index.html", spelling)
            .unwrap()
            .unwrap();
        assert_eq!(names.restore_source_text(&token, None).unwrap(), spelling);
        let folder = names
            .source_link("pages/index.html", "../assets/")
            .unwrap()
            .unwrap();
        assert_eq!(
            names
                .restore_source_text(&folder, Some("index.html"))
                .unwrap(),
            "assets/"
        );
        assert_eq!(
            names.resolve_path(&token).unwrap(),
            root.path().join("assets/private logo.svg")
        );
        let snapshot = names.snapshot();
        let mut loaded = FilenameAliases::from_snapshot(root.path().into(), snapshot).unwrap();
        loaded.discover().unwrap();
        assert_eq!(
            loaded
                .source_link("pages/index.html", spelling)
                .unwrap()
                .unwrap(),
            token
        );
        loaded.fork_namespace("child").unwrap();
        assert_eq!(loaded.restore_source_text(&token, None).unwrap(), spelling);
        assert_eq!(
            loaded.resolve_path(&token).unwrap(),
            root.path().join("assets/private logo.svg")
        );
        assert!(loaded
            .source_link("pages/index.html", "../../outside.svg")
            .unwrap()
            .is_none());
        let unknown = "__CA_NAME_unknown_source_9__";
        assert!(loaded.restore_source_text(unknown, None).is_err());
        std::fs::remove_file(root.path().join("assets/private logo.svg")).unwrap();
        loaded.discover().unwrap();
        assert!(loaded.resolve_path(&token).is_err());
        assert_eq!(loaded.restore_source_text(&token, None).unwrap(), spelling);
    }

    #[test]
    fn whole_site_created_root_file_snapshot_reloads() {
        let (root, mut names) = fixture();
        names.discover().unwrap();
        let path = names.resolve_write_path("./new.css").unwrap();
        assert_eq!(path, root.path().join("new.css"));
        std::fs::write(&path, ".card { color: blue; }").unwrap();
        names.discover().unwrap();
        let mut loaded =
            FilenameAliases::from_snapshot(root.path().into(), names.snapshot()).unwrap();
        loaded.discover().unwrap();
        let alias = loaded.project_path("new.css").unwrap();
        assert_eq!(loaded.resolve_path(&alias).unwrap(), path);
        assert_eq!(loaded.project_text("new.css").unwrap(), "new.css");
        assert!(!loaded
            .project_text("./new.css")
            .unwrap()
            .contains("new.css"));
    }

    #[test]
    fn whole_site_deleted_alias_cannot_create_a_synthetic_filename() {
        let (root, mut names) = fixture();
        let path = root.path().join("private.css");
        std::fs::write(&path, "a{}").unwrap();
        names.discover().unwrap();
        let alias = names.project_path("private.css").unwrap();
        std::fs::remove_file(path).unwrap();
        names.discover().unwrap();
        let before = names.snapshot();
        assert!(names.resolve_write_path(&alias).is_err());
        assert_eq!(names.snapshot(), before);
    }

    #[test]
    fn filename_aliases_metadata_round_trip_duplicates_unicode_and_case() {
        let (temp, mut aliases) = fixture();
        for path in [
            "secret-one/customer.rs",
            "secret-two/customer.rs",
            "秘密/École.rs",
            "Case.rs",
            "case.rs",
        ] {
            std::fs::create_dir_all(temp.path().join(path).parent().unwrap()).unwrap();
            std::fs::write(temp.path().join(path), "untouched source").unwrap();
        }
        aliases.discover().unwrap();
        let left = aliases.project_text("secret-one/customer.rs").unwrap();
        let right = aliases.project_text("secret-two/customer.rs").unwrap();
        assert_ne!(left, right);
        assert!(left.ends_with(".rs"));
        assert!(!left.contains("secret-one") && !left.contains("customer"));
        assert_eq!(
            aliases.resolve_path(&left).unwrap(),
            temp.path()
                .canonicalize()
                .unwrap()
                .join("secret-one/customer.rs")
        );
        assert_eq!(
            aliases.restore_text(&left).unwrap(),
            "secret-one/customer.rs"
        );
        let unicode = aliases.project_text("秘密/École.rs").unwrap();
        assert_eq!(aliases.restore_text(&unicode).unwrap(), "秘密/École.rs");
        assert_eq!(
            std::fs::read_to_string(aliases.resolve_path(&unicode).unwrap()).unwrap(),
            "untouched source"
        );
        assert_eq!(
            aliases.project_text(&format!("{left} {right}")).unwrap(),
            format!("{left} {right}")
        );
        let basename = aliases.project_text("customer.rs").unwrap();
        assert!(aliases.resolve_path(&basename).is_err());
        assert_eq!(aliases.restore_text(&basename).unwrap(), "customer.rs");
    }

    #[test]
    fn filename_aliases_snapshot_stable_incremental_and_forked() {
        let (temp, mut aliases) = fixture();
        std::fs::write(temp.path().join("original.rs"), "a").unwrap();
        assert!(aliases.discover().unwrap());
        let original = aliases.project_text("original.rs").unwrap();
        let json = serde_json::to_string(&aliases.snapshot()).unwrap();
        let mut resumed = FilenameAliases::from_snapshot(
            temp.path().into(),
            serde_json::from_str(&json).unwrap(),
        )
        .unwrap();
        resumed.discover().unwrap();
        assert_eq!(resumed.project_text("original.rs").unwrap(), original);
        resumed.fork_namespace("fork").unwrap();
        std::fs::write(temp.path().join("another.rs"), "b").unwrap();
        resumed.discover().unwrap();
        aliases.discover().unwrap();
        assert_eq!(resumed.project_text("original.rs").unwrap(), original);
        assert_ne!(
            resumed.project_text("another.rs").unwrap(),
            aliases.project_text("another.rs").unwrap()
        );
    }

    #[test]
    fn filename_aliases_block_unknown_traversal_creation_and_deleted_paths() {
        let (temp, mut aliases) = fixture();
        std::fs::write(temp.path().join("private.rs"), "a").unwrap();
        aliases.discover().unwrap();
        let alias = aliases.project_text("private.rs").unwrap();
        for bad in [
            format!("{alias}/../f999.rs"),
            "../outside.rs".into(),
            "__CA_PATH_test__/f999.rs".into(),
            "private.rs".into(),
        ] {
            assert!(aliases.resolve_path(&bad).is_err());
        }
        assert!(aliases.project_text("__CA_PATH_test__/f999.rs").is_err());
        std::fs::remove_file(temp.path().join("private.rs")).unwrap();
        aliases.discover().unwrap();
        assert!(aliases.resolve_path(&alias).is_err());
        assert_eq!(aliases.project_text("private.rs").unwrap(), alias);
    }

    #[test]
    fn filename_aliases_reserve_literal_alias_names_and_do_not_rewrite_insertions() {
        let (temp, mut aliases) = fixture();
        std::fs::create_dir(temp.path().join("__CA_PATH_test__")).unwrap();
        std::fs::write(temp.path().join("__CA_PATH_test__/f001.rs"), "a").unwrap();
        std::fs::write(temp.path().join("f001.rs"), "b").unwrap();
        std::fs::write(temp.path().join("__CA_NAME_test_1__"), "c").unwrap();
        aliases.discover().unwrap();
        let alias = aliases.project_text("__CA_PATH_test__/f001.rs").unwrap();
        assert!(alias.starts_with("__CA_PATH_test_1__/"));
        assert_eq!(
            aliases.resolve_path(&alias).unwrap(),
            temp.path()
                .canonicalize()
                .unwrap()
                .join("__CA_PATH_test__/f001.rs")
        );
        assert_eq!(aliases.project_text(&alias).unwrap(), alias);
        let name_alias = aliases.project_path("__CA_NAME_test_1__").unwrap();
        assert_eq!(
            aliases.project_text("__CA_NAME_test_1__").unwrap(),
            name_alias
        );
    }

    #[cfg(unix)]
    #[test]
    fn filename_aliases_symlinks_are_lexical_and_not_traversed() {
        let (temp, mut aliases) = fixture();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("must-not-discover.rs"), "a").unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("secret-link")).unwrap();
        aliases.discover().unwrap();
        assert_eq!(
            aliases.project_text("must-not-discover.rs").unwrap(),
            "must-not-discover.rs"
        );
        let alias = aliases.project_text("secret-link").unwrap();
        assert_eq!(
            aliases.resolve_path(&alias).unwrap(),
            temp.path().canonicalize().unwrap().join("secret-link")
        );
        assert_eq!(aliases.restore_text(&alias).unwrap(), "secret-link");
    }

    #[test]
    fn filename_aliases_wire_check_and_atomic_discovery_failure() {
        let (temp, mut aliases) = fixture();
        std::fs::write(temp.path().join("sensitive.rs"), "a").unwrap();
        aliases.discover().unwrap();
        let before = aliases.snapshot();
        assert!(aliases.validate_wire_text("open sensitive.rs").is_err());
        let projected = aliases.project_text("open sensitive.rs").unwrap();
        aliases.validate_wire_text(&projected).unwrap();
        let mut corrupt = aliases.snapshot();
        corrupt.entries[0].relative = "../outside".into();
        assert!(FilenameAliases::from_snapshot(temp.path().into(), corrupt).is_err());
        assert_eq!(aliases.snapshot(), before);
        #[cfg(unix)]
        {
            std::fs::write(temp.path().join("unsupported\\name.rs"), "a").unwrap();
            assert!(aliases.discover().is_err());
            assert_eq!(
                aliases.snapshot(),
                before,
                "failed discovery must not allocate partial aliases"
            );
        }
    }

    #[test]
    fn filename_aliases_keep_case_and_unicode_normalization_identities_distinct() {
        let (temp, aliases) = fixture();
        let mut snapshot = aliases.snapshot();
        for (index, relative) in ["Case.rs", "case.rs", "é.rs", "e\u{301}.rs"]
            .iter()
            .enumerate()
        {
            snapshot.entries.push(Entry {
                relative: (*relative).into(),
                alias: format!("__CA_PATH_test__/f{:03}.rs", index + 1),
                directory: false,
            });
        }
        let mut aliases = FilenameAliases::from_snapshot(temp.path().into(), snapshot).unwrap();
        let mut identities = BTreeSet::new();
        for spelling in ["Case.rs", "case.rs", "é.rs", "e\u{301}.rs"] {
            let alias = aliases.project_text(spelling).unwrap();
            assert!(identities.insert(alias.clone()));
            assert_eq!(aliases.restore_text(&alias).unwrap(), spelling);
        }
    }

    #[test]
    fn filename_census_permission_gap_preserves_siblings_and_recovers() {
        let (root, mut names) = fixture();
        let blocked = root.path().join("private-folder");
        let sibling = root.path().join("readable.rs");
        std::fs::create_dir(&blocked).unwrap();
        let blocked = blocked.canonicalize().unwrap();
        std::fs::write(&sibling, "safe").unwrap();
        names.discover().unwrap();
        let alias = names.project_path("readable.rs").unwrap();
        let census = inventory_names_with(&names.workspace, MAX_DESCENDANT_ENTRIES, |path| {
            if path == blocked {
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            } else {
                std::fs::read_dir(path)
            }
        })
        .unwrap();
        assert_eq!(census.names, names.census, "only coverage changed");
        assert!(names.apply_census(vec![census]).unwrap());
        assert!(names.coverage_note().contains("1 inaccessible folder"));
        assert_eq!(
            names.resolve_path(&alias).unwrap(),
            sibling.canonicalize().unwrap()
        );
        assert_eq!(names.restore_text(&alias).unwrap(), "readable.rs");
        let bare = names.project_text("readable.rs").unwrap();
        assert_ne!(bare, "readable.rs");
        assert!(
            names.resolve_path(&bare).is_err(),
            "incomplete discovery cannot prove a basename unique"
        );
        assert_eq!(names.restore_text(&bare).unwrap(), "readable.rs");
        assert!(names.discover().unwrap());
        assert!(names.coverage_note().is_empty());
        assert_eq!(names.project_text("readable.rs").unwrap(), alias);
        assert_eq!(names.restore_text(&bare).unwrap(), "readable.rs");
        assert!(
            inventory_names_with(&names.workspace, MAX_DESCENDANT_ENTRIES, |_| {
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            })
            .err()
            .unwrap()
            .contains("Workspace filename discovery failed")
        );
    }

    #[test]
    fn filename_census_budget_counts_repeated_entries_and_keeps_exact_paths() {
        let (root, mut names) = fixture();
        for folder in ["alpha", "beta", "gamma"] {
            std::fs::create_dir(root.path().join(folder)).unwrap();
            for file in ["repeated.rs", "repeated.txt"] {
                std::fs::write(root.path().join(folder).join(file), "x").unwrap();
            }
        }
        let census =
            inventory_names_with(&names.workspace, 3, |path| std::fs::read_dir(path)).unwrap();
        assert!(census.coverage.limited);
        assert_eq!(census.shallow.len(), 3, "root enumeration remains complete");
        assert_eq!(
            census.names.len(),
            5,
            "duplicate names still consume work budget"
        );
        names.apply_census(vec![census]).unwrap();
        assert!(names.coverage_note().contains("25,000"));
        let typed = names.project_path("alpha/repeated.rs").unwrap();
        assert_eq!(
            names.resolve_path(&typed).unwrap(),
            names.workspace.join("alpha/repeated.rs")
        );
        assert_eq!(
            names.resolve_write_path(&typed).unwrap(),
            names.workspace.join("alpha/repeated.rs")
        );
        assert_eq!(names.restore_text(&typed).unwrap(), "alpha/repeated.rs");
        let bare = names.project_text("repeated.rs").unwrap();
        assert!(bare.starts_with("__CA_NAME_"));
        assert!(names.resolve_path(&bare).is_err());
        let mut resumed =
            FilenameAliases::from_snapshot(names.workspace.clone(), names.snapshot()).unwrap();
        resumed.discover().unwrap();
        assert_eq!(
            resumed.resolve_path(&typed).unwrap(),
            names.workspace.join("alpha/repeated.rs")
        );
        assert_eq!(resumed.restore_text(&bare).unwrap(), "repeated.rs");
        assert!(resumed.coverage_note().is_empty());
    }

    #[test]
    fn filename_census_partial_late_root_leaf_stays_descriptive() {
        let (root, mut names) = fixture();
        std::fs::create_dir(root.path().join("deep")).unwrap();
        std::fs::write(root.path().join("late.rs"), "root").unwrap();
        let mut census =
            inventory_names_with(&names.workspace, 0, |path| std::fs::read_dir(path)).unwrap();
        // Simulate a root leaf beyond the shallow path-allocation window.
        census.shallow.remove("late.rs");
        names.apply_census(vec![census]).unwrap();
        assert!(!names
            .snapshot
            .entries
            .iter()
            .any(|entry| entry.relative == "late.rs"));
        let bare = names.project_text("late.rs").unwrap();
        assert!(bare.starts_with("__CA_NAME_"));
        assert!(names.resolve_path(&bare).is_err());
        assert_eq!(names.restore_text(&bare).unwrap(), "late.rs");
        let exact = names.project_text("./late.rs").unwrap();
        assert_eq!(
            names.resolve_path(&exact).unwrap(),
            names.workspace.join("late.rs")
        );
        assert_eq!(names.project_path("late.rs").unwrap(), exact);
    }

    #[test]
    fn filename_census_partial_on_demand_literal_collisions_are_atomic_across_scopes() {
        let (root, mut names) = fixture();
        std::fs::write(root.path().join("known.rs"), "x").unwrap();
        std::fs::create_dir(root.path().join("deep")).unwrap();
        let census =
            inventory_names_with(&names.workspace, 0, |path| std::fs::read_dir(path)).unwrap();
        names.apply_census(vec![census]).unwrap();
        let token = names.project_text("known.rs").unwrap();
        assert!(token.starts_with("__CA_NAME_"));
        let original_root = names.snapshot.root_alias.clone();
        for literal in [&token, &original_root] {
            let path = format!("deep/{literal}");
            std::fs::write(root.path().join(&path), "literal").unwrap();
            let before = names.snapshot();
            assert!(names.project_path(&path).is_err());
            assert!(names.source_link("index.html", &path).is_err());
            assert!(names.project_text(&path).is_err());
            assert_eq!(names.snapshot(), before);
        }
        let external = tempfile::tempdir().unwrap();
        let external = external.path().canonicalize().unwrap();
        std::fs::create_dir(external.join("deep")).unwrap();
        std::fs::write(external.join("deep").join(&token), "external literal").unwrap();
        names
            .authorize_roots(std::slice::from_ref(&external))
            .unwrap();
        // Refresh only shallow metadata; the conflicting native leaf is unseen.
        let census = names
            .census_roots()
            .iter()
            .map(|root| inventory_names_with(root, 0, |path| std::fs::read_dir(path)).unwrap())
            .collect();
        names.apply_census(census).unwrap();
        let before = names.snapshot();
        let target = external.join("deep").join(&token);
        assert!(names.project_path(target.to_str().unwrap()).is_err());
        assert!(names.project_text(target.to_str().unwrap()).is_err());
        assert!(names
            .source_link(
                external.join("index.html").to_str().unwrap(),
                &format!("deep/{token}")
            )
            .is_err());
        assert_eq!(names.snapshot(), before);
        assert_eq!(names.restore_text(&token).unwrap(), "known.rs");
    }
}
