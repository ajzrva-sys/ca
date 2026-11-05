//! Session-owned, metadata-only filename projection. This is a name boundary,
//! not a source-content anonymizer. All filesystem work belongs on a worker.
use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

const VERSION: u32 = 2;
const MAX_ENTRIES: usize = 262_144;
const MAX_BYTES: usize = 256 * 1024 * 1024;
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
    snapshot: Snapshot,
    /// Current metadata, distinct from historical mappings retained for replay.
    live: BTreeSet<String>,
    forward: BTreeMap<String, String>,
    backward: BTreeMap<String, String>,
    matcher: Option<AhoCorasick>,
    replacements: Vec<String>,
    scopes: BTreeMap<PathBuf, FilenameAliases>,
    authorized_scopes: BTreeSet<PathBuf>,
}

impl FilenameAliases {
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
                valid_relative(target)?;
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
            snapshot,
            live: BTreeSet::new(),
            forward: BTreeMap::new(),
            backward: BTreeMap::new(),
            matcher: None,
            replacements: Vec::new(),
            scopes,
            authorized_scopes: BTreeSet::new(),
        };
        this.rebuild()?;
        Ok(this)
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
                scope.snapshot.public_names = self.snapshot.public_names.clone();
                self.scopes.insert(root.clone(), scope);
            }
            active.insert(root);
        }
        self.authorized_scopes = active;
        for root in &self.authorized_scopes {
            self.scopes.get_mut(root).ok_or(ERROR)?.discover()?;
        }
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
        let mut scope = Self::new(root.clone(), &namespace)?;
        scope.discover()?;
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

    pub(crate) fn path_alias(&self, path: &Path) -> Result<String, String> {
        self.project_path(path.to_str().ok_or(ERROR)?)
    }

    pub(crate) fn matching_files(
        &self,
        root: &Path,
        pattern: &str,
    ) -> Result<BTreeSet<PathBuf>, String> {
        let pattern = glob::Pattern::new(pattern).map_err(|_| "Invalid projected filename glob")?;
        let mut matches = BTreeSet::new();
        for scope in std::iter::once(self).chain(
            self.scopes
                .iter()
                .filter(|(root, _)| self.authorized_scopes.contains(*root))
                .map(|(_, scope)| scope),
        ) {
            for entry in scope
                .snapshot
                .entries
                .iter()
                .filter(|entry| !entry.directory && scope.live.contains(&entry.relative))
            {
                let native = scope.workspace.join(&entry.relative);
                if !native.starts_with(root) {
                    continue;
                }
                let name = native
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or(ERROR)?;
                let descriptive = self.project_text(name)?;
                if pattern.matches(&entry.alias)
                    || pattern.matches(entry.alias.rsplit('/').next().unwrap_or(&entry.alias))
                    || pattern.matches(&descriptive)
                {
                    matches.insert(native);
                }
            }
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

    /// Enumerate every name, including hidden entries, without reading contents
    /// or descending through symlinks. Commit the new snapshot only on success.
    pub(crate) fn discover(&mut self) -> Result<bool, String> {
        let found = inventory(&self.workspace)?;
        let mut next = self.clone();
        next.live = found.keys().cloned().collect();
        let existing: BTreeMap<_, _> = next
            .snapshot
            .entries
            .iter()
            .map(|entry| (entry.relative.clone(), entry.clone()))
            .collect();
        // A literal filename that resembles our root must not be mistaken for
        // an already projected token. Reserve a different root before issuing
        // the first reference; once issued, a conflict withholds transmission.
        let raw_names: BTreeSet<_> = found
            .keys()
            .flat_map(|name| name.split('/').map(str::to_owned))
            .collect();
        if raw_names
            .iter()
            .any(|name| name.contains(&next.snapshot.root_alias))
        {
            if !next.snapshot.entries.is_empty() {
                return Err("Filename alias namespace conflicts with a newly discovered name; no private filenames were sent".into());
            }
            let base = format!("__CA_PATH_{}", next.snapshot.namespace);
            let mut counter = 1_u64;
            loop {
                let candidate = format!("{base}_{counter}__");
                if !raw_names.iter().any(|name| name.contains(&candidate)) {
                    next.snapshot.root_alias = candidate;
                    break;
                }
                counter = counter.checked_add(1).ok_or(LIMIT)?;
            }
        }
        if next
            .snapshot
            .source_spellings
            .keys()
            .any(|alias| raw_names.iter().any(|name| name.contains(alias)))
        {
            return Err("Source filename reference conflicts with a newly discovered name; request was withheld".into());
        }
        let mut directories = BTreeMap::new();
        directories.insert(String::new(), next.snapshot.root_alias.clone());
        for (relative, directory) in &found {
            let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
            let parent_alias = directories.get(parent).ok_or(ERROR)?;
            let alias = if let Some(entry) = existing.get(relative) {
                if entry.directory != *directory {
                    return Err(
                        "A registered filename changed kind; local action was blocked".into(),
                    );
                }
                entry.alias.clone()
            } else {
                let counter = if *directory {
                    &mut next.snapshot.next_directory
                } else {
                    &mut next.snapshot.next_file
                };
                let kind = if *directory { 'd' } else { 'f' };
                let suffix = if *directory { "" } else { extension(relative) };
                let tag = &next.snapshot.allocation_tag;
                let alias = loop {
                    let number = *counter;
                    *counter = counter.checked_add(1).ok_or(LIMIT)?;
                    let component = if tag.is_empty() {
                        format!("{kind}{number:03}{suffix}")
                    } else {
                        format!("{kind}{tag}_{number:03}{suffix}")
                    };
                    let alias = format!("{parent_alias}/{component}");
                    if !raw_names.contains(&component)
                        && !next
                            .snapshot
                            .entries
                            .iter()
                            .any(|entry| entry.alias == alias)
                    {
                        break alias;
                    }
                };
                next.snapshot.entries.push(Entry {
                    relative: relative.clone(),
                    alias: alias.clone(),
                    directory: *directory,
                });
                alias
            };
            if *directory {
                directories.insert(relative.clone(), alias);
            }
        }
        for name in &raw_names {
            if !next.snapshot.names.contains_key(name) && !next.disclosed_creation_leaf(name) {
                let alias = loop {
                    let number = next.snapshot.next_name;
                    next.snapshot.next_name = number.checked_add(1).ok_or(LIMIT)?;
                    let alias = format!("__CA_NAME_{}_{number}__", next.snapshot.namespace);
                    if !raw_names.iter().any(|original| original.contains(&alias))
                        && !next
                            .snapshot
                            .names
                            .values()
                            .any(|existing| existing == &alias)
                    {
                        break alias;
                    }
                };
                next.snapshot.names.insert(name.clone(), alias);
            }
        }
        if next.snapshot.entries.len() > MAX_ENTRIES || next.snapshot.names.len() > MAX_ENTRIES {
            return Err(LIMIT.into());
        }
        for root in next.authorized_scopes.clone() {
            next.scopes.get_mut(&root).ok_or(ERROR)?.discover()?;
        }
        next.rebuild()?;
        let changed = next.snapshot != self.snapshot;
        *self = next;
        Ok(changed)
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
        for (name, name_alias) in &self.snapshot.names {
            if let Some(entries) = basenames.get(name.as_str()) {
                if entries.len() == 1 {
                    self.forward.insert(name.clone(), entries[0].alias.clone());
                    continue;
                }
            }
            self.forward.insert(name.clone(), name_alias.clone());
            self.backward.insert(name_alias.clone(), name.clone());
        }
        for (alias, spelling) in &self.snapshot.source_spellings {
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
                if Path::new(original).is_absolute() || !original.contains('/') {
                    self.forward
                        .entry(original.clone())
                        .or_insert_with(|| alias.clone());
                }
            }
            for (alias, relative) in &names.backward {
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
        self.replacements = self.forward.values().cloned().collect();
        self.matcher = Some(
            AhoCorasickBuilder::new()
                .match_kind(MatchKind::LeftmostLongest)
                .build(self.forward.keys())
                .map_err(|_| ERROR)?,
        );
        Ok(())
    }

    /// Longest-match, non-recursive projection. Already issued references are
    /// protected, while literal alias-shaped filenames use their own mapping.
    pub(crate) fn project_text(&self, text: &str) -> Result<String, String> {
        self.project_text_with_protected(text, &[])
    }

    pub(crate) fn project_text_with_protected(
        &self,
        text: &str,
        opaque: &[std::ops::Range<usize>],
    ) -> Result<String, String> {
        let Some(matcher) = &self.matcher else {
            return Err(ERROR.into());
        };
        let protected = self.reference_ranges(text)?;
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        for found in matcher.find_iter(text) {
            if protected
                .iter()
                .any(|(start, end)| found.start() < *end && found.end() > *start)
                || opaque
                    .iter()
                    .any(|range| found.start() < range.end && found.end() > range.start)
            {
                continue;
            }
            out.push_str(&text[copied..found.start()]);
            out.push_str(&self.replacements[found.pattern().as_usize()]);
            copied = found.end();
        }
        out.push_str(&text[copied..]);
        Ok(out)
    }

    pub(crate) fn contains_aliases(&self, text: &str) -> bool {
        text.contains("__CA_PATH_") || text.contains("__CA_NAME_")
    }

    /// A source reference has two identities: its exact authored spelling for
    /// edits, and its workspace target for a model-issued native read.
    pub(crate) fn source_link(
        &mut self,
        source: &str,
        original: &str,
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
                    .source_link(&relative, original)?;
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
        if let Some((alias, _)) = self
            .snapshot
            .source_spellings
            .iter()
            .find(|(_, value)| **value == spelling)
        {
            return Ok(alias.clone());
        }
        if self.snapshot.source_spellings.len() >= MAX_ENTRIES {
            return Err(LIMIT.into());
        }
        let mut counter = self.snapshot.source_spellings.len() + 1;
        let alias = loop {
            let alias = format!("__CA_NAME_{}_source_{counter}__", self.snapshot.namespace);
            if !self.backward.contains_key(&alias)
                && !self.forward.keys().any(|name| name.contains(&alias))
            {
                break alias;
            }
            counter += 1;
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
        let protected = self.reference_ranges(text)?;
        let matches = self
            .matcher
            .as_ref()
            .ok_or(ERROR)?
            .find_iter(text)
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
            let source_record =
                record.filter(|record| record.source.is_some() || record.target.is_none());
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
    pub(crate) fn validate_wire_text(&self, text: &str) -> Result<(), String> {
        if self.project_text(text)? != text {
            return Err("A filename remains in the provider copy; request was withheld".into());
        }
        Ok(())
    }

    /// Project a typed local path without guessing from a bare basename in
    /// prose. Useful for file listings whose root entries otherwise lack './'.
    pub(crate) fn project_path(&self, path: &str) -> Result<String, String> {
        if let Some((_, scope)) = self
            .scopes
            .iter()
            .filter(|(root, _)| Path::new(path).starts_with(root))
            .max_by_key(|(root, _)| root.components().count())
        {
            return scope.project_path(path);
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
            .filter(|spelling| spelling.source.is_none())
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
            if leaf.contains('/') || leaf.contains("__CA_") || self.project_text(leaf)? != leaf {
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
        let ranges = self.reference_ranges(text)?;
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        for (start, end) in ranges {
            let token = &text[start..end];
            let original = self.backward.get(token).ok_or(ERROR)?;
            out.push_str(&text[copied..start]);
            out.push_str(original);
            copied = end;
        }
        out.push_str(&text[copied..]);
        Ok(out)
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

fn valid_namespace(namespace: &str) -> bool {
    !namespace.is_empty()
        && namespace.len() <= 64
        && namespace
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
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

fn inventory(root: &Path) -> Result<BTreeMap<String, bool>, String> {
    let mut found = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    let mut bytes = 0_usize;
    while let Some(directory) = stack.pop() {
        let entries = std::fs::read_dir(directory)
            .map_err(|_| "Workspace filename discovery failed; request was withheld")?;
        for entry in entries {
            let entry = entry.map_err(|_| ERROR)?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|_| ERROR)?
                .to_str()
                .ok_or("Non-UTF-8 filename is unsupported; request was withheld")?
                .to_owned();
            valid_relative(&relative)?;
            let kind = entry.file_type().map_err(|_| ERROR)?;
            let directory = kind.is_dir();
            bytes = bytes.saturating_add(relative.len());
            found.insert(relative, directory);
            if found.len() > MAX_ENTRIES || bytes > MAX_BYTES {
                return Err(LIMIT.into());
            }
            if directory {
                stack.push(path);
            }
        }
    }
    Ok(found)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, FilenameAliases) {
        let temp = tempfile::tempdir().unwrap();
        let aliases = FilenameAliases::new(temp.path().into(), "test").unwrap();
        (temp, aliases)
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
        assert!(aliases
            .resolve_path(&aliases.project_text("customer.rs").unwrap())
            .is_err());
        assert_eq!(
            aliases
                .restore_text(&aliases.project_text("customer.rs").unwrap())
                .unwrap(),
            "customer.rs"
        );
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
        aliases
            .validate_wire_text(&aliases.project_text("open sensitive.rs").unwrap())
            .unwrap();
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
        let aliases = FilenameAliases::from_snapshot(temp.path().into(), snapshot).unwrap();
        let mut identities = BTreeSet::new();
        for spelling in ["Case.rs", "case.rs", "é.rs", "e\u{301}.rs"] {
            let alias = aliases.project_text(spelling).unwrap();
            assert!(identities.insert(alias.clone()));
            assert_eq!(aliases.restore_text(&alias).unwrap(), spelling);
        }
    }
}
