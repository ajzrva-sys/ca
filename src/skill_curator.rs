use crate::skills::{
    SkillManageAction, SkillManageFile, SkillManageReceipt, SkillManageRequest, SkillRegistry,
    SkillSourceScope,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const VERSION: u32 = 1;
const MAX_STATE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ENTRIES: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CuratorSkillStatus {
    Active,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CuratorArchive {
    pub id: Uuid,
    pub original_path: PathBuf,
    pub archive_path: PathBuf,
    pub archived_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CuratorEntry {
    pub key: String,
    pub name: String,
    pub qualified_name: String,
    pub source_scope: SkillSourceScope,
    pub locator: String,
    pub digest: String,
    pub pinned: bool,
    pub first_seen_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub status: CuratorSkillStatus,
    pub archive: Option<CuratorArchive>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CuratorState {
    version: u32,
    revision: u64,
    entries: BTreeMap<String, CuratorEntry>,
}

impl Default for CuratorState {
    fn default() -> Self {
        Self {
            version: VERSION,
            revision: 0,
            entries: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CuratorJournal {
    version: u32,
    operation: String,
    stage: String,
    source: PathBuf,
    target: PathBuf,
    #[serde(default)]
    entry_key: Option<String>,
    #[serde(default)]
    archived_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuratorPolicy {
    pub enabled: bool,
    pub archive_after_days: u64,
    pub max_mutations: usize,
    pub eligible_scopes: BTreeSet<SkillSourceScope>,
}

impl Default for CuratorPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            archive_after_days: 90,
            max_mutations: 4,
            eligible_scopes: [SkillSourceScope::AutoDistill].into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CuratorAutomaticInput {
    pub last_used: BTreeMap<String, DateTime<Utc>>,
    pub reactivate: BTreeSet<String>,
    pub actions: Vec<CuratorAutomaticAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // automatic action plans are supplied by optional curator integrations
pub enum CuratorAutomaticAction {
    Pin {
        selector: String,
    },
    Unpin {
        selector: String,
    },
    Restore {
        selector: String,
    },
    Consolidate {
        selectors: Vec<String>,
        name: String,
        scope: SkillSourceScope,
    },
    Adopt {
        selector: String,
        target_name: String,
        target_scope: SkillSourceScope,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuratorRunReceipt {
    pub archived: Vec<String>,
    pub restored: Vec<String>,
    pub pinned: Vec<String>,
    pub unpinned: Vec<String>,
    pub consolidated: Vec<String>,
    pub adopted: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CuratorError {
    #[error("skill curator is read-only")]
    ReadOnly,
    #[error("skill curator state is invalid: {0}")]
    InvalidState(String),
    #[error("skill curator request is invalid: {0}")]
    Invalid(String),
    #[error("skill curator I/O failed: {0}")]
    Io(String),
    #[error("skill operation failed: {0}")]
    Skill(String),
}

#[derive(Debug, Clone)]
pub struct SkillCuratorStore {
    root: PathBuf,
    writable: bool,
}

impl SkillCuratorStore {
    pub fn new(root: impl Into<PathBuf>, writable: bool) -> Self {
        Self {
            root: root.into(),
            writable,
        }
    }

    pub fn entries(&self) -> Result<Vec<CuratorEntry>, CuratorError> {
        self.recover()?;
        Ok(self.load()?.entries.into_values().collect())
    }

    pub fn sync(
        &self,
        registry: &mut SkillRegistry,
        last_used: &BTreeMap<String, DateTime<Utc>>,
    ) -> Result<Vec<CuratorEntry>, CuratorError> {
        self.require_writable()?;
        self.recover()?;
        registry.refresh_portable();
        let mut state = self.load()?;
        let now = Utc::now();
        let mut current = BTreeSet::new();
        for instance in registry.instances().take(MAX_ENTRIES) {
            let key = entry_key(
                instance.source_scope,
                &instance.locator,
                &instance.metadata.name,
            );
            current.insert(key.clone());
            let prior = state.entries.get(&key).cloned();
            state.entries.insert(
                key.clone(),
                CuratorEntry {
                    key,
                    name: instance.metadata.name.clone(),
                    qualified_name: instance.qualified_name.clone(),
                    source_scope: instance.source_scope,
                    locator: instance.locator.clone(),
                    digest: instance.content_digest.clone(),
                    pinned: prior.as_ref().is_some_and(|entry| entry.pinned),
                    first_seen_at: prior.as_ref().map_or(now, |entry| entry.first_seen_at),
                    last_used_at: last_used
                        .get(&instance.instance_id)
                        .copied()
                        .or_else(|| prior.as_ref().and_then(|entry| entry.last_used_at)),
                    status: CuratorSkillStatus::Active,
                    archive: None,
                },
            );
        }
        state.entries.retain(|key, entry| {
            current.contains(key) || entry.status == CuratorSkillStatus::Archived
        });
        state.revision = state.revision.saturating_add(1);
        self.save(&state)?;
        Ok(state.entries.into_values().collect())
    }

    pub fn pin(
        &self,
        registry: &mut SkillRegistry,
        selector: &str,
        pinned: bool,
    ) -> Result<CuratorEntry, CuratorError> {
        self.require_writable()?;
        self.sync(registry, &BTreeMap::new())?;
        let instance = registry.inspect_instance(selector).map_err(skill_error)?;
        let key = entry_key(
            instance.source_scope,
            &instance.locator,
            &instance.metadata.name,
        );
        let mut state = self.load()?;
        let entry = state
            .entries
            .get_mut(&key)
            .ok_or_else(|| CuratorError::Invalid("synced skill disappeared".into()))?;
        entry.pinned = pinned;
        let receipt = entry.clone();
        state.revision = state.revision.saturating_add(1);
        self.save(&state)?;
        Ok(receipt)
    }

    pub fn archive(
        &self,
        registry: &mut SkillRegistry,
        selector: &str,
    ) -> Result<CuratorEntry, CuratorError> {
        self.require_writable()?;
        self.sync(registry, &BTreeMap::new())?;
        let instance = registry
            .inspect_instance(selector)
            .map_err(skill_error)?
            .clone();
        if !matches!(
            instance.source_scope,
            SkillSourceScope::Workspace | SkillSourceScope::User | SkillSourceScope::AutoDistill
        ) {
            return Err(CuratorError::Invalid(
                "only native writable skills can be archived".into(),
            ));
        }
        let key = entry_key(
            instance.source_scope,
            &instance.locator,
            &instance.metadata.name,
        );
        let mut state = self.load()?;
        let mut entry = state
            .entries
            .get(&key)
            .cloned()
            .ok_or_else(|| CuratorError::Invalid("skill is not synchronized".into()))?;
        if entry.pinned {
            return Err(CuratorError::Invalid(
                "pinned skills cannot be archived".into(),
            ));
        }
        let source = registry
            .instance_path(&instance.instance_id)
            .ok_or_else(|| CuratorError::Invalid("skill source is unavailable".into()))?
            .to_path_buf();
        ensure_plain_directory(&source)?;
        let digest = crate::skills::portable_skill_tree_digest(&source, instance.source_scope)
            .map_err(skill_error)?;
        if digest != instance.content_digest {
            return Err(CuratorError::Invalid("skill changed before archive".into()));
        }
        self.ensure_root()?;
        let id = Uuid::new_v4();
        let archive_parent = self.root.join("archive").join(id.to_string());
        std::fs::create_dir_all(&archive_parent).map_err(io_error)?;
        let target = archive_parent.join(&instance.metadata.name);
        let archived_at = Utc::now();
        let journal = self.write_journal(id, "archive", &source, &target, &key, archived_at)?;
        std::fs::rename(&source, &target).map_err(io_error)?;
        set_journal_stage(&journal, "moved")?;
        entry.status = CuratorSkillStatus::Archived;
        entry.archive = Some(CuratorArchive {
            id,
            original_path: source,
            archive_path: target,
            archived_at,
        });
        state.entries.insert(key, entry.clone());
        state.revision = state.revision.saturating_add(1);
        self.save(&state)?;
        commit_journal(&journal)?;
        registry.refresh_portable();
        Ok(entry)
    }

    pub fn restore(
        &self,
        registry: &mut SkillRegistry,
        key_or_name: &str,
    ) -> Result<CuratorEntry, CuratorError> {
        self.require_writable()?;
        self.recover()?;
        let mut state = self.load()?;
        let key = state
            .entries
            .iter()
            .find(|(key, entry)| {
                entry.status == CuratorSkillStatus::Archived
                    && (key.as_str() == key_or_name
                        || entry.name == key_or_name
                        || entry.qualified_name == key_or_name)
            })
            .map(|(key, _)| key.clone())
            .ok_or_else(|| CuratorError::Invalid("archived skill was not found".into()))?;
        let mut entry = state.entries[&key].clone();
        let archive = entry
            .archive
            .clone()
            .ok_or_else(|| CuratorError::InvalidState("archive receipt is missing".into()))?;
        ensure_under(&archive.archive_path, &self.root)?;
        ensure_plain_directory(&archive.archive_path)?;
        if archive.original_path.exists() {
            return Err(CuratorError::Invalid(
                "restore target already exists".into(),
            ));
        }
        let digest =
            crate::skills::portable_skill_tree_digest(&archive.archive_path, entry.source_scope)
                .map_err(skill_error)?;
        if digest != entry.digest {
            return Err(CuratorError::Invalid(
                "archived skill digest changed".into(),
            ));
        }
        let parent = archive
            .original_path
            .parent()
            .ok_or_else(|| CuratorError::Invalid("restore target has no parent".into()))?;
        std::fs::create_dir_all(parent).map_err(io_error)?;
        let journal = self.write_journal(
            archive.id,
            "restore",
            &archive.archive_path,
            &archive.original_path,
            &key,
            archive.archived_at,
        )?;
        std::fs::rename(&archive.archive_path, &archive.original_path).map_err(io_error)?;
        set_journal_stage(&journal, "moved")?;
        entry.status = CuratorSkillStatus::Active;
        entry.archive = None;
        state.entries.insert(key, entry.clone());
        state.revision = state.revision.saturating_add(1);
        self.save(&state)?;
        commit_journal(&journal)?;
        registry.refresh_portable();
        Ok(entry)
    }

    pub fn consolidate(
        &self,
        registry: &mut SkillRegistry,
        selectors: &[String],
        name: &str,
        scope: SkillSourceScope,
    ) -> Result<SkillManageReceipt, CuratorError> {
        self.require_writable()?;
        if !(2..=8).contains(&selectors.len()) {
            return Err(CuratorError::Invalid(
                "consolidation requires 2 to 8 skills".into(),
            ));
        }
        let mut related = Vec::new();
        let mut descriptions = Vec::new();
        for selector in selectors {
            let instance = registry.inspect_instance(selector).map_err(skill_error)?;
            related.push(instance.metadata.name.clone());
            descriptions.push(format!(
                "- `{}` — {}",
                instance.qualified_name, instance.metadata.description
            ));
        }
        related.sort();
        related.dedup();
        if related.len() != selectors.len() {
            return Err(CuratorError::Invalid(
                "consolidation selectors must be distinct".into(),
            ));
        }
        let related_yaml = related
            .iter()
            .map(|item| format!("  - {}", serde_json::to_string(item).unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("\n");
        let content = format!(
            "---\nname: {name}\ndescription: Consolidated router for {} related procedures.\ncategory: consolidated\nrelated_skills:\n{related_yaml}\n---\n# {name}\n\nActivate the narrow related skill that matches the task. This umbrella grants no additional authority.\n\n{}\n",
            related.len(),
            descriptions.join("\n")
        );
        create_skill(
            registry,
            name,
            scope,
            vec![SkillManageFile {
                path: "SKILL.md".into(),
                content: Some(content),
            }],
        )
    }

    pub fn adopt(
        &self,
        registry: &mut SkillRegistry,
        selector: &str,
        target_name: &str,
        target_scope: SkillSourceScope,
    ) -> Result<SkillManageReceipt, CuratorError> {
        self.require_writable()?;
        if !matches!(
            target_scope,
            SkillSourceScope::Workspace | SkillSourceScope::User | SkillSourceScope::AutoDistill
        ) {
            return Err(CuratorError::Invalid("adoption target is read-only".into()));
        }
        let instance = registry
            .inspect_instance(selector)
            .map_err(skill_error)?
            .clone();
        let files = instance
            .files
            .iter()
            .map(|file| {
                let path = file.relative_path.to_string_lossy().into_owned();
                let mut content = registry
                    .skill_view(&instance.instance_id, &path)
                    .map_err(skill_error)?;
                if path == "SKILL.md" && target_name != instance.metadata.name {
                    content = replace_frontmatter_name(&content, target_name)?;
                }
                Ok(SkillManageFile {
                    path,
                    content: Some(content),
                })
            })
            .collect::<Result<Vec<_>, CuratorError>>()?;
        create_skill(registry, target_name, target_scope, files)
    }

    pub fn run_automatic(
        &self,
        registry: &mut SkillRegistry,
        policy: &CuratorPolicy,
        input: &CuratorAutomaticInput,
        now: DateTime<Utc>,
    ) -> Result<CuratorRunReceipt, CuratorError> {
        let mut receipt = CuratorRunReceipt {
            archived: Vec::new(),
            restored: Vec::new(),
            pinned: Vec::new(),
            unpinned: Vec::new(),
            consolidated: Vec::new(),
            adopted: Vec::new(),
        };
        if !policy.enabled {
            return Ok(receipt);
        }
        if !(1..=3650).contains(&policy.archive_after_days)
            || !(1..=32).contains(&policy.max_mutations)
        {
            return Err(CuratorError::Invalid(
                "automatic policy is out of bounds".into(),
            ));
        }
        self.sync(registry, &input.last_used)?;
        for entry in self
            .entries()?
            .into_iter()
            .filter(|entry| {
                entry.status == CuratorSkillStatus::Archived
                    && input.reactivate.contains(&entry.key)
            })
            .take(policy.max_mutations)
        {
            self.restore(registry, &entry.key)?;
            receipt.restored.push(entry.key);
        }
        let mut actions = input.actions.clone();
        actions.sort_by_key(automatic_action_key);
        for action in actions
            .into_iter()
            .take(policy.max_mutations.saturating_sub(receipt.restored.len()))
        {
            match action {
                CuratorAutomaticAction::Pin { selector } => {
                    ensure_automatic_selector_scope(registry, policy, &selector)?;
                    let entry = self.pin(registry, &selector, true)?;
                    receipt.pinned.push(entry.key);
                }
                CuratorAutomaticAction::Unpin { selector } => {
                    ensure_automatic_selector_scope(registry, policy, &selector)?;
                    let entry = self.pin(registry, &selector, false)?;
                    receipt.unpinned.push(entry.key);
                }
                CuratorAutomaticAction::Restore { selector } => {
                    let entry = self
                        .entries()?
                        .into_iter()
                        .find(|entry| {
                            entry.status == CuratorSkillStatus::Archived
                                && (entry.key == selector
                                    || entry.name == selector
                                    || entry.qualified_name == selector)
                        })
                        .ok_or_else(|| {
                            CuratorError::Invalid("automatic restore target was not found".into())
                        })?;
                    ensure_automatic_scope(policy, entry.source_scope)?;
                    let entry = self.restore(registry, &selector)?;
                    receipt.restored.push(entry.key);
                }
                CuratorAutomaticAction::Consolidate {
                    selectors,
                    name,
                    scope,
                } => {
                    ensure_automatic_scope(policy, scope)?;
                    self.consolidate(registry, &selectors, &name, scope)?;
                    receipt.consolidated.push(name);
                }
                CuratorAutomaticAction::Adopt {
                    selector,
                    target_name,
                    target_scope,
                } => {
                    ensure_automatic_scope(policy, target_scope)?;
                    self.adopt(registry, &selector, &target_name, target_scope)?;
                    receipt.adopted.push(target_name);
                }
            }
        }
        let applied = receipt.restored.len()
            + receipt.pinned.len()
            + receipt.unpinned.len()
            + receipt.consolidated.len()
            + receipt.adopted.len();
        let remaining = policy.max_mutations.saturating_sub(applied);
        let threshold = now - Duration::days(policy.archive_after_days as i64);
        let mut candidates = self
            .entries()?
            .into_iter()
            .filter(|entry| {
                entry.status == CuratorSkillStatus::Active
                    && !entry.pinned
                    && policy.eligible_scopes.contains(&entry.source_scope)
                    && entry.last_used_at.unwrap_or(entry.first_seen_at) < threshold
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| left.key.cmp(&right.key));
        for entry in candidates.into_iter().take(remaining) {
            self.archive(registry, &entry.qualified_name)?;
            receipt.archived.push(entry.key);
        }
        self.sync(registry, &input.last_used)?;
        Ok(receipt)
    }

    fn state_path(&self) -> PathBuf {
        self.root.join("state.json")
    }

    fn require_writable(&self) -> Result<(), CuratorError> {
        self.writable.then_some(()).ok_or(CuratorError::ReadOnly)
    }

    fn ensure_root(&self) -> Result<(), CuratorError> {
        std::fs::create_dir_all(&self.root).map_err(io_error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700))
                .map_err(io_error)?;
        }
        Ok(())
    }

    fn load(&self) -> Result<CuratorState, CuratorError> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(CuratorState::default());
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(io_error)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_STATE_BYTES
        {
            return Err(CuratorError::InvalidState(
                "state is not bounded and regular".into(),
            ));
        }
        let mut bytes = Vec::new();
        File::open(path)
            .and_then(|file| file.take(MAX_STATE_BYTES + 1).read_to_end(&mut bytes))
            .map_err(io_error)?;
        let state: CuratorState = serde_json::from_slice(&bytes)
            .map_err(|error| CuratorError::InvalidState(error.to_string()))?;
        if state.version != VERSION || state.entries.len() > MAX_ENTRIES {
            return Err(CuratorError::InvalidState(
                "unsupported or oversized state".into(),
            ));
        }
        Ok(state)
    }

    fn save(&self, state: &CuratorState) -> Result<(), CuratorError> {
        self.ensure_root()?;
        let bytes = serde_json::to_vec_pretty(state)
            .map_err(|error| CuratorError::InvalidState(error.to_string()))?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err(CuratorError::InvalidState(
                "state exceeds byte bound".into(),
            ));
        }
        let temporary = self.root.join(format!(".state-{}.tmp", Uuid::new_v4()));
        write_private(&temporary, &bytes)?;
        std::fs::rename(temporary, self.state_path()).map_err(io_error)
    }

    fn write_journal(
        &self,
        id: Uuid,
        operation: &str,
        source: &Path,
        target: &Path,
        entry_key: &str,
        archived_at: DateTime<Utc>,
    ) -> Result<PathBuf, CuratorError> {
        let dir = self.root.join("journals").join(id.to_string());
        std::fs::create_dir_all(&dir).map_err(io_error)?;
        let path = dir.join("journal.json");
        let journal = CuratorJournal {
            version: VERSION,
            operation: operation.into(),
            stage: "prepared".into(),
            source: source.into(),
            target: target.into(),
            entry_key: Some(entry_key.into()),
            archived_at: Some(archived_at),
        };
        write_private(
            &path,
            &serde_json::to_vec_pretty(&journal)
                .map_err(|error| CuratorError::InvalidState(error.to_string()))?,
        )?;
        Ok(path)
    }

    fn recover(&self) -> Result<(), CuratorError> {
        let Ok(entries) = std::fs::read_dir(self.root.join("journals")) else {
            return Ok(());
        };
        for entry in entries.take(MAX_ENTRIES) {
            let path = entry.map_err(io_error)?.path().join("journal.json");
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let mut journal: CuratorJournal = serde_json::from_slice(&bytes)
                .map_err(|error| CuratorError::InvalidState(error.to_string()))?;
            if journal.version != VERSION {
                return Err(CuratorError::InvalidState(
                    "unsupported curator journal version".into(),
                ));
            }
            if matches!(journal.stage.as_str(), "prepared" | "moved") {
                self.require_writable()?;
            }
            match journal.stage.as_str() {
                "prepared" => {
                    if journal.target.exists() && !journal.source.exists() {
                        std::fs::rename(&journal.target, &journal.source).map_err(io_error)?;
                    }
                    self.reconcile_journal_state(&journal, false)?;
                    journal.stage = "rolled_back".into();
                    write_journal(&path, &journal)?;
                }
                "moved" => {
                    if journal.source.exists() || !journal.target.exists() {
                        return Err(CuratorError::InvalidState(
                            "moved curator journal does not match the filesystem".into(),
                        ));
                    }
                    self.reconcile_journal_state(&journal, true)?;
                    journal.stage = "committed".into();
                    write_journal(&path, &journal)?;
                }
                "committed" | "rolled_back" => {}
                _ => {
                    return Err(CuratorError::InvalidState(
                        "unknown curator journal stage".into(),
                    ))
                }
            }
        }
        Ok(())
    }

    fn reconcile_journal_state(
        &self,
        journal: &CuratorJournal,
        committed: bool,
    ) -> Result<(), CuratorError> {
        let Some(key) = journal.entry_key.as_deref() else {
            return Ok(());
        };
        let mut state = self.load()?;
        let Some(entry) = state.entries.get_mut(key) else {
            return Err(CuratorError::InvalidState(
                "curator journal entry is missing from state".into(),
            ));
        };
        let archived = match journal.operation.as_str() {
            "archive" => committed,
            "restore" => !committed,
            _ => {
                return Err(CuratorError::InvalidState(
                    "unknown curator journal operation".into(),
                ))
            }
        };
        if archived {
            let (original_path, archive_path) = if journal.operation == "archive" {
                (journal.source.clone(), journal.target.clone())
            } else {
                (journal.target.clone(), journal.source.clone())
            };
            entry.status = CuratorSkillStatus::Archived;
            entry.archive = Some(CuratorArchive {
                id: journal_id_from_path(&archive_path, &self.root)?,
                original_path,
                archive_path,
                archived_at: journal.archived_at.unwrap_or_else(Utc::now),
            });
        } else {
            entry.status = CuratorSkillStatus::Active;
            entry.archive = None;
        }
        state.revision = state.revision.saturating_add(1);
        self.save(&state)
    }
}

fn create_skill(
    registry: &mut SkillRegistry,
    name: &str,
    scope: SkillSourceScope,
    files: Vec<SkillManageFile>,
) -> Result<SkillManageReceipt, CuratorError> {
    registry
        .skill_manage(SkillManageRequest {
            action: SkillManageAction::Create,
            selector: name.into(),
            expected_full_tree_digest: "absent".into(),
            scope: Some(scope),
            relative_path: None,
            content: None,
            old_text: None,
            new_text: None,
            files,
        })
        .map_err(skill_error)
}

fn entry_key(scope: SkillSourceScope, locator: &str, name: &str) -> String {
    format!("{}:{locator}:{name}", scope.label())
}

fn automatic_action_key(action: &CuratorAutomaticAction) -> String {
    match action {
        CuratorAutomaticAction::Pin { selector } => format!("pin:{selector}"),
        CuratorAutomaticAction::Unpin { selector } => format!("unpin:{selector}"),
        CuratorAutomaticAction::Restore { selector } => format!("restore:{selector}"),
        CuratorAutomaticAction::Consolidate {
            selectors,
            name,
            scope,
        } => format!(
            "consolidate:{}:{name}:{}",
            scope.label(),
            selectors.join("\u{1f}")
        ),
        CuratorAutomaticAction::Adopt {
            selector,
            target_name,
            target_scope,
        } => format!("adopt:{}:{target_name}:{selector}", target_scope.label()),
    }
}

fn ensure_automatic_scope(
    policy: &CuratorPolicy,
    scope: SkillSourceScope,
) -> Result<(), CuratorError> {
    if !matches!(
        scope,
        SkillSourceScope::Workspace | SkillSourceScope::User | SkillSourceScope::AutoDistill
    ) || !policy.eligible_scopes.contains(&scope)
    {
        return Err(CuratorError::Invalid(
            "automatic curator action is outside the eligible native scopes".into(),
        ));
    }
    Ok(())
}

fn ensure_automatic_selector_scope(
    registry: &SkillRegistry,
    policy: &CuratorPolicy,
    selector: &str,
) -> Result<(), CuratorError> {
    let instance = registry.inspect_instance(selector).map_err(skill_error)?;
    ensure_automatic_scope(policy, instance.source_scope)
}

fn ensure_plain_directory(path: &Path) -> Result<(), CuratorError> {
    let metadata = std::fs::symlink_metadata(path).map_err(io_error)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(CuratorError::Invalid(
            "skill path is not a regular directory".into(),
        ));
    }
    Ok(())
}

fn ensure_under(path: &Path, root: &Path) -> Result<(), CuratorError> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(CuratorError::InvalidState(
            "archive escaped curator root".into(),
        ))
    }
}

fn journal_id_from_path(path: &Path, root: &Path) -> Result<Uuid, CuratorError> {
    let archive_root = root.join("archive");
    ensure_under(path, &archive_root)?;
    path.parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| CuratorError::InvalidState("archive journal ID is invalid".into()))
}

fn replace_frontmatter_name(content: &str, name: &str) -> Result<String, CuratorError> {
    let mut lines = content.lines().map(str::to_string).collect::<Vec<_>>();
    let end = lines
        .iter()
        .skip(1)
        .position(|line| line.trim() == "---")
        .map(|index| index + 1)
        .ok_or_else(|| CuratorError::Invalid("adopted SKILL.md has no frontmatter".into()))?;
    let index = lines[..end]
        .iter()
        .position(|line| line.trim_start().starts_with("name:"))
        .ok_or_else(|| CuratorError::Invalid("adopted SKILL.md has no name".into()))?;
    lines[index] = format!("name: {name}");
    Ok(lines.join("\n") + "\n")
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), CuratorError> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(io_error)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(io_error)
}

fn commit_journal(path: &Path) -> Result<(), CuratorError> {
    set_journal_stage(path, "committed")
}

fn set_journal_stage(path: &Path, stage: &str) -> Result<(), CuratorError> {
    let mut journal: CuratorJournal =
        serde_json::from_slice(&std::fs::read(path).map_err(io_error)?)
            .map_err(|error| CuratorError::InvalidState(error.to_string()))?;
    journal.stage = stage.into();
    write_journal(path, &journal)
}

fn write_journal(path: &Path, journal: &CuratorJournal) -> Result<(), CuratorError> {
    write_private(
        path,
        &serde_json::to_vec_pretty(journal)
            .map_err(|error| CuratorError::InvalidState(error.to_string()))?,
    )
}

fn io_error(error: std::io::Error) -> CuratorError {
    CuratorError::Io(error.to_string())
}

fn skill_error(error: crate::skills::SkillError) -> CuratorError {
    CuratorError::Skill(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::SkillDiscoveryRoot;

    fn registry(temp: &tempfile::TempDir) -> SkillRegistry {
        let root = temp.path().join("skills");
        for (name, related) in [("alpha", "beta"), ("beta", "alpha")] {
            let path = root.join(name);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(
                path.join("SKILL.md"),
                format!(
                    "---\nname: {name}\ndescription: {name} procedure\nrelated_skills: [{related}]\n---\n# {name}\n"
                ),
            )
            .unwrap();
        }
        SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::User,
                root,
                "user",
            )],
            &[],
        )
    }

    #[test]
    fn archive_restore_pin_and_sync_are_digest_bound() {
        let temp = tempfile::tempdir().unwrap();
        let mut registry = registry(&temp);
        let store = SkillCuratorStore::new(temp.path().join("curator"), true);
        assert_eq!(
            store.sync(&mut registry, &BTreeMap::new()).unwrap().len(),
            2
        );
        store.pin(&mut registry, "alpha", true).unwrap();
        assert!(store.archive(&mut registry, "alpha").is_err());
        store.pin(&mut registry, "alpha", false).unwrap();
        let archived = store.archive(&mut registry, "alpha").unwrap();
        assert!(registry.inspect_instance("alpha").is_err());
        store.restore(&mut registry, &archived.key).unwrap();
        assert!(registry.inspect_instance("alpha").is_ok());
    }

    #[test]
    fn crash_recovery_rolls_back_prepared_moves_and_completes_moved_transactions() {
        let temp = tempfile::tempdir().unwrap();
        let mut registry = registry(&temp);
        let store = SkillCuratorStore::new(temp.path().join("curator"), true);
        let entries = store.sync(&mut registry, &BTreeMap::new()).unwrap();

        let alpha = entries.iter().find(|entry| entry.name == "alpha").unwrap();
        let alpha_instance = registry.inspect_instance("alpha").unwrap();
        let alpha_source = registry
            .instance_path(&alpha_instance.instance_id)
            .unwrap()
            .to_path_buf();
        let alpha_id = Uuid::new_v4();
        let alpha_target = store
            .root
            .join("archive")
            .join(alpha_id.to_string())
            .join("alpha");
        std::fs::create_dir_all(alpha_target.parent().unwrap()).unwrap();
        let alpha_journal = store
            .write_journal(
                alpha_id,
                "archive",
                &alpha_source,
                &alpha_target,
                &alpha.key,
                Utc::now(),
            )
            .unwrap();
        std::fs::rename(&alpha_source, &alpha_target).unwrap();
        set_journal_stage(&alpha_journal, "moved").unwrap();

        let archived = store
            .entries()
            .unwrap()
            .into_iter()
            .find(|entry| entry.name == "alpha")
            .unwrap();
        assert_eq!(archived.status, CuratorSkillStatus::Archived);
        assert!(alpha_target.is_dir());

        let beta = entries.iter().find(|entry| entry.name == "beta").unwrap();
        registry.refresh_portable();
        let beta_instance = registry.inspect_instance("beta").unwrap();
        let beta_source = registry
            .instance_path(&beta_instance.instance_id)
            .unwrap()
            .to_path_buf();
        let beta_id = Uuid::new_v4();
        let beta_target = store
            .root
            .join("archive")
            .join(beta_id.to_string())
            .join("beta");
        std::fs::create_dir_all(beta_target.parent().unwrap()).unwrap();
        store
            .write_journal(
                beta_id,
                "archive",
                &beta_source,
                &beta_target,
                &beta.key,
                Utc::now(),
            )
            .unwrap();
        std::fs::rename(&beta_source, &beta_target).unwrap();

        store.sync(&mut registry, &BTreeMap::new()).unwrap();
        assert!(beta_source.is_dir());
        assert!(!beta_target.exists());
        assert!(registry.inspect_instance("beta").is_ok());

        let archive = archived.archive.clone().unwrap();
        let restore_journal = store
            .write_journal(
                archive.id,
                "restore",
                &archive.archive_path,
                &archive.original_path,
                &archived.key,
                archive.archived_at,
            )
            .unwrap();
        std::fs::rename(&archive.archive_path, &archive.original_path).unwrap();
        set_journal_stage(&restore_journal, "moved").unwrap();
        store.sync(&mut registry, &BTreeMap::new()).unwrap();
        assert!(registry.inspect_instance("alpha").is_ok());
        assert_eq!(
            store
                .entries()
                .unwrap()
                .into_iter()
                .find(|entry| entry.name == "alpha")
                .unwrap()
                .status,
            CuratorSkillStatus::Active
        );
    }

    #[test]
    fn consolidate_and_adopt_create_valid_native_skills() {
        let temp = tempfile::tempdir().unwrap();
        let mut registry = registry(&temp);
        let store = SkillCuratorStore::new(temp.path().join("curator"), true);
        store
            .consolidate(
                &mut registry,
                &["alpha".into(), "beta".into()],
                "umbrella",
                SkillSourceScope::User,
            )
            .unwrap();
        assert!(registry.inspect_instance("umbrella").is_ok());
        store
            .adopt(
                &mut registry,
                "alpha",
                "adopted-alpha",
                SkillSourceScope::User,
            )
            .unwrap();
        assert!(registry.inspect_instance("adopted-alpha").is_ok());
    }

    #[test]
    fn automatic_policy_is_opt_in_bounded_and_deterministic() {
        let temp = tempfile::tempdir().unwrap();
        let mut registry = registry(&temp);
        let store = SkillCuratorStore::new(temp.path().join("curator"), true);
        let now = Utc::now();
        store.sync(&mut registry, &BTreeMap::new()).unwrap();
        let mut state = store.load().unwrap();
        for entry in state.entries.values_mut() {
            entry.first_seen_at = now - Duration::days(60);
        }
        store.save(&state).unwrap();
        let policy = CuratorPolicy {
            enabled: true,
            archive_after_days: 30,
            max_mutations: 1,
            eligible_scopes: [SkillSourceScope::User].into_iter().collect(),
        };
        let receipt = store
            .run_automatic(
                &mut registry,
                &policy,
                &CuratorAutomaticInput::default(),
                now,
            )
            .unwrap();
        assert_eq!(receipt.archived, ["user:user/alpha:alpha"]);
    }

    #[test]
    fn automatic_policy_executes_explicit_actions_deterministically() {
        let temp = tempfile::tempdir().unwrap();
        let mut registry = registry(&temp);
        let store = SkillCuratorStore::new(temp.path().join("curator"), true);
        let policy = CuratorPolicy {
            enabled: true,
            archive_after_days: 30,
            max_mutations: 3,
            eligible_scopes: [SkillSourceScope::User].into_iter().collect(),
        };
        let receipt = store
            .run_automatic(
                &mut registry,
                &policy,
                &CuratorAutomaticInput {
                    last_used: BTreeMap::new(),
                    reactivate: BTreeSet::new(),
                    actions: vec![
                        CuratorAutomaticAction::Pin {
                            selector: "alpha".into(),
                        },
                        CuratorAutomaticAction::Consolidate {
                            selectors: vec!["alpha".into(), "beta".into()],
                            name: "umbrella".into(),
                            scope: SkillSourceScope::User,
                        },
                        CuratorAutomaticAction::Adopt {
                            selector: "alpha".into(),
                            target_name: "adopted-alpha".into(),
                            target_scope: SkillSourceScope::User,
                        },
                    ],
                },
                Utc::now(),
            )
            .unwrap();
        assert_eq!(receipt.adopted, ["adopted-alpha"]);
        assert_eq!(receipt.consolidated, ["umbrella"]);
        assert_eq!(receipt.pinned, ["user:user/alpha:alpha"]);
        assert!(registry.inspect_instance("adopted-alpha").is_ok());
        assert!(registry.inspect_instance("umbrella").is_ok());
        assert!(store
            .entries()
            .unwrap()
            .into_iter()
            .any(|entry| entry.name == "adopted-alpha"));
    }
}
