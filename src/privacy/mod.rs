//! Local, session-owned projections. Original bytes and alias dictionaries never
//! form part of a provider request. All expensive work is run by callers on workers.
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

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
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
    commit_gate: Mutex<()>,
    outbound: RwLock<Option<OutboundSnapshot>>,
    preview_cache: Mutex<preview::Cache>,
    state: Mutex<State>,
    task_broker: Mutex<Option<tasks::Broker>>,
    task_context: Mutex<Option<store::PrivateTaskContextRefV1>>,
    command_workspaces: Mutex<command_workspace::Workspaces>,
    root_authority: Mutex<Vec<PathBuf>>,
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

pub(crate) async fn scope<F: std::future::Future>(
    context: Option<Arc<Context>>,
    operation: F,
) -> F::Output {
    CURRENT.scope(context, operation).await
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
    owner
        .session_id
        .and_then(for_session)
        .or_else(|| owner.session_id.is_none().then(current).flatten())
        .filter(|context| context.enabled())
}

/// A prepared private edit remains bound even after its policy is disabled.
/// Read, shell and provider lookups continue to use the enabled-only helper.
pub(crate) fn for_edit_owner(owner: crate::jobs::JobOwner) -> Option<Arc<Context>> {
    owner
        .session_id
        .and_then(for_session)
        .or_else(|| owner.session_id.is_none().then(current).flatten())
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
        if reference
            .as_ref()
            .is_some_and(|reference| !reference.optional_policy)
            && matches!(std::fs::symlink_metadata(&policy_path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        {
            return Err(
                "Saved project privacy rules are missing; restore them before continuing".into(),
            );
        }
        let registry = match reference.as_ref() {
            Some(reference) => AliasRegistry::load(&scope, session_id, reference)?,
            None => AliasRegistry::new(session_id),
        };
        if registry.session_id() != session_id {
            return Err("Privacy alias registry belongs to another session".into());
        }
        Self::from_registry(
            scope,
            workspace,
            policy_path,
            CurrentPolicy::new(
                policy,
                crate::provider_filter::current().selected_values_only(),
            ),
            registry,
            reference,
            false,
        )
    }

    fn from_registry(
        scope: PathBuf,
        workspace: PathBuf,
        policy_path: PathBuf,
        policy: CurrentPolicy,
        registry: AliasRegistry,
        reference: Option<PrivacySessionRef>,
        ephemeral: bool,
    ) -> Result<Arc<Self>, String> {
        let filename_snapshot = registry
            .entries()
            .iter()
            .rev()
            .find(|entry| entry.kind.eq_ignore_ascii_case("filename_registry"))
            .map(|entry| {
                serde_json::from_str::<filenames::Snapshot>(&entry.value)
                    .map_err(|_| "Saved filename mappings are invalid")
            })
            .transpose()?;
        let filename_mode =
            crate::provider_filter::current().filenames() || filename_snapshot.is_some();
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
            } else if entry.kind.eq_ignore_ascii_case("filename_registry") {
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
            names.fork_namespace(&session_id.simple().to_string())?;
        }
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
            commit_gate: Mutex::new(()),
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
            }),
            task_broker: Mutex::new(None),
            task_context: Mutex::new(task_context),
            command_workspaces: Mutex::new(command_workspace::Workspaces::default()),
            root_authority: Mutex::new(Vec::new()),
        });
        contexts()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(session_id, Arc::downgrade(&context));
        Ok(context)
    }

    /// Start a no-session branch with the exact inherited token vocabulary and
    /// source receipts, but no writer, sidecars, edit tickets, or cached views.
    pub(crate) fn fork_ephemeral(&self, new_session_id: Uuid) -> Result<Arc<Self>, String> {
        self.refresh_policy()?;
        if for_session(new_session_id).is_some() {
            return Err("Privacy branch identity is already active".into());
        }
        let registry = self
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .registry
            .fork_in_memory(new_session_id)?;
        let fork = Self::from_registry(
            self.scope.clone(),
            self.workspace.clone(),
            self.policy_path.clone(),
            CurrentPolicy::new(self.stored_policy(), self.selected_only),
            registry,
            None,
            true,
        )?;
        *fork.task_context.lock().unwrap_or_else(|p| p.into_inner()) = self
            .task_context
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        Ok(fork)
    }

    pub(crate) fn enabled(&self) -> bool {
        if tasks::remote_binding().is_some() {
            return false;
        }
        self.policy
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .value
            .enabled
    }
    pub(crate) fn filenames_enabled(&self) -> bool {
        self.filename_mode
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
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .filenames
            .as_ref()
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
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let names = state
            .filenames
            .as_mut()
            .ok_or("Filename mappings are unavailable")?;
        let changed = names.discover()?;
        let snapshot = serde_json::to_string(&names.snapshot())
            .map_err(|_| "Filename mappings cannot be encoded")?;
        let previous = state
            .registry
            .entries()
            .iter()
            .rev()
            .find(|entry| entry.kind.eq_ignore_ascii_case("filename_registry"))
            .map(|entry| entry.value.as_str());
        if previous != Some(snapshot.as_str()) {
            state.registry.alias("filename_registry", &snapshot)?;
            self.private_revision
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        if changed {
            state.cache.clear();
        }
        Ok(())
    }
    pub(crate) fn private_revision(&self) -> u64 {
        self.private_revision
            .load(std::sync::atomic::Ordering::Acquire)
    }
    pub(crate) fn summary(&self) -> String {
        let policy = self.policy.read().unwrap_or_else(|p| p.into_inner());
        format!(
            "Project privacy · {} · {} rules",
            if policy.value.default_action == Action::Allow {
                "Selected values only"
            } else {
                "Hidden unless allowed"
            },
            policy.value.rules.len()
        )
    }
    pub(crate) fn policy(&self) -> Policy {
        self.policy
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .value
            .clone()
    }
    fn stored_policy(&self) -> Policy {
        self.policy
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .stored
            .clone()
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
        if tasks::remote_binding().is_some() {
            return Ok(false);
        }
        let _coordination = policy_coordination()
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let expected = self.stored_policy();
        let authoritative = policy_store::load(&self.policy_path)?;
        if expected != Policy::default()
            && matches!(std::fs::symlink_metadata(&self.policy_path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        {
            return Err(
                "Project privacy rules disappeared; restore them before sending another request"
                    .into(),
            );
        }
        Ok(self.install_policy(&authoritative))
    }

    pub(crate) fn check_policy(&self, revision: u64) -> Result<(), String> {
        if self.refresh_policy()? || self.policy().revision != revision || !self.enabled() {
            return Err(
                "Project privacy rules changed before transmission; prepare the request again"
                    .into(),
            );
        }
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
            *current = filename_policy(
                CurrentPolicy::new(policy.clone(), self.selected_only),
                self.filename_mode,
            );
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
        if source.len() > MAX_SOURCE_BYTES {
            return Err("Source exceeds the 8 MiB privacy parsing bound".into());
        }
        let source_digest = digest(source.as_bytes());
        let policy_digest =
            digest(&serde_json::to_vec(policy).map_err(|_| "Invalid privacy rules")?);
        // Ordinary source views bind the exact policy they parsed. The
        // deliberately stricter no-Allow view for unbound results carries its
        // separate host policy binding rather than claiming that synthetic
        // policy is the project's current configuration.
        let fingerprint = policy_binding.unwrap_or(&policy_digest);
        let key = format!("{path}:{source_digest}:{policy_digest}:{fingerprint}");
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let current = self.policy.read().unwrap_or_else(|p| p.into_inner());
        if current.digest != fingerprint {
            return Err(
                "Privacy policy changed before source projection; read the source again".into(),
            );
        }
        if let Some(cached) = state.cache.get(&key) {
            if cached.ticket.valid() {
                cached.ticket.touch();
                let projection = cached.projection.clone();
                let view = cached.view.clone();
                if publish_view {
                    state.views.insert(path.into(), view);
                }
                state.sources.insert(path.into(), source_digest);
                return Ok(projection);
            }
        }
        drop(current);
        // Filename-only mode activates the provider boundary, not automatic
        // source-data hiding. Keep the real policy binding above and below;
        // only the source parser receives this disabled projection policy.
        let filename_source_policy = self.filename_only().then(|| {
            let mut source_policy = policy.clone();
            source_policy.enabled = false;
            source_policy.rules.clear();
            source_policy
        });
        let source_policy = filename_source_policy.as_ref().unwrap_or(policy);
        let mut discovered = HashMap::<String, (String, usize)>::new();
        let web_source = self.selected_only
            && matches!(
                projection::source_format(path),
                "html" | "css" | "javascript" | "typescript" | "tsx"
            );
        let mut projected =
            projection::project_request(path, source, source_policy, &mut |kind, value| {
                if let Some((token, _)) = discovered.get(value) {
                    return Ok(token.clone());
                }
                let source_link = if web_source
                    && matches!(kind, policy::AliasKind::String | policy::AliasKind::Path)
                {
                    state
                        .filenames
                        .as_mut()
                        .map(|names| names.source_link(path, value))
                        .transpose()?
                        .flatten()
                } else {
                    None
                };
                let token = match source_link {
                    Some(token) => token,
                    None => state.registry.alias(kind.label(), value)?,
                };
                let order = discovered.len();
                discovered.insert(value.to_string(), (token.clone(), order));
                Ok(token)
            })?;
        if web_source {
            if let Some(names) = &mut state.filenames {
                projected.text = names.project_source_text(&projected.text)?;
            }
        }
        // A policy can change while parsing on this worker. Keep its read
        // guard through publication so no stale view can appear after the
        // policy install cleared the cache and edit bindings.
        let current = self.policy.read().unwrap_or_else(|p| p.into_inner());
        if current.digest != fingerprint {
            return Err(
                "Privacy policy changed during source projection; read the source again".into(),
            );
        }
        let mut dictionary_changed = false;
        let replacements = if policy.enabled && policy.rules.is_empty() {
            // With no disclosure rules every parsed content span is hidden.
            // The alias callback already collected each distinct original once;
            // do not repeat BTreeMap/HashSet work for every repeated source leaf.
            let mut ordered = vec![None; discovered.len()];
            for (original, (mut token, order)) in discovered {
                if !original.is_empty()
                    && !state.allowed.contains(&original)
                    && state.active.get(&original) != Some(&token)
                {
                    state.active.insert(original.clone(), token.clone());
                    dictionary_changed = true;
                }
                let padding = original.bytes().filter(|byte| *byte == b'\n').count();
                token.extend(std::iter::repeat_n('\n', padding));
                ordered[order] = Some((token, original));
            }
            ordered
                .into_iter()
                .map(|pair| pair.expect("discovery order is complete"))
                .collect::<Vec<_>>()
                .into()
        } else {
            // Explicit rules retain source-order Allow/Hide precedence,
            // including identities allowed by another selected region.
            for change in &projected.changes {
                if change.action == Action::Allow {
                    state.allowed.insert(change.original.clone());
                    dictionary_changed |= state.active.remove(&change.original).is_some();
                } else if !change.original.is_empty()
                    && !state.allowed.contains(&change.original)
                    && state.active.get(&change.original) != Some(&change.replacement)
                {
                    state
                        .active
                        .insert(change.original.clone(), change.replacement.clone());
                    dictionary_changed = true;
                }
            }
            view_replacements(&projected)
        };
        for change in projected
            .changes
            .iter()
            .filter(|change| change.action != Action::Allow)
        {
            if let Some(value) = literals::decoded(path, source, change) {
                if !state.allowed.contains(&value) {
                    let token = state.registry.alias("string", &value)?;
                    dictionary_changed |=
                        state.active.insert(value, token.clone()).as_ref() != Some(&token);
                }
            }
        }
        state.matcher_dirty = true;
        if dictionary_changed {
            self.private_revision
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        state.sources.insert(path.into(), source_digest.clone());
        let view = EditView {
            source_digest,
            policy_revision: policy.revision,
            policy_digest: fingerprint.to_owned(),
            projected: Arc::from(projected.text.as_str()),
            replacements,
        };
        if publish_view {
            state.views.insert(path.into(), view.clone());
        }
        let bytes = projection_cache_bytes(&projected, &view);
        let projected = Arc::new(projected);
        state.cache.retain(|_, entry| entry.ticket.valid());
        if let Some(ticket) = crate::cache_memory::reserve("privacy_projections", bytes) {
            state.cache.insert(
                key,
                CachedProjection {
                    projection: projected.clone(),
                    view,
                    ticket,
                },
            );
        }
        Ok(projected)
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
        let absolute = path.to_string();
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
        if !self.filename_mode && self.policy().action_for(&receipt.path, "/@path") != Action::Allow
        {
            let mut changed = false;
            for spelling in [&absolute, &receipt.path] {
                let alias = state.registry.alias("path", spelling)?;
                changed |= state
                    .active
                    .insert(spelling.clone(), alias.clone())
                    .as_ref()
                    != Some(&alias);
            }
            state.matcher_dirty = true;
            if changed {
                self.private_revision
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            }
        }
        state.registry.alias("source_receipt", &value)?;
        state.reads.insert(id.clone(), receipt);
        Ok(id)
    }

    pub(crate) fn project_retrieved(
        &self,
        text: &str,
        source: Option<&str>,
    ) -> Result<String, String> {
        let receipt = match source {
            Some(id) => Some(
                self.state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .reads
                    .get(id)
                    .cloned()
                    .ok_or(
                        "Private source receipt is unavailable; read or attach the source again",
                    )?,
            ),
            None => None,
        };
        if let Some(receipt) = receipt {
            if receipt.original != text {
                return Err("Private source identity is invalid".into());
            }
            if let Some(control) = &receipt.native_control {
                if digest(receipt.original.as_bytes()) != receipt.source_digest {
                    return Err("Private native control identity is invalid".into());
                }
                return native_control::project(self, &receipt.original, control);
            }
            if digest(receipt.source.as_bytes()) != receipt.source_digest {
                return Err("Private source identity is invalid".into());
            }
            let projection = self.project_source_shared(
                &receipt.path,
                &receipt.source,
                &self.policy(),
                None,
                true,
            )?;
            if let Some(attachment) = &receipt.attachment {
                let selected = if attachment.ranged {
                    projection
                        .text
                        .lines()
                        .skip(attachment.start_line.saturating_sub(1))
                        .take(attachment.end_line.saturating_sub(attachment.start_line) + 1)
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    projection.text.clone()
                };
                let selected = selected
                    .chars()
                    .take(attachment.max_chars)
                    .collect::<String>();
                let prefix = self.mask_known(&attachment.prefix)?;
                let suffix = self.mask_known(&attachment.suffix)?;
                return Ok(format!("{prefix}{selected}{suffix}"));
            }
            let selected =
                render_read(&projection.text, receipt.start, receipt.end, receipt.hashes);
            return read_projection::project(self, &selected, receipt.read_footer.as_ref());
        }
        if self.policy().default_action == Action::Allow {
            self.discover_selected_sources()?;
            return self.mask_known(text);
        }
        // Unknown retrieved text has no authority to claim a file's structure.
        // Preserve plain paragraph/list structure and withhold every content node.
        // An unbound result cannot borrow a real file's Allow rule, including
        // a project-wide glob: its source identity was never established.
        let current = self.policy.read().unwrap_or_else(|p| p.into_inner());
        let opaque_policy = Policy {
            enabled: true,
            revision: current.value.revision,
            ..Policy::default()
        };
        let fingerprint = current.digest.clone();
        drop(current);
        self.project_source_shared(
            "__retrieved__.txt",
            text,
            &opaque_policy,
            Some(&fingerprint),
            false,
        )
        .map(|p| p.text.clone())
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
        self.mask_known_shared(text)
            .map(|replacement| replacement.as_deref().unwrap_or(text).to_owned())
    }

    /// None is an unchanged, fully checked string. Shared immutable replacements
    /// let validation inspect an existing proof without allocating another full
    /// history string merely to compare it with its input.
    fn mask_known_shared(&self, text: &str) -> Result<Option<Arc<str>>, String> {
        let content = self.mask_content_known_shared(text)?;
        let input = content.as_deref().unwrap_or(text);
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(ticket) = state
            .safe_template_hashes
            .get(&(self.policy_fingerprint(), digest(input.as_bytes())))
            .filter(|ticket| ticket.valid())
        {
            state.registry.tokens_in(input)?;
            ticket.touch();
            return Ok(content);
        }
        if let Some(names) = &state.filenames {
            let protected = state.registry.tokens_in(input)?;
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

    pub(crate) fn restore_text(&self, text: &str) -> Result<String, String> {
        self.restore_with_spans(text).map(|(text, _)| text)
    }

    pub(crate) fn restore_with_spans(
        &self,
        text: &str,
    ) -> Result<(String, Vec<crate::retrieved_content::RetrievedSpan>), String> {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let tokens = state.registry.tokens_in(text)?;
        let mut result = String::new();
        let mut spans = Vec::new();
        let mut offset = 0;
        for token in tokens {
            result.push_str(&text[offset..token.start]);
            let original = state.registry.restore_exact(&text[token.clone()])?;
            let start = result.len();
            result.push_str(original);
            if result.len() > start {
                spans.push(crate::retrieved_content::RetrievedSpan {
                    source: None,
                    start,
                    end: result.len(),
                });
            }
            offset = token.end;
            let preserved = original.bytes().filter(|b| *b == b'\n').count();
            let available = text[offset..].bytes().take_while(|b| *b == b'\n').count();
            if preserved > 0 && available >= preserved {
                offset += preserved;
            }
        }
        result.push_str(&text[offset..]);
        if let Some(names) = &state.filenames {
            if !names.contains_aliases(&result) {
                return Ok((result, spans));
            }
            let restored = names.restore_text(&result)?;
            if restored != result {
                spans = crate::retrieved_content::full_span(&restored);
                result = restored;
            }
        }
        Ok((result, spans))
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

    pub(crate) fn commit_registry(&self) -> Result<(), String> {
        let _commit = self.commit_gate.lock().unwrap_or_else(|p| p.into_inner());
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
        let mut reference = state.registry.stage(&self.scope)?;
        reference.optional_policy = self.stored_policy() == Policy::default();
        reference.task_context = self
            .task_context
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if state.reference.as_ref() == Some(&reference) {
            return Ok(());
        }
        let mut session = owner
            .store
            .load_owned(&self.session_id.to_string(), &owner.lease)
            .map_err(|e| format!("Privacy save blocked: {e}"))?;
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
        self.discover_filename_grant_requests(request)?;
        self.refresh_filenames()?;
        request
            .tools
            .retain(|tool| self.check_filename_tool(&tool.function.name).is_ok());
        if self.filename_mode {
            // Only the provider copy receives contextual paths. Local listings
            // retain their ordinary basename presentation.
            let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let names = state
                .filenames
                .as_ref()
                .ok_or("Filename mappings are unavailable")?;
            let mut listing_roots = HashMap::new();
            for call in request
                .messages
                .iter()
                .flat_map(|message| message.tool_calls.iter().flatten())
            {
                if call.function.name != "list_dir" {
                    continue;
                }
                let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                    .map_err(|_| "Invalid listing arguments")?;
                let path = args["path"].as_str().unwrap_or(".");
                let root = if names.contains_aliases(path) {
                    names.resolve_path(path)?
                } else if Path::new(path).is_absolute() {
                    PathBuf::from(path)
                } else {
                    self.workspace.join(path)
                };
                if !names.authorized_path(&root) {
                    return Err("Listing context needs a current folder grant".into());
                }
                listing_roots.insert(call.id.clone(), root);
            }
            for message in &mut request.messages {
                let Some(root) = message
                    .tool_call_id
                    .as_ref()
                    .and_then(|id| listing_roots.get(id))
                else {
                    continue;
                };
                let Some(text) = message.content.as_str() else {
                    return Err("Unsupported directory result encoding".into());
                };
                if text.starts_with("Error") || text.starts_with("[tool failed]\n") {
                    continue;
                }
                let mut rows = Vec::new();
                for row in text.lines() {
                    if row == "[Directory listing limited by entry, output or deadline budget]" {
                        rows.push(row.into());
                        continue;
                    }
                    if names.is_known_path_alias(row) {
                        rows.push(row.into());
                        continue;
                    }
                    let child = root.join(row.strip_suffix('/').unwrap_or(row));
                    rows.push(
                        names
                            .project_path(child.to_str().ok_or("Unsupported listing filename")?)?,
                    );
                }
                message.content = crate::image_content::ModelContent::text(rows.join("\n"));
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
        for message in &mut request.messages {
            let authored_text = message.role != "tool"
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
            if let Some(text) = message.content.as_text_mut() {
                if let Some(replacement) = self.mask_known_shared(text)? {
                    *text = replacement.as_ref().to_owned();
                }
            } else {
                message.content = message.content.mask_privacy(self)?;
            }
            for call in message.tool_calls.as_mut().into_iter().flatten() {
                check_tool_name(self, &call.function.name)?;
                let mut value: serde_json::Value =
                    serde_json::from_str(&call.function.arguments)
                        .map_err(|_| "Privacy tool arguments are not valid JSON")?;
                arguments::project(self, &call.function.name, &mut value)?;
                call.function.arguments = value.to_string();
            }
        }
        for tool in &mut request.tools {
            check_tool_name(self, &tool.function.name)?;
            tool.function.description = self.mask_known(&tool.function.description)?;
            if self.filename_mode {
                tool.function.description.push_str(" Use complete discovered filename references for path arguments; '.' lists the workspace. Source link references can be opened directly. To create a file or directory, append one new plain leaf beneath a discovered directory reference and re-list after creation. For select_files folder/destination and move_files destination, use canonical directory references or host-issued batch folder references; select by public extensions to preserve hidden filenames. For HTML/CSS/JS links, insert a complete source-link or canonical file reference directly; never strip its components or prepend ../ or ./ to it. Ca restores the appropriate relative URL. Retain source references in edits. Registered built-ins use local privacy contracts. Commands run in a disposable writable project copy; original sources remain unchanged. Command artifacts have private references. Unknown or evaluated filename expressions and custom tools are unavailable.");
            }

            project_schema_text(self, &mut tool.function.parameters)?;
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
        fn check<'a>(
            context: &Context,
            value: &'a serde_json::Value,
            path: &mut Vec<&'a str>,
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
                        return check_tool_name(context, text);
                    }
                    if context.mask_known_shared(text)?.is_some() {
                        return Err("Private content remained in an immutable protocol field; use /filter continue for a fresh privacy-safe continuation".into());
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        path.push("*");
                        check(context, item, path)?;
                        path.pop();
                    }
                }
                serde_json::Value::Object(items) => {
                    let tool = wire_tool_location(path, items);
                    for (key, item) in items {
                        if tool && key == "name" {
                            check_tool_name(
                                context,
                                item.as_str().ok_or("Final tool name is not text")?,
                            )?;
                        } else if tool && key == "arguments" && item.is_string() {
                            let value: serde_json::Value =
                                serde_json::from_str(item.as_str().unwrap_or_default())
                                    .map_err(|_| "Final tool arguments are not valid JSON")?;
                            arguments::validate(
                                context,
                                items.get("name").and_then(serde_json::Value::as_str),
                                &value,
                            )?;
                        } else if tool
                            && matches!(key.as_str(), "input" | "args")
                            && item.is_object()
                        {
                            arguments::validate(
                                context,
                                items.get("name").and_then(serde_json::Value::as_str),
                                item,
                            )?;
                        } else if (tool && matches!(key.as_str(), "parameters" | "input_schema"))
                            || (matches!(
                                path.as_slice(),
                                ["generationConfig"] | ["generation_config"]
                            ) && matches!(key.as_str(), "responseSchema" | "response_schema"))
                        {
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
                        } else if !wire_control_field(path, key, item, items) {
                            path.push(key);
                            check(context, item, path)?;
                            path.pop();
                        }
                    }
                }
                _ => {}
            }
            Ok(())
        }
        check(self, &value, &mut Vec::new())?;
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

// Only real protocol envelopes grant control-field exemptions. Data nested in
// tool arguments/results cannot turn a property called `name` into authority.
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
        }
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
    ) -> Result<(), String> {
        match value {
            serde_json::Value::String(text) => {
                if !public.contains(text) && context.mask_known(text)? != *text {
                    return Err("A private value is required by an immutable tool schema; adjust its local policy before sending".into());
                }
            }
            serde_json::Value::Object(items) => {
                for (key, value) in items {
                    if !public.contains(key) && context.mask_known(key)? != *key {
                        return Err("A private field name is required by an immutable tool schema; adjust its local policy before sending".into());
                    }
                    inspect(context, value, public)?;
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    inspect(context, item, public)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut public = std::collections::BTreeSet::new();
    if let Some(spec) =
        name.and_then(|name| crate::agent::registry::ToolRegistry::builtins().get(name))
    {
        // Only text already published by the registered native schema is public.
        // A caller cannot add private keys by borrowing a builtin tool's name.
        collect(&spec.schema, &mut public);
    }
    inspect(context, value, &public)
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
