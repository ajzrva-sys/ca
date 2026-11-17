#![allow(dead_code)]
use nucleo::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const MAX_DISCOVERED_PATHS: usize = 50_000;
pub const MAX_PICKER_MATCHES: usize = 250;
pub const MAX_FRECENCY_RECORDS: usize = 1_000;
pub const MAX_COMPOSER_ATTACHMENTS: usize = 32;
pub const MAX_PICKER_QUERY_BYTES: usize = 512;
const MAX_MENTIONS: usize = 10;
const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_ATTACHED_CHARS: usize = 10_000;
const MAX_RESOURCE_CONTEXT_BYTES: usize = 64 * 1024;
const MAX_FRECENCY_BYTES: usize = 256 * 1024;
/// Reuse a workspace file index for this long. There is no filesystem watcher.
pub const WARM_INDEX_TTL: Duration = Duration::from_secs(4);
/// Scoring threads kept for the process and shared by every workspace matcher.
pub const WARM_MATCHER_THREADS: usize = 2;
/// Ignore-walk threads used to feed one workspace matcher.
pub const WARM_WALK_THREADS: usize = 8;
const WARM_INDEX_LIMIT: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PickerPhase {
    #[default]
    Idle,
    Indexing,
    Matching,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootSpec {
    pub id: String,
    pub path: PathBuf,
    pub label: String,
    pub read_only: bool,
    pub primary: bool,
}

impl RootSpec {
    pub fn from_workspace(workspace: &crate::workspace::WorkspaceContext) -> Vec<Self> {
        let mut labels = HashSet::new();
        workspace
            .roots()
            .iter()
            .map(|root| {
                let base = if root.primary {
                    "workspace".to_string()
                } else {
                    root.path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .filter(|name| !name.is_empty())
                        .unwrap_or("root")
                        .to_string()
                };
                let safe = bounded_label(&base, 48);
                let label = if labels.insert(safe.clone()) {
                    safe
                } else {
                    format!("{}-{}", safe, &root.id.to_string()[..8])
                };
                Self {
                    id: root.id.to_string(),
                    path: root.path.clone(),
                    label,
                    read_only: root.read_only,
                    primary: root.primary,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFile {
    pub root_id: String,
    pub root_label: String,
    pub relative: String,
    pub reference: String,
    pub read_only: bool,
}

impl IndexedFile {
    pub fn frecency_key(&self) -> FrecencyKey {
        FrecencyKey {
            root_id: self.root_id.clone(),
            relative: self.relative.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileIndexSnapshot {
    pub generation: u64,
    pub files: Arc<Vec<IndexedFile>>,
    pub discovered: usize,
    pub truncated: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerEntry {
    File(IndexedFile),
    Mcp(crate::mcp::McpResourceInfo),
}

impl PickerEntry {
    pub fn display(&self) -> String {
        bounded_label(
            &match self {
                Self::File(file) => format!(
                    "{}{}  {}",
                    if file.read_only { "[read-only] " } else { "" },
                    file.root_label,
                    file.relative
                ),
                Self::Mcp(resource) => format!("{}  {}", resource.server, resource.name),
            },
            512,
        )
    }
}

#[derive(Debug, Clone)]
pub struct MatchSnapshot {
    pub generation: u64,
    pub entries: Vec<PickerEntry>,
    pub total_matches: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrecencyKey {
    pub root_id: String,
    pub relative: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrecencyRecord {
    pub uses: u32,
    pub last_used_unix: i64,
}

pub type FrecencyMap = BTreeMap<String, FrecencyRecord>;

fn frecency_identity(key: &FrecencyKey) -> String {
    format!("{}\0{}", key.root_id, key.relative)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrecencyFile {
    version: u8,
    records: BTreeMap<String, FrecencyRecord>,
}

#[derive(Debug, Clone)]
pub struct FrecencyStore {
    path: PathBuf,
    writable: bool,
}

impl FrecencyStore {
    pub fn new(profile_data: &Path, writable: bool) -> Self {
        Self {
            path: profile_data.join("attachments/frecency.json"),
            writable,
        }
    }

    pub fn unavailable() -> Self {
        Self {
            path: PathBuf::new(),
            writable: false,
        }
    }

    pub fn load(&self) -> Result<FrecencyMap, String> {
        if self.path.as_os_str().is_empty() || !self.path.exists() {
            return Ok(BTreeMap::new());
        }
        let metadata = fs::symlink_metadata(&self.path)
            .map_err(|error| format!("could not inspect attachment ranking data: {error}"))?;
        if !metadata.file_type().is_file() || metadata.len() as usize > MAX_FRECENCY_BYTES {
            return Err("attachment ranking data is not a bounded regular file".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.mode() & 0o077 != 0 {
                return Err("attachment ranking data is not owner-only".into());
            }
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(&self.path)
            .map_err(|error| error.to_string())?
            .take((MAX_FRECENCY_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > MAX_FRECENCY_BYTES {
            return Err("attachment ranking data exceeds 256 KiB".into());
        }
        let file: FrecencyFile = serde_json::from_slice(&bytes)
            .map_err(|_| "attachment ranking data is invalid".to_string())?;
        if file.version != 1 || file.records.len() > MAX_FRECENCY_RECORDS {
            return Err("attachment ranking data has an unsupported shape".into());
        }
        for identity in file.records.keys() {
            validate_frecency_identity(identity)?;
        }
        Ok(file.records)
    }

    pub fn record(&self, key: &FrecencyKey, now_unix: i64) -> Result<FrecencyMap, String> {
        let identity = frecency_identity(key);
        validate_frecency_identity(&identity)?;
        let mut records = self.load().unwrap_or_default();
        let record = records.entry(identity).or_insert(FrecencyRecord {
            uses: 0,
            last_used_unix: now_unix,
        });
        record.uses = record.uses.saturating_add(1);
        record.last_used_unix = now_unix;
        while records.len() > MAX_FRECENCY_RECORDS {
            let Some(oldest) = records
                .iter()
                .min_by_key(|(identity, record)| {
                    (record.last_used_unix, record.uses, identity.as_str())
                })
                .map(|(identity, _)| identity.clone())
            else {
                break;
            };
            records.remove(&oldest);
        }
        if self.writable {
            self.save(&records)?;
        }
        Ok(records)
    }

    fn save(&self, records: &FrecencyMap) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(&FrecencyFile {
            version: 1,
            records: records.clone(),
        })
        .map_err(|error| error.to_string())?;
        if bytes.len() > MAX_FRECENCY_BYTES {
            return Err("attachment ranking data exceeds 256 KiB".into());
        }
        let parent = self
            .path
            .parent()
            .ok_or("attachment ranking path has no parent")?;
        if parent.exists()
            && fs::symlink_metadata(parent)
                .map_err(|error| error.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("attachment ranking directory cannot be a symlink".into());
        }
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|error| error.to_string())?;
        }
        if self.path.exists()
            && fs::symlink_metadata(&self.path)
                .map_err(|error| error.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("attachment ranking data cannot be a symlink".into());
        }
        let temp = parent.join(format!(".frecency-{}.tmp", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp).map_err(|error| error.to_string())?;
        let result = (|| {
            file.write_all(&bytes).map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            fs::rename(&temp, &self.path).map_err(|error| error.to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

fn validate_frecency_identity(identity: &str) -> Result<(), String> {
    if identity.is_empty()
        || identity.len() > 4_096
        || identity
            .chars()
            .any(|character| character.is_control() && character != '\0')
        || !identity.contains('\0')
        || identity.split_once('\0').is_none_or(|(root, relative)| {
            root.is_empty()
                || root.len() > 64
                || relative.is_empty()
                || relative.starts_with('/')
                || Path::new(relative)
                    .components()
                    .any(|component| matches!(component, Component::ParentDir))
        })
    {
        Err("attachment ranking identity is invalid".into())
    } else {
        Ok(())
    }
}

pub fn discover_files(
    roots: &[RootSpec],
    cwd: &Path,
    generation: u64,
    cancelled: &AtomicBool,
) -> FileIndexSnapshot {
    discover_indexed(roots, cwd, generation, cancelled, false)
}

/// Same bounded, ignore-aware walk as the composer picker, including paths
/// with spaces and non-ASCII names. The picker keeps its own narrower path rule.
pub fn discover_filter_files(
    roots: &[RootSpec],
    cwd: &Path,
    generation: u64,
    cancelled: &AtomicBool,
) -> FileIndexSnapshot {
    discover_indexed(roots, cwd, generation, cancelled, true)
}

fn discover_indexed(
    roots: &[RootSpec],
    cwd: &Path,
    generation: u64,
    cancelled: &AtomicBool,
    allow_spaces: bool,
) -> FileIndexSnapshot {
    if cancelled.load(Ordering::Relaxed) {
        return FileIndexSnapshot {
            generation,
            files: Arc::new(Vec::new()),
            discovered: 0,
            truncated: false,
            error: None,
        };
    }
    let loaded = load_warm_index(roots, cwd, None, cancelled, allow_spaces);
    FileIndexSnapshot {
        generation,
        files: loaded.files,
        discovered: loaded.discovered,
        truncated: loaded.truncated,
        error: loaded.error,
    }
}

fn safe_picker_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4_096
        && !value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
}

fn safe_filter_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4_096
        && !value.chars().any(|character| character.is_control())
        && Path::new(value)
            .components()
            .all(|component| !matches!(component, Component::ParentDir))
}

/// One fuzzy matcher per workspace, filled by a background ignore walk.
///
/// Repeat searches reuse it until `generation` changes, the workspace root
/// changes, or [`WARM_INDEX_TTL`] elapses. A pool that cannot spawn browses
/// one directory level instead of walking the tree. `walks` counts ignore
/// walks for that workspace, not one-level browses.
#[allow(dead_code)]
pub fn search_warm_index(
    roots: &[RootSpec],
    cwd: &Path,
    query: &str,
    frecency: &FrecencyMap,
    generation: u64,
) -> WarmIndexSearch {
    let loaded = load_warm_index(roots, cwd, Some(generation), &AtomicBool::new(false), true);
    let matches = if loaded.shallow {
        local_file_matches(&loaded.files, query, frecency)
    } else {
        warm_runtime()
            .and_then(|runtime| {
                pooled_matches(
                    &runtime.matcher_pool,
                    &loaded.files,
                    &loaded.matchers,
                    query,
                    frecency,
                )
            })
            .unwrap_or_else(|| local_file_matches(&loaded.files, query, frecency))
    };
    WarmIndexSearch {
        generation: loaded.generation,
        walks: loaded.walks,
        shallow: loaded.shallow,
        matches,
    }
}

#[derive(Debug, Clone)]
pub struct WarmIndexSearch {
    pub generation: u64,
    pub walks: u64,
    pub shallow: bool,
    pub matches: Vec<IndexedFile>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct WarmRootKey {
    id: String,
    path: PathBuf,
    label: String,
    read_only: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct WarmKey {
    roots: Vec<WarmRootKey>,
    cwd: PathBuf,
}

struct WarmIndex {
    files: Arc<Vec<IndexedFile>>,
    picker_files: Arc<Vec<IndexedFile>>,
    matchers: Vec<Arc<Mutex<Matcher>>>,
    generation: u64,
    refreshed_at: Instant,
    walks: u64,
    discovered: usize,
    truncated: bool,
    error: Option<String>,
}

struct LoadedIndex {
    files: Arc<Vec<IndexedFile>>,
    matchers: Vec<Arc<Mutex<Matcher>>>,
    generation: u64,
    walks: u64,
    shallow: bool,
    discovered: usize,
    truncated: bool,
    error: Option<String>,
}

struct WalkOutcome {
    files: Vec<IndexedFile>,
    truncated: bool,
    error: Option<String>,
}

struct WarmPoolPlan {
    matcher_threads: usize,
    walk_threads: usize,
}

struct WarmRuntime {
    #[allow(dead_code)]
    matcher_pool: JobPool,
}

struct JobPool {
    #[allow(dead_code)]
    tx: std::sync::mpsc::Sender<Box<dyn FnOnce() + Send>>,
}

#[cfg(test)]
thread_local! {
    static WARM_POOL_SPAWN_BLOCKED: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
fn force_warm_pool_spawn_failure(blocked: bool) {
    WARM_POOL_SPAWN_BLOCKED.with(|flag| flag.set(blocked));
}

fn warm_pool_spawn_blocked() -> bool {
    #[cfg(test)]
    {
        WARM_POOL_SPAWN_BLOCKED.with(Cell::get)
    }
    #[cfg(not(test))]
    {
        false
    }
}

fn warm_pool_sizes() -> WarmPoolPlan {
    WarmPoolPlan {
        matcher_threads: WARM_MATCHER_THREADS,
        walk_threads: WARM_WALK_THREADS,
    }
}

/// `None` when the matcher and walk threads must not be spawned.
fn warm_pool_plan() -> Option<WarmPoolPlan> {
    if warm_pool_spawn_blocked() {
        return None;
    }
    let plan = warm_pool_sizes();
    if plan.matcher_threads == 0 || plan.walk_threads == 0 {
        None
    } else {
        Some(plan)
    }
}

fn warm_runtime() -> Option<&'static WarmRuntime> {
    static RUNTIME: OnceLock<Option<WarmRuntime>> = OnceLock::new();
    RUNTIME.get_or_init(WarmRuntime::start).as_ref()
}

impl WarmRuntime {
    fn start() -> Option<Self> {
        let plan = warm_pool_sizes();
        if !spawn_probe(plan.walk_threads, "ca-attach-walk") {
            return None;
        }
        let matcher_pool = JobPool::spawn(plan.matcher_threads, "ca-attach-match").ok()?;
        Some(Self { matcher_pool })
    }
}

fn spawn_probe(threads: usize, name: &str) -> bool {
    if threads == 0 {
        return false;
    }
    let mut handles = Vec::with_capacity(threads);
    for index in 0..threads {
        match std::thread::Builder::new()
            .name(format!("{name}-probe-{index}"))
            .spawn(|| {})
        {
            Ok(handle) => handles.push(handle),
            Err(_) => {
                for handle in handles {
                    let _ = handle.join();
                }
                return false;
            }
        }
    }
    for handle in handles {
        let _ = handle.join();
    }
    true
}

impl JobPool {
    fn spawn(threads: usize, name: &str) -> Result<Self, std::io::Error> {
        if threads == 0 {
            return Err(std::io::Error::other("empty attachment index pool"));
        }
        let (tx, rx) = std::sync::mpsc::channel::<Box<dyn FnOnce() + Send>>();
        let rx = Arc::new(Mutex::new(rx));
        for index in 0..threads {
            let rx = Arc::clone(&rx);
            std::thread::Builder::new()
                .name(format!("{name}-{index}"))
                .spawn(move || job_pool_loop(rx))?;
        }
        Ok(Self { tx })
    }

    fn submit<T: Send + 'static>(
        &self,
        job: impl FnOnce() -> T + Send + 'static,
    ) -> std::sync::mpsc::Receiver<T> {
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = self.tx.send(Box::new(move || {
            let job = std::panic::AssertUnwindSafe(job);
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let _ = tx.send((job.0)());
            }));
        }));
        rx
    }
}

type JobReceiver = std::sync::mpsc::Receiver<Box<dyn FnOnce() + Send>>;

fn job_pool_loop(rx: Arc<Mutex<JobReceiver>>) {
    loop {
        let job = {
            let guard = rx.lock().unwrap_or_else(|error| error.into_inner());
            guard.recv()
        };
        let Ok(job) = job else {
            break;
        };
        let job = std::panic::AssertUnwindSafe(job);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || (job.0)()));
    }
}

fn warm_registry() -> &'static Mutex<HashMap<WarmKey, WarmIndex>> {
    static REGISTRY: OnceLock<Mutex<HashMap<WarmKey, WarmIndex>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn warm_key(roots: &[RootSpec], cwd: &Path) -> WarmKey {
    WarmKey {
        roots: roots
            .iter()
            .map(|root| WarmRootKey {
                id: root.id.clone(),
                path: canonical_path(&root.path),
                label: root.label.clone(),
                read_only: root.read_only,
            })
            .collect(),
        cwd: canonical_path(cwd),
    }
}

fn canonical_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn warm_index_is_fresh(index: &WarmIndex, generation: Option<u64>) -> bool {
    if Instant::now().saturating_duration_since(index.refreshed_at) >= WARM_INDEX_TTL {
        return false;
    }
    generation.is_none_or(|generation| generation == index.generation)
}

fn load_warm_index(
    roots: &[RootSpec],
    cwd: &Path,
    generation: Option<u64>,
    cancelled: &AtomicBool,
    allow_spaces: bool,
) -> LoadedIndex {
    if cancelled.load(Ordering::Relaxed) {
        return LoadedIndex::empty(generation.unwrap_or(0));
    }
    let Some(plan) = warm_pool_plan() else {
        return LoadedIndex::shallow(roots, cwd, generation, allow_spaces);
    };
    if warm_runtime().is_none() {
        return LoadedIndex::shallow(roots, cwd, generation, allow_spaces);
    }
    let key = warm_key(roots, cwd);
    if let Some(hit) = fresh_warm_index(&key, generation, allow_spaces) {
        return hit;
    }
    let walked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        walk_workspace(roots, cwd, cancelled, plan.walk_threads)
    }));
    if cancelled.load(Ordering::Relaxed) {
        return match walked {
            Ok(outcome) => LoadedIndex::from_outcome(outcome, generation, false, allow_spaces),
            Err(_) => LoadedIndex::shallow(roots, cwd, generation, allow_spaces),
        };
    }
    match walked {
        Ok(outcome) => remember_walk(key, outcome, generation, plan.matcher_threads, allow_spaces),
        Err(_) => LoadedIndex::shallow(roots, cwd, generation, allow_spaces),
    }
}

fn fresh_warm_index(
    key: &WarmKey,
    generation: Option<u64>,
    allow_spaces: bool,
) -> Option<LoadedIndex> {
    let registry = warm_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let index = registry.get(key)?;
    warm_index_is_fresh(index, generation).then(|| loaded_from(index, allow_spaces))
}

fn remember_walk(
    key: WarmKey,
    outcome: WalkOutcome,
    generation: Option<u64>,
    matcher_threads: usize,
    allow_spaces: bool,
) -> LoadedIndex {
    let mut registry = warm_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (walks, generation) = match registry.get(&key) {
        Some(existing) if warm_index_is_fresh(existing, generation) => {
            return loaded_from(existing, allow_spaces);
        }
        Some(existing) => (
            existing.walks.saturating_add(1),
            generation.unwrap_or(existing.generation),
        ),
        None => (1, generation.unwrap_or(0)),
    };
    let files = Arc::new(outcome.files);
    let discovered = files.len();
    let index = WarmIndex {
        picker_files: picker_view(&files),
        files,
        matchers: new_matchers(matcher_threads),
        generation,
        refreshed_at: Instant::now(),
        walks,
        discovered,
        truncated: outcome.truncated,
        error: outcome.error,
    };
    let loaded = loaded_from(&index, allow_spaces);
    registry.insert(key.clone(), index);
    evict_warm_indexes(&mut registry, &key);
    loaded
}

fn evict_warm_indexes(indexes: &mut HashMap<WarmKey, WarmIndex>, keep: &WarmKey) {
    while indexes.len() > WARM_INDEX_LIMIT {
        let oldest = indexes
            .iter()
            .filter(|(key, _)| *key != keep)
            .min_by_key(|(_, index)| index.refreshed_at)
            .map(|(key, _)| key.clone());
        let Some(oldest) = oldest else {
            break;
        };
        indexes.remove(&oldest);
    }
}

fn loaded_from(index: &WarmIndex, allow_spaces: bool) -> LoadedIndex {
    LoadedIndex {
        files: if allow_spaces {
            Arc::clone(&index.files)
        } else {
            Arc::clone(&index.picker_files)
        },
        matchers: index.matchers.clone(),
        generation: index.generation,
        walks: index.walks,
        shallow: false,
        discovered: if allow_spaces {
            index.discovered
        } else {
            index.picker_files.len()
        },
        truncated: index.truncated,
        error: index.error.clone(),
    }
}

impl LoadedIndex {
    fn empty(generation: u64) -> Self {
        Self {
            files: Arc::new(Vec::new()),
            matchers: Vec::new(),
            generation,
            walks: 0,
            shallow: false,
            discovered: 0,
            truncated: false,
            error: None,
        }
    }

    fn shallow(
        roots: &[RootSpec],
        cwd: &Path,
        generation: Option<u64>,
        allow_spaces: bool,
    ) -> Self {
        Self::from_outcome(browse_one_level(roots, cwd), generation, true, allow_spaces)
    }

    fn from_outcome(
        outcome: WalkOutcome,
        generation: Option<u64>,
        shallow: bool,
        allow_spaces: bool,
    ) -> Self {
        let files = Arc::new(outcome.files);
        let picker_files = picker_view(&files);
        let viewed = if allow_spaces {
            Arc::clone(&files)
        } else {
            Arc::clone(&picker_files)
        };
        let discovered = viewed.len();
        Self {
            files: viewed,
            matchers: Vec::new(),
            generation: generation.unwrap_or(0),
            walks: 0,
            shallow,
            discovered,
            truncated: outcome.truncated,
            error: outcome.error,
        }
    }
}

fn picker_view(files: &Arc<Vec<IndexedFile>>) -> Arc<Vec<IndexedFile>> {
    if files
        .iter()
        .all(|file| safe_picker_path(&file.relative) && safe_picker_path(&file.reference))
    {
        Arc::clone(files)
    } else {
        Arc::new(
            files
                .iter()
                .filter(|file| {
                    safe_picker_path(&file.relative) && safe_picker_path(&file.reference)
                })
                .cloned()
                .collect(),
        )
    }
}

fn new_matchers(threads: usize) -> Vec<Arc<Mutex<Matcher>>> {
    (0..threads)
        .map(|_| Arc::new(Mutex::new(Matcher::new(Config::DEFAULT.match_paths()))))
        .collect()
}

fn retained_matcher(files: &Arc<Vec<IndexedFile>>) -> Option<Arc<Mutex<Matcher>>> {
    let registry = warm_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    registry.values().find_map(|index| {
        (Arc::ptr_eq(&index.files, files) || Arc::ptr_eq(&index.picker_files, files))
            .then(|| index.matchers.first().cloned())
            .flatten()
    })
}

fn walk_workspace(
    roots: &[RootSpec],
    cwd: &Path,
    cancelled: &AtomicBool,
    threads: usize,
) -> WalkOutcome {
    let files = Arc::new(Mutex::new(Vec::new()));
    let truncated = Arc::new(AtomicBool::new(false));
    let error = Arc::new(Mutex::new(None));
    for root in roots {
        if cancelled.load(Ordering::Relaxed) || truncated.load(Ordering::Relaxed) {
            break;
        }
        let mut builder = ignore::WalkBuilder::new(&root.path);
        builder
            .standard_filters(true)
            .require_git(false)
            .follow_links(false)
            .threads(threads.max(1));
        let root = root.clone();
        let cwd = cwd.to_path_buf();
        let files = Arc::clone(&files);
        let truncated = Arc::clone(&truncated);
        let error = Arc::clone(&error);
        builder.build_parallel().run(move || {
            let root = root.clone();
            let cwd = cwd.clone();
            let files = Arc::clone(&files);
            let truncated = Arc::clone(&truncated);
            let error = Arc::clone(&error);
            Box::new(move |item| {
                visit_walk_item(&root, &cwd, cancelled, &files, &truncated, &error, item)
            })
        });
    }
    let mut files = match Arc::try_unwrap(files) {
        Ok(mutex) => mutex
            .into_inner()
            .unwrap_or_else(|poison| poison.into_inner()),
        Err(files) => {
            std::mem::take(&mut *files.lock().unwrap_or_else(|poison| poison.into_inner()))
        }
    };
    sort_indexed(&mut files);
    let error = match Arc::try_unwrap(error) {
        Ok(mutex) => mutex
            .into_inner()
            .unwrap_or_else(|poison| poison.into_inner()),
        Err(error) => error
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone(),
    };
    WalkOutcome {
        files,
        truncated: truncated.load(Ordering::Relaxed),
        error,
    }
}

fn visit_walk_item(
    root: &RootSpec,
    cwd: &Path,
    cancelled: &AtomicBool,
    files: &Mutex<Vec<IndexedFile>>,
    truncated: &AtomicBool,
    error: &Mutex<Option<String>>,
    item: Result<ignore::DirEntry, ignore::Error>,
) -> ignore::WalkState {
    if cancelled.load(Ordering::Relaxed) || truncated.load(Ordering::Relaxed) {
        return ignore::WalkState::Quit;
    }
    let entry = match item {
        Ok(entry) => entry,
        Err(_) => {
            note_inspect_error(error);
            return ignore::WalkState::Continue;
        }
    };
    let Some(file_type) = entry.file_type() else {
        return ignore::WalkState::Continue;
    };
    if !file_type.is_file() || file_type.is_symlink() {
        return ignore::WalkState::Continue;
    }
    let Some(file) = admit_indexed_file(root, cwd, entry.path()) else {
        return ignore::WalkState::Continue;
    };
    let mut files = files.lock().unwrap_or_else(|error| error.into_inner());
    if files.len() >= MAX_DISCOVERED_PATHS {
        truncated.store(true, Ordering::Relaxed);
        return ignore::WalkState::Quit;
    }
    files.push(file);
    ignore::WalkState::Continue
}

fn browse_one_level(roots: &[RootSpec], cwd: &Path) -> WalkOutcome {
    let mut files = Vec::new();
    let mut truncated = false;
    let mut error = None;
    'roots: for root in roots {
        let entries = match fs::read_dir(&root.path) {
            Ok(entries) => entries,
            Err(_) => {
                note_inspect_error_slot(&mut error);
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    note_inspect_error_slot(&mut error);
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => {
                    note_inspect_error_slot(&mut error);
                    continue;
                }
            };
            if !file_type.is_file() || file_type.is_symlink() {
                continue;
            }
            if files.len() == MAX_DISCOVERED_PATHS {
                truncated = true;
                break 'roots;
            }
            let Some(file) = admit_indexed_file(root, cwd, &entry.path()) else {
                continue;
            };
            files.push(file);
        }
    }
    sort_indexed(&mut files);
    WalkOutcome {
        files,
        truncated,
        error,
    }
}

fn admit_indexed_file(root: &RootSpec, cwd: &Path, path: &Path) -> Option<IndexedFile> {
    let relative = path.strip_prefix(&root.path).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    let relative = relative.to_string_lossy().replace('\\', "/");
    let reference = path
        .strip_prefix(cwd)
        .ok()
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    if !safe_filter_path(&relative) || !safe_filter_path(&reference) {
        return None;
    }
    Some(IndexedFile {
        root_id: root.id.clone(),
        root_label: root.label.clone(),
        relative,
        reference,
        read_only: root.read_only,
    })
}

fn sort_indexed(files: &mut [IndexedFile]) {
    files.sort_by(|left, right| {
        left.root_label
            .cmp(&right.root_label)
            .then_with(|| left.relative.cmp(&right.relative))
    });
}

fn note_inspect_error(slot: &Mutex<Option<String>>) {
    let mut guard = slot.lock().unwrap_or_else(|error| error.into_inner());
    if guard.is_none() {
        *guard = Some("Some workspace entries could not be inspected".to_string());
    }
}

fn note_inspect_error_slot(slot: &mut Option<String>) {
    if slot.is_none() {
        *slot = Some("Some workspace entries could not be inspected".to_string());
    }
}

fn local_file_matches(
    files: &Arc<Vec<IndexedFile>>,
    query: &str,
    frecency: &FrecencyMap,
) -> Vec<IndexedFile> {
    rank_matches(files, &[], query, frecency, 0, &AtomicBool::new(false))
        .entries
        .into_iter()
        .filter_map(|entry| match entry {
            PickerEntry::File(file) => Some(file),
            PickerEntry::Mcp(_) => None,
        })
        .collect()
}

fn pooled_matches(
    pool: &JobPool,
    files: &Arc<Vec<IndexedFile>>,
    matchers: &[Arc<Mutex<Matcher>>],
    query: &str,
    frecency: &FrecencyMap,
) -> Option<Vec<IndexedFile>> {
    let (query, _) = split_picker_range(query);
    if files.is_empty() || matchers.is_empty() {
        return Some(Vec::new());
    }
    let workers = matchers.len().min(files.len());
    let ranges = split_ranges(files.len(), workers);
    let mut receivers = Vec::with_capacity(ranges.len());
    for ((start, end), matcher) in ranges.into_iter().zip(matchers.iter()) {
        let files = Arc::clone(files);
        let matcher = Arc::clone(matcher);
        let query = query.to_string();
        receivers.push(pool.submit(move || score_range(files, matcher, query, start, end)));
    }
    let mut scored_indexes = Vec::new();
    for receiver in receivers {
        scored_indexes.extend(receiver.recv().ok()?);
    }
    let mut scored = scored_indexes
        .into_iter()
        .filter_map(|(score, index)| {
            let file = files.get(index)?;
            Some((score, frecency_for(file, frecency), file))
        })
        .collect::<Vec<_>>();
    sort_file_matches(&mut scored);
    Some(
        scored
            .into_iter()
            .take(MAX_PICKER_MATCHES)
            .map(|(_, _, file)| file.clone())
            .collect(),
    )
}

fn split_ranges(len: usize, workers: usize) -> Vec<(usize, usize)> {
    let workers = workers.min(len).max(1);
    let mut ranges = Vec::with_capacity(workers);
    let mut start = 0usize;
    for index in 0..workers {
        let remaining = workers - index;
        let take = (len - start).div_ceil(remaining);
        let end = start + take;
        ranges.push((start, end));
        start = end;
    }
    ranges
}

fn score_range(
    files: Arc<Vec<IndexedFile>>,
    matcher: Arc<Mutex<Matcher>>,
    query: String,
    start: usize,
    end: usize,
) -> Vec<(u32, usize)> {
    let mut guard = matcher.lock().unwrap_or_else(|error| error.into_inner());
    let pattern = Pattern::new(
        &query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut scored = Vec::new();
    for index in start..end {
        let Some(file) = files.get(index) else {
            continue;
        };
        let mut utf32 = Vec::new();
        let Some(score) = pattern.score(
            nucleo::Utf32Str::new(&file.relative, &mut utf32),
            &mut guard,
        ) else {
            continue;
        };
        scored.push((score, index));
    }
    scored
}

fn frecency_for(file: &IndexedFile, frecency: &FrecencyMap) -> FrecencyRecord {
    frecency
        .get(&frecency_identity(&file.frecency_key()))
        .copied()
        .unwrap_or(FrecencyRecord {
            uses: 0,
            last_used_unix: 0,
        })
}

fn sort_file_matches(scored: &mut [(u32, FrecencyRecord, &IndexedFile)]) {
    scored.sort_by(
        |(left_score, left_record, left), (right_score, right_record, right)| {
            right_score
                .cmp(left_score)
                .then_with(|| right_record.uses.cmp(&left_record.uses))
                .then_with(|| right_record.last_used_unix.cmp(&left_record.last_used_unix))
                .then_with(|| left.root_label.cmp(&right.root_label))
                .then_with(|| left.relative.cmp(&right.relative))
        },
    );
}

pub fn rank_matches(
    files: &Arc<Vec<IndexedFile>>,
    resources: &[crate::mcp::McpResourceInfo],
    query: &str,
    frecency: &FrecencyMap,
    generation: u64,
    cancelled: &AtomicBool,
) -> MatchSnapshot {
    let (query, _) = split_picker_range(query);
    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let retained = retained_matcher(files);
    let mut retained_guard = retained
        .as_ref()
        .map(|matcher| matcher.lock().unwrap_or_else(|error| error.into_inner()));
    let mut owned_matcher;
    let matcher = if let Some(guard) = retained_guard.as_mut() {
        &mut **guard
    } else {
        owned_matcher = Matcher::new(Config::DEFAULT.match_paths());
        &mut owned_matcher
    };
    let mut scored = Vec::new();
    for file in files.iter() {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let mut utf32 = Vec::new();
        let Some(score) = pattern.score(nucleo::Utf32Str::new(&file.relative, &mut utf32), matcher)
        else {
            continue;
        };
        let record = frecency
            .get(&frecency_identity(&file.frecency_key()))
            .copied()
            .unwrap_or(FrecencyRecord {
                uses: 0,
                last_used_unix: 0,
            });
        scored.push((score, record, file));
    }
    scored.sort_by(
        |(left_score, left_record, left), (right_score, right_record, right)| {
            right_score
                .cmp(left_score)
                .then_with(|| right_record.uses.cmp(&left_record.uses))
                .then_with(|| right_record.last_used_unix.cmp(&left_record.last_used_unix))
                .then_with(|| left.root_label.cmp(&right.root_label))
                .then_with(|| left.relative.cmp(&right.relative))
        },
    );
    let mut resource_matches = resources
        .iter()
        .filter_map(|resource| {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            let searchable = format!("{} {} {}", resource.server, resource.name, resource.uri);
            let mut utf32 = Vec::new();
            pattern
                .score(nucleo::Utf32Str::new(&searchable, &mut utf32), matcher)
                .map(|score| (score, resource))
        })
        .collect::<Vec<_>>();
    resource_matches.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.server.cmp(&right.server))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.uri.cmp(&right.uri))
    });
    let total_matches = scored.len().saturating_add(resource_matches.len());
    // Keep a small bounded resource section visible even when a large workspace
    // supplies enough file matches to fill the entire snapshot.
    let resource_limit = resource_matches.len().min(25);
    let file_limit = MAX_PICKER_MATCHES.saturating_sub(resource_limit);
    let mut entries = scored
        .iter()
        .take(file_limit)
        .map(|(_, _, file)| PickerEntry::File((*file).clone()))
        .collect::<Vec<_>>();
    entries.extend(
        resource_matches
            .into_iter()
            .take(resource_limit)
            .map(|(_, resource)| PickerEntry::Mcp(resource.clone())),
    );
    MatchSnapshot {
        generation,
        entries,
        total_matches,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerAttachmentKind {
    File {
        root_id: String,
        relative: String,
    },
    Mcp {
        server: String,
        uri: String,
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerAttachment {
    pub reference: String,
    pub kind: ComposerAttachmentKind,
}

impl ComposerAttachment {
    pub fn from_picker(entry: &PickerEntry) -> Self {
        match entry {
            PickerEntry::File(file) => Self {
                reference: file.reference.clone(),
                kind: ComposerAttachmentKind::File {
                    root_id: file.root_id.clone(),
                    relative: file.relative.clone(),
                },
            },
            PickerEntry::Mcp(resource) => {
                let digest =
                    Sha256::digest(format!("{}\0{}", resource.server, resource.uri).as_bytes());
                let short = digest[..8]
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                Self {
                    reference: format!("mcp:{}:{short}", resource.server),
                    kind: ComposerAttachmentKind::Mcp {
                        server: resource.server.clone(),
                        uri: resource.uri.clone(),
                        name: resource.name.clone(),
                    },
                }
            }
        }
    }

    pub fn token(&self) -> String {
        format!("@{}", self.reference)
    }

    pub fn is_referenced_by(&self, input: &str) -> bool {
        input.split_whitespace().any(|token| {
            token == self.token()
                || matches!(&self.kind, ComposerAttachmentKind::File { .. })
                    && token
                        .strip_prefix(&self.token())
                        .is_some_and(|suffix| suffix.starts_with('#'))
        })
    }
}

pub fn has_selected_mcp_attachments(input: &str, selected: &[ComposerAttachment]) -> bool {
    selected.iter().any(|attachment| {
        matches!(&attachment.kind, ComposerAttachmentKind::Mcp { .. })
            && attachment.is_referenced_by(input)
    })
}

pub fn retain_unreferenced(input: &str, selected: &mut Vec<ComposerAttachment>) {
    selected.retain(|attachment| !attachment.is_referenced_by(input));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

pub fn split_picker_range(filter: &str) -> (&str, Option<&str>) {
    let Some((query, range)) = filter.rsplit_once('#') else {
        return (filter, None);
    };
    if range.is_empty()
        || range
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
    {
        (query, Some(range))
    } else {
        (filter, None)
    }
}

pub fn parse_line_range(value: &str) -> Result<LineRange, String> {
    let (start, end) = value
        .split_once('-')
        .map_or((value, value), |(start, end)| (start, end));
    if start.is_empty()
        || end.is_empty()
        || !start.bytes().all(|byte| byte.is_ascii_digit())
        || !end.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("line range must be #N or #N-M".into());
    }
    let start = start
        .parse::<usize>()
        .map_err(|_| "line range is too large".to_string())?;
    let end = end
        .parse::<usize>()
        .map_err(|_| "line range is too large".to_string())?;
    if start == 0 || end == 0 || start > end {
        return Err("line range must be positive and ordered".into());
    }
    Ok(LineRange { start, end })
}

pub fn range_suffix(filter: &str) -> Result<String, String> {
    let (_, range) = split_picker_range(filter);
    match range {
        None => Ok(String::new()),
        Some("") => Err("finish the line range before selecting a file".into()),
        Some(range) => parse_line_range(range).map(|_| format!("#{range}")),
    }
}

#[cfg(test)]
pub fn expand_file_mentions(
    input: &str,
    workspace: &crate::workspace::WorkspaceContext,
    selected: &[ComposerAttachment],
) -> Result<String, String> {
    expand_file_mentions_with_spans(input, workspace, selected).map(|(text, _)| text)
}

/// Keep the authored composer text separate from host-appended file evidence.
pub fn expand_file_mentions_with_spans(
    input: &str,
    workspace: &crate::workspace::WorkspaceContext,
    selected: &[ComposerAttachment],
) -> Result<(String, Vec<crate::retrieved_content::RetrievedSpan>), String> {
    expand_file_mentions_impl(input, workspace, selected, false)
        .map(|(text, spans, _)| (text, spans))
}

/// Host-only source evidence captured at the actual attachment read. These
/// fields are never inferred from attachment text or provider metadata.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct AttachmentSource {
    #[serde(default)]
    pub receipt_id: String,
    pub path: String,
    pub source: String,
    pub original: String,
    pub start_line: usize,
    pub end_line: usize,
    pub prefix: String,
    pub suffix: String,
    pub max_chars: usize,
    pub ranged: bool,
}

impl std::fmt::Debug for AttachmentSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AttachmentSource")
            .field("source_bytes", &self.source.len())
            .field("attached_bytes", &self.original.len())
            .field("start_line", &self.start_line)
            .field("end_line", &self.end_line)
            .finish_non_exhaustive()
    }
}

/// Return source evidence separately so the session owner can register it on
/// its privacy worker before sending, including when expansion predates binding.
pub(crate) fn expand_file_mentions_with_sources(
    input: &str,
    workspace: &crate::workspace::WorkspaceContext,
    selected: &[ComposerAttachment],
) -> Result<
    (
        String,
        Vec<crate::retrieved_content::RetrievedSpan>,
        Vec<AttachmentSource>,
    ),
    String,
> {
    expand_file_mentions_impl(input, workspace, selected, true)
}

fn expand_file_mentions_impl(
    input: &str,
    workspace: &crate::workspace::WorkspaceContext,
    selected: &[ComposerAttachment],
    capture_sources: bool,
) -> Result<
    (
        String,
        Vec<crate::retrieved_content::RetrievedSpan>,
        Vec<AttachmentSource>,
    ),
    String,
> {
    let mentions = mention_tokens(input, workspace, selected)?;
    if mentions.is_empty() {
        return Ok((input.to_string(), Vec::new(), Vec::new()));
    }
    let mut result = input.to_string();
    let mut spans = Vec::new();
    let mut sources = Vec::new();
    for mention in mentions.into_iter().take(MAX_MENTIONS) {
        if mention.reference.starts_with("mcp:") {
            continue;
        }
        let selected_file = selected.iter().find_map(|attachment| {
            let ComposerAttachmentKind::File { root_id, relative } = &attachment.kind else {
                return None;
            };
            (attachment.reference == mention.reference)
                .then_some((root_id.as_str(), relative.as_str()))
        });
        let resolved = if let Some((root_id, relative)) = selected_file {
            let root = workspace
                .roots()
                .iter()
                .find(|root| root.id.to_string() == root_id)
                .ok_or_else(|| {
                    format!("attachment root is no longer available: {}", mention.raw)
                })?;
            root.path.join(relative)
        } else {
            let raw_path = Path::new(&mention.reference);
            let lexical = if raw_path.is_absolute() {
                raw_path.to_path_buf()
            } else {
                workspace.cwd().join(raw_path)
            };
            if fs::symlink_metadata(&lexical).is_ok() {
                let lexical_root = workspace
                    .roots()
                    .iter()
                    .find(|root| lexical.starts_with(&root.path))
                    .ok_or_else(|| {
                        format!("attachment is outside workspace roots: {}", mention.raw)
                    })?;
                reject_symlink_components(&lexical_root.path, &lexical)?;
            }
            match workspace.resolve(&mention.reference, crate::workspace::PathOperation::Read) {
                Ok(path) => path,
                Err(crate::workspace::WorkspaceError::Missing(_)) => {
                    let start = result.len();
                    result.push_str(&format!("\n\n(file not found: {})", mention.raw));
                    spans.push(crate::retrieved_content::RetrievedSpan {
                        source: None,
                        start,
                        end: result.len(),
                    });
                    continue;
                }
                Err(error) => {
                    return Err(format!("attachment rejected: {error}"));
                }
            }
        };
        let start = result.len();
        let mut source_id = None;
        if let Some(source) =
            append_path(&mut result, workspace, &resolved, &mention, capture_sources)?
        {
            source_id = Some(source.receipt_id.clone());
            sources.push(source);
        }
        spans.push(crate::retrieved_content::RetrievedSpan {
            source: source_id,
            start,
            end: result.len(),
        });
    }
    Ok((result, spans, sources))
}

/// References established by complete host-appended sections, never by prose.
pub(crate) fn refreshed_attachment_references(
    text: &str,
    spans: &[crate::retrieved_content::RetrievedSpan],
) -> Vec<String> {
    if crate::retrieved_content::validate_spans(text, spans).is_err() {
        return Vec::new();
    }
    spans
        .iter()
        .take(MAX_MENTIONS)
        .filter_map(|span| {
            let section = &text[span.start..span.end];
            let (end, reference) = file_section_at(section)?;
            (end == section.len()).then_some(reference)
        })
        .collect()
}

/// Strip only complete legacy sections whose exact reference was freshly
/// attached. This is a request projection; saved history is left unchanged.
pub(crate) fn remove_legacy_attachment_sections(text: &str, references: &[String]) -> String {
    const OPEN: &str = "\n\n--- Attached file: ";
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    let mut count = 0;
    while count < MAX_MENTIONS {
        let Some(start) = rest.find(OPEN) else {
            break;
        };
        result.push_str(&rest[..start]);
        let section = &rest[start..];
        let Some((end, reference)) = file_section_at(section) else {
            result.push_str(section);
            return result;
        };
        if !references.contains(&reference) {
            result.push_str(&section[..end]);
        }
        rest = &section[end..];
        count += 1;
    }
    result.push_str(rest);
    result
}

fn file_section_at(section: &str) -> Option<(usize, String)> {
    const OPEN: &str = "\n\n--- Attached file: ";
    const CLOSE: &str = "\n--- End file ---";
    let tail = section.strip_prefix(OPEN)?;
    let header_end = tail.find(" ---\n")?;
    if header_end > MAX_PICKER_QUERY_BYTES + 64 {
        return None;
    }
    let header = &tail[..header_end];
    let reference = if let Some((reference, range)) = header.rsplit_once(" lines ") {
        let range = parse_line_range(range).ok()?;
        format!("{reference}#{}-{}", range.start, range.end)
    } else {
        header.to_string()
    };
    if reference.is_empty() || reference.chars().any(char::is_whitespace) {
        return None;
    }
    let body_start = OPEN.len() + header_end + 5;
    let mut bounded_end = section
        .len()
        .min(body_start + MAX_ATTACHED_CHARS * 4 + CLOSE.len());
    while !section.is_char_boundary(bounded_end) {
        bounded_end -= 1;
    }
    for (relative, _) in section[body_start..bounded_end].match_indices(CLOSE) {
        let end = body_start + relative + CLOSE.len();
        let suffix = &section[end..];
        if suffix.is_empty() || suffix.starts_with(OPEN) || is_persisted_image_suffix(suffix) {
            return Some((end, reference));
        }
    }
    None
}

fn is_persisted_image_suffix(mut text: &str) -> bool {
    for _ in 0..MAX_COMPOSER_ATTACHMENTS + crate::image_content::MAX_IMAGES_PER_REQUEST {
        if let Some(tail) = text.strip_prefix("\n\n(MCP resource unavailable: ") {
            let Some(end) = tail.find(')') else {
                return false;
            };
            if end == 0 || end > MAX_PICKER_QUERY_BYTES || tail[..end].chars().any(char::is_control)
            {
                return false;
            }
            text = &tail[end + 1..];
            if text.is_empty() {
                return true;
            }
            continue;
        }
        let Some(tail) = text.strip_prefix("\n\n[Attached image: image/png ") else {
            return false;
        };
        let Some((dimensions, rest)) = tail.split_once("; unavailable after this process]") else {
            return false;
        };
        let Some((width, height)) = dimensions.split_once('×') else {
            return false;
        };
        if ![width, height]
            .into_iter()
            .all(|value| value.parse::<u32>().is_ok_and(|value| value > 0))
        {
            return false;
        }
        if rest.is_empty() {
            return true;
        }
        text = rest;
    }
    false
}

/// Return only host-validated explicit file references from the composer.
/// Ordinary prose is deliberately ignored, even when it contains a path-like
/// substring without an `@` token.
pub fn explicit_file_paths(
    input: &str,
    workspace: &crate::workspace::WorkspaceContext,
    selected: &[ComposerAttachment],
) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for mention in mention_tokens(input, workspace, selected)?
        .into_iter()
        .take(MAX_MENTIONS)
    {
        if mention.reference.starts_with("mcp:") {
            continue;
        }
        let selected_file = selected.iter().find_map(|attachment| {
            let ComposerAttachmentKind::File { root_id, relative } = &attachment.kind else {
                return None;
            };
            (attachment.reference == mention.reference)
                .then_some((root_id.as_str(), relative.as_str()))
        });
        let resolved = if let Some((root_id, relative)) = selected_file {
            let root = workspace
                .roots()
                .iter()
                .find(|root| root.id.to_string() == root_id)
                .ok_or_else(|| {
                    format!("attachment root is no longer available: {}", mention.raw)
                })?;
            let lexical = root.path.join(relative);
            reject_symlink_components(&root.path, &lexical)?;
            fs::canonicalize(&lexical)
                .map_err(|_| format!("attachment is unavailable: {}", mention.raw))?
        } else {
            let raw_path = Path::new(&mention.reference);
            let lexical = if raw_path.is_absolute() {
                raw_path.to_path_buf()
            } else {
                workspace.cwd().join(raw_path)
            };
            if fs::symlink_metadata(&lexical).is_ok() {
                let lexical_root = workspace
                    .roots()
                    .iter()
                    .find(|root| lexical.starts_with(&root.path))
                    .ok_or_else(|| {
                        format!("attachment is outside workspace roots: {}", mention.raw)
                    })?;
                reject_symlink_components(&lexical_root.path, &lexical)?;
            }
            match workspace.resolve(&mention.reference, crate::workspace::PathOperation::Read) {
                Ok(path) => path,
                Err(crate::workspace::WorkspaceError::Missing(_)) => continue,
                Err(error) => return Err(format!("attachment rejected: {error}")),
            }
        };
        if !paths.contains(&resolved) {
            paths.push(resolved);
        }
    }
    Ok(paths)
}

#[derive(Debug)]
struct Mention {
    raw: String,
    reference: String,
    range: Option<LineRange>,
}

fn mention_tokens(
    input: &str,
    workspace: &crate::workspace::WorkspaceContext,
    selected: &[ComposerAttachment],
) -> Result<Vec<Mention>, String> {
    let mut mentions = Vec::new();
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'@' {
            index += 1;
            continue;
        }
        // `@` inside a token is a shell prompt or an email address
        // (`user@host`), not a composer file mention. Mentions are inserted
        // as their own whitespace-delimited token.
        if index > 0 && !bytes[index - 1].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
            end += 1;
        }
        if end == index + 1 {
            index = end;
            continue;
        }
        let token = &input[index + 1..end];
        // Prefer exact existing/selected names, including names ending in
        // punctuation. Otherwise punctuation belongs to the surrounding prose.
        let exact = |value: &str| {
            selected
                .iter()
                .any(|attachment| attachment.reference == value)
                || fs::symlink_metadata(workspace.cwd().join(value)).is_ok()
        };
        let mut raw = token;
        while !exact(raw) && !matches!(raw, "." | "..") {
            let Some(last) = raw.chars().next_back() else {
                break;
            };
            if !matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}') {
                break;
            }
            let next = &raw[..raw.len() - last.len_utf8()];
            if next.is_empty() {
                break;
            }
            raw = next;
        }
        let (reference, range) = match split_picker_range(raw) {
            (path, Some(suffix)) => (path, Some(parse_line_range(suffix)?)),
            _ => (raw, None),
        };
        mentions.push(Mention {
            raw: raw.to_string(),
            reference: reference.to_string(),
            range,
        });
        index = end;
    }
    Ok(mentions)
}

fn append_path(
    result: &mut String,
    workspace: &crate::workspace::WorkspaceContext,
    resolved: &Path,
    mention: &Mention,
    capture_source: bool,
) -> Result<Option<AttachmentSource>, String> {
    let root = workspace
        .roots()
        .iter()
        .find(|root| resolved.starts_with(&root.path))
        .ok_or_else(|| format!("attachment is outside workspace roots: {}", mention.raw))?;
    reject_symlink_components(&root.path, resolved)?;
    let metadata = fs::symlink_metadata(resolved)
        .map_err(|_| format!("attachment is unavailable: {}", mention.raw))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("attachment cannot be a symlink: {}", mention.raw));
    }
    if metadata.is_dir() {
        return Err(format!("attachment is not a regular file: {}", mention.raw));
    }
    if !metadata.is_file() {
        return Err(format!("attachment is not a regular file: {}", mention.raw));
    }
    if metadata.len() as usize > MAX_FILE_BYTES {
        return Err(format!("attachment exceeds 1 MiB: {}", mention.raw));
    }
    let bytes =
        fs::read(resolved).map_err(|_| format!("attachment could not be read: {}", mention.raw))?;
    if let Err(error) = crate::provider_filter::scan_outbound(&bytes) {
        return Err(crate::provider_filter::withheld_message(&error));
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err(format!("attachment changed while reading: {}", mention.raw));
    }
    let content = std::str::from_utf8(&bytes)
        .map_err(|_| format!("attachment is not UTF-8 text: {}", mention.raw))?;
    let (body, range_label) = if let Some(range) = mention.range {
        let lines = content
            .split_terminator('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .collect::<Vec<_>>();
        if range.end > lines.len() {
            return Err(format!(
                "attachment range #{}-{} exceeds {} lines",
                range.start,
                range.end,
                lines.len()
            ));
        }
        (
            lines[range.start - 1..range.end].join("\n"),
            format!(" lines {}-{}", range.start, range.end),
        )
    } else {
        (content.to_string(), String::new())
    };
    let body = body.chars().take(MAX_ATTACHED_CHARS).collect::<String>();
    let prefix = format!(
        "\n\n--- Attached file: {}{} ---\n",
        mention.reference, range_label
    );
    let suffix = "\n--- End file ---".to_string();
    let original = format!("{prefix}{body}{suffix}");
    result.push_str(&original);
    if !capture_source {
        return Ok(None);
    }
    Ok(Some(AttachmentSource {
        receipt_id: uuid::Uuid::new_v4().to_string(),
        path: resolved.to_string_lossy().into_owned(),
        source: content.to_string(),
        original,
        start_line: mention.range.map_or(1, |range| range.start),
        end_line: mention
            .range
            .map_or_else(|| content.split_terminator('\n').count(), |range| range.end),
        prefix,
        suffix,
        max_chars: MAX_ATTACHED_CHARS,
        ranged: mention.range.is_some(),
    }))
}

fn reject_symlink_components(root: &Path, target: &Path) -> Result<(), String> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| "attachment is outside workspace roots".to_string())?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err("attachment path is invalid".into());
        };
        current.push(component);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| "attachment path changed during validation".to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("attachment path cannot contain symlinks".into());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceResolution {
    pub process_context: Option<String>,
    pub unavailable_markers: Vec<String>,
}

pub async fn resolve_mcp_attachments(
    input: &str,
    selected: &[ComposerAttachment],
    manager: Option<&Arc<crate::mcp::McpManager>>,
) -> ResourceResolution {
    let mut resolution = ResourceResolution::default();
    let mut context = String::new();
    for attachment in selected.iter().take(MAX_COMPOSER_ATTACHMENTS) {
        if !attachment.is_referenced_by(input) {
            continue;
        }
        let ComposerAttachmentKind::Mcp { server, uri, name } = &attachment.kind else {
            continue;
        };
        let Some(manager) = manager else {
            resolution.unavailable_markers.push(format!(
                "(MCP resource unavailable: {})",
                attachment.reference
            ));
            continue;
        };
        match manager.read_resource(server, uri).await {
            Ok(body) => {
                let section = format!(
                    "\n\n--- MCP resource: {} ({}) ---\n{}\n--- End MCP resource ---",
                    attachment.reference,
                    bounded_label(name, 128),
                    body
                );
                append_utf8_bounded(&mut context, &section, MAX_RESOURCE_CONTEXT_BYTES);
            }
            Err(_) => resolution.unavailable_markers.push(format!(
                "(MCP resource unavailable: {})",
                attachment.reference
            )),
        }
    }
    if !context.is_empty() {
        resolution.process_context = Some(context);
    }
    resolution
}

fn append_utf8_bounded(target: &mut String, value: &str, maximum: usize) {
    if target.len() >= maximum {
        return;
    }
    let mut end = value.len().min(maximum - target.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    target.push_str(&value[..end]);
}

fn bounded_label(value: &str, maximum: usize) -> String {
    let safe = crate::terminal_text::sanitize_terminal_text(value);
    if safe.len() <= maximum {
        return safe;
    }
    let mut end = maximum;
    while !safe.is_char_boundary(end) {
        end -= 1;
    }
    safe[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_ranges_are_strict_and_picker_suffix_is_separate() {
        assert_eq!(
            parse_line_range("12").unwrap(),
            LineRange { start: 12, end: 12 }
        );
        assert_eq!(
            parse_line_range("12-40").unwrap(),
            LineRange { start: 12, end: 40 }
        );
        for invalid in ["", "0", "4-3", "1-", "-2", "one", "1-2-3"] {
            assert!(parse_line_range(invalid).is_err(), "accepted {invalid:?}");
        }
        assert_eq!(
            split_picker_range("src/main#12-40"),
            ("src/main", Some("12-40"))
        );
        assert_eq!(split_picker_range("src/#name"), ("src/#name", None));
    }

    #[test]
    fn attachment_sentence_punctuation_resolves_file_and_line_range() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::write(root.join("RUN_INPUTS.md"), "FIRST\nEVIDENCE\n").unwrap();
        let workspace = crate::workspace::WorkspaceContext::from_primary(root.clone(), root);
        for input in [
            "Use @RUN_INPUTS.md.",
            "Use @RUN_INPUTS.md, then continue",
            "Read @RUN_INPUTS.md#2.",
        ] {
            let expanded = expand_file_mentions(input, &workspace, &[]).unwrap();
            assert!(expanded.contains("EVIDENCE"), "{expanded}");
            assert!(!expanded.contains("file not found"), "{expanded}");
            assert!(expanded.starts_with(input));
            assert_eq!(
                explicit_file_paths(input, &workspace, &[]).unwrap().len(),
                1
            );
        }
        fs::write(
            workspace.cwd().join("RUN_INPUTS.md."),
            "EXACT_PUNCTUATED_NAME",
        )
        .unwrap();
        let expanded = expand_file_mentions("Use @RUN_INPUTS.md.", &workspace, &[]).unwrap();
        assert!(expanded.contains("EXACT_PUNCTUATED_NAME"));
    }

    #[test]
    fn expansion_revalidates_range_regular_file_and_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::write(root.join("sample.txt"), "one\ntwo\nthree\n").unwrap();
        let workspace =
            crate::workspace::WorkspaceContext::from_primary(root.clone(), root.clone());
        let expanded = expand_file_mentions("check @sample.txt#2-3", &workspace, &[]).unwrap();
        assert!(expanded.contains("lines 2-3"));
        assert!(expanded.contains("two\nthree"));
        assert!(expand_file_mentions("@sample.txt#3-4", &workspace, &[]).is_err());
        assert!(expand_file_mentions("@sample.txt#1-", &workspace, &[]).is_err());
        assert!(expand_file_mentions("@../sample.txt", &workspace, &[]).is_err());
        assert!(expand_file_mentions("@.", &workspace, &[]).is_err());
        fs::write(root.join("large.txt"), vec![b'x'; MAX_FILE_BYTES + 1]).unwrap();
        assert!(expand_file_mentions("@large.txt", &workspace, &[]).is_err());
        let prompt = "aji@Andrews-MacBook-Pro bin % echo hi\nuser@example.com";
        assert_eq!(
            expand_file_mentions(prompt, &workspace, &[]).unwrap(),
            prompt
        );
        let missing = expand_file_mentions("read @missing.txt", &workspace, &[]).unwrap();
        assert!(missing.contains("(file not found: missing.txt)"));
        assert!(!missing.contains("Andrews-MacBook-Pro"));

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("sample.txt"), root.join("linked.txt")).unwrap();
            assert!(expand_file_mentions("@linked.txt", &workspace, &[]).is_err());
        }
    }

    #[test]
    fn annotated_expansion_marks_only_host_file_sections() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::write(root.join("sample.txt"), "évidence\nsecond\n").unwrap();
        let workspace =
            crate::workspace::WorkspaceContext::from_primary(root.clone(), root.clone());
        let authored = "Keep this instruction @sample.txt#1-1";
        let (expanded, spans) = expand_file_mentions_with_spans(authored, &workspace, &[]).unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(&expanded[..spans[0].start], authored);
        assert_eq!(
            &expanded[spans[0].start..spans[0].end],
            "\n\n--- Attached file: sample.txt lines 1-1 ---\névidence\n--- End file ---"
        );
        assert_eq!(
            refreshed_attachment_references(&expanded, &spans),
            ["sample.txt#1-1"]
        );
        assert!(refreshed_attachment_references(&expanded, &[]).is_empty());
    }

    #[test]
    fn legacy_file_replacement_preserves_known_image_suffixes() {
        let file = "authored\n\n--- Attached file: a.txt ---\nold file\n--- End file ---";
        let images = "\n\n[Attached image: image/png 1×1; unavailable after this process]\n\n[Attached image: image/png 2×1; unavailable after this process]";
        assert_eq!(
            remove_legacy_attachment_sections(&format!("{file}{images}"), &["a.txt".into()]),
            format!("authored{images}")
        );
        let malformed = format!("{file}{images}private trailing data");
        assert_eq!(
            remove_legacy_attachment_sections(&malformed, &["a.txt".into()]),
            malformed
        );
        let unavailable = "\n\n(MCP resource unavailable: mcp:fixture/private)";
        assert_eq!(
            remove_legacy_attachment_sections(
                &format!("{file}{unavailable}{images}"),
                &["a.txt".into()]
            ),
            format!("authored{unavailable}{images}")
        );
        let arbitrary = format!("{file}{unavailable}\nuser text");
        assert_eq!(
            remove_legacy_attachment_sections(&arbitrary, &["a.txt".into()]),
            arbitrary
        );
    }

    #[test]
    fn refreshed_attachments_replace_only_matching_complete_legacy_sections() {
        let original = "Authored text\n\n--- Attached file: a.txt ---\nold a\n--- End file ---\n\n--- Attached file: b.txt lines 2-3 ---\nold b\n--- End file ---";
        assert_eq!(
            remove_legacy_attachment_sections(original, &["a.txt".into()]),
            "Authored text\n\n--- Attached file: b.txt lines 2-3 ---\nold b\n--- End file ---"
        );
        assert_eq!(
            remove_legacy_attachment_sections(original, &["b.txt#1-2".into()]),
            original
        );
        let truncated = "text\n\n--- Attached file: a.txt ---\nnot complete";
        assert_eq!(
            remove_legacy_attachment_sections(truncated, &["a.txt".into()]),
            truncated
        );
        let embedded_close = "text\n\n--- Attached file: a.txt ---\nbody\n--- End file ---\nstill body\n--- End file ---";
        assert_eq!(
            remove_legacy_attachment_sections(embedded_close, &["a.txt".into()]),
            "text"
        );
    }

    #[test]
    fn discovery_respects_ignore_multiple_roots_and_cap_cancellation() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        fs::write(first.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(first.path().join("kept.txt"), "ok").unwrap();
        fs::write(first.path().join("ignored.txt"), "no").unwrap();
        fs::write(second.path().join("second.txt"), "ok").unwrap();
        let roots = vec![
            RootSpec {
                id: "one".into(),
                path: first.path().canonicalize().unwrap(),
                label: "workspace".into(),
                read_only: false,
                primary: true,
            },
            RootSpec {
                id: "two".into(),
                path: second.path().canonicalize().unwrap(),
                label: "second".into(),
                read_only: true,
                primary: false,
            },
        ];
        let snapshot = discover_files(&roots, first.path(), 7, &AtomicBool::new(false));
        assert_eq!(snapshot.generation, 7);
        assert!(snapshot
            .files
            .iter()
            .any(|file| file.relative == "kept.txt"));
        assert!(snapshot
            .files
            .iter()
            .any(|file| file.relative == "second.txt" && file.read_only));
        assert!(!snapshot
            .files
            .iter()
            .any(|file| file.relative == "ignored.txt"));

        let cancelled = AtomicBool::new(true);
        assert!(discover_files(&roots, first.path(), 8, &cancelled)
            .files
            .is_empty());
    }

    #[test]
    fn filter_discovery_keeps_spaces_and_unicode_and_skips_ignored_files() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(".gitignore"), "secret.txt\n").unwrap();
        fs::write(root.path().join("my notes.txt"), "spaced").unwrap();
        fs::write(root.path().join("界面.md"), "unicode").unwrap();
        fs::write(root.path().join("secret.txt"), "hidden").unwrap();
        let roots = vec![RootSpec {
            id: "one".into(),
            path: root.path().canonicalize().unwrap(),
            label: "workspace".into(),
            read_only: false,
            primary: true,
        }];
        let filter = discover_filter_files(&roots, root.path(), 3, &AtomicBool::new(false));
        let names: Vec<_> = filter
            .files
            .iter()
            .map(|file| file.relative.as_str())
            .collect();
        assert!(names.contains(&"my notes.txt"), "{names:?}");
        assert!(names.contains(&"界面.md"), "{names:?}");
        assert!(!names.contains(&"secret.txt"), "{names:?}");
        let picker = discover_files(&roots, root.path(), 4, &AtomicBool::new(false));
        assert!(!picker
            .files
            .iter()
            .any(|file| file.relative == "my notes.txt"));
        assert!(picker.files.iter().any(|file| file.relative == "界面.md"));
    }

    #[test]
    fn ranking_prefers_fuzzy_score_then_frecency_and_is_bounded() {
        let files = Arc::new(vec![
            IndexedFile {
                root_id: "root".into(),
                root_label: "workspace".into(),
                relative: "src/main.rs".into(),
                reference: "src/main.rs".into(),
                read_only: false,
            },
            IndexedFile {
                root_id: "root".into(),
                root_label: "workspace".into(),
                relative: "misc/main-result-snapshot.txt".into(),
                reference: "misc/main-result-snapshot.txt".into(),
                read_only: false,
            },
        ]);
        let mut frecency = BTreeMap::new();
        frecency.insert(
            frecency_identity(&files[1].frecency_key()),
            FrecencyRecord {
                uses: 9,
                last_used_unix: 99,
            },
        );
        let snapshot = rank_matches(
            &files,
            &[],
            "main.rs",
            &frecency,
            3,
            &AtomicBool::new(false),
        );
        assert_eq!(snapshot.generation, 3);
        assert!(snapshot.entries.len() <= MAX_PICKER_MATCHES);
        assert!(
            matches!(&snapshot.entries[0], PickerEntry::File(file) if file.relative == "src/main.rs")
        );
    }

    #[test]
    fn frecency_is_relative_bounded_and_bare_is_process_only() {
        let temp = tempfile::tempdir().unwrap();
        let store = FrecencyStore::new(temp.path(), true);
        let key = FrecencyKey {
            root_id: "root".into(),
            relative: "src/main.rs".into(),
        };
        let records = store.record(&key, 10).unwrap();
        assert_eq!(records[&frecency_identity(&key)].uses, 1);
        assert!(
            !fs::read_to_string(temp.path().join("attachments/frecency.json"))
                .unwrap()
                .contains(temp.path().to_string_lossy().as_ref())
        );

        let bare = FrecencyStore::unavailable();
        let records = bare.record(&key, 11).unwrap();
        assert_eq!(records[&frecency_identity(&key)].uses, 1);

        fs::write(
            temp.path().join("attachments/frecency.json"),
            b"not valid json",
        )
        .unwrap();
        let recovered = store.record(&key, 12).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(store.load().unwrap(), recovered);

        let mut full = BTreeMap::new();
        for index in 0..MAX_FRECENCY_RECORDS {
            full.insert(
                frecency_identity(&FrecencyKey {
                    root_id: "root".into(),
                    relative: format!("src/{index}.rs"),
                }),
                FrecencyRecord {
                    uses: 1,
                    last_used_unix: index as i64,
                },
            );
        }
        store.save(&full).unwrap();
        let new_key = FrecencyKey {
            root_id: "root".into(),
            relative: "new.rs".into(),
        };
        let bounded = store
            .record(&new_key, MAX_FRECENCY_RECORDS as i64 + 1)
            .unwrap();
        assert_eq!(bounded.len(), MAX_FRECENCY_RECORDS);
        assert!(bounded.contains_key(&frecency_identity(&new_key)));
    }

    fn warm_roots(path: PathBuf, id: &str) -> Vec<RootSpec> {
        vec![RootSpec {
            id: id.into(),
            path,
            label: "workspace".into(),
            read_only: false,
            primary: true,
        }]
    }

    fn warm_relatives(search: &WarmIndexSearch) -> Vec<String> {
        search
            .matches
            .iter()
            .map(|file| file.relative.clone())
            .collect()
    }

    struct WarmSpawnFailure;

    impl WarmSpawnFailure {
        fn arm() -> Self {
            force_warm_pool_spawn_failure(true);
            Self
        }
    }

    impl Drop for WarmSpawnFailure {
        fn drop(&mut self) {
            force_warm_pool_spawn_failure(false);
        }
    }

    #[test]
    fn warm_index_reuses_one_walk_until_generation_or_root_changes() {
        force_warm_pool_spawn_failure(false);
        assert_eq!(WARM_MATCHER_THREADS, 2);
        assert_eq!(WARM_WALK_THREADS, 8);
        assert!(WARM_INDEX_TTL >= Duration::from_secs(2));
        assert!(WARM_INDEX_TTL <= Duration::from_secs(5));

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::create_dir(root.join("nested")).unwrap();
        fs::write(root.join("notes.md"), "notes").unwrap();
        fs::write(root.join("nested").join("warm-target.txt"), "keep").unwrap();
        fs::write(root.join("nested").join("other.log"), "log").unwrap();
        let roots = warm_roots(root.clone(), "workspace");
        let frecency = BTreeMap::new();

        let first = search_warm_index(&roots, &root, "warm-target.txt", &frecency, 1);
        assert!(!first.shallow, "first search should walk");
        assert_eq!(first.walks, 1);
        assert_eq!(first.generation, 1);
        assert!(
            first
                .matches
                .iter()
                .any(|file| file.relative == "nested/warm-target.txt"),
            "{:?}",
            warm_relatives(&first)
        );

        let second = search_warm_index(&roots, &root, "warm-target.txt", &frecency, 1);
        assert_eq!(second.walks, 1, "repeat search walked again");
        assert!(!second.shallow);
        assert_eq!(warm_relatives(&second), warm_relatives(&first));

        let bumped = search_warm_index(&roots, &root, "warm-target.txt", &frecency, 2);
        assert_eq!(bumped.walks, 2, "generation bump should walk");
        assert!(bumped
            .matches
            .iter()
            .any(|file| file.relative == "nested/warm-target.txt"));

        let other = tempfile::tempdir().unwrap();
        let other_root = other.path().canonicalize().unwrap();
        fs::write(other_root.join("warm-target.txt"), "other").unwrap();
        fs::write(other_root.join("alone.md"), "alone").unwrap();
        let other_roots = warm_roots(other_root.clone(), "other");
        let moved = search_warm_index(&other_roots, &other_root, "warm-target.txt", &frecency, 2);
        assert_eq!(moved.walks, 1, "a different root should walk");
        assert_eq!(warm_relatives(&moved), vec!["warm-target.txt".to_string()]);
    }

    #[test]
    fn warm_index_browse_is_one_level_when_pool_cannot_spawn() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::create_dir(root.join("nested")).unwrap();
        fs::write(root.join("top-target.txt"), "top").unwrap();
        fs::write(root.join("nested").join("hidden-target.txt"), "deep").unwrap();
        fs::write(root.join("readme.md"), "readme").unwrap();
        let roots = warm_roots(root.clone(), "shallow");
        let _guard = WarmSpawnFailure::arm();
        let found = search_warm_index(&roots, &root, "top-target.txt", &BTreeMap::new(), 1);
        assert!(found.shallow);
        assert_eq!(found.walks, 0);
        assert!(
            found
                .matches
                .iter()
                .any(|file| file.relative == "top-target.txt"),
            "{:?}",
            warm_relatives(&found)
        );
        assert!(
            found
                .matches
                .iter()
                .all(|file| !file.relative.contains('/')),
            "one-level browse walked into nested/: {:?}",
            warm_relatives(&found)
        );
    }
}
