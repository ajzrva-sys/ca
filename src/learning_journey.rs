//! Owner-only, metadata-only skill and learning lifecycle ledger.
//!
//! This file never stores prompts, transcript excerpts, tool arguments/results,
//! memory text, or secrets. Existing mutation journals remain authoritative.

use crate::skills::{SkillLoadSet, SkillRegistry};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

const MAX_EVENTS: usize = 20_000;
const MAX_LEDGER_BYTES: u64 = 8 * 1024 * 1024;
const STALE_LOCK_SECS: u64 = 60;
const LOCK_RETRY_ATTEMPTS: usize = 500;
const MAX_GRAPH_BYTES: usize = 64 * 1024;
const MAX_GRAPH_SKILLS: usize = 200;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LearningEventKind {
    SkillInvocation,
    SkillCompletion,
    SkillFailure,
    SkillCorrection,
    SkillRefinement,
    BundleInvocation,
    MemoryChange,
    LearningRun,
    AssetMutation,
    Undo,
    JournalRecovery,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct LearningEvent {
    pub id: String,
    pub at: DateTime<Utc>,
    pub kind: LearningEventKind,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<uuid::Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub journal_id: Option<uuid::Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_digest: Option<String>,
    #[serde(default)]
    pub related_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LedgerFile {
    version: u32,
    #[serde(default)]
    events: Vec<LearningEvent>,
}

#[derive(Debug, Clone)]
pub(crate) struct LearningLedger {
    path: PathBuf,
    writable: bool,
    process_lock: Arc<Mutex<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillOutcome {
    Completed,
    Failed,
    Cancelled,
}

impl LearningLedger {
    pub(crate) fn new(path: PathBuf, writable: bool) -> Self {
        let process_lock = process_lock_for(&path);
        Self {
            path,
            writable,
            process_lock,
        }
    }

    pub(crate) fn record_skill_invocation(
        &self,
        loads: &SkillLoadSet,
        registry: &SkillRegistry,
        workspace_digest: Option<String>,
        correction: bool,
    ) -> Result<Option<String>, String> {
        if loads.is_empty() {
            return Ok(None);
        }
        let group_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let mut events = Vec::new();
        let load_count = loads.iter().count();
        for load in loads.iter() {
            let instance = registry
                .inspect_instance(&load.instance_id)
                .map_err(|error| error.to_string())?;
            events.push(skill_event(
                now,
                LearningEventKind::SkillInvocation,
                "started",
                &group_id,
                instance,
                workspace_digest.clone(),
            ));
            if correction && load_count == 1 {
                events.push(skill_event(
                    now,
                    LearningEventKind::SkillCorrection,
                    "explicit_correction",
                    &group_id,
                    instance,
                    workspace_digest.clone(),
                ));
            }
        }
        if load_count > 1 {
            events.push(LearningEvent {
                id: event_id(&format!("bundle-invocation:{group_id}")),
                at: now,
                kind: LearningEventKind::BundleInvocation,
                outcome: if correction {
                    "skill_set_correction"
                } else {
                    "skill_set_started"
                }
                .into(),
                group_id: Some(group_id.clone()),
                skill_instance_id: None,
                skill_name: None,
                skill_digest: None,
                bundle_name: None,
                run_id: None,
                journal_id: None,
                workspace_digest,
                related_ids: loads.iter().map(|load| load.instance_id.clone()).collect(),
            });
        }
        self.append(events)?;
        Ok(Some(group_id))
    }

    pub(crate) fn record_skill_outcome(
        &self,
        group_id: Option<&str>,
        loads: &SkillLoadSet,
        registry: &SkillRegistry,
        outcome: SkillOutcome,
        workspace_digest: Option<String>,
    ) -> Result<(), String> {
        let Some(group_id) = group_id else {
            return Ok(());
        };
        let (kind, label) = match outcome {
            SkillOutcome::Completed => (LearningEventKind::SkillCompletion, "completed"),
            SkillOutcome::Failed => (LearningEventKind::SkillFailure, "failed"),
            SkillOutcome::Cancelled => (LearningEventKind::SkillFailure, "cancelled"),
        };
        let now = Utc::now();
        let mut events = Vec::new();
        let load_count = loads.iter().count();
        if outcome != SkillOutcome::Failed || load_count == 1 {
            for load in loads.iter() {
                let instance = registry
                    .inspect_instance(&load.instance_id)
                    .map_err(|error| error.to_string())?;
                events.push(skill_event(
                    now,
                    kind,
                    label,
                    group_id,
                    instance,
                    workspace_digest.clone(),
                ));
            }
        }
        if load_count > 1 {
            events.push(LearningEvent {
                id: event_id(&format!("bundle-outcome:{group_id}:{label}")),
                at: now,
                kind: LearningEventKind::BundleInvocation,
                outcome: format!("skill_set_{label}"),
                group_id: Some(group_id.into()),
                skill_instance_id: None,
                skill_name: None,
                skill_digest: None,
                bundle_name: None,
                run_id: None,
                journal_id: None,
                workspace_digest,
                related_ids: loads.iter().map(|load| load.instance_id.clone()).collect(),
            });
        }
        self.append(events)
    }

    pub(crate) fn record_named_bundle_invocation(
        &self,
        group_id: &str,
        name: &str,
        digest: &str,
        loads: &SkillLoadSet,
    ) -> Result<(), String> {
        self.append(vec![LearningEvent {
            id: event_id(&format!("named-bundle:{group_id}:{name}:{digest}")),
            at: Utc::now(),
            kind: LearningEventKind::BundleInvocation,
            outcome: "bundle_started".into(),
            group_id: Some(group_id.into()),
            skill_instance_id: None,
            skill_name: None,
            skill_digest: Some(digest.into()),
            bundle_name: Some(name.into()),
            run_id: None,
            journal_id: None,
            workspace_digest: None,
            related_ids: loads.iter().map(|load| load.instance_id.clone()).collect(),
        }])
    }

    pub(crate) fn record_skill_mutation(
        &self,
        outcome: &str,
        name: &str,
        instance_id: &str,
        digest: &str,
        journal_id: uuid::Uuid,
    ) -> Result<String, String> {
        let id = event_id(&format!(
            "skill-mutation:{journal_id}:{instance_id}:{digest}:{outcome}"
        ));
        self.append(vec![LearningEvent {
            id: id.clone(),
            at: Utc::now(),
            kind: LearningEventKind::SkillRefinement,
            outcome: bounded_label(outcome),
            group_id: None,
            skill_instance_id: Some(instance_id.into()),
            skill_name: Some(bounded_label(name)),
            skill_digest: Some(digest.into()),
            bundle_name: None,
            run_id: None,
            journal_id: Some(journal_id),
            workspace_digest: None,
            related_ids: Vec::new(),
        }])?;
        Ok(id)
    }

    pub(crate) fn record_bundle_mutation(
        &self,
        outcome: &str,
        name: &str,
        digest: &str,
        journal_id: uuid::Uuid,
        related_ids: Vec<String>,
    ) -> Result<String, String> {
        let id = event_id(&format!(
            "bundle-mutation:{journal_id}:{name}:{digest}:{outcome}"
        ));
        self.append(vec![LearningEvent {
            id: id.clone(),
            at: Utc::now(),
            kind: LearningEventKind::AssetMutation,
            outcome: bounded_label(outcome),
            group_id: None,
            skill_instance_id: None,
            skill_name: None,
            skill_digest: Some(digest.into()),
            bundle_name: Some(bounded_label(name)),
            run_id: None,
            journal_id: Some(journal_id),
            workspace_digest: None,
            related_ids: related_ids.into_iter().take(5).collect(),
        }])?;
        Ok(id)
    }

    pub(crate) fn record_metadata_event(
        &self,
        kind: LearningEventKind,
        outcome: &str,
        run_id: Option<uuid::Uuid>,
        journal_id: Option<uuid::Uuid>,
        related_ids: Vec<String>,
    ) -> Result<String, String> {
        let id = if run_id.is_some() || journal_id.is_some() {
            event_id(&format!(
                "{kind:?}:{outcome}:{}:{}",
                run_id.map_or_else(String::new, |id| id.to_string()),
                journal_id.map_or_else(String::new, |id| id.to_string())
            ))
        } else {
            event_id(&format!("{kind:?}:{}", uuid::Uuid::new_v4()))
        };
        self.append(vec![LearningEvent {
            id: id.clone(),
            at: Utc::now(),
            kind,
            outcome: bounded_label(outcome),
            group_id: None,
            skill_instance_id: None,
            skill_name: None,
            skill_digest: None,
            bundle_name: None,
            run_id,
            journal_id,
            workspace_digest: None,
            related_ids: related_ids.into_iter().take(32).collect(),
        }])?;
        Ok(id)
    }

    pub(crate) fn events(&self) -> Result<Vec<LearningEvent>, String> {
        Ok(self.load()?.events)
    }

    pub(crate) fn skill_last_used(&self) -> Result<BTreeMap<String, DateTime<Utc>>, String> {
        let mut used = BTreeMap::new();
        for event in self.events()? {
            if event.kind == LearningEventKind::SkillInvocation {
                if let Some(instance_id) = event.skill_instance_id {
                    used.entry(instance_id)
                        .and_modify(|at: &mut DateTime<Utc>| *at = (*at).max(event.at))
                        .or_insert(event.at);
                }
            }
        }
        Ok(used)
    }

    #[cfg(test)]
    pub(crate) fn render(
        &self,
        filter: &str,
        registry: &SkillRegistry,
        memory_count: usize,
        legacy_journal_count: usize,
    ) -> Result<String, String> {
        self.render_with_memories(
            filter,
            registry,
            &[],
            &[],
            memory_count,
            legacy_journal_count,
        )
    }

    pub(crate) fn render_with_memories(
        &self,
        filter: &str,
        registry: &SkillRegistry,
        user_memories: &[crate::memory::MemoryRecord],
        workspace_memories: &[crate::memory::MemoryRecord],
        memory_count: usize,
        legacy_journal_count: usize,
    ) -> Result<String, String> {
        let events = self.events()?;
        let filter = filter.trim();
        if !filter.is_empty()
            && !matches!(
                filter,
                "skills" | "memory" | "runs" | "suggestions" | "graph"
            )
            && !filter.starts_with("skill ")
        {
            if let Some(event) = events.iter().find(|event| event.id == filter) {
                return Ok(render_event_detail(event));
            }
            return Err(format!("unknown learning event: {filter}"));
        }
        let mut lines = vec![
            "Learning journey".to_string(),
            "Metadata only · prompts, transcripts, tool data, and secrets are never recorded."
                .into(),
            String::new(),
        ];
        if filter == "suggestions" {
            lines.extend(render_suggestions(&events, registry));
            return Ok(lines.join("\n"));
        }
        if filter == "graph" {
            return Ok(render_graph(
                &events,
                registry,
                user_memories,
                workspace_memories,
            ));
        }
        let selected_skill = filter
            .strip_prefix("skill ")
            .map(str::trim)
            .map(|selector| {
                registry
                    .inspect_instance(selector)
                    .map(|instance| instance.instance_id.clone())
                    .map_err(|error| error.to_string())
            })
            .transpose()?;
        let filtered = events.iter().filter(|event| {
            if let Some(instance_id) = selected_skill.as_deref() {
                return event.skill_instance_id.as_deref() == Some(instance_id);
            }
            match filter {
                "skills" => matches!(
                    event.kind,
                    LearningEventKind::SkillInvocation
                        | LearningEventKind::SkillCompletion
                        | LearningEventKind::SkillFailure
                        | LearningEventKind::SkillCorrection
                        | LearningEventKind::SkillRefinement
                        | LearningEventKind::BundleInvocation
                ),
                "memory" => event.kind == LearningEventKind::MemoryChange,
                "runs" => event.kind == LearningEventKind::LearningRun,
                _ => true,
            }
        });
        let mut shown = 0usize;
        for event in filtered.rev().take(100) {
            shown += 1;
            lines.push(format!(
                "{} · {} · {:?} · {}{}",
                event.id,
                event.at.to_rfc3339(),
                event.kind,
                event.outcome,
                event
                    .skill_name
                    .as_ref()
                    .map(|name| format!(" · {name}"))
                    .unwrap_or_default()
            ));
        }
        if shown == 0 {
            lines.push("No matching lifecycle events.".into());
        }
        lines.push(String::new());
        lines.push(format!(
            "Legacy inventory (not fabricated history): {} discovered skill(s) · {} memory record(s) · {} existing learning/mutation journal(s)",
            registry.instances().count(),
            memory_count,
            legacy_journal_count
        ));
        lines.push("Suggestions: /learning suggestions".into());
        Ok(lines.join("\n"))
    }

    fn append(&self, additions: Vec<LearningEvent>) -> Result<(), String> {
        if additions.is_empty() || !self.writable {
            return Ok(());
        }
        let _guard = self
            .process_lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "learning ledger has no parent".to_string())?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let lock_path = self.path.with_extension("lock");
        let lock = acquire_ledger_lock(&lock_path)?;
        let result = (|| -> Result<(), String> {
            let mut file = self.load()?;
            let known = file
                .events
                .iter()
                .map(|event| event.id.clone())
                .collect::<BTreeSet<_>>();
            file.events.extend(
                additions
                    .into_iter()
                    .filter(|event| !known.contains(&event.id)),
            );
            if file.events.len() > MAX_EVENTS {
                let drain = file.events.len() - MAX_EVENTS;
                file.events.drain(..drain);
            }
            write_atomic(
                &self.path,
                &serde_json::to_vec_pretty(&file).map_err(|error| error.to_string())?,
            )
        })();
        drop(lock);
        let _ = fs::remove_file(lock_path);
        result
    }

    fn load(&self) -> Result<LedgerFile, String> {
        if !self.path.exists() {
            return Ok(LedgerFile {
                version: 1,
                events: Vec::new(),
            });
        }
        let metadata = fs::symlink_metadata(&self.path).map_err(|error| error.to_string())?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_LEDGER_BYTES
        {
            return Err("learning ledger must be a bounded owner file".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.mode() & 0o077 != 0 || metadata.nlink() != 1 {
                return Err("learning ledger must be an owner-only single-link file".into());
            }
        }
        let file: LedgerFile =
            serde_json::from_slice(&fs::read(&self.path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        if file.version != 1 || file.events.len() > MAX_EVENTS {
            return Err("learning ledger has an unsupported shape".into());
        }
        Ok(file)
    }
}

fn process_lock_for(path: &Path) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();
    let locks = LOCKS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut locks = locks.lock().unwrap_or_else(|poison| poison.into_inner());
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(path).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(path.to_path_buf(), Arc::downgrade(&lock));
    lock
}

fn skill_event(
    at: DateTime<Utc>,
    kind: LearningEventKind,
    outcome: &str,
    group_id: &str,
    instance: &crate::skills::SkillInstance,
    workspace_digest: Option<String>,
) -> LearningEvent {
    LearningEvent {
        id: event_id(&format!(
            "{kind:?}:{group_id}:{}:{}",
            instance.instance_id, instance.content_digest
        )),
        at,
        kind,
        outcome: outcome.into(),
        group_id: Some(group_id.into()),
        skill_instance_id: Some(instance.instance_id.clone()),
        skill_name: Some(instance.metadata.name.clone()),
        skill_digest: Some(instance.content_digest.clone()),
        bundle_name: None,
        run_id: None,
        journal_id: None,
        workspace_digest,
        related_ids: Vec::new(),
    }
}

fn event_id(key: &str) -> String {
    format!(
        "evt-{}",
        &format!("{:x}", Sha256::digest(key.as_bytes()))[..20]
    )
}

fn bounded_label(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(160)
        .collect()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > MAX_LEDGER_BYTES {
        return Err("learning ledger exceeds its byte bound".into());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "learning ledger has no parent".to_string())?;
    let temp = parent.join(format!(".journey-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<(), String> {
        let mut file = options.open(&temp).map_err(|error| error.to_string())?;
        file.write_all(bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        fs::rename(&temp, path).map_err(|error| error.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

fn acquire_ledger_lock(path: &Path) -> Result<fs::File, String> {
    fn create(path: &Path) -> std::io::Result<fs::File> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        writeln!(file, "{now}")?;
        file.sync_all()?;
        Ok(file)
    }

    for attempt in 0..=LOCK_RETRY_ATTEMPTS {
        match create(path) {
            Ok(file) => return Ok(file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = match fs::symlink_metadata(path) {
                    Ok(metadata) => metadata,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::NotFound
                            && attempt < LOCK_RETRY_ATTEMPTS =>
                    {
                        std::thread::yield_now();
                        continue;
                    }
                    Err(error) => return Err(error.to_string()),
                };
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err("learning ledger lock is not a regular owner file".into());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if metadata.mode() & 0o077 != 0 {
                        return Err("learning ledger lock permissions are not owner-only".into());
                    }
                }
                let created = fs::read_to_string(path)
                    .ok()
                    .and_then(|value| value.trim().parse::<u64>().ok());
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                if created.is_none_or(|created| now.saturating_sub(created) <= STALE_LOCK_SECS) {
                    if attempt < LOCK_RETRY_ATTEMPTS {
                        std::thread::sleep(StdDuration::from_millis(2));
                        continue;
                    }
                    return Err(
                        "learning ledger is busy; lifecycle event was not attributed".into(),
                    );
                }
                fs::remove_file(path).map_err(|error| error.to_string())?;
            }
            Err(_) => {
                return Err("learning ledger is busy; lifecycle event was not attributed".into());
            }
        }
    }
    Err("learning ledger is busy; lifecycle event was not attributed".into())
}

fn render_event_detail(event: &LearningEvent) -> String {
    let mut lines = vec![
        format!("Learning event {}", event.id),
        format!("At: {}", event.at.to_rfc3339()),
        format!("Kind: {:?}", event.kind),
        format!("Outcome: {}", event.outcome),
    ];
    for (label, value) in [
        ("Group", event.group_id.as_deref()),
        ("Skill instance", event.skill_instance_id.as_deref()),
        ("Skill digest", event.skill_digest.as_deref()),
        ("Bundle", event.bundle_name.as_deref()),
        ("Workspace", event.workspace_digest.as_deref()),
        (
            "Run",
            event
                .run_id
                .as_ref()
                .map(uuid::Uuid::as_hyphenated)
                .map(|value| value.to_string())
                .as_deref(),
        ),
        (
            "Journal",
            event
                .journal_id
                .as_ref()
                .map(uuid::Uuid::as_hyphenated)
                .map(|value| value.to_string())
                .as_deref(),
        ),
    ] {
        if let Some(value) = value {
            lines.push(format!("{label}: {value}"));
        }
    }
    if !event.related_ids.is_empty() {
        lines.push(format!("Related IDs: {}", event.related_ids.join(", ")));
    }
    if let Some(run_id) = event.run_id {
        lines.push(format!("Audit: /learn status {run_id}"));
        lines.push(format!("Undo: /learn undo {run_id}"));
    }
    lines.join("\n")
}

fn render_suggestions(events: &[LearningEvent], registry: &SkillRegistry) -> Vec<String> {
    let now = Utc::now();
    let recent = now - Duration::days(30);
    let mut lines = vec![
        "Maintenance suggestions (never applied automatically)".into(),
        String::new(),
    ];
    let mut by_digest = BTreeMap::<(String, String), (usize, usize, Option<DateTime<Utc>>)>::new();
    for event in events {
        let (Some(name), Some(digest)) = (&event.skill_name, &event.skill_digest) else {
            continue;
        };
        let entry = by_digest
            .entry((name.clone(), digest.clone()))
            .or_insert((0, 0, None));
        if event.at >= recent
            && event.kind == LearningEventKind::SkillFailure
            && event.outcome == "failed"
        {
            entry.0 += 1;
        }
        if event.at >= recent && event.kind == LearningEventKind::SkillCorrection {
            entry.1 += 1;
        }
        if event.kind == LearningEventKind::SkillCompletion {
            entry.2 = Some(entry.2.map_or(event.at, |last| last.max(event.at)));
        }
    }
    for ((name, digest), (failures, corrections, last_success)) in by_digest {
        if corrections >= 1 || failures >= 2 {
            lines.push(format!(
                "Refine {name} [{}]: /learn refine skill {name} using attributed failures and corrections",
                &digest[..12.min(digest.len())]
            ));
        }
        if last_success.is_some_and(|last| last < now - Duration::days(90)) {
            lines.push(format!(
                "Review stale skill {name}: /learn review stale skill {name}"
            ));
        }
    }
    let mut groups = BTreeMap::<Vec<String>, usize>::new();
    for event in events.iter().filter(|event| {
        event.at >= recent
            && event.kind == LearningEventKind::BundleInvocation
            && event.outcome == "skill_set_completed"
            && (2..=5).contains(&event.related_ids.len())
    }) {
        let mut ids = event.related_ids.clone();
        ids.sort();
        *groups.entry(ids).or_default() += 1;
    }
    for (ids, _count) in groups.into_iter().filter(|(_, count)| *count >= 3) {
        let names = ids
            .iter()
            .filter_map(|id| registry.inspect_instance(id).ok())
            .map(|instance| instance.qualified_name.clone())
            .collect::<Vec<_>>();
        if names.len() == ids.len() {
            lines.push(format!(
                "Bundle repeated skill set: /bundles create suggested-bundle {}",
                names.join(" ")
            ));
        }
    }
    if lines.len() == 2 {
        lines.push("No deterministic maintenance suggestions.".into());
    }
    lines
}

#[derive(Debug, Default)]
struct SkillGraphStats {
    uses: usize,
    completed: usize,
    failed: usize,
    last_activity: Option<DateTime<Utc>>,
}

fn render_graph(
    events: &[LearningEvent],
    registry: &SkillRegistry,
    user_memories: &[crate::memory::MemoryRecord],
    workspace_memories: &[crate::memory::MemoryRecord],
) -> String {
    let instances = registry
        .instances()
        .take(MAX_GRAPH_SKILLS)
        .collect::<Vec<_>>();
    let current = instances
        .iter()
        .map(|instance| {
            (
                instance.instance_id.as_str(),
                (instance.content_digest.as_str(), *instance),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut stats = BTreeMap::<String, SkillGraphStats>::new();
    let mut successful_groups = BTreeMap::<String, BTreeSet<String>>::new();
    for event in events {
        let (Some(instance_id), Some(digest)) = (
            event.skill_instance_id.as_deref(),
            event.skill_digest.as_deref(),
        ) else {
            continue;
        };
        let Some((current_digest, _instance)) = current.get(instance_id) else {
            continue;
        };
        if digest != *current_digest {
            continue;
        }
        let entry = stats.entry(instance_id.to_string()).or_default();
        entry.last_activity = Some(
            entry
                .last_activity
                .map_or(event.at, |last| last.max(event.at)),
        );
        match event.kind {
            LearningEventKind::SkillInvocation => entry.uses += 1,
            LearningEventKind::SkillCompletion => {
                entry.completed += 1;
                if let Some(group) = &event.group_id {
                    successful_groups
                        .entry(group.clone())
                        .or_default()
                        .insert(instance_id.to_string());
                }
            }
            LearningEventKind::SkillFailure => entry.failed += 1,
            _ => {}
        }
    }

    let display_name = |instance: &crate::skills::SkillInstance| {
        if registry
            .inspect_instance(&instance.metadata.name)
            .is_ok_and(|resolved| resolved.instance_id == instance.instance_id)
        {
            instance.metadata.name.clone()
        } else {
            instance.qualified_name.clone()
        }
    };
    let mut declared_edges = BTreeSet::<(String, String)>::new();
    let mut unresolved = BTreeSet::<(String, String)>::new();
    for instance in &instances {
        let source = display_name(instance);
        for selector in &instance.metadata.related_skills {
            match registry.inspect_instance(selector) {
                Ok(target) if target.instance_id != instance.instance_id => {
                    let target = display_name(target);
                    let edge = if source <= target {
                        (source.clone(), target)
                    } else {
                        (target, source.clone())
                    };
                    declared_edges.insert(edge);
                }
                _ => {
                    unresolved.insert((source.clone(), selector.clone()));
                }
            }
        }
    }
    let mut observed_edges = BTreeMap::<(String, String), usize>::new();
    for ids in successful_groups.values() {
        let ids = ids.iter().collect::<Vec<_>>();
        for left_index in 0..ids.len() {
            for right_index in left_index + 1..ids.len() {
                let Some((_, left)) = current.get(ids[left_index].as_str()) else {
                    continue;
                };
                let Some((_, right)) = current.get(ids[right_index].as_str()) else {
                    continue;
                };
                let mut edge = (display_name(left), display_name(right));
                if edge.0 > edge.1 {
                    edge = (edge.1, edge.0);
                }
                *observed_edges.entry(edge).or_default() += 1;
            }
        }
    }

    let mut memories = user_memories
        .iter()
        .chain(workspace_memories.iter())
        .take(128)
        .collect::<Vec<_>>();
    memories.sort_by(|left, right| {
        left.scope
            .label()
            .cmp(right.scope.label())
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut memory_edges = Vec::<(usize, String, usize)>::new();
    for (memory_index, memory) in memories.iter().enumerate() {
        let memory_terms = graph_terms(&memory.text);
        for instance in &instances {
            let skill_text = format!(
                "{} {} {} {}",
                instance.metadata.name,
                instance.metadata.description,
                instance.metadata.category.as_deref().unwrap_or_default(),
                instance.metadata.related_skills.join(" ")
            );
            let skill_terms = graph_terms(&skill_text);
            let overlap = memory_terms.intersection(&skill_terms).count();
            if overlap >= 2 {
                memory_edges.push((memory_index, display_name(instance), overlap));
            }
        }
    }
    memory_edges.sort_by(|left, right| {
        right
            .2
            .cmp(&left.2)
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| left.1.cmp(&right.1))
    });
    memory_edges.truncate(256);

    let used = stats.values().filter(|item| item.uses > 0).count();
    let mut lines = vec![
        "Learning graph".to_string(),
        "Local TUI graph · bounded current skill metadata plus active-scope memory identities; no transcripts or memory bodies."
            .into(),
        format!(
            "{} skill(s) · {used} used · {} memory node(s) · {} memory link(s) · {} declared relation(s) · {} observed relation(s)",
            instances.len(),
            memories.len(),
            memory_edges.len(),
            declared_edges.len(),
            observed_edges.len()
        ),
        String::new(),
        "Skills".into(),
    ];
    for instance in &instances {
        let item = stats.get(&instance.instance_id);
        let uses = item.map_or(0, |item| item.uses);
        let completed = item.map_or(0, |item| item.completed);
        let failed = item.map_or(0, |item| item.failed);
        let category = instance.metadata.category.as_deref().unwrap_or("general");
        lines.push(format!(
            "{} · uses {uses} · completed {completed} · failed {failed} · category {category}",
            display_name(instance)
        ));
    }
    lines.push(String::new());
    lines.push("Memory nodes".into());
    for (index, memory) in memories.iter().enumerate() {
        let summary = graph_memory_summary(&memory.text);
        lines.push(format!(
            "M{:03} [{} / {}] · {}",
            index + 1,
            memory.scope.label(),
            memory.kind.label(),
            summary
        ));
    }
    if memories.is_empty() {
        lines.push("No active-scope native memory nodes.".into());
    }
    lines.push(String::new());
    lines.push("Relationships".into());
    for (left, right) in declared_edges {
        lines.push(format!("declared · {left} ↔ {right}"));
    }
    for ((left, right), count) in observed_edges {
        lines.push(format!(
            "observed · {left} ↔ {right} · {count} successful co-use{}",
            if count == 1 { "" } else { "s" }
        ));
    }
    for (source, target) in unresolved {
        lines.push(format!("unresolved · {source} → {target}"));
    }
    for (memory_index, skill, overlap) in &memory_edges {
        lines.push(format!(
            "lexical · M{:03} ──({overlap})──▶ {skill}",
            memory_index + 1
        ));
    }
    if lines.last().is_some_and(|line| line == "Relationships") {
        lines.push("No declared or observed relationships.".into());
    }
    lines.push(String::new());
    lines.push("Clusters".into());
    let mut clusters = BTreeMap::<String, (BTreeSet<String>, BTreeSet<String>)>::new();
    for instance in &instances {
        let category = instance.metadata.category.as_deref().unwrap_or("general");
        clusters
            .entry(category.to_string())
            .or_default()
            .0
            .insert(display_name(instance));
    }
    for (memory_index, skill, _) in &memory_edges {
        let category = instances
            .iter()
            .find(|instance| display_name(instance) == *skill)
            .and_then(|instance| instance.metadata.category.clone())
            .unwrap_or_else(|| "general".into());
        clusters
            .entry(category)
            .or_default()
            .1
            .insert(format!("M{:03}", memory_index + 1));
    }
    for (category, (skills, memories)) in clusters {
        lines.push(format!(
            "{} · skills [{}] · memories [{}]",
            category,
            skills.into_iter().collect::<Vec<_>>().join(", "),
            memories.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    let density_denominator = instances.len().saturating_mul(memories.len());
    lines.push(format!(
        "Graph density · {}/{} possible memory-skill links",
        memory_edges.len(),
        density_denominator
    ));
    let mut output = String::new();
    for line in lines {
        let separator = usize::from(!output.is_empty());
        if output.len() + separator + line.len() > MAX_GRAPH_BYTES {
            let marker = "\n… graph truncated at 64 KiB";
            if output.len() + marker.len() <= MAX_GRAPH_BYTES {
                output.push_str(marker);
            }
            break;
        }
        if separator == 1 {
            output.push('\n');
        }
        output.push_str(&line);
    }
    output
}

fn graph_terms(value: &str) -> BTreeSet<String> {
    const STOP: [&str; 16] = [
        "and", "are", "but", "for", "from", "into", "not", "that", "the", "their", "then", "this",
        "use", "using", "with", "your",
    ];
    value
        .split(|character: char| !character.is_alphanumeric())
        .map(str::to_ascii_lowercase)
        .filter(|term| term.len() >= 3 && !STOP.contains(&term.as_str()))
        .take(256)
        .collect()
}

fn graph_memory_summary(value: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(value.as_bytes()));
    format!(
        "metadata-only · {} normalized term(s) · digest {}",
        graph_terms(value).len(),
        &digest[..12]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::{SkillDiscoveryRoot, SkillSourceScope};

    #[test]
    fn ledger_is_metadata_only_idempotent_and_attributes_exact_digests() {
        let temp = tempfile::tempdir().unwrap();
        let skill = temp.path().join("skills/demo");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\nDo the bounded thing.\n",
        )
        .unwrap();
        let registry = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                temp.path().join("skills"),
                "test",
            )],
            &[],
        );
        let mut loads = SkillLoadSet::default();
        registry.load_for_task("demo", true, &mut loads).unwrap();
        let ledger = LearningLedger::new(temp.path().join("journey.json"), true);
        let group = ledger
            .record_skill_invocation(&loads, &registry, Some("workspace".into()), true)
            .unwrap();
        ledger
            .record_skill_outcome(
                group.as_deref(),
                &loads,
                &registry,
                SkillOutcome::Failed,
                Some("workspace".into()),
            )
            .unwrap();
        let events = ledger.events().unwrap();
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|event| event.skill_digest.is_some()));
        let qualified = registry
            .inspect_instance("demo")
            .unwrap()
            .qualified_name
            .clone();
        let filtered = ledger
            .render(&format!("skill {qualified}"), &registry, 0, 0)
            .unwrap();
        assert!(filtered.contains("demo"));
        let detail = ledger.render(&events[0].id, &registry, 0, 0).unwrap();
        assert!(detail.contains("Skill digest:"));
        assert!(detail.contains("Workspace: workspace"));
        let raw = fs::read_to_string(temp.path().join("journey.json")).unwrap();
        assert!(!raw.contains("Do the bounded thing"));
    }

    #[test]
    fn concurrent_writers_preserve_every_metadata_event() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("journey.json");
        let ledger = LearningLedger::new(path.clone(), true);
        let threads = (0..8)
            .map(|index| {
                let ledger = LearningLedger::new(path.clone(), true);
                std::thread::spawn(move || {
                    ledger
                        .record_metadata_event(
                            LearningEventKind::LearningRun,
                            &format!("run_{index}"),
                            Some(uuid::Uuid::from_u128(index + 1)),
                            None,
                            Vec::new(),
                        )
                        .unwrap();
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(ledger.events().unwrap().len(), 8);
    }

    #[test]
    fn ledger_handles_for_one_path_share_the_process_lane() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("journey.json");
        let first = LearningLedger::new(path.clone(), true);
        let second = LearningLedger::new(path, true);
        let other = LearningLedger::new(temp.path().join("other.json"), true);
        assert!(Arc::ptr_eq(&first.process_lock, &second.process_lock));
        assert!(!Arc::ptr_eq(&first.process_lock, &other.process_lock));
    }

    #[test]
    fn journal_linked_metadata_events_are_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = LearningLedger::new(temp.path().join("journey.json"), true);
        let journal = uuid::Uuid::new_v4();
        for _ in 0..2 {
            ledger
                .record_metadata_event(
                    LearningEventKind::AssetMutation,
                    "bundle_created",
                    None,
                    Some(journal),
                    vec!["bundle".into()],
                )
                .unwrap();
        }
        assert_eq!(ledger.events().unwrap().len(), 1);
    }

    #[test]
    fn mutation_events_pin_digest_locator_and_journal_without_payloads() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = LearningLedger::new(temp.path().join("journey.json"), true);
        let skill_journal = uuid::Uuid::new_v4();
        let skill_digest = "a".repeat(64);
        for _ in 0..2 {
            ledger
                .record_skill_mutation(
                    "committed",
                    "demo",
                    "instance-1",
                    &skill_digest,
                    skill_journal,
                )
                .unwrap();
        }
        let bundle_journal = uuid::Uuid::new_v4();
        ledger
            .record_bundle_mutation(
                "bundle_created",
                "release",
                &"b".repeat(64),
                bundle_journal,
                vec!["instance-1".into(), "instance-2".into()],
            )
            .unwrap();
        let events = ledger.events().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].journal_id, Some(skill_journal));
        assert_eq!(events[0].skill_instance_id.as_deref(), Some("instance-1"));
        assert_eq!(
            events[0].skill_digest.as_deref(),
            Some(skill_digest.as_str())
        );
        assert_eq!(events[1].journal_id, Some(bundle_journal));
        assert_eq!(events[1].bundle_name.as_deref(), Some("release"));
        assert_eq!(events[1].related_ids, ["instance-1", "instance-2"]);
    }

    #[test]
    fn stale_owner_lock_is_recovered_without_dropping_the_event() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("journey.json");
        let lock = path.with_extension("lock");
        fs::write(&lock, "0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let ledger = LearningLedger::new(path, true);
        ledger
            .record_metadata_event(
                LearningEventKind::JournalRecovery,
                "recovered",
                None,
                None,
                Vec::new(),
            )
            .unwrap();
        assert_eq!(ledger.events().unwrap().len(), 1);
        assert!(!lock.exists());
    }

    #[test]
    fn suggestions_follow_exact_failure_correction_staleness_and_bundle_thresholds() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills");
        for name in ["alpha", "beta", "gamma"] {
            let skill = root.join(name);
            fs::create_dir_all(&skill).unwrap();
            fs::write(
                skill.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {name}\n---\nUse {name}.\n"),
            )
            .unwrap();
        }
        let registry = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                &root,
                "test",
            )],
            &[],
        );
        let ledger = LearningLedger::new(temp.path().join("journey.json"), true);

        let mut alpha = SkillLoadSet::default();
        registry.load_for_task("alpha", true, &mut alpha).unwrap();
        for _ in 0..2 {
            let group = ledger
                .record_skill_invocation(&alpha, &registry, None, false)
                .unwrap();
            ledger
                .record_skill_outcome(
                    group.as_deref(),
                    &alpha,
                    &registry,
                    SkillOutcome::Failed,
                    None,
                )
                .unwrap();
        }
        let correction = ledger
            .record_skill_invocation(&alpha, &registry, None, true)
            .unwrap();
        ledger
            .record_skill_outcome(
                correction.as_deref(),
                &alpha,
                &registry,
                SkillOutcome::Completed,
                None,
            )
            .unwrap();
        let alpha_instance = registry.inspect_instance("alpha").unwrap();
        ledger
            .append(vec![skill_event(
                Utc::now() - Duration::days(100),
                LearningEventKind::SkillCompletion,
                "completed",
                "old-success",
                alpha_instance,
                None,
            )])
            .unwrap();
        let gamma_instance = registry.inspect_instance("gamma").unwrap();
        ledger
            .append(vec![skill_event(
                Utc::now() - Duration::days(100),
                LearningEventKind::SkillCompletion,
                "completed",
                "old-gamma-success",
                gamma_instance,
                None,
            )])
            .unwrap();

        let mut pair = alpha.clone();
        registry.load_for_task("beta", true, &mut pair).unwrap();
        let failed_group = ledger
            .record_skill_invocation(&pair, &registry, None, false)
            .unwrap();
        ledger
            .record_skill_outcome(
                failed_group.as_deref(),
                &pair,
                &registry,
                SkillOutcome::Failed,
                None,
            )
            .unwrap();
        for _ in 0..3 {
            let group = ledger
                .record_skill_invocation(&pair, &registry, None, false)
                .unwrap();
            ledger
                .record_skill_outcome(
                    group.as_deref(),
                    &pair,
                    &registry,
                    SkillOutcome::Completed,
                    None,
                )
                .unwrap();
        }

        let events = ledger.events().unwrap();
        assert!(!events.iter().any(|event| {
            event.group_id.as_deref() == failed_group.as_deref()
                && event.kind == LearningEventKind::SkillFailure
                && event.skill_instance_id.is_some()
        }));
        let suggestions = ledger.render("suggestions", &registry, 0, 0).unwrap();
        assert!(suggestions.contains("Refine alpha"));
        assert!(suggestions.contains("Review stale skill gamma"));
        assert!(suggestions.contains("/bundles create suggested-bundle"));
    }

    #[test]
    fn graph_combines_current_usage_declared_links_and_successful_co_use() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills");
        for (name, related) in [
            ("alpha", "related_skills: [beta, missing]\n"),
            ("beta", "category: verification\n"),
        ] {
            let skill = root.join(name);
            fs::create_dir_all(&skill).unwrap();
            fs::write(
                skill.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {name}\n{related}---\nUse {name}.\n"),
            )
            .unwrap();
        }
        let registry = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                &root,
                "test",
            )],
            &[],
        );
        let ledger = LearningLedger::new(temp.path().join("journey.json"), true);
        let mut pair = SkillLoadSet::default();
        registry.load_for_task("alpha", true, &mut pair).unwrap();
        registry.load_for_task("beta", true, &mut pair).unwrap();
        for _ in 0..2 {
            let group = ledger
                .record_skill_invocation(&pair, &registry, None, false)
                .unwrap();
            ledger
                .record_skill_outcome(
                    group.as_deref(),
                    &pair,
                    &registry,
                    SkillOutcome::Completed,
                    None,
                )
                .unwrap();
        }
        let alpha = registry.inspect_instance("alpha").unwrap();
        ledger
            .append(vec![LearningEvent {
                id: "stale-digest".into(),
                at: Utc::now(),
                kind: LearningEventKind::SkillFailure,
                outcome: "failed".into(),
                group_id: Some("stale".into()),
                skill_instance_id: Some(alpha.instance_id.clone()),
                skill_name: Some(alpha.metadata.name.clone()),
                skill_digest: Some("0".repeat(64)),
                bundle_name: None,
                run_id: None,
                journal_id: None,
                workspace_digest: None,
                related_ids: Vec::new(),
            }])
            .unwrap();

        let memory = crate::memory::MemoryRecord::new(
            crate::memory::MemoryScope::User,
            crate::memory::MemoryKind::UserWorkflow,
            "Use the beta verification procedure for release checks.",
            crate::memory::MemoryProvenance::Manual,
        )
        .unwrap();
        let graph = ledger
            .render_with_memories("graph", &registry, &[memory], &[], 1, 0)
            .unwrap();
        assert!(graph.contains("Learning graph"));
        assert!(graph.contains("alpha · uses 2 · completed 2 · failed 0"));
        assert!(graph.contains("declared · alpha ↔ beta"));
        assert!(graph.contains("observed · alpha ↔ beta · 2 successful co-uses"));
        assert!(graph.contains("unresolved · alpha → missing"));
        assert!(graph.contains("M001 [user / workflow]"));
        assert!(graph.contains("metadata-only"));
        assert!(!graph.contains("release checks"));
        assert!(graph.contains("lexical · M001"));
        assert!(graph.contains("Clusters"));
        assert!(graph.contains("Graph density"));
        assert!(graph.len() <= 64 * 1024);
    }
}
