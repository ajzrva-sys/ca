//! Local, session-owned projections. Original bytes and alias dictionaries never
//! form part of a provider request. All expensive work is run by callers on workers.
#![allow(dead_code)]
mod arguments;
#[cfg(test)]
mod builtin_tests;
pub(crate) mod command_workspace;
#[cfg(test)]
mod filename_tests;
pub(crate) mod filenames;
mod literals;
pub(crate) mod native_control;
pub(crate) mod policy;
pub(crate) mod policy_store;
mod preview;
pub(crate) mod projection;
pub(crate) mod read_projection;
pub(crate) mod shell;
#[cfg(test)]
mod site_tests;
pub(crate) mod store;
pub(crate) mod tasks;
pub(crate) mod tool_contract;

#[cfg(test)]
mod client_performance_tests;
#[cfg(test)]
mod client_tests;
#[cfg(test)]
mod name_tests;
#[cfg(test)]
mod origin_tests;
#[cfg(test)]
mod performance_tests;
#[cfg(test)]
mod preview_tests;
#[cfg(test)]
mod task_filter_tests;
#[cfg(test)]
mod task_tests;
#[cfg(test)]
mod tests;

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use policy::{Action, Policy};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};
use store::{AliasRegistry, PrivacySessionRef};
use uuid::Uuid;

const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_WIRE_BYTES: usize = 128 * 1024 * 1024;

// Fixed host protocol guidance; dynamic description text remains private data.
const FILENAME_TOOL_GUIDANCE: &str = " Use complete discovered filename references for path arguments; '.' lists the workspace. Issued references are identifiers, not filenames to search for; pass them unchanged. Different child spellings in a successful listing do not change the requested target; use that native result for the supplied reference. Source link references can be opened directly. To create a file or directory, append one new plain leaf beneath a discovered directory reference and re-list after creation. For select_files folder/destination and move_files destination, use canonical directory references or host-issued batch folder references; select by public extensions to preserve hidden filenames. For HTML/CSS/JS links, insert a complete source-link or canonical file reference directly; never strip its components or prepend ../ or ./ to it. Ca restores the appropriate relative URL. Retain source references in edits. Registered built-ins use local privacy contracts. Commands run in a disposable writable project copy; original sources remain unchanged. Command artifacts have private references. Unknown or evaluated filename expressions and custom tools are unavailable.";

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceLink {
    pub id: String,
    pub path: String,
    pub digest: String,
}

#[derive(Clone)]
pub(crate) struct OutboundSnapshot {
    pub generation: u64,
    pub original: Arc<String>,
    pub sent: Arc<String>,
    pub changes: Arc<str>,
    pub latest: Arc<str>,
    pub pane: Arc<str>,
    pub opaque_blocks: usize,
    pub sources: Vec<SourceLink>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ReadReceipt {
    #[serde(default)]
    id: Option<String>,
    path: String,
    source: String,
    source_digest: String,
    original: String,
    start: usize,
    end: usize,
    hashes: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    read_footer: Option<read_projection::Footer>,
    #[serde(default)]
    attachment: Option<crate::attachments::AttachmentSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_control: Option<native_control::TypedNativeControl>,
}

struct CachedProjection {
    projection: Arc<projection::Projection>,
    view: EditView,
    ticket: crate::cache_memory::Ticket,
}

struct ExactMask {
    input: Arc<str>,
    replacement: Option<Arc<str>>,
    input_edges: [u8; 32],
    replacement_edges: Option<[u8; 32]>,
    paths_scanned: bool,
    ticket: crate::cache_memory::Ticket,
}

struct CachedMask {
    /// None means the validated input is unchanged, so do not retain a second
    /// full transcript copy merely to cache that result.
    replacement: Option<Arc<str>>,
    ticket: crate::cache_memory::Ticket,
}

#[derive(Clone)]
struct CommitOwner {
    store: crate::session::SessionStore,
    lease: crate::session_access::SessionWriterLease,
}

#[derive(Clone)]
struct BindingAdvance {
    before: Option<crate::session::SessionBinding>,
    after: Option<crate::session::SessionBinding>,
    reference: PrivacySessionRef,
}

struct State {
    registry: AliasRegistry,
    filenames: Option<filenames::FilenameAliases>,
    reference: Option<PrivacySessionRef>,
    active: BTreeMap<String, String>,
    task_names: BTreeMap<String, String>,
    allowed: std::collections::BTreeSet<String>,
    matcher: Option<AhoCorasick>,
    matcher_values: Vec<(String, String)>,
    matcher_dirty: bool,
    mask_cache: HashMap<(u64, u64, String), CachedMask>,
    exact_masks: Vec<ExactMask>,
    mask_cache_generation: Option<(u64, u64)>,
    safe_template_hashes: HashMap<(String, String), crate::cache_memory::Ticket>,
    cache: HashMap<String, CachedProjection>,
    reads: HashMap<String, ReadReceipt>,
    sources: BTreeMap<String, String>,
    views: HashMap<String, EditView>,
    prepared_edits: HashMap<String, EditView>,
    classified_revision: Option<u64>,
    scanned_authored: std::collections::HashSet<String>,
    advances: Vec<BindingAdvance>,
    pending_source_links: Vec<SourceLink>,
}

#[derive(Clone)]
struct EditView {
    source_digest: String,
    policy_revision: u64,
    policy_digest: String,
    projected: Arc<str>,
    replacements: Arc<[(String, String)]>,
}

struct CurrentPolicy {
    selected_only: bool,
    stored: Policy,
    value: Policy,
    digest: String,
}

impl CurrentPolicy {
    fn for_task(stored: Policy) -> Self {
        let mut policy = Self::new(stored, true);
        if policy.stored.enabled {
            policy.value.default_action = policy.stored.default_action;
            policy.digest =
                digest(&serde_json::to_vec(&policy.value).expect("privacy policy is serializable"));
        }
        policy
    }

    fn new(stored: Policy, selected_only: bool) -> Self {
        let mut value = stored.clone();
        if selected_only {
            value.enabled = true;
            value.default_action = Action::Allow;
        }
        let digest = digest(&serde_json::to_vec(&value).expect("privacy policy is serializable"));
        Self {
            selected_only,
            stored,
            value,
            digest,
        }
    }
}

pub(crate) fn overlay_policy(stored: Policy, selected_only: bool, filenames: bool) -> Policy {
    filename_policy(CurrentPolicy::new(stored, selected_only), filenames).value
}

pub(crate) fn startup_override_note(
    stored: &Policy,
    selected_only: bool,
    filenames: bool,
) -> Option<String> {
    let effective = overlay_policy(stored.clone(), selected_only, filenames);
    if effective == *stored {
        return None;
    }
    let mut note =
        String::from("Startup override is active. Saving updates the project policy only.");
    if selected_only {
        note.push_str(" Selected values only is active for this process.");
    } else if filenames && effective.rules != stored.rules {
        note.push_str(" Filename masking is active while project rules are off.");
    }
    Some(note)
}

/// Filename projection enables the provider boundary without enabling saved,
/// currently disabled source selections or changing their on-disk policy.
fn filename_policy(mut policy: CurrentPolicy, filenames: bool) -> CurrentPolicy {
    if filenames && !policy.value.enabled {
        policy.value.enabled = true;
        policy.value.default_action = Action::Allow;
        policy.value.rules.clear();
        policy.digest =
            digest(&serde_json::to_vec(&policy.value).expect("privacy policy is serializable"));
    }
    policy
}

pub(crate) struct Context {
    pub session_id: Uuid,
    scope: PathBuf,
    workspace: PathBuf,
    policy_path: PathBuf,
    policy: RwLock<CurrentPolicy>,
    selected_only: bool,
    filename_mode: bool,
    filename_content_only: bool,
    owner: Mutex<Option<CommitOwner>>,
    ephemeral: std::sync::atomic::AtomicBool,
    private_revision: std::sync::atomic::AtomicU64,
    commit_gate: Arc<Mutex<()>>,
    task_generation: u64,
    outbound: RwLock<Option<OutboundSnapshot>>,
    preview_cache: Mutex<preview::Cache>,
    state: Mutex<State>,
    task_broker: Mutex<Option<tasks::Broker>>,
    task_context: Mutex<Option<store::PrivateTaskContextRefV1>>,
    command_workspaces: Mutex<command_workspace::Workspaces>,
    root_authority: Mutex<Vec<PathBuf>>,
    task_filter: Option<Arc<crate::provider_filter::ProviderFilter>>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrivacyContext")
            .field("session", &self.session_id)
            .field("enabled", &self.enabled())
            .finish_non_exhaustive()
    }
}

fn contexts() -> &'static Mutex<HashMap<Uuid, Weak<Context>>> {
    static CONTEXTS: OnceLock<Mutex<HashMap<Uuid, Weak<Context>>>> = OnceLock::new();
    CONTEXTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn policy_coordination() -> &'static Mutex<()> {
    static POLICY_COORDINATION: OnceLock<Mutex<()>> = OnceLock::new();
    POLICY_COORDINATION.get_or_init(|| Mutex::new(()))
}

tokio::task_local! { static CURRENT: Option<Arc<Context>>; }

#[cfg(test)]
thread_local! {
    static BEFORE_BINDING_PUBLISH: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

pub(crate) async fn scope<F: std::future::Future>(
    context: Option<Arc<Context>>,
    operation: F,
) -> F::Output {
    CURRENT.scope(context, operation).await
}

/// Blocking workers do not inherit the async task local. Responses replay
/// restores filename aliases through `privacy::current()`, so request
/// preparation on a worker must enter this scope first.
pub(crate) fn scope_sync<R>(context: Option<Arc<Context>>, operation: impl FnOnce() -> R) -> R {
    CURRENT.sync_scope(context, operation)
}

pub(crate) fn current() -> Option<Arc<Context>> {
    CURRENT.try_with(Clone::clone).ok().flatten().or_else(|| {
        crate::owned_tools::current_recovery().and_then(|owner| for_session(owner.session_id))
    })
}

pub(crate) fn for_session(session: Uuid) -> Option<Arc<Context>> {
    contexts()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&session)
        .and_then(Weak::upgrade)
}

pub(crate) fn for_owner(owner: crate::jobs::JobOwner) -> Option<Arc<Context>> {
    for_edit_owner(owner).filter(|context| context.enabled())
}

/// A prepared private edit remains bound even after its policy is disabled.
/// Read, shell and provider lookups continue to use the enabled-only helper.
pub(crate) fn for_edit_owner(owner: crate::jobs::JobOwner) -> Option<Arc<Context>> {
    CURRENT
        .try_with(Clone::clone)
        .ok()
        .flatten()
        .filter(|context| owner.session_id.is_none_or(|id| id == context.session_id))
        .or_else(|| {
            owner
                .session_id
                .and_then(for_session)
                .or_else(|| owner.session_id.is_none().then(current).flatten())
        })
}

pub(crate) fn sync_session_binding(session: &mut crate::session::SessionV4) {
    let Some(context) = for_session(session.id) else {
        return;
    };
    let state = context.state.lock().unwrap_or_else(|p| p.into_inner());
    for advance in &state.advances {
        if session.storage_binding == advance.before {
            session.storage_binding = advance.after.clone();
            session.privacy = Some(advance.reference.clone());
        }
    }
}

impl Context {
    /// Called on a blocking worker. Opening an inactive project writes nothing.
    pub(crate) fn open(
        scope: PathBuf,
        workspace: PathBuf,
        session_id: Uuid,
        reference: Option<PrivacySessionRef>,
    ) -> Result<Arc<Self>, String> {
        let workspace = std::fs::canonicalize(&workspace)
            .map_err(|e| format!("Privacy workspace unavailable: {e}"))?;
        if let Some(existing) = for_session(session_id) {
            if existing.scope == scope && existing.workspace == workspace {
                let current = existing
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .reference
                    .clone();
                if current == reference {
                    return Ok(existing);
                }
                existing.clear_owner();
                contexts()
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&session_id);
            } else {
                return Err("Privacy session belongs to a different workspace or profile".into());
            }
        }
        let workspace = std::fs::canonicalize(&workspace)
            .map_err(|e| format!("Privacy workspace unavailable: {e}"))?;
        let policy_path = scope
            .join("privacy/projects")
            .join(digest(workspace.as_os_str().as_encoded_bytes()))
            .join("policy.json");
        let policy = policy_store::load(&policy_path)?;
        let registry = match reference.as_ref() {
            Some(reference) => AliasRegistry::load(&scope, session_id, reference)?,
            None => AliasRegistry::new(session_id),
        };
        if registry.session_id() != session_id {
            return Err("Privacy alias registry belongs to another session".into());
        }
        Self::from_registry_with_task(
            scope,
            workspace,
            policy_path,
            CurrentPolicy::new(
                policy,
                crate::provider_filter::process_current().selected_values_only(),
            ),
            registry,
            reference,
            false,
            None,
            true,
        )
    }

    #[cfg(test)]
    fn from_registry(
        scope: PathBuf,
        workspace: PathBuf,
        policy_path: PathBuf,
        policy: CurrentPolicy,
        registry: AliasRegistry,
        reference: Option<PrivacySessionRef>,
        ephemeral: bool,
    ) -> Result<Arc<Self>, String> {
        Self::from_registry_with_task(
            scope,
            workspace,
            policy_path,
            policy,
            registry,
            reference,
            ephemeral,
            None,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_registry_with_task(
        scope: PathBuf,
        workspace: PathBuf,
        policy_path: PathBuf,
        policy: CurrentPolicy,
        registry: AliasRegistry,
        reference: Option<PrivacySessionRef>,
        ephemeral: bool,
        task_filter: Option<Arc<crate::provider_filter::ProviderFilter>>,
        register: bool,
    ) -> Result<Arc<Self>, String> {
        let task_filter = match task_filter {
            Some(filter) => Some(filter),
            None => registry
                .entries()
                .iter()
                .rev()
                .find(|entry| entry.kind == "TASK_FILTER")
                .map(|entry| {
                    let record: crate::task_filter::Record = serde_json::from_str(&entry.value)
                        .map_err(|_| "Saved task filter is invalid")?;
                    crate::provider_filter::ProviderFilter::for_task(
                        crate::provider_filter::process_current(),
                        record,
                    )
                })
                .transpose()?,
        };
        let filter = task_filter
            .clone()
            .unwrap_or_else(crate::provider_filter::process_current);
        let policy = if task_filter.is_some() {
            CurrentPolicy::for_task(policy.stored)
        } else {
            policy
        };
        let filename_snapshot: Option<filenames::Snapshot> = None;
        let _ = registry
            .entries()
            .iter()
            .rev()
            .find(|entry| entry.kind.eq_ignore_ascii_case("filename_registry"));
        let filename_mode = false;
        let _ = filter.filenames();
        let filename_content_only = filename_mode && !policy.selected_only;
        let selected_only = policy.selected_only;
        let policy = filename_policy(policy, filename_mode);
        let session_id = registry.session_id();
        let task_context = reference.as_ref().and_then(|r| r.task_context.clone());
        let mut reads = HashMap::new();
        let mut sources = BTreeMap::new();
        let mut active = BTreeMap::new();
        let mut task_names = BTreeMap::new();
        for entry in registry.entries() {
            if entry.kind.eq_ignore_ascii_case("source_receipt") {
                let receipt: ReadReceipt = serde_json::from_str(&entry.value)
                    .map_err(|_| "Invalid private source receipt")?;
                if receipt.native_control.is_none() {
                    sources.insert(receipt.path.clone(), receipt.source_digest.clone());
                }
                let key = receipt
                    .id
                    .clone()
                    .unwrap_or_else(|| format!("legacy:{}", digest(receipt.original.as_bytes())));
                reads.insert(key, receipt);
            } else if entry.kind.eq_ignore_ascii_case("filename_registry")
                || entry.kind.eq_ignore_ascii_case("native_digest")
                || entry.kind == "TASK_FILTER"
            {
                // Local-only metadata, never an active content substitution.
            } else if entry.kind == "task_path" {
                task_names.insert(entry.value.clone(), entry.token.clone());
            } else {
                active.insert(entry.value.clone(), entry.token.clone());
            }
        }
        if selected_only || policy.value.default_action == Action::Allow {
            active.clear();
            for entry in registry
                .entries()
                .iter()
                .filter(|entry| matches!(entry.kind.as_str(), "JOB" | "WATCH" | "AGENT"))
            {
                active.insert(entry.value.clone(), entry.token.clone());
            }
        }
        let mut filenames = if let Some(snapshot) = filename_snapshot {
            Some(filenames::FilenameAliases::from_snapshot(
                workspace.clone(),
                snapshot,
            )?)
        } else if filename_mode {
            Some(filenames::FilenameAliases::new(
                workspace.clone(),
                &session_id.simple().to_string(),
            )?)
        } else {
            None
        };
        if let Some(names) = &mut filenames {
            let namespace = task_filter
                .as_ref()
                .and_then(|filter| filter.task_record())
                .map(|record| record.task_id)
                .unwrap_or(session_id);
            names.fork_namespace(&namespace.simple().to_string())?;
        }
        static NEXT_CONTEXT_GENERATION: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1);
        let task_generation =
            NEXT_CONTEXT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let commit_gate = for_session(session_id)
            .map(|context| Arc::clone(&context.commit_gate))
            .unwrap_or_else(|| Arc::new(Mutex::new(())));
        let context = Arc::new(Self {
            session_id,
            scope,
            workspace,
            policy_path,
            policy: RwLock::new(policy),
            selected_only,
            filename_mode,
            filename_content_only,
            owner: Mutex::new(None),
            ephemeral: std::sync::atomic::AtomicBool::new(ephemeral),
            private_revision: std::sync::atomic::AtomicU64::new(1),
            commit_gate,
            task_generation,
            outbound: RwLock::new(None),
            preview_cache: Mutex::new(preview::Cache::default()),
            state: Mutex::new(State {
                registry,
                filenames,
                reference,
                active,
                task_names,
                allowed: Default::default(),
                matcher: None,
                matcher_values: Vec::new(),
                matcher_dirty: true,
                mask_cache: HashMap::new(),
                exact_masks: Vec::new(),
                mask_cache_generation: None,
                safe_template_hashes: HashMap::new(),
                cache: HashMap::new(),
                reads,
                sources,
                views: HashMap::new(),
                prepared_edits: HashMap::new(),
                classified_revision: None,
                scanned_authored: Default::default(),
                advances: Vec::new(),
                pending_source_links: Vec::new(),
            }),
            task_broker: Mutex::new(None),
            task_context: Mutex::new(task_context),
            command_workspaces: Mutex::new(command_workspace::Workspaces::default()),
            root_authority: Mutex::new(Vec::new()),
            task_filter,
        });
        if register {
            context.activate_task_filter();
        }
        Ok(context)
    }

    /// Start a no-session branch with the exact inherited token vocabulary and
    /// source receipts, but no writer, sidecars, edit tickets, or cached views.
    pub(crate) fn fork_ephemeral(&self, new_session_id: Uuid) -> Result<Arc<Self>, String> {
        self.refresh_policy()?;
        if for_session(new_session_id).is_some() {
            return Err("Privacy branch identity is already active".into());
        }
        let registry = {
            let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let mut registry = state.registry.fork_in_memory(new_session_id)?;
            // Lazy path/name references may have been issued since the last
            // request commit. Fork the current vocabulary, not an older ledger
            // snapshot, while leaving the parent's registry untouched.
            if let Some(names) = &state.filenames {
                let snapshot = serde_json::to_string(&names.snapshot())
                    .map_err(|_| "Filename mappings cannot be encoded")?;
                registry.alias("filename_registry", &snapshot)?;
            }
            registry
        };
        let fork = Self::from_registry_with_task(
            self.scope.clone(),
            self.workspace.clone(),
            self.policy_path.clone(),
            CurrentPolicy::new(self.stored_policy(), self.selected_only),
            registry,
            None,
            true,
            self.task_filter.clone(),
            true,
        )?;
        *fork.task_context.lock().unwrap_or_else(|p| p.into_inner()) = self
            .task_context
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        Ok(fork)
    }

    pub(crate) fn task_filter(&self) -> Option<Arc<crate::provider_filter::ProviderFilter>> {
        self.task_filter.clone()
    }

    pub(crate) fn task_filter_policy(&self) -> Option<crate::task_filter::Policy> {
        self.task_filter
            .as_ref()
            .and_then(|filter| filter.task_record())
            .map(|record| record.policy)
    }

    /// Detached until Send. Old workers retain their existing context and filter.
    pub(crate) fn with_task_filter(
        &self,
        policy: crate::task_filter::Policy,
    ) -> Result<Arc<Self>, String> {
        policy.validate()?;
        self.refresh_policy()?;
        let mut registry = {
            let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let mut registry = state.registry.fork_for_task()?;
            if let Some(names) = &state.filenames {
                registry.alias(
                    "filename_registry",
                    &serde_json::to_string(&names.snapshot())
                        .map_err(|_| "Filename mappings cannot be encoded")?,
                )?;
            }
            registry
        };
        let prior = self
            .task_filter
            .as_ref()
            .and_then(|filter| filter.task_record());
        let record = crate::task_filter::Record {
            version: 1,
            task_id: Uuid::new_v4(),
            policy,
            restorations: prior
                .as_ref()
                .map(|record| record.restorations.clone())
                .unwrap_or_default(),
            urls: prior.map(|record| record.urls).unwrap_or_else(|| {
                crate::provider_filter::process_current()
                    .task_url_map()
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone()
            }),
        };
        let filter = crate::provider_filter::ProviderFilter::for_task(
            crate::provider_filter::process_current(),
            record,
        )?;
        registry.alias(
            "TASK_FILTER",
            &serde_json::to_string(&filter.task_record())
                .map_err(|_| "Task filter cannot be encoded")?,
        )?;
        let context = Self::from_registry_with_task(
            self.scope.clone(),
            self.workspace.clone(),
            self.policy_path.clone(),
            CurrentPolicy::new(self.stored_policy(), true),
            registry,
            self.state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .reference
                .clone(),
            self.ephemeral.load(std::sync::atomic::Ordering::Acquire),
            Some(filter),
            false,
        )?;
        *context.owner.lock().unwrap_or_else(|p| p.into_inner()) =
            self.owner.lock().unwrap_or_else(|p| p.into_inner()).clone();
        *context
            .root_authority
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = self
            .root_authority
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        *context
            .task_context
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = self
            .task_context
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        Ok(context)
    }

    pub(crate) fn activate_task_filter(self: &Arc<Self>) {
        contexts()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(self.session_id, Arc::downgrade(self));
    }

    pub(crate) fn commit_task_filter(self: &Arc<Self>) -> Result<(), String> {
        let _commit = self.commit_gate.lock().unwrap_or_else(|p| p.into_inner());
        let previous = for_session(self.session_id)
            .filter(|current| !std::ptr::eq(current.as_ref(), self.as_ref()));
        if let Some(current) = &previous {
            if current.task_generation > self.task_generation {
                return Err("A newer task replaced this preview; review it again.".into());
            }
            self.merge_task_history(current)?;
        }
        let previous_urls = previous.as_ref().map(|context| {
            context
                .task_filter
                .clone()
                .unwrap_or_else(crate::provider_filter::process_current)
                .task_url_map()
        });
        let mut url_guard = previous_urls
            .as_ref()
            .map(|urls| urls.lock().unwrap_or_else(|p| p.into_inner()));
        if let (Some(active), Some(filter)) = (&url_guard, &self.task_filter) {
            let map = filter.task_url_map();
            let mut reviewed = map.lock().unwrap_or_else(|p| p.into_inner());
            let common = active.len().min(reviewed.len());
            if active[..common] != reviewed[..common] {
                return Err(
                    "URL aliases changed while reviewing; review the refreshed task.".into(),
                );
            }
            if active.len() > reviewed.len() {
                *reviewed = active.to_vec();
            }
        }
        self.commit_registry_locked()?;
        if let (Some(active), Some(shared), Some(filter)) =
            (&mut url_guard, &previous_urls, &self.task_filter)
        {
            let reviewed = filter
                .task_url_map()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            **active = reviewed;
            filter.share_task_url_map(Arc::clone(shared));
        }
        self.activate_task_filter();
        Ok(())
    }

    pub(crate) fn apply_remembered_task_selection(
        &self,
        suggested: &mut crate::task_filter::Policy,
    ) -> Result<(), String> {
        let edits = policy_store::load_task_selection(
            &self.policy_path.with_file_name("task-filter-edits.json"),
        )?;
        edits.apply(suggested);
        suggested.validate()
    }

    pub(crate) fn remember_task_selection(
        &self,
        suggested: &crate::task_filter::Policy,
        selected: &crate::task_filter::Policy,
    ) -> Result<(), String> {
        policy_store::remember_task_selection(
            &self.policy_path.with_file_name("task-filter-edits.json"),
            suggested,
            selected,
        )
    }

    /// Project known local inputs using this exact detached task snapshot.
    pub(crate) fn preview_task(
        &self,
        original: &str,
    ) -> Result<crate::task_filter::Preview, String> {
        self.refresh_filenames()?;
        self.discover_selected_sources()?;
        let receipts = self
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .reads
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut projected = original.to_string();
        for receipt in receipts {
            if !receipt.original.is_empty() && projected.contains(&receipt.original) {
                let key = receipt
                    .id
                    .clone()
                    .unwrap_or_else(|| format!("legacy:{}", digest(receipt.original.as_bytes())));
                let replacement = self.project_retrieved(&receipt.original, Some(&key))?;
                projected = projected.replace(&receipt.original, &replacement);
            }
        }
        projected = self.mask_known(&projected)?;
        let filter = self
            .task_filter
            .as_ref()
            .ok_or("Task filter preview is unavailable")?;
        let provider_copy = filter.preview_task_text(&projected);
        let blocked = filter.preview_blocked(&projected);
        let mut changes = crate::task_filter::describe_changes(original, &provider_copy);
        if let Some(reason) = &blocked {
            changes.insert_str(
                0,
                &format!("Blocked by an explicit stop rule: {reason}\n\n"),
            );
        }
        Ok(crate::task_filter::Preview {
            original: original.into(),
            provider_copy,
            changes,
            coverage: self.task_filter_coverage(),
            blocked,
            filename_samples: self.filename_preview_samples(),
        })
    }

    /// Local proof rows for the pre-Send preview. Empty files and dotfiles win.
    fn filename_preview_samples(&self) -> Vec<(String, String)> {
        if !self.filename_mode {
            return Vec::new();
        }
        let files = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let Some(names) = state.filenames.as_ref() else {
                return Vec::new();
            };
            names.shallow_files()
        };
        let dotfile = files
            .iter()
            .find(|(name, _)| name.starts_with('.') && name != "." && name != "..")
            .cloned();
        let empty = files.iter().find(|(name, _)| {
            std::fs::metadata(self.workspace.join(name))
                .map(|metadata| metadata.len() == 0)
                .unwrap_or(false)
        });
        let mut samples = Vec::new();
        if let Some(dotfile) = dotfile {
            samples.push(dotfile);
        }
        if let Some(empty) = empty.cloned() {
            if samples.iter().all(|(name, _)| name != &empty.0) {
                samples.push(empty);
            }
        }
        if samples.is_empty() {
            samples.extend(files.into_iter().take(2));
        }
        samples
    }

    /// Project a successful listing before it can leave the process.
    /// Local `model_text()` stays unchanged. Errors stay raw.
    pub(crate) fn project_tool_output(
        &self,
        name: &str,
        arguments: &str,
        output: &str,
    ) -> Result<String, String> {
        if !self.filename_mode || !matches!(name, "list_dir" | "glob") {
            return Ok(output.to_string());
        }
        if output.starts_with("Error") || output.starts_with("[tool failed]") {
            return Ok(output.to_string());
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let names = state
            .filenames
            .as_mut()
            .ok_or("Filename mappings are unavailable")?;
        if name == "glob" {
            let mut rows = Vec::new();
            for row in output.lines() {
                if row.is_empty()
                    || row.starts_with("No files")
                    || row.starts_with('[')
                    || names.is_known_path_alias(row)
                {
                    rows.push(row.to_string());
                    continue;
                }
                rows.push(names.project_path(row)?);
            }
            return Ok(rows.join("\n"));
        }
        let args: serde_json::Value =
            serde_json::from_str(arguments).map_err(|_| "Invalid listing arguments")?;
        let path = args["path"].as_str().unwrap_or(".");
        let root = names.listing_root(path, &self.workspace)?;
        if !names.authorized_path(&root) {
            return Err("Listing context needs a current folder grant".into());
        }
        let mut rows = Vec::new();
        let mut withheld = 0usize;
        for row in output.lines() {
            if row == "[Directory listing limited by entry, output or deadline budget]"
                || names.is_known_path_alias(row)
            {
                rows.push(row.to_string());
                continue;
            }
            let child = root.join(row.strip_suffix('/').unwrap_or(row));
            match child.to_str().map(|text| names.project_path(text)) {
                Some(Ok(alias)) => rows.push(alias),
                _ => withheld += 1,
            }
        }
        if withheld > 0 {
            rows.push(format!("[{withheld} listing rows withheld]"));
        }
        Ok(rows.join("\n"))
    }

    pub(crate) fn task_filter_coverage(&self) -> String {
        let note = self
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .filenames
            .as_ref()
            .map(filenames::FilenameAliases::coverage_note)
            .unwrap_or_default();
        if note.is_empty() {
            crate::task_filter::COVERAGE.into()
        } else {
            format!("{}\n{}", note, crate::task_filter::COVERAGE)
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        false
    }
    pub(crate) fn filenames_enabled(&self) -> bool {
        false
    }
    pub(crate) fn filename_only(&self) -> bool {
        self.filename_content_only && !self.stored_policy().enabled
    }
    pub(crate) fn check_filename_tool(&self, name: &str) -> Result<(), String> {
        if !self.filename_mode
            || crate::agent::registry::ToolRegistry::builtins()
                .get(name)
                .is_some_and(|spec| spec.privacy.callable())
        {
            Ok(())
        } else {
            Err("This custom tool is unsupported without a filename privacy contract; use a registered native tool".into())
        }
    }
    pub(crate) fn set_tool_roots(&self, workspace: &crate::workspace::WorkspaceContext) {
        *self
            .root_authority
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = workspace.execution_roots();
    }
    pub(crate) fn authorize_tool_roots(
        &self,
        workspace: &crate::workspace::WorkspaceContext,
    ) -> Result<(), String> {
        self.set_tool_roots(workspace);
        self.refresh_authorized_roots()
    }
    fn refresh_authorized_roots(&self) -> Result<(), String> {
        if !self.filename_mode {
            return Ok(());
        }
        let mut roots = self
            .root_authority
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        roots.extend(self.command_artifact_roots());
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state
            .filenames
            .as_mut()
            .ok_or("Filename mappings are unavailable")?
            .authorize_roots(&roots)?;
        state.cache.clear();
        state.exact_masks.clear();
        state.mask_cache.clear();
        Ok(())
    }
    pub(crate) fn project_filename_path(&self, path: &Path) -> Result<String, String> {
        if !self.filename_mode {
            return path
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| "Unsupported native filename encoding".to_string());
        }
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .filenames
            .as_mut()
            .ok_or("Filename mappings are unavailable")?
            .path_alias(path)
    }
    pub(crate) fn project_query_source(&self, path: &Path, source: &str) -> Result<String, String> {
        self.project_source_shared(&self.relative(path), source, &self.policy(), None, false)
            .map(|projection| projection.text.clone())
    }
    pub(crate) fn resolve_filename_path(
        &self,
        reference: &str,
        create: bool,
    ) -> Result<PathBuf, String> {
        if !self.filename_mode {
            return Ok(PathBuf::from(reference));
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let names = state
            .filenames
            .as_mut()
            .ok_or("Filename mappings are unavailable")?;
        if create {
            names.resolve_write_path(reference)
        } else {
            names.resolve_path(reference)
        }
    }
    fn resolve_filename_grant_path(&self, reference: &str) -> Result<PathBuf, String> {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .filenames
            .as_ref()
            .ok_or("Filename mappings are unavailable")?
            .resolve_grant_request(reference)
    }
    fn filename_path_allowed(&self, path: &Path) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .filenames
            .as_ref()
            .is_some_and(|names| names.authorized_path(path))
    }
    /// Discover on a worker; commit the snapshot through the session CAS before send.
    pub(crate) fn refresh_filenames(&self) -> Result<(), String> {
        if !self.filename_mode {
            return Ok(());
        }
        self.refresh_authorized_roots()?;
        let roots = self
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .filenames
            .as_ref()
            .ok_or("Filename mappings are unavailable")?
            .census_roots();
        // The complete metadata walk may visit millions of entries. Do not hold
        // the UI-facing privacy state lock while collecting its unique names.
        let census = filenames::collect_census(&roots)?;
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let names = state
            .filenames
            .as_mut()
            .ok_or("Filename mappings are unavailable")?;
        let changed = names.apply_census(census)?;
        let snapshot = names.snapshot();
        // A newly created target can make an unchanged source link executable
        // even when its basename already existed elsewhere in the census.
        state.cache.clear();
        // Invalidate old prepared/cache proofs before any fallible persistence
        // work: even a failed save must not authorize the earlier raw spelling.
        if changed {
            self.private_revision
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            state.cache.clear();
            state.exact_masks.clear();
            state.mask_cache.clear();
            state.safe_template_hashes.clear();
        }
        let snapshot =
            serde_json::to_string(&snapshot).map_err(|_| "Filename mappings cannot be encoded")?;
        let previous = state
            .registry
            .entries()
            .iter()
            .rev()
            .find(|entry| entry.kind.eq_ignore_ascii_case("filename_registry"))
            .map(|entry| entry.value.as_str());
        if previous != Some(snapshot.as_str()) {
            if !changed {
                self.private_revision
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            }
            state.registry.alias("filename_registry", &snapshot)?;
        }
        Ok(())
    }
    pub(crate) fn private_revision(&self) -> u64 {
        self.private_revision
            .load(std::sync::atomic::Ordering::Acquire)
    }
    pub(crate) fn summary(&self) -> String {
        let policy = self.policy.read().unwrap_or_else(|p| p.into_inner());
        let mode = if policy.value.default_action == Action::Allow {
            "Selected values only"
        } else {
            "Hidden unless allowed"
        };
        if self.filename_mode {
            format!(
                "Project privacy · {mode} · filenames on · {} selected values",
                policy.value.rules.len()
            )
        } else {
            format!(
                "Project privacy · {mode} · {} rules",
                policy.value.rules.len()
            )
        }
    }
    pub(crate) fn policy(&self) -> Policy {
        self.policy
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .value
            .clone()
    }
    pub(crate) fn stored_policy(&self) -> Policy {
        self.policy
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .stored
            .clone()
    }

    pub(crate) fn editor_preview_policy(&self, stored: Policy) -> Policy {
        if self.task_filter.is_some() {
            return filename_policy(CurrentPolicy::for_task(stored), self.filename_mode).value;
        }
        let filenames_only = self.filename_mode && !self.selected_only && !stored.enabled;
        let mut effective = overlay_policy(stored, self.selected_only, self.filename_mode);
        // Match the source parser on the request path in filename-only mode.
        if filenames_only {
            effective.enabled = false;
        }
        effective
    }

    pub(crate) fn startup_override_note(&self) -> Option<String> {
        startup_override_note(
            &self.stored_policy(),
            self.selected_only,
            self.filename_mode,
        )
    }

    pub(crate) fn policy_fingerprint(&self) -> String {
        self.policy
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .digest
            .clone()
    }

    pub(crate) fn replay_policy_digest(&self) -> String {
        // Equal rule text in an unrelated workspace/profile does not establish
        // coverage of that project's originals. Forks within this scope retain
        // the same binding without tying it to a particular session UUID.
        let mut hash = Sha256::new();
        let policy = if self.filename_mode {
            format!("{}:filenames.v1", self.policy_fingerprint())
        } else {
            self.policy_fingerprint()
        };
        for part in [
            self.scope.as_os_str().as_encoded_bytes(),
            self.workspace.as_os_str().as_encoded_bytes(),
            policy.as_bytes(),
        ] {
            hash.update((part.len() as u64).to_le_bytes());
            hash.update(part);
        }
        format!("{:x}", hash.finalize())
    }

    pub(crate) fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Tests pin `~` to the fixture workspace. Production reads `HOME` once,
    /// when the filename map is opened, which is what `list_dir` expands.
    #[cfg(test)]
    pub(crate) fn set_filename_home_for_test(&self, home: PathBuf) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(names) = state.filenames.as_mut() {
            names.set_home_for_test(home);
        }
    }
    pub(crate) fn set_owner(
        &self,
        store: crate::session::SessionStore,
        lease: crate::session_access::SessionWriterLease,
    ) {
        *self.owner.lock().unwrap_or_else(|p| p.into_inner()) = Some(CommitOwner { store, lease });
    }
    pub(crate) fn clear_owner(&self) {
        *self.owner.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
    pub(crate) fn allow_ephemeral(&self) {
        self.ephemeral
            .store(true, std::sync::atomic::Ordering::Release);
    }
    pub(crate) fn commits_without_saved_owner(&self) -> bool {
        self.ephemeral.load(std::sync::atomic::Ordering::Acquire)
    }
    pub(crate) fn observed_sources(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .sources
            .keys()
            .cloned()
            .collect()
    }
    pub(crate) fn source_key(&self, path: &str) -> Result<String, String> {
        self.resolve_source(path).map(|path| self.relative(&path))
    }
    pub(crate) fn outbound_snapshot(&self) -> Option<OutboundSnapshot> {
        self.outbound
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub(crate) fn save_policy(&self, draft: Policy) -> Result<(), String> {
        let _coordination = policy_coordination()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let expected = self.stored_policy();
        let committed = policy_store::save(&self.policy_path, &expected, &draft)?;
        self.install_policy(&committed);
        Ok(())
    }

    /// Reload before choosing the enabled/disabled provider path, on a worker.
    /// A same-revision manual edit is still a different semantic snapshot.
    pub(crate) fn refresh_policy(&self) -> Result<bool, String> {
        Ok(false)
    }

    pub(crate) fn check_policy(&self, _revision: u64) -> Result<(), String> {
        Ok(())
    }

    // Caller holds policy_coordination so request-boundary refreshes cannot
    // observe a half-installed policy with old classification/cache state.
    fn install_policy(&self, policy: &Policy) -> bool {
        let changed = self.install_policy_snapshot(policy);
        let targets = {
            let mut contexts = contexts().lock().unwrap_or_else(|p| p.into_inner());
            contexts.retain(|_, context| context.strong_count() > 0);
            contexts
                .values()
                .filter_map(Weak::upgrade)
                .filter(|context| context.policy_path == self.policy_path)
                .collect::<Vec<_>>()
        };
        for context in targets {
            if !std::ptr::eq(context.as_ref(), self) {
                context.install_policy_snapshot(policy);
            }
        }
        changed
    }

    fn install_policy_snapshot(&self, policy: &Policy) -> bool {
        {
            let mut current = self.policy.write().unwrap_or_else(|p| p.into_inner());
            if current.stored == *policy {
                return false;
            }
            let updated = if self.task_filter.is_some() {
                CurrentPolicy::for_task(policy.clone())
            } else {
                CurrentPolicy::new(policy.clone(), self.selected_only)
            };
            *current = filename_policy(updated, self.filename_mode);
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.active = if self.selected_only || policy.default_action == Action::Allow {
            BTreeMap::new()
        } else {
            state
                .registry
                .entries()
                .iter()
                .filter(|entry| {
                    !entry.kind.eq_ignore_ascii_case("source_receipt")
                        && !entry.kind.eq_ignore_ascii_case("filename_registry")
                        && !entry.kind.eq_ignore_ascii_case("native_digest")
                        && entry.kind != "TASK_FILTER"
                })
                .map(|entry| (entry.value.clone(), entry.token.clone()))
                .collect()
        };
        state.allowed.clear();
        state.safe_template_hashes.clear();
        state.matcher_dirty = true;
        state.cache.clear();
        state.views.clear();
        // Keep bounded tombstones until their exact pending invocation is
        // consumed. Dropping them would let disabling privacy erase the proof
        // that an already-prepared write still requires a source-bound check.
        for edit in state.prepared_edits.values_mut() {
            edit.source_digest = "invalidated-policy".into();
            edit.projected = Arc::from("");
            edit.replacements = Arc::from([]);
        }
        state.classified_revision = None;
        state.scanned_authored.clear();
        self.private_revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        true
    }

    pub(crate) fn preview_source(
        &self,
        path: &str,
        policy: &Policy,
    ) -> Result<(String, projection::Projection), String> {
        let resolved = self.resolve_source(path)?;
        let metadata =
            std::fs::metadata(&resolved).map_err(|e| format!("Source unavailable: {e}"))?;
        if metadata.len() > MAX_SOURCE_BYTES as u64 {
            return Err("Source exceeds the 8 MiB privacy parsing bound".into());
        }
        let original =
            std::fs::read_to_string(&resolved).map_err(|_| "Source must be readable UTF-8 text")?;
        let path = self.relative(&resolved);
        // Draft previews must never change the active request policy, private
        // dictionary, edit bindings, or committed alias allocation.
        let mut registry = self
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .registry
            .clone();
        let result = projection::project(&path, &original, policy, &mut |kind, value| {
            registry.alias(kind.label(), value)
        })?;
        Ok((original, result))
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.workspace)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn resolve_source(&self, path: &str) -> Result<PathBuf, String> {
        let path = Path::new(path);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace.join(path)
        };
        let resolved = std::fs::canonicalize(resolved).map_err(|_| "Source path is unavailable")?;
        if !resolved.starts_with(&self.workspace) && !self.filename_path_allowed(&resolved) {
            return Err("Select a source inside a currently granted folder".into());
        }
        Ok(resolved)
    }

    pub(crate) fn project_source(
        &self,
        path: &str,
        source: &str,
    ) -> Result<Arc<projection::Projection>, String> {
        self.project_source_shared(path, source, &self.policy(), None, true)
    }

    /// Classify restored web edits before their native preview is published.
    /// Reading the same committed source later must not retrofit its URLs into
    /// an already-sent preview. This does not issue a source-read/edit view.
    pub(crate) fn classify_restored_edit(&self, path: &Path, source: &str) -> Result<(), String> {
        if self.filename_mode && self.selected_only {
            let relative = self.relative(path);
            if matches!(
                projection::source_format(&relative),
                "html" | "css" | "javascript" | "typescript" | "tsx"
            ) {
                if !self.filename_path_allowed(path) {
                    return Err("Restored source needs a current folder grant".into());
                }
                self.project_source_shared(&relative, source, &self.policy(), None, false)?;
            }
        }
        Ok(())
    }

    fn project_source_shared(
        &self,
        path: &str,
        source: &str,
        policy: &Policy,
        policy_binding: Option<&str>,
        publish_view: bool,
    ) -> Result<Arc<projection::Projection>, String> {
        let _ = (self, path, policy, policy_binding, publish_view);
        if source.len() > MAX_SOURCE_BYTES {
            return Err("Source exceeds the 8 MiB privacy parsing bound".into());
        }
        Ok(Arc::new(projection::Projection {
            text: source.to_owned(),
            changes: Vec::new(),
            nodes: Vec::new(),
            digest: digest(source.as_bytes()),
        }))
    }

    #[cfg(test)]
    pub(crate) fn record_read(
        &self,
        path: &str,
        source: &str,
        original: &str,
        start: usize,
        end: usize,
        hashes: bool,
    ) -> Result<String, String> {
        self.record_read_inner(path, source, original, (start, end), hashes, None)
    }

    pub(crate) fn record_read_capped(
        &self,
        path: &str,
        source: &str,
        original: &str,
        range: (usize, usize),
        hashes: bool,
        footer: read_projection::Footer,
    ) -> Result<String, String> {
        self.record_read_inner(path, source, original, range, hashes, Some(footer))
    }

    fn record_read_inner(
        &self,
        path: &str,
        source: &str,
        original: &str,
        range: (usize, usize),
        hashes: bool,
        read_footer: Option<read_projection::Footer>,
    ) -> Result<String, String> {
        let (start, end) = range;
        let path = self.relative(Path::new(path));
        let source_digest = self.project_source(&path, source)?.digest.clone();
        let id = self
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .reads
            .values()
            .find(|receipt| {
                receipt.path == path
                    && receipt.source_digest == source_digest
                    && receipt.original == original
                    && receipt.start == start
                    && receipt.end == end
                    && receipt.hashes == hashes
                    && receipt.read_footer == read_footer
                    && receipt.attachment.is_none()
                    && receipt.id.is_some()
            })
            .and_then(|receipt| receipt.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let receipt = ReadReceipt {
            id: Some(id.clone()),
            path,
            source: source.into(),
            source_digest,
            original: original.into(),
            start,
            end,
            hashes,
            read_footer,
            attachment: None,
            native_control: None,
        };
        let value = serde_json::to_string(&receipt)
            .map_err(|_| "Private source receipt cannot be encoded")?;
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.registry.alias("source_receipt", &value)?;
        state.pending_source_links.retain(|link| link.id != id);
        state.pending_source_links.push(SourceLink {
            id: id.clone(),
            path: receipt.path.clone(),
            digest: receipt.source_digest.clone(),
        });
        state.reads.insert(id.clone(), receipt);
        Ok(id)
    }

    pub(crate) fn project_retrieved(
        &self,
        text: &str,
        source: Option<&str>,
    ) -> Result<String, String> {
        let _ = (self, source);
        Ok(text.to_owned())
    }

    pub(crate) fn record_attachment(
        &self,
        attachment: &crate::attachments::AttachmentSource,
    ) -> Result<(), String> {
        if !self.enabled() {
            return Ok(());
        }
        let path = self.relative(Path::new(&attachment.path));
        self.project_source(&path, &attachment.source)?;
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut changed = false;
        if !self.filename_mode {
            for spelling in [&attachment.path, &path] {
                if !spelling.is_empty() {
                    let alias = state.registry.alias("path", spelling)?;
                    changed |= state
                        .active
                        .insert(spelling.clone(), alias.clone())
                        .as_ref()
                        != Some(&alias);
                }
            }
        }
        state.matcher_dirty = true;
        if changed {
            self.private_revision
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        let receipt = ReadReceipt {
            id: (!attachment.receipt_id.is_empty()).then(|| attachment.receipt_id.clone()),
            path,
            source: attachment.source.clone(),
            source_digest: digest(attachment.source.as_bytes()),
            original: attachment.original.clone(),
            start: attachment.start_line,
            end: attachment.end_line,
            hashes: false,
            read_footer: None,
            attachment: Some(attachment.clone()),
            native_control: None,
        };
        let value =
            serde_json::to_string(&receipt).map_err(|_| "Attachment origin cannot be encoded")?;
        state.registry.alias("source_receipt", &value)?;
        let key = receipt
            .id
            .clone()
            .unwrap_or_else(|| format!("legacy:{}", digest(attachment.original.as_bytes())));
        state.reads.insert(key, receipt);
        Ok(())
    }

    pub(crate) fn mask_known(&self, text: &str) -> Result<String, String> {
        Ok(text.to_owned())
    }

    /// None is an unchanged, fully checked string. Shared immutable replacements
    /// let validation inspect an existing proof without allocating another full
    /// history string merely to compare it with its input.
    fn mask_known_shared(&self, text: &str) -> Result<Option<Arc<str>>, String> {
        if self.exact_read_projection(text)?.is_some() {
            return Ok(None);
        }
        let authorities = self
            .task_filter
            .as_ref()
            .map(|filter| filter.issued_url_authorities(text))
            .unwrap_or_default();
        if !authorities.is_empty() {
            // Final URL masking runs after native-read projection. Preserve
            // only its issued authorities, never the read or complete URL:
            // each surrounding/path/query segment still receives all checks.
            // Splitting also prevents filename discovery from interpreting
            // the whole synthetic URL as a new private pathname.
            let mut output = String::with_capacity(text.len());
            let mut offset = 0;
            for span in authorities {
                let gap = &text[offset..span.start];
                let masked = self.mask_known_unprotected_shared(gap)?;
                output.push_str(masked.as_deref().unwrap_or(gap));
                output.push_str(&text[span.clone()]);
                offset = span.end;
            }
            let gap = &text[offset..];
            let masked = self.mask_known_unprotected_shared(gap)?;
            output.push_str(masked.as_deref().unwrap_or(gap));
            return Ok((output != text).then(|| Arc::from(output)));
        }
        self.mask_known_unprotected_shared(text)
    }

    fn mask_known_unprotected_shared(&self, text: &str) -> Result<Option<Arc<str>>, String> {
        let content = self.mask_content_known_shared(text)?;
        let input = content.as_deref().unwrap_or(text);
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(ticket) = state
            .safe_template_hashes
            .get(&(self.policy_fingerprint(), digest(input.as_bytes())))
            .filter(|ticket| ticket.valid())
        {
            state.registry.tokens_in(input)?;
            ticket.touch();
            return Ok(content);
        }
        let protected = state.registry.tokens_in(input)?;
        if let Some(names) = &mut state.filenames {
            let projected = names.project_text_with_protected(input, &protected)?;
            return Ok((projected != text).then(|| Arc::from(projected)));
        }
        Ok(content)
    }
    fn mask_content_known_shared(&self, text: &str) -> Result<Option<Arc<str>>, String> {
        let current = self.policy.read().unwrap_or_else(|p| p.into_inner());
        let policy_fingerprint = current.digest.clone();
        let policy_revision = current.value.revision;
        drop(current);
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let private_revision = self
            .private_revision
            .load(std::sync::atomic::Ordering::Acquire);
        let generation = (private_revision, policy_revision);
        if state.mask_cache_generation != Some(generation) {
            state.mask_cache.clear();
            state.exact_masks.clear();
            state.mask_cache_generation = Some(generation);
        }
        // Full equality is the authority: no pointer, partial hash, or sampled
        // content can grant a match. Large unchanged blocks avoid rehashing and
        // copying tens of MiB at each prepare/final-validation boundary.
        let large = text.len() >= 8192;
        if large {
            let edges = text_edges(text);
            for cached in &state.exact_masks {
                if !cached.ticket.valid() {
                    continue;
                }
                if cached.input_edges == edges && cached.input.as_ref() == text {
                    cached.ticket.touch();
                    return Ok(cached.replacement.clone());
                }
                if cached.replacement_edges == Some(edges)
                    && cached.replacement.as_deref() == Some(text)
                {
                    cached.ticket.touch();
                    return Ok(None);
                }
            }
        }
        let text_digest = digest(text.as_bytes());
        let template_key = (policy_fingerprint, text_digest.clone());
        if let Some(ticket) = state.safe_template_hashes.get(&template_key) {
            if ticket.valid() {
                state.registry.tokens_in(text)?;
                ticket.touch();
                return Ok(None);
            }
        }
        let key = (private_revision, policy_revision, text_digest);
        if !large {
            if let Some(cached) = state.mask_cache.get(&key) {
                if cached.ticket.valid() {
                    cached.ticket.touch();
                    return Ok(cached.replacement.clone());
                }
            }
            state.mask_cache.remove(&key);
        }
        // Validate all reserved references before creating an exact proof.
        let protected = state.registry.tokens_in(text)?;
        rebuild_matcher(&mut state)?;
        let mut output: Option<String> = None;
        let mut offset = 0;
        let mut protected_index = 0;
        if let Some(matcher) = &state.matcher {
            for found in matcher.find_iter(text) {
                let start = found.start();
                let end = found.end();
                while protected
                    .get(protected_index)
                    .is_some_and(|range| range.end <= start)
                {
                    protected_index += 1;
                }
                if protected
                    .get(protected_index)
                    .is_some_and(|range| range.start < end && start < range.end)
                {
                    continue;
                }
                let (original, replacement) = &state.matcher_values[found.pattern().as_usize()];
                if is_word(original) && ((!boundary(text, start)) || (!boundary(text, end))) {
                    continue;
                }
                let output = output.get_or_insert_with(|| String::with_capacity(text.len()));
                output.push_str(&text[offset..start]);
                output.push_str(replacement);
                offset = end;
            }
        }
        if let Some(output) = &mut output {
            output.push_str(&text[offset..]);
        }
        let retained = output.filter(|output| output != text).map(Arc::<str>::from);
        if large {
            state.exact_masks.retain(|entry| entry.ticket.valid());
            let bytes = text
                .len()
                .saturating_add(retained.as_ref().map_or(0, |text| text.len()))
                .saturating_add(256);
            if let Some(ticket) = crate::cache_memory::reserve("privacy_masks", bytes) {
                let paths_scanned = state.scanned_authored.contains(&key.2);
                state.exact_masks.push(ExactMask {
                    input: Arc::from(text),
                    replacement: retained.clone(),
                    input_edges: text_edges(text),
                    replacement_edges: retained.as_deref().map(text_edges),
                    paths_scanned,
                    ticket,
                });
            }
        } else {
            let bytes = retained
                .as_ref()
                .map_or(0, |text| text.len())
                .saturating_add(192);
            if state.mask_cache.len().is_multiple_of(64) {
                state.mask_cache.retain(|_, entry| entry.ticket.valid());
            }
            if let Some(ticket) = crate::cache_memory::reserve("privacy_masks", bytes) {
                state.mask_cache.insert(
                    key,
                    CachedMask {
                        replacement: retained.clone(),
                        ticket,
                    },
                );
            }
        }
        Ok(retained)
    }

    pub(crate) fn skill_selector(&self, selector: &str) -> String {
        let restored = self
            .restore_text(selector)
            .unwrap_or_else(|_| selector.to_string());
        crate::skills::canonical_skill_selector(&restored)
    }

    pub(crate) fn restore_text(&self, text: &str) -> Result<String, String> {
        let task_restored = self
            .task_filter
            .as_ref()
            .map(|filter| filter.restore_text(text));
        self.restore_with_spans(task_restored.as_deref().unwrap_or(text))
            .map(|(text, _)| text)
    }

    pub(crate) fn restore_with_spans(
        &self,
        text: &str,
    ) -> Result<(String, Vec<crate::retrieved_content::RetrievedSpan>), String> {
        let _ = self;
        Ok((text.to_owned(), Vec::new()))
    }

    pub(crate) fn restore_arguments(
        &self,
        value: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        match value {
            serde_json::Value::String(text) => {
                Ok(serde_json::Value::String(self.restore_text(text)?))
            }
            serde_json::Value::Array(items) => {
                items.iter().map(|v| self.restore_arguments(v)).collect()
            }
            serde_json::Value::Object(items) => {
                let mut result = serde_json::Map::new();
                for (key, value) in items {
                    result.insert(key.clone(), self.restore_arguments(value)?);
                }
                Ok(serde_json::Value::Object(result))
            }
            _ => Ok(value.clone()),
        }
    }

    /// Graph persistence calls this only on its blocking worker. Keep its
    /// binding check and write ordered with private receipt publication.
    pub(crate) fn with_session_save<T>(&self, operation: impl FnOnce() -> T) -> T {
        let _commit = self.commit_gate.lock().unwrap_or_else(|p| p.into_inner());
        operation()
    }

    pub(crate) fn commit_registry(&self) -> Result<(), String> {
        let _commit = self.commit_gate.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(current) = for_session(self.session_id).filter(|current| {
            !std::ptr::eq(current.as_ref(), self) && current.task_generation > self.task_generation
        }) {
            current.merge_task_history(self)?;
            return current.commit_registry_locked();
        }
        self.commit_registry_locked()
    }

    fn merge_task_history(&self, older: &Self) -> Result<(), String> {
        if self.session_id != older.session_id
            || self.scope != older.scope
            || self.workspace != older.workspace
        {
            return Err("Private task history belongs to a different session".into());
        }
        let (registry, filenames, reads, sources) = {
            let state = older.state.lock().unwrap_or_else(|p| p.into_inner());
            (
                state.registry.clone(),
                state.filenames.clone(),
                state.reads.clone(),
                state.sources.clone(),
            )
        };
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.registry.merge_task_history(&registry)?;
        if let (Some(current), Some(older)) = (&mut state.filenames, filenames) {
            current.merge_task_history(&older)?;
        }
        for (id, receipt) in reads {
            state.reads.entry(id).or_insert(receipt);
        }
        for (path, hash) in sources {
            state.sources.entry(path).or_insert(hash);
        }
        state.classified_revision = None;
        state.cache.clear();
        state.mask_cache.clear();
        state.exact_masks.clear();
        state.safe_template_hashes.clear();
        Ok(())
    }

    fn commit_registry_locked(&self) -> Result<(), String> {
        let Some(owner) = self.owner.lock().unwrap_or_else(|p| p.into_inner()).clone() else {
            return if self.ephemeral.load(std::sync::atomic::Ordering::Acquire) {
                Ok(())
            } else {
                Err("Saved-session ownership is required before sending private aliases; retry when the session is ready".into())
            };
        };
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(names) = &state.filenames {
            let snapshot = serde_json::to_string(&names.snapshot())
                .map_err(|_| "Filename mappings cannot be encoded")?;
            state.registry.alias("filename_registry", &snapshot)?;
        }
        if let Some(record) = self
            .task_filter
            .as_ref()
            .and_then(|filter| filter.task_record())
        {
            state.registry.alias(
                "TASK_FILTER",
                &serde_json::to_string(&record).map_err(|_| "Task filter cannot be encoded")?,
            )?;
        }
        let mut reference = state.registry.stage(&self.scope)?;
        reference.optional_policy = self.stored_policy() == Policy::default();
        reference.task_context = self
            .task_context
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let mut session = owner
            .store
            .load_owned(&self.session_id.to_string(), &owner.lease)
            .map_err(|e| format!("Privacy save blocked: {e}"))?;
        if state.reference.as_ref() == Some(&reference)
            && session.privacy.as_ref() == Some(&reference)
        {
            return Ok(());
        }
        let before = session.storage_binding.clone();
        session.privacy = Some(reference.clone());
        // Drop the registry mutex before save_owned's binding synchronization.
        drop(state);
        let save = owner.store.save_owned(&mut session, &owner.lease);
        let committed = match save {
            Ok(_) => session,
            Err(error) => {
                let readback = owner
                    .store
                    .load_owned(&self.session_id.to_string(), &owner.lease)
                    .map_err(|_| format!("Privacy save needs reconciliation: {error}"))?;
                if readback.privacy.as_ref() != Some(&reference) {
                    return Err(format!(
                        "Privacy save blocked; retry the same request: {error}"
                    ));
                }
                readback
            }
        };
        #[cfg(test)]
        BEFORE_BINDING_PUBLISH.with(|hook| {
            if let Some(pause) = hook.borrow_mut().take() {
                pause();
            }
        });
        state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.reference = Some(reference.clone());
        state.advances.push(BindingAdvance {
            before,
            after: committed.storage_binding,
            reference,
        });
        Ok(())
    }

    pub(crate) fn prepare_request(
        &self,
        request: &mut crate::request::PreparedModelRequest,
    ) -> Result<u64, String> {
        self.refresh_policy()?;
        if !self.enabled() {
            return Err(
                "Project privacy changed during request preparation; prepare it again".into(),
            );
        }
        if request.imported_history.is_some() {
            return Err("Imported native history cannot satisfy active project source privacy. Use source-bound conversation history or an explicitly selected term/identifier-only profile for archived data; no request was sent.".into());
        }
        let replay_policy = self.replay_policy_digest();
        for item in request
            .messages
            .iter()
            .flat_map(|message| &message.response_items)
        {
            if item.opaque_source().is_some_and(|source| {
                request
                    .runtime_source
                    .as_ref()
                    .is_none_or(|route| source.same_transport(route))
                    && source.privacy_policy_digest.as_deref() != Some(replay_policy.as_str())
            }) {
                return Err("Opaque provider history has no matching project privacy receipt; use /filter continue for a fresh privacy-safe continuation".into());
            }
        }
        // Authenticate provider-issued plaintext against canonical history before
        // projecting either copy. Opaque state and previously sent prefixes keep
        // their existing immutable replay checks at the transport boundary.
        let replay_source = request.runtime_source.clone();
        if let Some(source) = &replay_source {
            for message in &request.messages {
                crate::agent::provider::response_output_items_for_replay(message, source)
                    .map_err(|error| error.to_string())?;
            }
        }
        self.discover_filename_grant_requests(request)?;
        self.refresh_filenames()?;
        let structured_output = request.structured_output().cloned();
        if let Some(contract) = &structured_output {
            let reports = request
                .tools
                .iter()
                .filter(|tool| {
                    tool.function.name == crate::headless_contract::STRUCTURED_OUTPUT_TOOL
                })
                .collect::<Vec<_>>();
            if reports.len() != 1 || !contract.matches_tool(reports[0]) {
                return Err("Host structured-output definition changed after admission".into());
            }
        }
        request.tools.retain(|tool| {
            self.check_filename_tool(&tool.function.name).is_ok()
                || structured_output
                    .as_ref()
                    .is_some_and(|contract| contract.matches_tool(tool))
        });
        let mut directory_evidence = std::collections::HashSet::new();
        if self.filename_mode {
            // Only the provider copy receives contextual paths. Local listings
            // retain their ordinary basename presentation.
            let duplicates = (0..request.messages.len())
                .filter_map(|index| {
                    crate::context_observation::duplicate_listing_basis(&request.messages, index)
                        .map(|basis| (index, basis))
                })
                .collect::<HashMap<_, _>>();
            let mut safe_markers = Vec::new();
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let names = state
                .filenames
                .as_mut()
                .ok_or("Filename mappings are unavailable")?;
            let failed_calls = request
                .messages
                .iter()
                .filter(|message| message.content.tool_result_success() == Some(false))
                .filter_map(|message| message.tool_call_id.as_deref())
                .collect::<std::collections::HashSet<_>>();
            let mut listing_roots = HashMap::new();
            for call in request
                .messages
                .iter()
                .flat_map(|message| message.tool_calls.iter().flatten())
            {
                if call.function.name != "list_dir" || failed_calls.contains(call.id.as_str()) {
                    continue;
                }
                let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                    .map_err(|_| "Invalid listing arguments")?;
                let path = args["path"].as_str().unwrap_or(".");
                let root = names.listing_root(path, &self.workspace)?;
                if !names.authorized_path(&root) {
                    return Err("Listing context needs a current folder grant".into());
                }
                listing_roots.insert(call.id.clone(), (root, path.to_owned()));
            }
            for (index, message) in request.messages.iter_mut().enumerate() {
                if message.content.tool_result_success() == Some(false) {
                    continue;
                }
                let Some((root, requested)) = message
                    .tool_call_id
                    .as_ref()
                    .and_then(|id| listing_roots.get(id))
                else {
                    continue;
                };
                if message.content.is_context_marker() {
                    if let Some(basis) = duplicates.get(&index) {
                        let canonical = names
                            .project_path(root.to_str().ok_or("Unsupported listing directory")?)?;
                        let requested = if names.is_known_path_alias(requested) {
                            requested
                        } else {
                            &canonical
                        };
                        let original = message
                            .content
                            .as_str()
                            .ok_or("Unsupported directory result encoding")?;
                        let clarified = format!("[CA directory result: list_dir succeeded for requested reference {requested}; it resolves to the same directory as canonical reference {canonical}. Full listing is retained in call {basis}.]\n{original}");
                        names.restore_text(&clarified)?;
                        safe_markers.push(clarified.clone());
                        directory_evidence.insert(index);
                        message.content =
                            crate::image_content::ModelContent::context_marker(clarified)
                                .with_tool_result_status(true);
                    }
                    continue;
                }
                let Some((text, authored_suffix)) = message.content.split_authored_suffix() else {
                    return Err("Unsupported directory result encoding".into());
                };
                if text.starts_with("Error")
                    || text.starts_with("[tool failed]\n")
                    || text == "[listing withheld]"
                {
                    continue;
                }
                let mut rows = Vec::new();
                let mut withheld = 0usize;
                for row in text.lines() {
                    if row == "[Directory listing limited by entry, output or deadline budget]"
                        || row.ends_with("listing rows withheld]")
                    {
                        continue;
                    }
                    if names.is_known_path_alias(row) {
                        rows.push(row.into());
                        continue;
                    }
                    let child = root.join(row.strip_suffix('/').unwrap_or(row));
                    match child.to_str().map(|text| names.project_path(text)) {
                        Some(Ok(alias)) => rows.push(alias),
                        _ => withheld += 1,
                    }
                }
                if withheld > 0 {
                    rows.push(format!("[{withheld} listing rows withheld]"));
                }
                message.content = crate::image_content::ModelContent::text(format!(
                    "{}{authored_suffix}",
                    rows.join("\n")
                ));
            }
            for marker in safe_markers {
                state.registry.tokens_in(&marker)?;
                state
                    .safe_template_hashes
                    .retain(|_, ticket| ticket.valid());
                let key = (self.policy_fingerprint(), digest(marker.as_bytes()));
                if !state.safe_template_hashes.contains_key(&key) {
                    if state.safe_template_hashes.len() >= 4096 {
                        return Err("Directory evidence preview budget is full".into());
                    }
                    let ticket = crate::cache_memory::reserve("privacy_directory_evidence", 256)
                        .ok_or("Directory evidence preview budget is unavailable")?;
                    state.safe_template_hashes.insert(key, ticket);
                }
            }
        }

        self.refresh_classification()?;
        self.discover_selected_sources()?;
        if !self.filename_mode {
            self.discover_path_references(request)?;
            self.bind_task_paths(request)?;
        }
        let revision = self.policy().revision;
        // First discover all retrieved values; only then mask authored text so a
        // newly read identity is also hidden in earlier messages in this request.
        for (index, message) in request.messages.iter_mut().enumerate() {
            let authored_text = directory_evidence.contains(&index)
                || crate::context_pruning::fixed_evidence_instruction(
                    &message.role,
                    message.content.as_str(),
                )
                || message.role != "tool"
                    && message.content.as_str().is_some()
                    && message
                        .content
                        .text_retrieved_spans()
                        .is_some_and(|spans| spans.is_empty());
            if !authored_text {
                message.content = message
                    .content
                    .project_privacy(self, message.role == "tool")?;
            }
        }
        for message in &mut request.messages {
            if !crate::context_pruning::fixed_evidence_instruction(
                &message.role,
                message.content.as_str(),
            ) {
                if message.content.has_host_instructions() {
                    message.content = message.content.mask_privacy(self)?;
                } else if let Some(text) = message.content.as_text_mut() {
                    if let Some(replacement) = self.mask_known_shared(text)? {
                        *text = replacement.as_ref().to_owned();
                    }
                } else {
                    message.content = message.content.mask_privacy(self)?;
                }
            }
            for call in message.tool_calls.as_mut().into_iter().flatten() {
                check_tool_name_with_structured_output(
                    self,
                    &call.function.name,
                    structured_output.as_ref(),
                )?;
                let mut value: serde_json::Value =
                    serde_json::from_str(&call.function.arguments)
                        .map_err(|_| "Privacy tool arguments are not valid JSON")?;
                if host_report_name(structured_output.as_ref(), &call.function.name) {
                    arguments::project_contract_report(
                        self,
                        structured_output.as_ref().unwrap(),
                        &mut value,
                    )?;
                } else {
                    arguments::project(self, &call.function.name, &mut value)?;
                }
                call.function.arguments = value.to_string();
            }
            // A newly discovered filename can change plain argument data and
            // readable reasoning summaries in this outgoing copy. Keep call
            // identity and opaque payloads exact. Canonical session history is
            // outside this request-only copy.
            for response in &mut message.response_items {
                let crate::agent::stream::ProviderResponseItem::ResponsesOutput {
                    source,
                    item,
                    ..
                } = response
                else {
                    continue;
                };
                if replay_source.as_ref() != Some(source) {
                    continue;
                }
                if item["type"] == "function_call" {
                    let call = message
                        .tool_calls
                        .as_ref()
                        .and_then(|calls| {
                            calls
                                .iter()
                                .find(|call| Some(call.id.as_str()) == item["call_id"].as_str())
                        })
                        .ok_or("Authenticated response call is unavailable")?;
                    item["arguments"] = call.function.arguments.clone().into();
                }
                project_responses_reasoning_plaintext(self, item)?;
                project_responses_message_plaintext(self, item)?;
            }
        }
        for tool in &mut request.tools {
            let host_report = host_report_name(structured_output.as_ref(), &tool.function.name);
            check_tool_name_with_structured_output(
                self,
                &tool.function.name,
                structured_output.as_ref(),
            )?;
            if !host_report
                && !host_builtin_description(self, &tool.function.name, &tool.function.description)
            {
                tool.function.description = self.mask_known(&tool.function.description)?;
            }
            if self.filename_mode && !host_report {
                tool.function.description.push_str(FILENAME_TOOL_GUIDANCE);
            }

            // Exact host schemas are protocol vocabulary. Check their admitted
            // shape before projection could rewrite a title or description.
            if host_report
                && structured_output
                    .as_ref()
                    .unwrap()
                    .matches_native_schema(&tool.function.parameters, false)?
            {
                continue;
            }
            project_schema_text(self, &mut tool.function.parameters)?;
            if host_report {
                check_bound_structured_output_schema(
                    self,
                    structured_output.as_ref().unwrap(),
                    &tool.function.parameters,
                    false,
                )?;
                continue;
            }
            let custom = crate::agent::registry::ToolRegistry::builtins()
                .get(&tool.function.name)
                .is_none();
            check_schema_for_tool(
                self,
                &tool.function.parameters,
                (!custom).then_some(tool.function.name.as_str()),
            )?;
        }
        if self.filename_mode {
            let public = serde_json::to_string(&request.messages)
                .map_err(|_| "Public history cannot be inspected")?;
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let names = state
                .filenames
                .as_mut()
                .ok_or("Filename mappings are unavailable")?;
            names.declare_public_names(&public)?;
            let snapshot = serde_json::to_string(&names.snapshot())
                .map_err(|_| "Filename mappings cannot be encoded")?;
            state.registry.alias("filename_registry", &snapshot)?;
        }
        self.commit_registry()?;
        if self.policy().revision != revision {
            return Err("Privacy rules changed during preparation; retry this request".into());
        }
        Ok(revision)
    }

    fn discover_selected_sources(&self) -> Result<(), String> {
        let policy = self.policy();
        if policy.default_action != Action::Allow {
            return Ok(());
        }
        let mut paths = std::collections::BTreeSet::new();
        for rule in policy
            .rules
            .iter()
            .filter(|rule| rule.action != Action::Allow)
        {
            let wildcard = rule.file.find(['*', '?', '[', '{']);
            let Some(wildcard) = wildcard else {
                let path = self.workspace.join(&rule.file);
                if path.is_file() {
                    paths.insert(self.resolve_source(&rule.file)?);
                }
                continue;
            };
            let prefix = &rule.file[..wildcard];
            let directory = prefix
                .rsplit_once('/')
                .map_or("", |(directory, _)| directory);
            let directory = self.workspace.join(directory);
            if !directory.is_dir() {
                continue;
            }
            let directory = self.resolve_source(&directory.to_string_lossy())?;
            let matcher = globset::GlobBuilder::new(&rule.file)
                .literal_separator(true)
                .backslash_escape(false)
                .build()
                .map_err(|_| "Invalid selected-source pattern")?
                .compile_matcher();
            let mut walk = ignore::WalkBuilder::new(directory);
            walk.hidden(false)
                .ignore(false)
                .parents(false)
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false)
                .follow_links(false);
            for entry in walk.build() {
                let entry = entry.map_err(|_| "Selected privacy sources could not be inspected")?;
                if !entry.path().is_file() {
                    continue;
                }
                let path = self.relative(entry.path());
                if matcher.is_match(&path)
                    || (rule.file == "*" && matcher.is_match(entry.file_name()))
                {
                    paths.insert(self.resolve_source(&path)?);
                }
            }
        }
        for path in paths {
            let metadata =
                std::fs::metadata(&path).map_err(|_| "Selected privacy source is unavailable")?;
            if metadata.len() > MAX_SOURCE_BYTES as u64 {
                return Err("Selected source exceeds the 8 MiB privacy parsing bound".into());
            }
            let source = std::fs::read_to_string(&path)
                .map_err(|_| "Selected privacy source must be UTF-8")?;
            let relative = self.relative(&path);
            self.project_source_shared(&relative, &source, &policy, None, false)?;
            if !self.filename_mode && policy.action_for(&relative, "/@path") != Action::Allow {
                let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                for spelling in [&relative, &path.to_string_lossy().into_owned()] {
                    let token = state.registry.alias("path", spelling)?;
                    if state
                        .active
                        .insert(spelling.clone(), token.clone())
                        .as_ref()
                        != Some(&token)
                    {
                        state.matcher_dirty = true;
                        self.private_revision
                            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                    }
                }
            }
        }
        Ok(())
    }

    fn discover_path_references(
        &self,
        request: &crate::request::PreparedModelRequest,
    ) -> Result<(), String> {
        let mut candidates = std::collections::BTreeSet::new();
        let mut allowed_paths = std::collections::BTreeSet::new();
        if self.policy().action_for(".", "/@path") == Action::Allow {
            allowed_paths.insert(self.workspace.to_string_lossy().into_owned());
        } else {
            candidates.insert(self.workspace.to_string_lossy().into_owned());
        }
        for message in &request.messages {
            if message.role == "tool" {
                continue;
            }
            for block in message.content.blocks() {
                let crate::image_content::ModelContentBlock::Text(text) = block else {
                    continue;
                };
                let scanned_exactly = if text.len() >= 8192 {
                    let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                    let generation = (self.private_revision(), self.policy().revision);
                    let edges = text_edges(text);
                    state.mask_cache_generation == Some(generation)
                        && state.exact_masks.iter().any(|proof| {
                            if proof.paths_scanned
                                && proof.input_edges == edges
                                && proof.ticket.valid()
                                && proof.input.as_ref() == text.as_str()
                            {
                                proof.ticket.touch();
                                true
                            } else {
                                false
                            }
                        })
                } else {
                    false
                };
                if scanned_exactly {
                    continue;
                }
                let identity = digest(text.as_bytes());
                if !self
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .scanned_authored
                    .insert(identity)
                {
                    continue;
                }
                for word in text
                    .split(|ch: char| ch.is_whitespace() || "'\"`<>[]{}(),;".contains(ch))
                    .take(100_000)
                {
                    let word = word.trim_matches(':').trim_start_matches('@');
                    if word.is_empty()
                        || word.len() > 4096
                        || word.contains("://")
                        || word.contains('@')
                    {
                        continue;
                    }
                    if !(word.contains('/')
                        || word
                            .rsplit_once('.')
                            .is_some_and(|(_, suffix)| !suffix.is_empty() && suffix.len() <= 16)
                        || matches!(word, "Makefile" | "Dockerfile" | "CMakeLists.txt"))
                    {
                        continue;
                    }
                    let path = Path::new(word);
                    let candidate = if path.is_absolute() {
                        path.to_path_buf()
                    } else {
                        self.workspace.join(path)
                    };
                    if !candidate.starts_with(&self.workspace) {
                        continue;
                    }
                    if let Ok(resolved) = std::fs::canonicalize(&candidate) {
                        if resolved.starts_with(&self.workspace) {
                            let relative = self.relative(&resolved);
                            if self.policy().action_for(&relative, "/@path") != Action::Allow {
                                candidates.insert(word.to_string());
                                candidates.insert(relative);
                                candidates.insert(resolved.to_string_lossy().into_owned());
                            } else {
                                allowed_paths.insert(word.to_string());
                                allowed_paths.insert(relative);
                                allowed_paths.insert(resolved.to_string_lossy().into_owned());
                            }
                        }
                    }
                }
            }
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut changed = false;
        for path in allowed_paths {
            changed |= state.active.remove(&path).is_some();
            state.allowed.insert(path);
        }
        for path in candidates {
            if state.allowed.contains(&path) {
                continue;
            }
            let token = state.registry.alias("path", &path)?;
            changed |= state.active.insert(path, token.clone()).as_ref() != Some(&token);
        }
        if changed {
            state.matcher_dirty = true;
            self.private_revision
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        Ok(())
    }

    pub(crate) fn refresh_classification(&self) -> Result<(), String> {
        let policy = self.policy();
        let fingerprint =
            digest(&serde_json::to_vec(&policy).map_err(|_| "Privacy rules cannot be encoded")?);
        let reads = {
            let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if state.classified_revision == Some(policy.revision) {
                return Ok(());
            }
            let mut seen = std::collections::BTreeSet::new();
            state
                .reads
                .values()
                .filter(|receipt| receipt.native_control.is_none())
                .filter_map(|receipt| {
                    let key = (receipt.path.clone(), receipt.source_digest.clone());
                    seen.insert(key).then(|| {
                        (
                            receipt.path.clone(),
                            receipt.source.clone(),
                            receipt.source_digest.clone(),
                        )
                    })
                })
                .collect::<Vec<_>>()
        };
        let mut allowed = std::collections::BTreeSet::new();
        let mut private_paths = std::collections::BTreeSet::new();
        let mut projections = Vec::with_capacity(reads.len());
        for (path, source, source_digest) in reads {
            if digest(source.as_bytes()) != source_digest {
                return Err(
                    "Saved private source identity is invalid; read the source again".into(),
                );
            }
            // Every saved source participates, including formerly allowed
            // values with no alias yet and receipts absent from compacted chat
            // history. The shared projection cache makes this a warm no-op.
            let projected = self.project_source_shared(&path, &source, &policy, None, true)?;
            let selected = policy.for_path(&path);
            for change in &projected.changes {
                if change.action == Action::Allow {
                    allowed.insert(change.original.clone());
                }
            }
            if selected.action_for("/@path") == Action::Allow {
                allowed.insert(path.clone());
                allowed.insert(self.workspace.join(&path).to_string_lossy().into_owned());
            } else {
                private_paths.insert(path.clone());
                private_paths.insert(self.workspace.join(&path).to_string_lossy().into_owned());
            }
            projections.push(projected);
        }
        let _coordination = policy_coordination()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if self.policy_fingerprint() != fingerprint {
            return Err("Privacy policy changed while classifying saved sources; retry".into());
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut changed = state.allowed != allowed;
        let previous = state.active.len();
        state.active.retain(|value, _| !allowed.contains(value));
        changed |= previous != state.active.len();
        // Reapply hidden changes after computing the complete allow set, so
        // an earlier policy's temporary allow state cannot suppress a value
        // that has just become private.
        for projection in projections {
            for change in &projection.changes {
                if change.action != Action::Allow
                    && !change.original.is_empty()
                    && !allowed.contains(&change.original)
                {
                    changed |= state
                        .active
                        .insert(change.original.clone(), change.replacement.clone())
                        .as_ref()
                        != Some(&change.replacement);
                }
            }
        }
        for path in private_paths {
            if !self.filename_mode && !path.is_empty() && !allowed.contains(&path) {
                let alias = state.registry.alias("path", &path)?;
                changed |= state.active.insert(path, alias.clone()).as_ref() != Some(&alias);
            }
        }
        state.allowed = allowed;
        state.matcher_dirty |= changed;
        state.classified_revision = Some(policy.revision);
        if changed {
            self.private_revision
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        Ok(())
    }

    pub(crate) fn validate_wire(
        &self,
        body: &[u8],
        revision: u64,
        fingerprint: &str,
    ) -> Result<(), String> {
        self.validate_wire_with_structured_output(body, revision, fingerprint, None)
    }

    pub(crate) fn validate_wire_with_structured_output(
        &self,
        body: &[u8],
        revision: u64,
        fingerprint: &str,
        structured_output: Option<&crate::headless_contract::StructuredOutputContract>,
    ) -> Result<(), String> {
        self.validate_wire_with_receipts(body, revision, fingerprint, structured_output, None)
    }

    pub(crate) fn validate_wire_with_receipts(
        &self,
        body: &[u8],
        revision: u64,
        fingerprint: &str,
        structured_output: Option<&crate::headless_contract::StructuredOutputContract>,
        host: Option<&crate::host_instructions::WireReceipt>,
    ) -> Result<(), String> {
        self.check_policy(revision)?;
        if self.policy_fingerprint() != fingerprint {
            return Err("Privacy rules changed after request preparation; prepare it again".into());
        }
        self.refresh_classification()?;
        if self.policy_fingerprint() != fingerprint {
            return Err(
                "Privacy rules changed during source classification; prepare it again".into(),
            );
        }
        if body.len() > MAX_WIRE_BYTES {
            return Err("Privacy request exceeds the 128 MiB inspection bound".into());
        }
        if self.policy().revision != revision {
            return Err("Privacy rules changed before transmission; retry this request".into());
        }
        let value: serde_json::Value = serde_json::from_slice(body)
            .map_err(|_| "Privacy request is not valid protocol JSON")?;
        let value = match host {
            Some(host) => host.validation_copy(&value)?,
            None => value,
        };
        fn check<'a>(
            context: &Context,
            value: &'a serde_json::Value,
            path: &mut Vec<&'a str>,
            structured_output: Option<&crate::headless_contract::StructuredOutputContract>,
        ) -> Result<(), String> {
            match value {
                serde_json::Value::String(text) => {
                    if path.as_slice()
                        == [
                            "toolConfig",
                            "functionCallingConfig",
                            "allowedFunctionNames",
                            "*",
                        ]
                    {
                        return check_tool_name_with_structured_output(
                            context,
                            text,
                            structured_output,
                        );
                    }
                    if context.mask_known_shared(text)?.is_some() {
                        return Err(immutable_wire_error(path));
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        path.push("*");
                        check(context, item, path, structured_output)?;
                        path.pop();
                    }
                }
                serde_json::Value::Object(items) => {
                    let tool = wire_tool_location(path, items);
                    for (key, item) in items {
                        if fixed_evidence_wire(path, key, item, items) {
                            // Exact static first-party system instruction only.
                        } else if tool && key == "name" {
                            check_tool_name_with_structured_output(
                                context,
                                item.as_str().ok_or("Final tool name is not text")?,
                                structured_output,
                            )?;
                        } else if tool && key == "arguments" && item.is_string() {
                            let value: serde_json::Value =
                                serde_json::from_str(item.as_str().unwrap_or_default())
                                    .map_err(|_| "Final tool arguments are not valid JSON")?;
                            if host_report_name(
                                structured_output,
                                items
                                    .get("name")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or_default(),
                            ) {
                                arguments::validate_contract_report(
                                    context,
                                    structured_output.unwrap(),
                                    &value,
                                )?;
                            } else {
                                arguments::validate(
                                    context,
                                    items.get("name").and_then(serde_json::Value::as_str),
                                    &value,
                                )?;
                            }
                        } else if tool
                            && matches!(key.as_str(), "input" | "args")
                            && item.is_object()
                        {
                            if host_report_name(
                                structured_output,
                                items
                                    .get("name")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or_default(),
                            ) {
                                arguments::validate_contract_report(
                                    context,
                                    structured_output.unwrap(),
                                    item,
                                )?;
                            } else {
                                arguments::validate(
                                    context,
                                    items.get("name").and_then(serde_json::Value::as_str),
                                    item,
                                )?;
                            }
                        } else if (tool && matches!(key.as_str(), "parameters" | "input_schema"))
                            || (matches!(
                                path.as_slice(),
                                ["generationConfig"] | ["generation_config"]
                            ) && matches!(key.as_str(), "responseSchema" | "response_schema"))
                        {
                            if host_report_name(
                                structured_output,
                                items
                                    .get("name")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or_default(),
                            ) && wire_tool_definition_location(path)
                            {
                                check_bound_structured_output_schema(
                                    context,
                                    structured_output.unwrap(),
                                    item,
                                    matches!(
                                        path.as_slice(),
                                        ["tools", "*", "functionDeclarations", "*"]
                                    ),
                                )?;
                                continue;
                            }
                            let custom = items
                                .get("name")
                                .and_then(serde_json::Value::as_str)
                                .is_none_or(|name| {
                                    crate::agent::registry::ToolRegistry::builtins()
                                        .get(name)
                                        .is_none()
                                });
                            check_schema_for_tool(
                                context,
                                item,
                                (!custom).then(|| items["name"].as_str()).flatten(),
                            )?;
                        } else if tool
                            && key == "description"
                            && wire_tool_definition_location(path)
                            && host_report_name(
                                structured_output,
                                items
                                    .get("name")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or_default(),
                            )
                            && item.as_str()
                                == Some(
                                    structured_output
                                        .unwrap()
                                        .tool_definition()
                                        .function
                                        .description
                                        .as_str(),
                                )
                        {
                            // This exact description is published by the process-owned contract.
                        } else if tool
                            && key == "description"
                            && wire_tool_definition_location(path)
                            && items
                                .get("name")
                                .and_then(serde_json::Value::as_str)
                                .zip(item.as_str())
                                .is_some_and(|(name, text)| {
                                    host_builtin_description(context, name, text)
                                })
                        {
                            // Exact registry prose (and the fixed filename suffix)
                            // is host instruction, never source-discovered data.
                        } else if let Some(prefix) =
                            filename_guidance_prefix(context, path, key, items, item)
                        {
                            // Only this exact host suffix has protocol meaning.
                            // Caller-supplied prefix text retains the normal check.
                            if context.mask_known_shared(prefix)?.is_some() {
                                let mut location = path.clone();
                                location.push(key);
                                return Err(immutable_wire_error(&location));
                            }
                        } else if !wire_control_field(path, key, item, items) {
                            path.push(key);
                            check(context, item, path, structured_output)?;
                            path.pop();
                        }
                    }
                }
                _ => {}
            }
            Ok(())
        }
        check(self, &value, &mut Vec::new(), structured_output)?;
        // The provenance-aware traversal above inspects data through the same
        // filename/content projection as preparation. Protocol identifiers,
        // builtin schema controls and provider-encrypted reasoning stay exact.
        Ok(())
    }

    pub(crate) fn publish_wire(&self, original: &[u8], sent: &[u8]) -> Result<(), String> {
        if original.len() > MAX_WIRE_BYTES || sent.len() > MAX_WIRE_BYTES {
            return Err("Privacy preview exceeds the 128 MiB inspection bound".into());
        }
        let rendered = self
            .preview_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .render(original, sent)?;
        let sources = {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let mut links = std::mem::take(&mut state.pending_source_links);
            let text = String::from_utf8_lossy(original);
            for receipt in state.reads.values() {
                if links.len() >= 64 {
                    break;
                }
                let Some(id) = receipt.id.clone() else {
                    continue;
                };
                if links.iter().any(|link| link.id == id) {
                    continue;
                }
                if receipt.path.len() >= 3 && text.contains(receipt.path.as_str()) {
                    links.push(SourceLink {
                        id,
                        path: receipt.path.clone(),
                        digest: receipt.source_digest.clone(),
                    });
                }
            }
            links
        };
        let mut slot = self.outbound.write().unwrap_or_else(|p| p.into_inner());
        let generation = slot.as_ref().map_or(1, |s| s.generation.wrapping_add(1));
        *slot = Some(OutboundSnapshot {
            generation,
            original: rendered.original,
            sent: rendered.sent,
            changes: rendered.changes,
            latest: rendered.latest,
            pane: rendered.pane,
            opaque_blocks: rendered.opaque_blocks,
            sources,
        });
        Ok(())
    }

    pub(crate) fn translate_edit(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.check_filename_tool(name)?;
        if self.filename_mode && matches!(name, "select_files" | "move_files") {
            self.refresh_filenames()?;
            return tasks::filename_batch_arguments(self, name, args);
        }
        if name == "write_shell_stdin"
            && args["input"]
                .as_str()
                .is_some_and(|text| text.contains("__CA_"))
        {
            return Err("Alias-bearing stdin has no literal argument boundary; use a complete guarded command".into());
        }
        if !self.filename_mode && matches!(name, "run_privileged" | "switch_branch") {
            return Err("This opaque source-changing tool cannot enforce project privacy edit bindings; use mapped file tools or perform the operation locally".into());
        }
        if name == "run_command" {
            if self.filename_mode {
                self.refresh_filenames()?;
                if let Some(command) = args["command"].as_str() {
                    self.state
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .filenames
                        .as_mut()
                        .ok_or("Filename mappings are unavailable")?
                        .declare_public_names(command)?;
                }
            }
            let mut restored = args.clone();
            if let Some(command) = args["command"].as_str() {
                let command = shell::restore(command, |token| {
                    self.state
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .registry
                        .restore_exact(token)
                        .map(str::to_owned)
                })?;
                restored["command"] = serde_json::Value::String(if self.filename_mode {
                    shell::restore_filename_references(&command, |reference| {
                        self.resolve_filename_path(reference, true)
                            .map(|path| path.to_string_lossy().into_owned())
                    })?
                } else {
                    command
                });
            }
            if let Some(cwd) = args["cwd"].as_str() {
                restored["cwd"] = serde_json::Value::String(if self.filename_mode {
                    self.resolve_filename_path(cwd, false)?
                        .to_string_lossy()
                        .into_owned()
                } else {
                    self.restore_text(cwd)?
                });
            }
            return Ok(restored);
        }
        let filename_arguments = if self.filename_mode {
            self.refresh_filenames()?;
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let names = state
                .filenames
                .as_mut()
                .ok_or("Filename mappings are unavailable")?;
            let mut resolved = args.clone();
            if let Some(spec) = crate::agent::registry::ToolRegistry::builtins().get(name) {
                for field in &spec.resources {
                    if matches!(
                        field.kind,
                        crate::agent::registry::ResourceKind::Path
                            | crate::agent::registry::ResourceKind::WorkingDirectory
                    ) {
                        if let Some(path) =
                            args.get(field.field).and_then(serde_json::Value::as_str)
                        {
                            if name == "generate_image"
                                && field.field == "image_url"
                                && url::Url::parse(path)
                                    .is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
                            {
                                continue;
                            }
                            let real = if matches!(
                                name,
                                "write_file" | "create_directory" | "fetch" | "browser"
                            ) && self.selected_only
                            {
                                names.resolve_write_path(path)?
                            } else {
                                names.resolve_path(path)?
                            };
                            let target = if real.exists() {
                                std::fs::canonicalize(&real)
                                    .map_err(|_| "Filename alias is unavailable")?
                            } else {
                                let mut parent = real.as_path();
                                while !parent.exists() {
                                    parent =
                                        parent.parent().ok_or("New path parent is unavailable")?;
                                }
                                let canonical = std::fs::canonicalize(parent)
                                    .map_err(|_| "New path parent is unavailable")?;
                                canonical.join(
                                    real.strip_prefix(parent)
                                        .map_err(|_| "New path cannot be resolved")?,
                                )
                            };
                            if !names.authorized_path(&target) {
                                return Err("Resolved filename is outside the workspace".into());
                            }
                            resolved[field.field] = serde_json::Value::String(
                                real.to_str().ok_or("Unsupported filename encoding")?.into(),
                            );
                        }
                    }
                }
            }
            let edit_source = resolved
                .get("path")
                .and_then(serde_json::Value::as_str)
                .map(|path| self.relative(Path::new(path)));
            for field in [
                "content",
                "old_string",
                "new_string",
                "patch",
                "replacement",
            ] {
                if let Some(text) = args.get(field).and_then(serde_json::Value::as_str) {
                    if names.contains_aliases(text) {
                        if self.selected_only {
                            resolved[field] = serde_json::Value::String(
                                names.restore_source_text(text, edit_source.as_deref())?,
                            );
                            continue;
                        }
                        return Err("Filename aliases in edit content cannot preserve the original relative spelling; use an edit containing only public source text".into());
                    }
                    // Only typed path fields are reversible without a source-spelling receipt.
                    resolved[field] = serde_json::Value::String(text.to_owned());
                }
            }
            Some(resolved)
        } else {
            None
        };
        let args = filename_arguments.as_ref().unwrap_or(args);
        if self.filename_only()
            && matches!(
                name,
                "write_file" | "edit_file" | "apply_patch" | "hashline_edit" | "delete_file"
            )
        {
            return Ok(args.clone());
        }
        let mut local_args = args.clone();
        if self.filename_mode {
            tool_contract::restore_extra_paths(self, name, &mut local_args)?;
        }
        let args = &local_args;
        if matches!(name, "glob" | "search_files") {
            return Ok(args.clone());
        }
        let mut restored = tool_contract::restore_local_payload(self, args)?;
        if self.filename_mode {
            for field in ["url", "image_url"] {
                if let Some(value) = args[field].as_str().filter(|value| {
                    url::Url::parse(value).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
                }) {
                    let value = self
                        .state
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .filenames
                        .as_ref()
                        .ok_or("Filename mappings are unavailable")?
                        .restore_url_references(value)?;
                    restored[field] = serde_json::Value::String(self.restore_text(&value)?);
                }
            }
        }
        if name == "generate_image" {
            if let Some(prompt) = args["prompt"].as_str() {
                restored["prompt"] = serde_json::Value::String(self.mask_known(prompt)?);
            }
        }
        if !matches!(
            name,
            "write_file" | "edit_file" | "apply_patch" | "hashline_edit" | "delete_file"
        ) {
            return Ok(restored);
        }
        let path = restored
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or("Private edit has no source path")?;
        let resolved = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.workspace.join(path)
        };
        let resolved = if resolved.exists() {
            std::fs::canonicalize(&resolved)
                .map_err(|_| "Protected edit path cannot be resolved")?
        } else {
            let parent = resolved.parent().ok_or("Protected edit has no parent")?;
            std::fs::canonicalize(parent)
                .map_err(|_| "Protected edit parent cannot be resolved")?
                .join(
                    resolved
                        .file_name()
                        .ok_or("Protected edit has no filename")?,
                )
        };
        if !self.filename_path_allowed(&resolved) && !resolved.starts_with(&self.workspace) {
            return Err("Protected edit source must belong to a currently granted folder".into());
        }
        restored["path"] = serde_json::Value::String(resolved.to_string_lossy().into_owned());
        let relative = self.relative(&resolved);
        let existing = std::fs::read_to_string(&resolved);
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Ok(source) = &existing {
            let view = state.views.get(&relative).ok_or(
                "Read the protected file before editing it so ca can bind the projected view",
            )?;
            if view.policy_revision != self.policy().revision
                || view.policy_digest != self.policy_fingerprint()
                || view.source_digest != digest(source.as_bytes())
            {
                return Err(
                    "Protected source or privacy rules changed; read the file again before editing"
                        .into(),
                );
            }
            // Hash anchors bind the projected file. Translate to the real line
            // only while the exact source/view identity remains unchanged.
            if name == "hashline_edit" {
                for key in ["start_anchor", "end_anchor"] {
                    if let Some(anchor) = args.get(key).and_then(serde_json::Value::as_str) {
                        let projected_lines = view.projected.lines().collect::<Vec<_>>();
                        let matches = projected_lines
                            .iter()
                            .enumerate()
                            .filter(|(_, line)| crate::agent::tools::line_anchor(line) == anchor)
                            .map(|(index, _)| index)
                            .collect::<Vec<_>>();
                        if matches.len() != 1 {
                            return Err(
                                "Projected line anchor is stale or ambiguous; read the file again"
                                    .into(),
                            );
                        }
                        if let Some(line) = source.lines().nth(matches[0]) {
                            restored[key] =
                                serde_json::Value::String(crate::agent::tools::line_anchor(line));
                        }
                    }
                }
            }
            for key in [
                "content",
                "old_string",
                "new_string",
                "patch",
                "replacement",
            ] {
                if let Some(raw) = args.get(key).and_then(serde_json::Value::as_str) {
                    let mut value = raw.to_string();
                    // Exact, source-bound replacements are performed in one
                    // pass by longest token, never recursively on originals.
                    let replacements = &view.replacements;
                    value = replace_view_tokens(
                        &value,
                        replacements,
                        &view.projected,
                        &state.registry,
                    )?;
                    restored[key] = serde_json::Value::String(value);
                }
            }
        } else if name != "write_file" {
            return Err("Protected edit source is unavailable".into());
        }
        Ok(restored)
    }

    pub(crate) fn prepare_edit_ticket(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.refresh_policy()?;
        if self.filename_only() {
            return self.translate_edit(name, args);
        }
        let key = edit_key(name, args)?;
        if self
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .prepared_edits
            .contains_key(&key)
        {
            return Ok(args.clone());
        }
        let restored = self.translate_edit(name, args)?;
        if !matches!(
            name,
            "write_file" | "edit_file" | "apply_patch" | "hashline_edit" | "delete_file"
        ) {
            return Ok(restored);
        }
        let path = restored
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or("Protected edit has no path")?;
        let path = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.workspace.join(path)
        };
        let relative = self.relative(&path);
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let view = match state.views.get(&relative) {
            Some(view) => view.clone(),
            None if name == "write_file" && !path.exists() => EditView {
                source_digest: "missing".into(),
                policy_revision: self.policy().revision,
                policy_digest: self.policy_fingerprint(),
                projected: Arc::from(""),
                replacements: Arc::from([]),
            },
            None => return Err("Read the protected source before preparing its edit".into()),
        };
        if state.prepared_edits.len() >= 256 {
            return Err(
                "Too many pending private edits; finish or cancel an edit before preparing another"
                    .into(),
            );
        }
        let key = edit_key(name, &restored)?;
        state.prepared_edits.insert(key, view);
        Ok(restored)
    }

    pub(crate) fn consume_edit_ticket(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<(), String> {
        self.refresh_policy()?;
        let key = edit_key(name, args)?;
        let view = self
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .prepared_edits
            .remove(&key)
            .ok_or(
                "Protected edit lacks a current host preparation ticket; read and prepare it again",
            )?;
        let path = args
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or("Protected edit has no path")?;
        let path = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.workspace.join(path)
        };
        let source = match std::fs::read(&path) {
            Ok(source) => digest(&source),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "missing".into(),
            Err(_) => return Err("Protected edit source cannot be read".into()),
        };
        if view.policy_revision != self.policy().revision
            || view.policy_digest != self.policy_fingerprint()
            || source != view.source_digest
        {
            return Err("Protected source or privacy policy changed after preparation; read and prepare it again".into());
        }
        Ok(())
    }

    pub(crate) fn consume_edit_ticket_if_required(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<(), String> {
        self.refresh_policy()?;
        if self.filename_only() {
            return Ok(());
        }
        let key = edit_key(name, args)?;
        if !self.enabled()
            && !self
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .prepared_edits
                .contains_key(&key)
        {
            return Ok(());
        }
        self.consume_edit_ticket(name, args)
    }
}

fn host_report_name(
    contract: Option<&crate::headless_contract::StructuredOutputContract>,
    name: &str,
) -> bool {
    contract.is_some() && name == crate::headless_contract::STRUCTURED_OUTPUT_TOOL
}

fn check_tool_name_with_structured_output(
    context: &Context,
    name: &str,
    contract: Option<&crate::headless_contract::StructuredOutputContract>,
) -> Result<(), String> {
    if host_report_name(contract, name) {
        Ok(())
    } else {
        check_tool_name(context, name)
    }
}

fn check_bound_structured_output_schema(
    context: &Context,
    contract: &crate::headless_contract::StructuredOutputContract,
    value: &serde_json::Value,
    google: bool,
) -> Result<(), String> {
    if contract.matches_native_schema(value, google)? {
        return Ok(());
    }
    check_structured_output_schema(context, value)
}

fn check_structured_output_schema(
    context: &Context,
    value: &serde_json::Value,
) -> Result<(), String> {
    use serde_json::Value;
    fn private_text(context: &Context, text: &str) -> Result<(), String> {
        if context.mask_known(text)? == text {
            Ok(())
        } else {
            Err("A private value is required by the structured-output schema".into())
        }
    }
    fn schema(context: &Context, value: &Value, depth: usize) -> Result<(), String> {
        if depth > 64 {
            return Err("Structured-output schema exceeds the privacy nesting bound".into());
        }
        let Some(items) = value.as_object() else {
            return Ok(());
        };
        for (key, value) in items {
            match key.as_str() {
                "properties" | "patternProperties" | "$defs" | "definitions"
                | "dependentSchemas" => {
                    if let Some(fields) = value.as_object() {
                        for (name, child) in fields {
                            private_text(context, name)?;
                            schema(context, child, depth + 1)?;
                        }
                    }
                }
                "dependentRequired" | "dependencies" => {
                    if let Some(fields) = value.as_object() {
                        for (name, child) in fields {
                            private_text(context, name)?;
                            if child.is_object() {
                                schema(context, child, depth + 1)?;
                            } else {
                                check_schema_for_tool(context, child, None)?;
                            }
                        }
                    }
                }
                "type" => {
                    let public_type = |value: &Value| {
                        value.as_str().is_some_and(|value| {
                            matches!(
                                value,
                                "null"
                                    | "boolean"
                                    | "object"
                                    | "array"
                                    | "number"
                                    | "string"
                                    | "integer"
                            )
                        })
                    };
                    if !public_type(value)
                        && !value
                            .as_array()
                            .is_some_and(|items| items.iter().all(public_type))
                    {
                        check_schema_for_tool(context, value, None)?;
                    }
                }
                "items"
                | "additionalItems"
                | "additionalProperties"
                | "unevaluatedItems"
                | "unevaluatedProperties"
                | "propertyNames"
                | "contains"
                | "if"
                | "then"
                | "else"
                | "not"
                | "contentSchema" => {
                    if let Some(items) = value.as_array() {
                        for child in items {
                            schema(context, child, depth + 1)?;
                        }
                    } else {
                        schema(context, value, depth + 1)?;
                    }
                }
                "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                    if let Some(items) = value.as_array() {
                        for child in items {
                            schema(context, child, depth + 1)?;
                        }
                    }
                }
                "minLength" | "maxLength" | "minimum" | "maximum" | "exclusiveMinimum"
                | "exclusiveMaximum" | "multipleOf" | "minItems" | "maxItems" | "uniqueItems"
                | "minContains" | "maxContains" | "minProperties" | "maxProperties"
                | "readOnly" | "writeOnly" | "deprecated" => {}
                "$ref" | "$dynamicRef" => {
                    if let Some(reference) = value.as_str() {
                        // This prefix was introduced by the host's value wrapper.
                        private_text(
                            context,
                            reference
                                .strip_prefix("#/properties/value")
                                .unwrap_or(reference),
                        )?;
                    }
                }
                "enum" | "const" | "default" | "examples" => {
                    arguments::validate(context, None, value)?;
                }
                "required" | "description" | "title" | "$comment" | "$id" | "$schema"
                | "$anchor" | "$dynamicAnchor" | "format" | "pattern" | "contentEncoding"
                | "contentMediaType" => {
                    check_schema_for_tool(context, value, None)?;
                }
                _ => {
                    private_text(context, key)?;
                    check_schema_for_tool(context, value, None)?;
                }
            }
        }
        Ok(())
    }
    let properties = value
        .get("properties")
        .and_then(Value::as_object)
        .ok_or("Host structured-output wrapper changed")?;
    if value.as_object().is_none_or(|items| items.len() != 4)
        || value.get("type").and_then(Value::as_str) != Some("object")
        || value.get("additionalProperties") != Some(&Value::Bool(false))
        || value.get("required") != Some(&serde_json::json!(["value"]))
        || properties.len() != 1
        || !properties.contains_key("value")
    {
        return Err("Host structured-output wrapper changed".into());
    }
    schema(context, &properties["value"], 0)
}

fn check_tool_name(context: &Context, name: &str) -> Result<(), String> {
    if crate::agent::registry::ToolRegistry::builtins()
        .get(name)
        .is_none()
        && context.mask_known(name)? != name
    {
        return Err("A private custom tool name is required by the provider protocol; adjust its local policy before sending".into());
    }
    Ok(())
}

#[cfg(test)]
mod structured_output_tests {
    use super::*;
    use crate::headless_contract::{StructuredOutputContract, STRUCTURED_OUTPUT_TOOL};
    use crate::request::{PreparedModelRequest, RequestKind};
    use serde_json::{json, Value};

    #[test]
    fn filename_duplicate_listing_clarifies_authenticated_target_and_retained_basis() {
        use crate::agent::client::{ChatMessage, FunctionCall, ToolCallMsg};
        use crate::image_content::ModelContent;
        let settings = crate::provider_filter::ProviderFilterConfig {
            enabled: true,
            hide: crate::provider_filter::FilterHide {
                filenames: true,
                ..Default::default()
            },
            ..Default::default()
        };
        crate::provider_filter::with_test_filter(&settings, || {
            let temporary = tempfile::tempdir().unwrap();
            let workspace = temporary.path().join("workspace");
            let directory = workspace.join("private-directory");
            std::fs::create_dir_all(&directory).unwrap();
            let workspace = workspace.canonicalize().unwrap();
            let directory = workspace.join("private-directory");
            for name in [
                "directory",
                "succeeded",
                "reference",
                "retained",
                "reconfirmed",
            ] {
                std::fs::write(workspace.join(name), "").unwrap();
            }
            let rows = (0..50)
                .map(|index| format!("private-entry-{index:02}.txt"))
                .collect::<Vec<_>>();
            for row in &rows {
                std::fs::write(directory.join(row), "").unwrap();
            }
            let output = rows.join("\n");
            let context = Context::open(
                temporary.path().join("profile"),
                workspace.clone(),
                Uuid::new_v4(),
                None,
            )
            .unwrap();
            context.allow_ephemeral();
            let canonical = context.project_filename_path(&directory).unwrap();
            let requested = context
                .state
                .lock()
                .unwrap()
                .filenames
                .as_mut()
                .unwrap()
                .source_link("page.html", "./private-directory")
                .unwrap()
                .unwrap();
            assert_ne!(requested, canonical);
            assert_eq!(
                context.resolve_filename_path(&requested, false).unwrap(),
                directory
            );
            let native_arguments = json!({"path":directory}).to_string();
            crate::context_observation::capture_host(
                "list_dir",
                &native_arguments,
                &output,
                true,
                workspace.to_str().unwrap(),
                &[directory.to_string_lossy().into_owned()],
                None,
            )
            .unwrap();
            let exchange = |id: &str, reference: &str| {
                let arguments = json!({"path":reference}).to_string();
                let observation =
                    crate::context_observation::bind_native_result("list_dir", &arguments, &output)
                        .unwrap();
                vec![
                    ChatMessage {
                        role: "assistant".into(),
                        content: ModelContent::text(""),
                        tool_calls: Some(vec![ToolCallMsg {
                            id: id.into(),
                            call_type: "function".into(),
                            function: FunctionCall {
                                name: "list_dir".into(),
                                arguments,
                                arguments_retrieved_spans: Vec::new(),
                            },
                        }]),
                        tool_call_id: None,
                        response_items: Vec::new(),
                    },
                    ChatMessage {
                        role: "tool".into(),
                        content: ModelContent::text(&output)
                            .with_context_observation(Some(observation))
                            .with_tool_result_status(true),
                        tool_calls: None,
                        tool_call_id: Some(id.into()),
                        response_items: Vec::new(),
                    },
                ]
            };
            let mut messages = exchange("first-list", &canonical);
            crate::context_observation::project(&mut messages);
            messages.extend(exchange("second-list", &requested));
            assert_eq!(
                crate::context_observation::project(&mut messages).results,
                1
            );
            let marker = messages[3].content.as_str().unwrap().to_owned();
            assert_eq!(
                crate::context_observation::duplicate_listing_basis(&messages, 3).as_deref(),
                Some("first-list")
            );
            let before_repeat = serde_json::to_value(&messages).unwrap();
            crate::context_observation::project(&mut messages);
            assert_eq!(serde_json::to_value(&messages).unwrap(), before_repeat);
            assert!(!messages[1].content.is_context_marker());
            assert_eq!(messages[1].content.as_str(), Some(output.as_str()));

            let mut forged = messages.clone();
            forged[3].content = ModelContent::context_marker(&marker).with_tool_result_status(true);
            assert!(crate::context_observation::duplicate_listing_basis(&forged, 3).is_none());
            let mut imported = messages.clone();
            let observation = serde_json::from_value(
                serde_json::to_value(messages[3].content.context_observation().unwrap()).unwrap(),
            )
            .unwrap();
            imported[3].content = imported[3]
                .content
                .clone()
                .with_context_observation(Some(observation));
            assert!(crate::context_observation::duplicate_listing_basis(&imported, 3).is_none());
            let mut altered = messages.clone();
            *altered[3].content.as_text_mut().unwrap() += " private-directory";
            assert!(crate::context_observation::duplicate_listing_basis(&altered, 3).is_none());
            let mut missing_basis = messages.clone();
            missing_basis[1].content = ModelContent::context_marker("omitted");
            assert!(
                crate::context_observation::duplicate_listing_basis(&missing_basis, 3).is_none()
            );

            let mut request = PreparedModelRequest::new(
                RequestKind::Subagent,
                "fixture",
                "model",
                messages,
                Vec::new(),
                Default::default(),
            );
            let revision = context.prepare_request(&mut request).unwrap();
            let clarified = request.messages[3].content.as_str().unwrap();
            assert!(clarified.contains(&format!(
                "list_dir succeeded for requested reference {requested}"
            )));
            assert!(clarified.contains(&format!(
                "same directory as canonical reference {canonical}"
            )));
            assert!(clarified.contains("Full listing is retained in call first-list."));
            assert!(clarified.ends_with(&marker));
            assert!(!clarified.contains("private-directory"));
            assert!(!request.messages[1]
                .content
                .as_str()
                .unwrap()
                .contains("private-entry"));
            let wire = crate::provider_filter::process_current()
                .prepare_request(
                    &serde_json::to_vec(&json!({"messages":request.messages})).unwrap(),
                )
                .unwrap();
            context
                .validate_wire(&wire, revision, &context.policy_fingerprint())
                .unwrap();
        });
    }

    #[test]
    fn task_filter_native_read_url_authorities_retain_only_issued_spans() {
        use crate::agent::client::ChatMessage;
        use crate::image_content::ModelContent;
        let settings = crate::provider_filter::ProviderFilterConfig {
            enabled: true,
            phrases: vec!["HALT-CANARY".into()],
            ..Default::default()
        };
        crate::provider_filter::with_test_filter(&settings, || {
            let temporary = tempfile::tempdir().unwrap();
            let workspace = temporary.path().join("workspace");
            std::fs::create_dir_all(&workspace).unwrap();
            for name in ["invalid", "private-note.txt"] {
                std::fs::write(workspace.join(name), "").unwrap();
            }
            let source = "https://www.conventionalcommits.org/";
            let path = workspace.join("guide.md");
            std::fs::write(&path, source).unwrap();
            let base = Context::open(
                temporary.path().join("profile"),
                workspace,
                Uuid::new_v4(),
                None,
            )
            .unwrap();
            base.allow_ephemeral();
            let context = base.with_task_filter(Default::default()).unwrap();
            let unrelated = base.with_task_filter(Default::default()).unwrap();
            let raw = render_read(source, 1, 1, false);
            let receipt = context
                .record_read(path.to_str().unwrap(), source, &raw, 1, 1, false)
                .unwrap();
            let mut request = PreparedModelRequest::new(
                RequestKind::Subagent,
                "fixture",
                "model",
                vec![ChatMessage {
                    role: "tool".into(),
                    content: ModelContent::text(raw)
                        .with_source_receipt(Some(receipt))
                        .with_tool_result_status(true),
                    tool_calls: None,
                    tool_call_id: Some("read-guide".into()),
                    response_items: Vec::new(),
                }],
                Vec::new(),
                Default::default(),
            );
            let revision = context.prepare_request(&mut request).unwrap();
            let prepared = request.messages[0].content.as_str().unwrap();
            assert!(context.exact_read_projection(prepared).unwrap().is_some());
            let filter = context.task_filter().unwrap();
            let wire = filter
                .prepare_request(
                    &serde_json::to_vec(&json!({"messages":request.messages})).unwrap(),
                )
                .unwrap();
            let body: Value = serde_json::from_slice(&wire).unwrap();
            let final_read = body["messages"][0]["content"].as_str().unwrap();
            assert_ne!(final_read, prepared);
            assert!(context.exact_read_projection(final_read).unwrap().is_none());
            let authority = "https://ca-r01.invalid";
            assert!(final_read.contains(authority));
            assert!(!final_read.contains("conventionalcommits"));
            context
                .validate_wire(&wire, revision, &context.policy_fingerprint())
                .unwrap();
            assert_eq!(
                context.restore_text(final_read).unwrap(),
                render_read(source, 1, 1, false)
            );

            // The dictionary belongs to this task, not the process or another
            // independently prepared task. Lookalikes do not borrow its entry.
            let issued_authorities = filter.issued_url_authorities(authority);
            assert_eq!(issued_authorities.len(), 1);
            assert_eq!(issued_authorities[0], 0..authority.len());
            let unrelated_url = format!("{authority}/");
            assert!(unrelated
                .task_filter()
                .unwrap()
                .issued_url_authorities(&unrelated_url)
                .is_empty());
            let ordinary = unrelated
                .mask_known_unprotected_shared(&unrelated_url)
                .unwrap();
            assert_eq!(
                unrelated.mask_known(&unrelated_url).unwrap(),
                ordinary.as_deref().unwrap_or(&unrelated_url)
            );
            for forged in [
                "https://ca-r02.invalid/",
                "https://ca-r1.invalid/",
                "https://ca-r01.invalid.evil/",
                "https://ca-r01.invalid:443/",
                "http://ca-r01.invalid/",
                "https://private-note.txt@ca-r01.invalid/",
            ] {
                assert!(filter.issued_url_authorities(forged).is_empty());
                let ordinary = context.mask_known_unprotected_shared(forged).unwrap();
                assert_eq!(
                    context.mask_known(forged).unwrap(),
                    ordinary.as_deref().unwrap_or(forged)
                );
            }
            let private_source = "<h1>SENSITIVE-CUSTOMER</h1>";
            std::fs::write(context.workspace.join("customer.html"), private_source).unwrap();
            let projected = context
                .project_source("customer.html", private_source)
                .unwrap();
            assert!(!projected.text.contains("SENSITIVE-CUSTOMER"));
            assert!(!context
                .mask_known("SENSITIVE-CUSTOMER")
                .unwrap()
                .contains("SENSITIVE-CUSTOMER"));
            let private = format!("{authority}/private-note.txt?key=SENSITIVE-CUSTOMER#private-note.txt private-note.txt SENSITIVE-CUSTOMER");
            let masked = context.mask_known(&private).unwrap();
            assert!(masked.starts_with(authority));
            assert!(!masked.contains("private-note.txt"));
            assert!(!masked.contains("SENSITIVE-CUSTOMER"));
            let unissued = format!("{authority}/unissued-path/file");
            assert_eq!(context.mask_known(&unissued).unwrap(), unissued);
            assert!(filter
                .scan_prepared_request(format!("{authority}/HALT-CANARY").as_bytes())
                .is_err());
        });
    }

    fn request(schema: Value) -> (StructuredOutputContract, PreparedModelRequest) {
        let contract = StructuredOutputContract::from_value(schema).unwrap();
        let request = PreparedModelRequest::new(
            RequestKind::Inference,
            "fixture",
            "model",
            Vec::new(),
            vec![contract.tool_definition()],
            Default::default(),
        )
        .with_structured_output(Some(&contract));
        (contract, request)
    }

    #[test]
    fn failed_listing_result_stays_a_filtered_diagnostic() {
        use crate::agent::client::{ChatMessage, FunctionCall, ToolCallMsg};
        use crate::agent::registry::ToolExecutionResult;
        use crate::image_content::ModelContent;
        let settings = crate::provider_filter::ProviderFilterConfig {
            enabled: true,
            hide: crate::provider_filter::FilterHide {
                filenames: true,
                ..Default::default()
            },
            ..Default::default()
        };
        crate::provider_filter::with_test_filter(&settings, || {
            let temporary = tempfile::tempdir().unwrap();
            let workspace = temporary.path().join("workspace");
            std::fs::create_dir_all(workspace.join("blocked")).unwrap();
            std::fs::write(workspace.join("private-diagnostic.txt"), "local data").unwrap();
            let context = Context::open(
                temporary.path().join("profile"),
                workspace,
                uuid::Uuid::new_v4(),
                None,
            )
            .unwrap();
            context.allow_ephemeral();
            let reference = context
                .project_filename_path(std::path::Path::new("blocked"))
                .unwrap();
            let denied = ToolExecutionResult::Denied("Child resource is outside inherited scope: symlink scope cannot be admitted; private-diagnostic.txt".into());
            assert!(!denied.model_text().starts_with("Error"));
            let mut request = PreparedModelRequest::new(
                RequestKind::Subagent,
                "fixture",
                "model",
                vec![
                    ChatMessage {
                        role: "assistant".into(),
                        content: ModelContent::text(""),
                        tool_calls: Some(vec![ToolCallMsg {
                            id: "denied-list".into(),
                            call_type: "function".into(),
                            function: FunctionCall {
                                name: "list_dir".into(),
                                arguments: json!({"path":reference}).to_string(),
                                arguments_retrieved_spans: Vec::new(),
                            },
                        }]),
                        tool_call_id: None,
                        response_items: Vec::new(),
                    },
                    ChatMessage {
                        role: "tool".into(),
                        content: ModelContent::text(denied.model_text())
                            .with_retrieved_spans(crate::retrieved_content::full_span(
                                denied.model_text(),
                            ))
                            .with_tool_result_status(denied.is_success()),
                        tool_calls: None,
                        tool_call_id: Some("denied-list".into()),
                        response_items: Vec::new(),
                    },
                ],
                Vec::new(),
                Default::default(),
            );
            let revision = context.prepare_request(&mut request).unwrap();
            let diagnostic = request.messages[1].content.as_str().unwrap();
            assert!(diagnostic.starts_with("[tool failed]\n"));
            assert!(diagnostic.contains("Child resource is outside inherited scope"));
            assert!(!diagnostic.contains("private-diagnostic.txt"));
            assert!(diagnostic.contains("__CA_"));
            context
                .validate_wire(
                    &serde_json::to_vec(&json!({"messages":request.messages})).unwrap(),
                    revision,
                    &context.policy_fingerprint(),
                )
                .unwrap();
        });
    }

    #[test]
    fn failed_listing_invalid_reference_does_not_block_filtered_continuation() {
        use crate::agent::client::{ChatMessage, FunctionCall, ToolCallMsg};
        use crate::image_content::ModelContent;
        let settings = crate::provider_filter::ProviderFilterConfig {
            enabled: true,
            hide: crate::provider_filter::FilterHide {
                filenames: true,
                ..Default::default()
            },
            ..Default::default()
        };
        crate::provider_filter::with_test_filter(&settings, || {
            let temporary = tempfile::tempdir().unwrap();
            let workspace = temporary.path().join("workspace");
            std::fs::create_dir_all(workspace.join("private-folder")).unwrap();
            let context = Context::open(
                temporary.path().join("profile"),
                workspace,
                uuid::Uuid::new_v4(),
                None,
            )
            .unwrap();
            context.allow_ephemeral();
            let reference = context
                .project_filename_path(Path::new("private-folder"))
                .unwrap();
            let invalid_reference = format!("{reference}/");
            assert!(context
                .resolve_filename_path(&invalid_reference, false)
                .is_err());
            for success in [None, Some(true), Some(false)] {
                let diagnostic =
                    "Use a complete discovered filename alias; private-folder was not listed";
                let mut content = ModelContent::text(diagnostic)
                    .with_retrieved_spans(crate::retrieved_content::full_span(diagnostic));
                if let Some(success) = success {
                    content = content.with_tool_result_status(success);
                }
                let mut request = PreparedModelRequest::new(
                    RequestKind::Subagent,
                    "fixture",
                    "model",
                    vec![
                        ChatMessage {
                            role: "assistant".into(),
                            content: ModelContent::text(""),
                            tool_calls: Some(vec![ToolCallMsg {
                                id: "invalid-list".into(),
                                call_type: "function".into(),
                                function: FunctionCall {
                                    name: "list_dir".into(),
                                    arguments: json!({"path": invalid_reference}).to_string(),
                                    arguments_retrieved_spans: Vec::new(),
                                },
                            }]),
                            tool_call_id: None,
                            response_items: Vec::new(),
                        },
                        ChatMessage {
                            role: "tool".into(),
                            content,
                            tool_calls: None,
                            tool_call_id: Some("invalid-list".into()),
                            response_items: Vec::new(),
                        },
                    ],
                    Vec::new(),
                    Default::default(),
                );
                let prepared = context.prepare_request(&mut request);
                if success != Some(false) {
                    assert!(prepared
                        .unwrap_err()
                        .contains("complete discovered filename alias"));
                    continue;
                }
                let revision = prepared.unwrap();
                let diagnostic = request.messages[1].content.as_str().unwrap();
                assert!(diagnostic.starts_with("[tool failed]\n"));
                assert!(!diagnostic.contains("private-folder"));
                let arguments: Value = serde_json::from_str(
                    &request.messages[0].tool_calls.as_ref().unwrap()[0]
                        .function
                        .arguments,
                )
                .unwrap();
                assert_eq!(arguments["path"], invalid_reference);
                context
                    .validate_wire(
                        &serde_json::to_vec(&json!({"messages": request.messages})).unwrap(),
                        revision,
                        &context.policy_fingerprint(),
                    )
                    .unwrap();
                assert!(context
                    .resolve_filename_path(&invalid_reference, false)
                    .is_err());
            }
        });
    }

    #[test]
    fn native_automatic_map_schema_preserves_protocol_and_private_report_boundaries() {
        let filter = crate::provider_filter::ProviderFilterConfig {
            enabled: true,
            hide: crate::provider_filter::FilterHide {
                filenames: true,
                ..Default::default()
            },
            ..Default::default()
        };
        crate::provider_filter::with_test_filter(&filter, || {
            let temporary = tempfile::tempdir().unwrap();
            let workspace = temporary.path().join("workspace");
            std::fs::create_dir(&workspace).unwrap();
            for name in [
                "http",
                "tasks",
                "body",
                "ResultSchema",
                "Configuration",
                "human_review",
                "evidence.txt",
            ] {
                std::fs::write(workspace.join(name), "private filename collision").unwrap();
            }
            let context = Context::open(
                temporary.path().join("profile"),
                workspace,
                uuid::Uuid::new_v4(),
                None,
            )
            .unwrap();
            context.allow_ephemeral();
            context.refresh_filenames().unwrap();
            let contract = crate::automatic_tasks::output_contract().unwrap();
            let definition = contract.tool_definition();
            let parameters = definition.function.parameters.clone();
            assert_ne!(context.mask_known("http").unwrap(), "http");
            let mut projected_schema = parameters.clone();
            project_schema_text(&context, &mut projected_schema).unwrap();
            assert_ne!(
                projected_schema, parameters,
                "native titles and descriptions collide too"
            );
            let description = "/properties/value/properties/graph/properties/tasks/items/properties/produces/items/description";
            assert!(parameters.pointer(description).is_some());
            assert_ne!(
                projected_schema.pointer(description),
                parameters.pointer(description)
            );
            let (_, mut caller_request) = request(contract.schema().clone());
            assert!(
                context.prepare_request(&mut caller_request).is_err(),
                "identical caller JSON cannot grant host schema authority"
            );

            let mut prepared = PreparedModelRequest::new(
                RequestKind::Subagent,
                "fixture",
                "model",
                Vec::new(),
                vec![definition.clone()],
                Default::default(),
            )
            .with_structured_output(Some(&contract));
            let mut changed = prepared.clone();
            changed.tools[0].function.parameters["properties"]["value"]["description"] =
                json!("http");
            assert!(context.prepare_request(&mut changed).is_err());
            let revision = context.prepare_request(&mut prepared).unwrap();
            assert_eq!(prepared.tools[0].function.parameters, parameters);
            let fingerprint = context.policy_fingerprint();
            let google_parameters = crate::tool_schema::translate(
                STRUCTURED_OUTPUT_TOOL,
                &parameters,
                crate::tool_schema::Profile::Google,
                crate::tool_schema::Policy::Compatible,
            )
            .unwrap()
            .schema
            .into_owned();
            let bodies = [
                (
                    json!({"tools":[definition]}),
                    "/tools/0/function/parameters",
                ),
                (
                    json!({"tools":[{"type":"function","name":STRUCTURED_OUTPUT_TOOL,"description":definition.function.description,"parameters":parameters}]}),
                    "/tools/0/parameters",
                ),
                (
                    json!({"tools":[{"name":STRUCTURED_OUTPUT_TOOL,"description":definition.function.description,"input_schema":parameters}]}),
                    "/tools/0/input_schema",
                ),
                (
                    json!({"tools":[{"functionDeclarations":[{"name":STRUCTURED_OUTPUT_TOOL,"description":definition.function.description,"parameters":google_parameters}]}]}),
                    "/tools/0/functionDeclarations/0/parameters",
                ),
            ];
            for (body, schema_path) in &bodies {
                context
                    .validate_wire_with_structured_output(
                        &serde_json::to_vec(body).unwrap(),
                        revision,
                        &fingerprint,
                        Some(&contract),
                    )
                    .unwrap();
                let mut changed = body.clone();
                changed.pointer_mut(schema_path).unwrap()["description"] = json!("http");
                assert!(context
                    .validate_wire_with_structured_output(
                        &serde_json::to_vec(&changed).unwrap(),
                        revision,
                        &fingerprint,
                        Some(&contract),
                    )
                    .is_err());
            }

            let report = json!({"value":{"title":"Map","body":"## Shared purpose and constraints\nKeep body and http local.\n## Task: tasks\nRead evidence.txt.\n","tasks":["tasks"],"graph":{"budget":{"allow_unpriced":true,"max_physical_requests":24},"tasks":[{"task_id":"tasks","tools":["read_file"],"paths":["evidence.txt"],"commands":[],"evidence":[{"kind":"human_review"}]}]}}});
            contract.validate_value(&report["value"]).unwrap();
            assert!(arguments::validate_contract_report(&context, &contract, &report).is_err());
            let mut projected = report.clone();
            arguments::project_contract_report(&context, &contract, &mut projected).unwrap();
            assert_ne!(projected["value"]["body"], report["value"]["body"]);
            assert_ne!(projected["value"]["tasks"], report["value"]["tasks"]);
            assert_ne!(
                projected["value"]["graph"]["tasks"][0]["paths"],
                report["value"]["graph"]["tasks"][0]["paths"]
            );
            arguments::validate_contract_report(&context, &contract, &projected).unwrap();
            let restored = context.restore_arguments(&projected).unwrap();
            assert_eq!(restored, report);
            contract.validate_value(&restored["value"]).unwrap();
            let mut changed = bodies[0].0.clone();
            changed["messages"] = json!([{"role":"assistant","tool_calls":[{"id":"map-report","type":"function","function":{"name":STRUCTURED_OUTPUT_TOOL,"arguments":projected.to_string()}}]}]);
            context
                .validate_wire_with_structured_output(
                    &serde_json::to_vec(&changed).unwrap(),
                    revision,
                    &fingerprint,
                    Some(&contract),
                )
                .unwrap();
            changed["messages"][0]["tool_calls"][0]["function"]["arguments"] =
                report.to_string().into();
            assert!(context
                .validate_wire_with_structured_output(
                    &serde_json::to_vec(&changed).unwrap(),
                    revision,
                    &fingerprint,
                    Some(&contract),
                )
                .is_err());
        });
    }

    #[test]
    fn native_repair_schema_keeps_exact_host_controls_and_private_payload_boundaries() {
        let filter = crate::provider_filter::ProviderFilterConfig {
            enabled: true,
            hide: crate::provider_filter::FilterHide {
                filenames: true,
                ..Default::default()
            },
            ..Default::default()
        };
        crate::provider_filter::with_test_filter(&filter, || {
            let temporary = tempfile::tempdir().unwrap();
            let workspace = temporary.path().join("workspace");
            std::fs::create_dir(&workspace).unwrap();
            for name in ["status", "replacement", "incomplete", "diagnosis"] {
                std::fs::write(workspace.join(name), "private filename collision").unwrap();
            }
            let context = Context::open(
                temporary.path().join("profile"),
                workspace,
                uuid::Uuid::new_v4(),
                None,
            )
            .unwrap();
            context.allow_ephemeral();
            context.refresh_filenames().unwrap();
            let contract = crate::narrow_worker::RepairContract::output_contract().unwrap();
            let definition = contract.tool_definition();
            let caller_schema = definition.function.parameters["properties"]["value"].clone();
            let (_, mut caller_request) = request(caller_schema);
            assert!(
                context.prepare_request(&mut caller_request).is_err(),
                "the same caller-defined schema cannot claim native control provenance"
            );

            let mut prepared = PreparedModelRequest::new(
                RequestKind::Subagent,
                "fixture",
                "model",
                Vec::new(),
                vec![definition.clone()],
                Default::default(),
            )
            .with_structured_output(Some(&contract));
            let revision = context.prepare_request(&mut prepared).unwrap();
            let fingerprint = context.policy_fingerprint();
            let parameters = definition.function.parameters.clone();
            let google_parameters = crate::tool_schema::translate(
                STRUCTURED_OUTPUT_TOOL,
                &parameters,
                crate::tool_schema::Profile::Google,
                crate::tool_schema::Policy::Compatible,
            )
            .unwrap()
            .schema
            .into_owned();
            let bodies = [
                json!({"tools":[definition]}),
                json!({"tools":[{"type":"function","name":STRUCTURED_OUTPUT_TOOL,"description":definition.function.description,"parameters":parameters}]}),
                json!({"tools":[{"name":STRUCTURED_OUTPUT_TOOL,"description":definition.function.description,"input_schema":parameters}]}),
                json!({"tools":[{"functionDeclarations":[{"name":STRUCTURED_OUTPUT_TOOL,"description":definition.function.description,"parameters":google_parameters}]}]}),
            ];
            for body in &bodies {
                context
                    .validate_wire_with_structured_output(
                        &serde_json::to_vec(body).unwrap(),
                        revision,
                        &fingerprint,
                        Some(&contract),
                    )
                    .unwrap();
            }

            let mut changed = bodies[0].clone();
            changed["tools"][0]["function"]["parameters"]["properties"]["value"]["properties"]
                ["replacement"]["description"] = json!("replacement");
            assert!(
                context
                    .validate_wire_with_structured_output(
                        &serde_json::to_vec(&changed).unwrap(),
                        revision,
                        &fingerprint,
                        Some(&contract),
                    )
                    .is_err(),
                "injected schema text cannot borrow a native field's spelling"
            );
            let mut changed = bodies[0].clone();
            changed["tools"][0]["function"]["parameters"]["properties"]["value"]["properties"]
                ["status"]["enum"] = json!(["replacement", "incomplete", "diagnosis"]);
            assert!(
                context
                    .validate_wire_with_structured_output(
                        &serde_json::to_vec(&changed).unwrap(),
                        revision,
                        &fingerprint,
                        Some(&contract),
                    )
                    .is_err(),
                "native controls remain bound to their exact schema positions"
            );
            let mut changed = bodies[0].clone();
            changed["messages"] = json!([{"role":"user","content":"replacement"}]);
            assert!(
                context
                    .validate_wire_with_structured_output(
                        &serde_json::to_vec(&changed).unwrap(),
                        revision,
                        &fingerprint,
                        Some(&contract),
                    )
                    .is_err(),
                "ordinary data does not inherit native schema authority"
            );
            assert!(arguments::validate_report(
                &context,
                &parameters,
                &json!({"value":{"status":"replacement","replacement":"replacement","diagnosis":""}}),
            )
            .is_err(), "replacement source text remains private data");
        });
    }

    #[test]
    fn native_cargo_schema_and_infer_offsets_keep_exact_control_positions() {
        let filter = crate::provider_filter::ProviderFilterConfig {
            enabled: true,
            hide: crate::provider_filter::FilterHide {
                filenames: true,
                ..Default::default()
            },
            ..Default::default()
        };
        crate::provider_filter::with_test_filter(&filter, || {
            let temporary = tempfile::tempdir().unwrap();
            let workspace = temporary.path().join("workspace");
            std::fs::create_dir(&workspace).unwrap();
            for name in [
                "findings",
                "group_id",
                "cause",
                "evidence_index",
                "group-1",
                "0",
                "2",
                "90",
                "123",
            ] {
                std::fs::write(workspace.join(name), "private filename collision").unwrap();
            }
            let context = Context::open(
                temporary.path().join("profile"),
                workspace,
                uuid::Uuid::new_v4(),
                None,
            )
            .unwrap();
            context.allow_ephemeral();
            context.refresh_filenames().unwrap();
            let contract =
                StructuredOutputContract::native_cargo_diagnostics(&["group-1".into()]).unwrap();
            let definition = contract.tool_definition();
            let (_, mut caller_request) = request(contract.schema().clone());
            assert!(context.prepare_request(&mut caller_request).is_err());
            let mut prepared = PreparedModelRequest::new(
                RequestKind::Inference,
                "fixture",
                "model",
                Vec::new(),
                vec![definition.clone()],
                Default::default(),
            )
            .with_structured_output(Some(&contract));
            let revision = context.prepare_request(&mut prepared).unwrap();
            let fingerprint = context.policy_fingerprint();
            let report = json!({"value":{"findings":[{"group_id":"group-1","cause":"observed failure","evidence_index":0}]}});
            let mut projected = report.clone();
            arguments::project_contract_report(&context, &contract, &mut projected).unwrap();
            assert_eq!(projected, report);
            arguments::validate_contract_report(&context, &contract, &report).unwrap();
            assert!(
                arguments::validate_report(&context, &definition.function.parameters, &report)
                    .is_err(),
                "generic numeric report data stays private"
            );
            let body = json!({"tools":[definition],"messages":[{"role":"assistant","tool_calls":[{"id":"native-report","type":"function","function":{"name":STRUCTURED_OUTPUT_TOOL,"arguments":report.to_string()}}]}]});
            context
                .validate_wire_with_structured_output(
                    &serde_json::to_vec(&body).unwrap(),
                    revision,
                    &fingerprint,
                    Some(&contract),
                )
                .unwrap();
            for changed in [
                json!({"value":{"findings":[{"group_id":"group-1","cause":0,"evidence_index":0}]}}),
                json!({"value":{"findings":[{"group_id":"group-1","cause":"observed failure","evidence_index":2}]}}),
                json!({"value":{"evidence_index":0,"findings":[]}}),
            ] {
                assert!(
                    arguments::validate_contract_report(&context, &contract, &changed).is_err()
                );
            }

            let infer = json!({"task":"Inspect immutable evidence", "context_refs":[{"artifact_id":"artifact317","stream":"stderr","start":0,"end":2}], "output_schema":{"type":"object","properties":{"answer":{"type":"string"}}}, "budget":{"max_requests":2,"timeout_secs":90}});
            crate::agent::registry::ToolRegistry::builtins()
                .validate("infer", &infer)
                .unwrap();
            let mut projected = infer.clone();
            arguments::project(&context, "infer", &mut projected).unwrap();
            assert_eq!(projected, infer);
            arguments::validate(&context, Some("infer"), &infer).unwrap();
            for number in [0, 2, 90, 123] {
                let mut schema_data = infer.clone();
                schema_data["output_schema"]["properties"]["answer"] =
                    json!({"type":"integer","enum":[number]});
                assert!(
                    arguments::project(&context, "infer", &mut schema_data).is_err(),
                    "caller output_schema numbers remain data: {number}"
                );
                let data =
                    json!({"context_refs":{"start":number},"budget":{"max_requests":number}});
                assert!(
                    arguments::validate(&context, None, &data).is_err(),
                    "the same names under unregistered data grant no controls: {number}"
                );
            }
        });
    }

    #[test]
    fn host_report_contract_preserves_only_bound_protocol_and_checks_dynamic_schema() {
        let filter = crate::provider_filter::ProviderFilterConfig {
            enabled: true,
            hide: crate::provider_filter::FilterHide {
                filenames: true,
                ..Default::default()
            },
            ..Default::default()
        };
        crate::provider_filter::with_test_filter(&filter, || {
            let temporary = tempfile::tempdir().unwrap();
            let workspace = temporary.path().join("workspace");
            std::fs::create_dir(&workspace).unwrap();
            for name in [
                STRUCTURED_OUTPUT_TOOL,
                "value",
                "object",
                "string",
                "PrivateSchemaKey317",
                "PrivateSchemaEnum317",
                "PrivateSchemaDescription317",
                "structured",
                "123",
            ] {
                std::fs::write(workspace.join(name), "private filename collision").unwrap();
            }
            let context = Context::open(
                temporary.path().join("profile"),
                workspace,
                uuid::Uuid::new_v4(),
                None,
            )
            .unwrap();
            context.allow_ephemeral();
            context.refresh_filenames().unwrap();
            let good = json!({"type":"object","additionalProperties":false,"required":["answer"],"properties":{"answer":{"type":"string"}}});
            let (contract, mut prepared) = request(good.clone());
            let revision = context.prepare_request(&mut prepared).unwrap();
            assert_eq!(prepared.tools.len(), 1);
            assert_eq!(prepared.tools[0].function.name, STRUCTURED_OUTPUT_TOOL);
            let body = serde_json::to_vec(&json!({"tools":prepared.tools,"messages":[{
                "role":"assistant","tool_calls":[{"id":"report","type":"function","function":{
                    "name":STRUCTURED_OUTPUT_TOOL,"arguments":json!({"value":{"answer":"public answer"}}).to_string()
                }}]
            }]})).unwrap();
            let fingerprint = context.policy_fingerprint();
            assert!(
                context
                    .validate_wire(&body, revision, &fingerprint)
                    .is_err(),
                "unmarked report name must retain custom-tool privacy checks"
            );
            context
                .validate_wire_with_structured_output(
                    &body,
                    revision,
                    &fingerprint,
                    Some(&contract),
                )
                .unwrap();
            let (_, mut unmarked) = request(good.clone());
            unmarked = unmarked.with_structured_output(None);
            context.prepare_request(&mut unmarked).unwrap();
            assert!(unmarked.tools.is_empty());

            for schema in [
                json!({"type":"object","properties":{"PrivateSchemaKey317":{"type":"string"}}}),
                json!({"type":"object","properties":{"object":{"type":"string"}}}),
                json!({"type":"object","properties":{"answer":{"type":"string","enum":["PrivateSchemaEnum317"]}}}),
                json!({"type":"object","properties":{"answer":{"type":"integer","enum":[123]}}}),
            ] {
                let (_, mut prepared) = request(schema);
                assert!(
                    context.prepare_request(&mut prepared).is_err(),
                    "caller schema data cannot borrow protocol vocabulary"
                );
            }
            let (numeric_contract, mut numeric_prepared) =
                request(json!({"type":"object","properties":{"limit":{"type":"integer"}}}));
            let numeric_revision = context.prepare_request(&mut numeric_prepared).unwrap();
            let original_arguments = json!({"value":{"limit":123}});
            let mut projected_arguments = original_arguments.clone();
            assert!(
                arguments::project_report(
                    &context,
                    &numeric_contract.tool_definition().function.parameters,
                    &mut projected_arguments
                )
                .is_err(),
                "report data cannot borrow builtin numeric controls"
            );
            assert_eq!(
                projected_arguments, original_arguments,
                "typed private data must not be coerced"
            );
            let numeric_body =
                serde_json::to_vec(&json!({"tools":numeric_prepared.tools,"messages":[{
                    "role":"assistant","tool_calls":[{"id":"report","type":"function","function":{
                        "name":STRUCTURED_OUTPUT_TOOL,"arguments":original_arguments.to_string()
                    }}]
                }]}))
                .unwrap();
            assert!(
                context
                    .validate_wire_with_structured_output(
                        &numeric_body,
                        numeric_revision,
                        &fingerprint,
                        Some(&numeric_contract)
                    )
                    .is_err(),
                "numeric report data remains private at the final wire boundary"
            );
            let (_, mut changed) = request(good);
            changed.tools[0].function.parameters["properties"]["value"]["properties"]["injected"] =
                json!({"type":"string"});
            assert!(
                context.prepare_request(&mut changed).is_err(),
                "definition drift cannot retain the host marker"
            );
            let mut changed: Value = serde_json::from_slice(&body).unwrap();
            changed["tools"][0]["function"]["parameters"]["unexpected"] =
                json!("PrivateSchemaDescription317");
            assert!(context
                .validate_wire_with_structured_output(
                    &serde_json::to_vec(&changed).unwrap(),
                    revision,
                    &fingerprint,
                    Some(&contract)
                )
                .is_err());
            let mut changed: Value = serde_json::from_slice(&body).unwrap();
            changed["messages"][0]["tool_calls"][0]["function"]["description"] =
                json!(contract.tool_definition().function.description);
            assert!(
                context
                    .validate_wire_with_structured_output(
                        &serde_json::to_vec(&changed).unwrap(),
                        revision,
                        &fingerprint,
                        Some(&contract)
                    )
                    .is_err(),
                "call-record data cannot borrow a tool definition's public description"
            );
            let mut changed: Value = serde_json::from_slice(&body).unwrap();
            changed["messages"][0]["tool_calls"][0]["function"]["parameters"] =
                contract.tool_definition().function.parameters;
            assert!(
                context
                    .validate_wire_with_structured_output(
                        &serde_json::to_vec(&changed).unwrap(),
                        revision,
                        &fingerprint,
                        Some(&contract)
                    )
                    .is_err(),
                "call-record data cannot borrow a tool definition's public schema vocabulary"
            );
            let mut changed: Value = serde_json::from_slice(&body).unwrap();
            changed["tools"][0]["function"]["description"] = json!("PrivateSchemaDescription317");
            assert!(context
                .validate_wire_with_structured_output(
                    &serde_json::to_vec(&changed).unwrap(),
                    revision,
                    &fingerprint,
                    Some(&contract)
                )
                .is_err());
        });
    }
}

// Only real protocol envelopes grant control-field exemptions. Data nested in
// tool arguments/results cannot turn a property called `name` into authority.
fn wire_tool_definition_location(path: &[&str]) -> bool {
    matches!(
        path,
        ["tools", "*"] | ["tools", "*", "function"] | ["tools", "*", "functionDeclarations", "*"]
    )
}

fn wire_tool_location(path: &[&str], items: &serde_json::Map<String, serde_json::Value>) -> bool {
    match path {
        ["tools", "*"]
        | ["tools", "*", "function"]
        | ["tools", "*", "functionDeclarations", "*"]
        | ["messages", "*", "tool_calls", "*", "function"]
        | ["contents", "*", "parts", "*", "functionCall"]
        | ["contents", "*", "parts", "*", "functionResponse"]
        | ["tool_choice", "function"] => true,
        ["input", "*"] => {
            items.get("type").and_then(serde_json::Value::as_str) == Some("function_call")
        }
        ["messages", "*", "content", "*"] => {
            items.get("type").and_then(serde_json::Value::as_str) == Some("tool_use")
        }
        ["tool_choice"] => items.contains_key("name"),
        _ => false,
    }
}

fn fixed_evidence_wire(
    path: &[&str],
    key: &str,
    value: &serde_json::Value,
    items: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    let exact = |value: &serde_json::Value| {
        value.as_str() == Some(crate::context_pruning::EVIDENCE_GUIDANCE)
    };
    match path {
        ["messages", "*"] | ["input", "*"]
            if key == "content"
                && items.get("role").and_then(serde_json::Value::as_str) == Some("system") =>
        {
            exact(value)
                || value.as_array().is_some_and(|blocks| {
                    blocks.len() == 1
                        && blocks[0].as_object().is_some_and(|block| {
                            block.len() == 2
                                && block
                                    .get("type")
                                    .and_then(serde_json::Value::as_str)
                                    .is_some_and(|kind| matches!(kind, "text" | "input_text"))
                                && block.get("text").is_some_and(exact)
                        })
                })
        }
        ["system", "*"]
            if key == "text"
                && items.get("type").and_then(serde_json::Value::as_str) == Some("text") =>
        {
            exact(value)
        }
        ["systemInstruction", "parts", "*"] | ["system_instruction", "parts", "*"]
            if key == "text" =>
        {
            exact(value)
        }
        _ => false,
    }
}

fn host_builtin_description(context: &Context, name: &str, text: &str) -> bool {
    let text = if context.filename_mode {
        text.strip_suffix(FILENAME_TOOL_GUIDANCE).unwrap_or(text)
    } else {
        text
    };
    crate::agent::registry::ToolRegistry::builtins()
        .get(name)
        .is_some_and(|spec| text == spec.description)
}

fn filename_guidance_prefix<'a>(
    context: &Context,
    path: &[&str],
    key: &str,
    items: &serde_json::Map<String, serde_json::Value>,
    value: &'a serde_json::Value,
) -> Option<&'a str> {
    if key != "description"
        || !context.filename_mode
        || !matches!(
            path,
            ["tools", "*"]
                | ["tools", "*", "function"]
                | ["tools", "*", "functionDeclarations", "*"]
        )
        || !wire_tool_location(path, items)
        || items
            .get("name")
            .and_then(serde_json::Value::as_str)
            .and_then(|name| crate::agent::registry::ToolRegistry::builtins().get(name))
            .is_none()
    {
        return None;
    }
    value.as_str()?.strip_suffix(FILENAME_TOOL_GUIDANCE)
}

fn public_reasoning_effort(value: &serde_json::Value) -> bool {
    value.as_str().is_some_and(|text| {
        text.parse::<crate::config::ReasoningEffort>()
            .is_ok_and(|effort| effort.to_string() == text)
    })
}

fn host_tool_note(suffix: &str) -> bool {
    let suffix = suffix.strip_prefix('\n').unwrap_or(suffix);
    !suffix.is_empty()
        && suffix.lines().all(|line| {
            line.is_empty()
                || line.starts_with("Skill directory ")
                || line.starts_with("background command ")
                || line.starts_with("background subagent ")
                || line.starts_with("The user killed background ")
        })
}

fn immutable_wire_error(path: &[&str]) -> String {
    // Only fixed protocol labels may enter a diagnostic. Schema property names
    // and arbitrary caller keys can themselves contain private data.
    let location = path
        .iter()
        .map(|part| match *part {
            "*"
            | "messages"
            | "input"
            | "contents"
            | "system"
            | "systemInstruction"
            | "system_instruction"
            | "parts"
            | "text"
            | "content"
            | "role"
            | "type"
            | "reasoning_content"
            | "reasoning_effort"
            | "reasoning"
            | "thinking"
            | "effort"
            | "tools"
            | "function"
            | "functionDeclarations"
            | "parameters"
            | "input_schema"
            | "description"
            | "title"
            | "tool_calls"
            | "arguments"
            | "name"
            | "properties"
            | "required"
            | "items"
            | "enum"
            | "const"
            | "default"
            | "model"
            | "instructions"
            | "summary"
            | "tool_choice"
            | "toolConfig"
            | "functionCallingConfig" => *part,
            _ => "[field]",
        })
        .collect::<Vec<_>>()
        .join(".");
    format!("Private content remained in an immutable protocol field at {location}; use /filter continue for a fresh privacy-safe continuation")
}

#[cfg(test)]
#[test]
fn immutable_wire_diagnostic_redacts_untrusted_path_keys() {
    let error = immutable_wire_error(&["messages", "*", "content", "customer-secret\nvalue"]);
    assert!(error.contains("messages.*.content.[field]"));
    assert!(!error.contains("customer-secret"));
    assert!(!error.contains('\n'));
    assert!(
        immutable_wire_error(&["messages", "*", "reasoning_content"])
            .contains("messages.*.reasoning_content")
    );
}

fn wire_control_field(
    path: &[&str],
    key: &str,
    value: &serde_json::Value,
    items: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    match path {
        [] => {
            matches!(
                key,
                "model" | "service_tier" | "include" | "prompt_cache_key"
            ) || (key == "tool_choice"
                && value
                    .as_str()
                    .is_some_and(|s| matches!(s, "auto" | "none" | "required" | "any")))
                || (key == "reasoning_effort" && public_reasoning_effort(value))
        }
        ["reasoning"] => key == "effort" && public_reasoning_effort(value),
        ["messages", "*"] => matches!(key, "role" | "tool_call_id"),
        ["messages", "*", "tool_calls", "*"] => matches!(key, "id" | "type"),
        ["input", "*"] => {
            matches!(key, "id" | "type" | "role" | "call_id")
                || (key == "encrypted_content"
                    && items.get("type").and_then(serde_json::Value::as_str) == Some("reasoning"))
        }
        ["messages", "*", "content", "*"] => {
            matches!(key, "type" | "id" | "tool_use_id")
                || (matches!(key, "signature" | "data")
                    && match items.get("type").and_then(serde_json::Value::as_str) {
                        Some("thinking") => key == "signature",
                        Some("redacted_thinking") => true,
                        _ => false,
                    })
        }
        ["input", "*", "content", "*"]
        | ["input", "*", "summary", "*"]
        | ["system", "*"]
        | ["tools", "*"]
        | ["tool_choice"] => key == "type",
        ["contents", "*"] | ["systemInstruction"] | ["system_instruction"] => key == "role",
        ["contents", "*", "parts", "*"]
        | ["systemInstruction", "parts", "*"]
        | ["system_instruction", "parts", "*"] => {
            matches!(key, "thoughtSignature" | "thought_signature")
        }
        ["contents", "*", "parts", "*", "functionCall"]
        | ["contents", "*", "parts", "*", "functionResponse"] => key == "id",
        ["toolConfig", "functionCallingConfig"] => key == "mode",
        _ => false,
    }
}

pub(crate) fn mask_value_strings(
    context: &Context,
    value: &mut serde_json::Value,
) -> Result<(), String> {
    match value {
        serde_json::Value::String(text) => *text = context.mask_known(text)?,
        serde_json::Value::Array(items) => {
            for value in items {
                mask_value_strings(context, value)?;
            }
        }
        serde_json::Value::Object(items) => {
            for value in items.values_mut() {
                mask_value_strings(context, value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn check_schema_for_tool(
    context: &Context,
    value: &serde_json::Value,
    name: Option<&str>,
) -> Result<(), String> {
    fn collect(value: &serde_json::Value, public: &mut std::collections::BTreeSet<String>) {
        match value {
            serde_json::Value::String(text) => {
                public.insert(text.clone());
            }
            serde_json::Value::Object(items) => {
                for (key, value) in items {
                    public.insert(key.clone());
                    collect(value, public);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    collect(item, public);
                }
            }
            _ => {}
        }
    }
    fn inspect(
        context: &Context,
        value: &serde_json::Value,
        public: &std::collections::BTreeSet<String>,
        builtin_owner: bool,
        host_grant: bool,
    ) -> Result<(), String> {
        match value {
            serde_json::Value::String(text) => {
                // spawn_agent names its grants in an enum. Those names are host
                // protocol, even when a workspace file uses the same spelling.
                // A caller-added enum value, and any custom tool schema, is not.
                let host_tool_name = builtin_owner
                    && host_grant
                    && crate::agent::registry::ToolRegistry::builtins()
                        .get(text.as_str())
                        .is_some();
                if !host_tool_name && !public.contains(text) && context.mask_known(text)? != *text {
                    return Err("A private value is required by an immutable tool schema; adjust its local policy before sending".into());
                }
            }
            serde_json::Value::Object(items) => {
                for (key, value) in items {
                    if !public.contains(key) && context.mask_known(key)? != *key {
                        return Err("A private field name is required by an immutable tool schema; adjust its local policy before sending".into());
                    }
                    inspect(
                        context,
                        value,
                        public,
                        builtin_owner,
                        builtin_owner && key == "enum",
                    )?;
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    inspect(context, item, public, builtin_owner, host_grant)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut public = std::collections::BTreeSet::new();
    let builtin_owner = if let Some(spec) =
        name.and_then(|name| crate::agent::registry::ToolRegistry::builtins().get(name))
    {
        // Only text already published by the registered native schema is public.
        // A caller cannot add private keys by borrowing a builtin tool's name.
        collect(&spec.schema, &mut public);
        true
    } else {
        false
    };
    inspect(context, value, &public, builtin_owner, false)
}

fn project_responses_reasoning_plaintext(
    context: &Context,
    item: &mut serde_json::Value,
) -> Result<(), String> {
    if item.get("type").and_then(serde_json::Value::as_str) != Some("reasoning") {
        return Ok(());
    }
    for field in ["summary", "content"] {
        let Some(parts) = item
            .get_mut(field)
            .and_then(serde_json::Value::as_array_mut)
        else {
            continue;
        };
        for part in parts {
            let masked = {
                let Some(text) = part.get("text").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                context.mask_known(text)?
            };
            part["text"] = serde_json::Value::String(masked);
        }
    }
    Ok(())
}

fn project_responses_message_plaintext(
    context: &Context,
    item: &mut serde_json::Value,
) -> Result<(), String> {
    if item.get("type").and_then(serde_json::Value::as_str) != Some("message") {
        return Ok(());
    }
    let Some(parts) = item
        .get_mut("content")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(());
    };
    // validate_wire uses mask_known. A short census name is still private.
    for part in parts {
        let masked = {
            let Some(text) = part.get("text").and_then(serde_json::Value::as_str) else {
                continue;
            };
            context.mask_known(text)?
        };
        part["text"] = serde_json::Value::String(masked);
    }
    Ok(())
}

fn project_schema_text(context: &Context, value: &mut serde_json::Value) -> Result<(), String> {
    match value {
        serde_json::Value::Object(items) => {
            for (key, value) in items {
                if matches!(key.as_str(), "description" | "title") {
                    if let Some(text) = value.as_str() {
                        *value = serde_json::Value::String(context.mask_known(text)?);
                    }
                } else if !matches!(key.as_str(), "enum" | "const" | "default" | "examples") {
                    project_schema_text(context, value)?;
                }
            }
        }
        serde_json::Value::Array(items) => {
            for value in items {
                project_schema_text(context, value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn edit_key(name: &str, args: &serde_json::Value) -> Result<String, String> {
    serde_json::to_vec(&(name, args))
        .map(|value| digest(&value))
        .map_err(|_| "Protected edit arguments cannot be encoded".into())
}

fn projection_cache_bytes(projection: &projection::Projection, view: &EditView) -> usize {
    let nodes = projection
        .nodes
        .iter()
        .map(|node| {
            node.selector
                .capacity()
                .saturating_add(node.label.capacity())
        })
        .sum::<usize>();
    let changes = projection
        .changes
        .iter()
        .map(|change| {
            change
                .selector
                .capacity()
                .saturating_add(change.original.capacity())
                .saturating_add(change.replacement.capacity())
        })
        .sum::<usize>();
    let replacements = view
        .replacements
        .iter()
        .map(|(token, original)| token.capacity().saturating_add(original.capacity()))
        .sum::<usize>();
    projection
        .text
        .capacity()
        .saturating_add(projection.digest.capacity())
        .saturating_add(
            projection
                .nodes
                .capacity()
                .saturating_mul(std::mem::size_of::<projection::Node>()),
        )
        .saturating_add(
            projection
                .changes
                .capacity()
                .saturating_mul(std::mem::size_of::<projection::Change>()),
        )
        .saturating_add(nodes)
        .saturating_add(changes)
        .saturating_add(replacements)
        .saturating_add(view.projected.len())
        .saturating_add(
            view.replacements
                .len()
                .saturating_mul(std::mem::size_of::<(String, String)>()),
        )
        .saturating_add(view.source_digest.capacity())
        .saturating_add(view.policy_digest.capacity())
        .saturating_add(256)
}

fn rebuild_matcher(state: &mut State) -> Result<(), String> {
    if !state.matcher_dirty {
        return Ok(());
    }
    state.matcher_values = state
        .active
        .iter()
        .filter(|(value, _)| globally_private_literal(value))
        .map(|(v, r)| (v.clone(), r.clone()))
        .collect();
    state.matcher = if state.matcher_values.is_empty() {
        None
    } else {
        Some(
            AhoCorasickBuilder::new()
                .match_kind(MatchKind::LeftmostLongest)
                .build(state.matcher_values.iter().map(|(v, _)| v.as_str()))
                .map_err(|_| "Private-value matcher exceeds its bounds")?,
        )
    };
    state.matcher_dirty = false;
    Ok(())
}
fn text_edges(text: &str) -> [u8; 32] {
    let mut edges = [0; 32];
    let first = text.len().min(16);
    edges[..first].copy_from_slice(&text.as_bytes()[..first]);
    let last = text.len().min(16);
    edges[16..16 + last].copy_from_slice(&text.as_bytes()[text.len() - last..]);
    edges
}

fn is_word(text: &str) -> bool {
    text.chars().all(|c| c.is_alphanumeric() || c == '_')
}
fn globally_private_literal(value: &str) -> bool {
    !value.is_empty() && !value.chars().all(char::is_whitespace)
        // The plan deliberately retains ordinary syntax and whitespace. A
        // formatting literal ':' or '/' must not mask every JSON key boundary
        // or path separator in authored instructions. Exact source-node views
        // remain opaque; meaningful punctuation strings remain private too.
        && !(value.len() == 1 && value.as_bytes()[0].is_ascii_punctuation())
}
fn boundary(text: &str, at: usize) -> bool {
    if at == 0 || at == text.len() {
        return true;
    }
    let before = text[..at].chars().next_back().unwrap_or(' ');
    let after = text[at..].chars().next().unwrap_or(' ');
    !(before.is_alphanumeric() || before == '_') || !(after.is_alphanumeric() || after == '_')
}
fn view_replacements(projection: &projection::Projection) -> Arc<[(String, String)]> {
    let mut seen = std::collections::HashSet::new();
    projection
        .changes
        .iter()
        .filter(|change| change.action != Action::Allow && seen.insert(change.replacement.as_str()))
        .map(|change| {
            (
                format!(
                    "{}{}",
                    change.replacement,
                    "\n".repeat(
                        change
                            .original
                            .bytes()
                            .filter(|byte| *byte == b'\n')
                            .count()
                    )
                ),
                change.original.clone(),
            )
        })
        .collect::<Vec<_>>()
        .into()
}

fn replace_view_tokens(
    text: &str,
    replacements: &[(String, String)],
    source_view: &str,
    registry: &AliasRegistry,
) -> Result<String, String> {
    let entries = replacements
        .iter()
        .filter(|(key, _)| !key.is_empty())
        .map(|(key, value)| (key.trim_end_matches('\n'), (key.as_str(), value.as_str())))
        .collect::<HashMap<_, _>>();
    let mut result = String::with_capacity(text.len());
    let mut offset = 0;
    while let Some(found) = text[offset..].find(store::TOKEN_PREFIX) {
        let start = offset + found;
        result.push_str(&text[offset..start]);
        let body = start + store::TOKEN_PREFIX.len();
        let end = text[body..]
            .find(store::TOKEN_SUFFIX)
            .map(|end| body + end + store::TOKEN_SUFFIX.len())
            .ok_or("Edit contains an incomplete private reference")?;
        let token = &text[start..end];
        if let Some((key, original)) = entries.get(token) {
            if !text[start..].starts_with(key) {
                return Err(
                    "Edit changed a multiline private reference; retain its projected line padding"
                        .into(),
                );
            }
            result.push_str(original);
            offset = start + key.len();
        } else {
            // New values may come from another protected read or a local
            // calculation in this chat. The file ticket still binds the target;
            // only a known renderer reference in this exact registry can expand.
            if source_view.contains(token) {
                return Err("Edit contains an ambiguous literal private reference; alias that literal and read the file again".into());
            }
            let original = registry.restore_exact(token)?;
            result.push_str(original);
            let padding = original.bytes().filter(|byte| *byte == b'\n').count();
            offset = end;
            if padding > 0
                && text[end..].bytes().take(padding).all(|byte| byte == b'\n')
                && text.len() - end >= padding
            {
                offset += padding;
            }
        }
    }
    result.push_str(&text[offset..]);
    Ok(result)
}

pub(crate) fn render_read(source: &str, start: usize, end: usize, hashes: bool) -> String {
    let mut out = String::new();
    for (index, line) in source
        .lines()
        .enumerate()
        .filter(|(index, _)| (start..=end).contains(&(index + 1)))
    {
        if hashes {
            out.push_str(&crate::agent::tools::line_anchor(line));
            out.push(' ');
        }
        out.push_str(line);
        out.push('\n');
        let _ = index;
    }
    out
}

#[cfg(test)]
mod selected_tests;

#[cfg(test)]
mod imported_native_history_tests {
    use super::*;
    use crate::request::{PreparedModelRequest, PromptCacheScope, RequestKind, RequestStore};

    #[test]
    fn imported_native_history_cannot_bypass_active_project_privacy_canary() {
        crate::provider_filter::with_test_filter(&Default::default(), || {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("workspace");
            std::fs::create_dir(&workspace).unwrap();
            let context = Context::open(
                root.path().join("profile"),
                workspace.clone(),
                Uuid::new_v4(),
                None,
            )
            .unwrap();
            context.allow_ephemeral();
            let mut policy = context.policy();
            policy.enabled = true;
            policy.default_action = policy::Action::Alias;
            context.save_policy(policy).unwrap();
            let canary = "imported-project-private-canary";
            let original = format!("{{\"name\":\"{canary}\"}}");
            std::fs::write(workspace.join("private.json"), &original).unwrap();
            let projection = context.project_source("private.json", &original).unwrap();
            assert!(!projection.text.contains(canary));
            let path = root.path().join("history.json");
            std::fs::write(&path, serde_json::to_vec(&serde_json::json!({
                "schema":crate::provider_history::SCHEMA,"provider":"openai-codex","protocol":"openai_responses","model":"fixture",
                "input":[
                    {"type":"function_call","call_id":"old-read","name":"read_file","arguments":"{\"path\":\"private.json\"}"},
                    {"type":"function_call_output","call_id":"old-read","output":format!("private.json: {original}")},
                    {"type":"compaction","id":"comp1","encrypted_content":"foreign opaque state without local privacy receipt"}
                ]
            })).unwrap()).unwrap();
            let store = RequestStore::default();
            store.set_imported_history(
                context.session_id,
                crate::provider_history::load_native(&path).unwrap(),
            );
            let mut prepared = PreparedModelRequest::new(
                RequestKind::MainTurn,
                "openai-codex",
                "fixture",
                Vec::new(),
                Vec::new(),
                Default::default(),
            )
            .with_prompt_cache(PromptCacheScope::conversation(context.session_id), 0);
            store.prepare_imported_history(&mut prepared).unwrap();
            let error = context.prepare_request(&mut prepared).unwrap_err();
            assert!(error.contains("Imported native history"), "{error}");
            assert!(error.contains("project source privacy"), "{error}");
            assert_eq!(
                std::fs::read_to_string(workspace.join("private.json")).unwrap(),
                original
            );
            prepared.imported_history = None;
            assert!(
                context.prepare_request(&mut prepared).is_ok(),
                "ordinary canonical history remains supported"
            );
        });
    }
}
