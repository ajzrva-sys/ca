//! Durable execution services. The host admits runs and supplies all authority.
use anyhow::{bail, Context, Result};
pub use ca_automation_protocol::execution::*;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::{
    fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    io::{AsRawFd, FromRawFd},
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

/// Advisory ownership lasts until the file descriptor closes, never until a TTL.
#[derive(Debug)]
pub struct ResourceLease {
    file: File,
}
impl ResourceLease {
    pub fn acquire(root: &Path, resource: &str) -> Result<Self> {
        private_dir(root)?;
        let name = format!("{:x}.lock", Sha256::digest(resource.as_bytes()));
        let file = owner_open(&root.join(name), true)?;
        #[cfg(unix)]
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(std::io::Error::last_os_error()).context("automation resource is busy");
        }
        #[cfg(not(unix))]
        bail!("durable resource ownership is unavailable on this platform");
        Ok(Self { file })
    }

    /// Prove this retained claim owns the exact private resource binding.
    pub fn validate_binding(&self, root: &Path, resource: &str) -> Result<()> {
        private_dir(root)?;
        let name = format!("{:x}.lock", Sha256::digest(resource.as_bytes()));
        let expected = owner_open(&root.join(name), false)?;
        let metadata = self.file.metadata()?;
        let expected_metadata = expected.metadata()?;
        #[cfg(unix)]
        {
            if !metadata.is_file()
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.mode() & 0o077 != 0
                || metadata.nlink() != 1
                || metadata.dev() != expected_metadata.dev()
                || metadata.ino() != expected_metadata.ino()
            {
                bail!("automation resource ownership identity changed");
            }
            if unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(std::io::Error::last_os_error())
                    .context("automation resource claim is not held");
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = (metadata, expected_metadata);
            bail!("durable resource ownership is unavailable on this platform")
        }
    }

    /// The host may clear CLOEXEC for this descriptor in its owned worker's
    /// pre-exec hook. Closing the host copy must not unlock inherited copies.
    #[cfg(unix)]
    pub fn raw_fd(&self) -> i32 {
        self.file.as_raw_fd()
    }
}

// flock ownership is released by closing the last inherited open description.
// An explicit LOCK_UN here would also release a live worker's inherited lease.
type AdoptedResourceBinding = (PathBuf, std::collections::BTreeSet<String>);
static ADOPTED_RESOURCES: std::sync::OnceLock<std::sync::Mutex<Option<AdoptedResourceBinding>>> =
    std::sync::OnceLock::new();

/// Validate and retain the leases inherited from a ca-owned worker's parent.
/// Descriptors correspond to the context's sorted, deduplicated resource keys.
/// Call this before starting execution; dropping the returned handles does not
/// release the original inherited descriptors while the worker is alive.
pub fn adopt_resource_leases(
    context: &ExecutionContext,
    fds: &[i32],
) -> Result<Vec<ResourceLease>> {
    #[cfg(unix)]
    {
        context.validate().map_err(anyhow::Error::msg)?;
        private_dir(&context.lease_root)?;
        let resources: std::collections::BTreeSet<_> = context.resources.iter().cloned().collect();
        if resources.len() != fds.len()
            || fds.iter().any(|fd| *fd < 3)
            || fds.iter().collect::<std::collections::BTreeSet<_>>().len() != fds.len()
        {
            bail!("inherited automation resource descriptors do not match the run");
        }
        let mut leases = Vec::with_capacity(fds.len());
        for (resource, fd) in resources.iter().zip(fds) {
            let name = format!("{:x}.lock", Sha256::digest(resource.as_bytes()));
            leases.push(adopt_lease_file(&context.lease_root.join(name), *fd)?);
        }
        let root = context.lease_root.canonicalize()?;
        let mut adopted = ADOPTED_RESOURCES
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
            .map_err(|_| anyhow::anyhow!("inherited resource registry is unavailable"))?;
        if adopted
            .as_ref()
            .is_some_and(|binding| binding != &(root.clone(), resources.clone()))
        {
            bail!("worker cannot adopt a different automation resource binding");
        }
        *adopted = Some((root, resources));
        Ok(leases)
    }
    #[cfg(not(unix))]
    {
        let _ = (context, fds);
        bail!("inherited automation resource ownership is unavailable on this platform")
    }
}

/// Retain the inherited executor claim so a dead dispatching parent is not
/// mistaken for an abandoned run while its owned worker is still running.
pub fn adopt_execution_lease(context: &ExecutionContext, fd: i32) -> Result<ResourceLease> {
    #[cfg(unix)]
    {
        let saved = RunStore::load_context(&context.state_root, &context.run_id)?;
        if saved != *context {
            bail!("inherited execution authority changed");
        }
        let root = context.state_root.join("runs").join(&context.run_id);
        private_dir(&root)?;
        let name = format!("{:x}.lock", Sha256::digest(b"executor"));
        adopt_lease_file(&root.join(name), fd)
    }
    #[cfg(not(unix))]
    {
        let _ = (context, fd);
        bail!("inherited executor ownership is unavailable on this platform")
    }
}

#[cfg(unix)]
fn adopt_lease_file(path: &Path, fd: i32) -> Result<ResourceLease> {
    if fd < 3 {
        bail!("inherited automation descriptor is invalid");
    }
    let expected = owner_open(path, false)?;
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
    if duplicate < 0 {
        return Err(std::io::Error::last_os_error())
            .context("duplicate inherited automation ownership");
    }
    let file = unsafe { File::from_raw_fd(duplicate) };
    let metadata = file.metadata()?;
    let expected_metadata = expected.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
        || metadata.dev() != expected_metadata.dev()
        || metadata.ino() != expected_metadata.ino()
    {
        bail!("inherited automation ownership identity changed");
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(std::io::Error::last_os_error())
            .context("inherited automation ownership is unavailable");
    }
    // The trusted worker retains both descriptions for liveness, but ordinary
    // physical commands must not receive its inherited authority. A selected
    // trusted nested worker may explicitly clear CLOEXEC in its pre-exec hook.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(std::io::Error::last_os_error())
            .context("contain inherited automation ownership descriptor");
    }
    Ok(ResourceLease { file })
}

#[derive(Debug, Clone)]
pub struct RunStore {
    root: PathBuf,
    context: ExecutionContext,
    journal_gate: Arc<Mutex<()>>,
}

type JournalGates = std::collections::BTreeMap<PathBuf, Weak<Mutex<()>>>;
static JOURNAL_GATES: OnceLock<Mutex<JournalGates>> = OnceLock::new();

fn journal_gate(root: &Path) -> Result<Arc<Mutex<()>>> {
    let key = root.canonicalize()?;
    let mut gates = JOURNAL_GATES
        .get_or_init(|| Mutex::new(JournalGates::new()))
        .lock()
        .map_err(|_| anyhow::anyhow!("automation journal serialization is unavailable"))?;
    gates.retain(|_, gate| gate.strong_count() != 0);
    if let Some(gate) = gates.get(&key).and_then(Weak::upgrade) {
        return Ok(gate);
    }
    let gate = Arc::new(Mutex::new(()));
    gates.insert(key, Arc::downgrade(&gate));
    Ok(gate)
}

struct JournalLease<'a> {
    // Close the OS description before releasing local serialization.
    _file: ResourceLease,
    _local: MutexGuard<'a, ()>,
}
tokio::task_local! { pub static EXECUTION_STORE: RunStore; }
pub fn current_run_store() -> Option<RunStore> {
    EXECUTION_STORE.try_with(Clone::clone).ok()
}

static WORKER_CONTAINMENT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Called only by a ca-owned worker after its parent established the group.
/// Typed child commands inherit this group for parent-owned descendant cleanup.
pub fn enable_worker_containment() -> Result<()> {
    #[cfg(unix)]
    {
        if unsafe { libc::getpgrp() } != unsafe { libc::getpid() } {
            bail!("native worker must own its containment process group");
        }
        WORKER_CONTAINMENT.store(true, std::sync::atomic::Ordering::Release);
    }
    #[cfg(not(unix))]
    bail!("native worker process containment is unavailable");
    Ok(())
}

/// A nested ca-owned worker may join its admitted parent's cleanup group after
/// adopting the parent's exact resource descriptors.
pub fn enable_inherited_worker_containment(expected_group: i32) -> Result<()> {
    #[cfg(unix)]
    {
        if expected_group <= 0 || unsafe { libc::getpgrp() } != expected_group {
            bail!("nested native worker containment group changed");
        }
        if !ADOPTED_RESOURCES
            .get()
            .and_then(|registry| registry.lock().ok())
            .is_some_and(|binding| binding.is_some())
        {
            bail!("nested native worker has not adopted its resource authority");
        }
        WORKER_CONTAINMENT.store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = expected_group;
        bail!("nested native worker containment is unavailable on this platform")
    }
}

pub(crate) fn worker_contained() -> bool {
    WORKER_CONTAINMENT.load(std::sync::atomic::Ordering::Acquire)
}
pub fn machine_lease_root() -> Result<PathBuf> {
    #[cfg(test)]
    {
        Ok(std::fs::canonicalize(std::env::temp_dir())?.join(format!(
            "ca-automation-test-resources-{}",
            std::process::id()
        )))
    }
    #[cfg(not(test))]
    {
        let home = PathBuf::from(
            std::env::var_os("HOME").context("HOME is unavailable for automation resources")?,
        )
        .canonicalize()?;
        Ok(home.join(".local/share/ca/automation-resources/v2"))
    }
}

/// Drop resource lock files nothing is holding. Every resource key ever leased
/// leaves an empty flock target behind; without this the lease root grows one
/// file per distinct key for the lifetime of the install. Held locks are kept
/// because they are the flock target a live lease still validates against.
/// Returns how many lock files were removed.
pub fn sweep_unheld_resource_locks(root: &Path) -> Result<usize> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    let mut removed = 0;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("lock") {
            continue;
        }
        if !lock_is_unheld(&path) {
            continue;
        }
        let _ = fs::remove_file(&path);
        removed += 1;
    }
    Ok(removed)
}

/// True when the lock file exists and nothing currently holds its flock.
#[cfg(unix)]
fn lock_is_unheld(path: &Path) -> bool {
    use std::os::fd::AsRawFd;
    let Ok(file) = owner_open(path, false) else {
        return false;
    };
    let held = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    held == 0
}

#[cfg(not(unix))]
fn lock_is_unheld(_path: &Path) -> bool {
    // Without flock there is no way to distinguish a live claim from a leftover.
    false
}
impl RunStore {
    pub fn context(&self) -> &ExecutionContext {
        &self.context
    }

    /// Publication inside an admitted worker must name a resource present in
    /// both its frozen context and the descriptors proven at worker startup.
    pub fn require_adopted_resource(&self, resource: &str) -> Result<()> {
        if !worker_contained() || !self.context.resources.iter().any(|key| key == resource) {
            bail!("automation resource was not admitted for this worker");
        }
        let root = self.context.lease_root.canonicalize()?;
        let adopted = ADOPTED_RESOURCES
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
            .map_err(|_| anyhow::anyhow!("inherited resource registry is unavailable"))?;
        if !adopted.as_ref().is_some_and(|(adopted_root, resources)| {
            *adopted_root == root && resources.contains(resource)
        }) {
            bail!("worker has not adopted the admitted automation resource");
        }
        Ok(())
    }

    pub fn open(context: &ExecutionContext) -> Result<Self> {
        context.validate().map_err(anyhow::Error::msg)?;
        private_dir(&context.state_root)?;
        let root = context.state_root.join("runs").join(&context.run_id);
        private_dir(&root)?;
        let store = Self {
            journal_gate: journal_gate(&root)?,
            root,
            context: context.clone(),
        };
        let _lease = store.lock()?;
        let envelope = store.root.join("context.json");
        if envelope.exists() {
            let saved: ExecutionContext = read(&envelope, 256 * 1024)?;
            if saved != *context {
                bail!("run identity is already bound to different authority");
            }
        } else {
            atomic_write(&envelope, context)?;
            store.write(&ExecutionReceipt {
                schema: RECEIPT_SCHEMA.into(),
                run_id: context.run_id.clone(),
                definition_digest: context.definition_digest.clone(),
                resource_binding_digest: context.resource_binding_digest.clone(),
                policy_digest: context.policy_digest.clone(),
                state: ExecutionState::Prepared,
                actions: Vec::new(),
                result: None,
                reason: None,
            })?;
        }
        drop(_lease);
        Ok(store)
    }
    fn lock(&self) -> Result<JournalLease<'_>> {
        // Local writers queue rather than racing nonblocking flock in bursts.
        // The OS lock still protects independent processes and inherited fds.
        let local = self
            .journal_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("automation journal serialization is unavailable"))?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            match ResourceLease::acquire(&self.root, "journal") {
                Ok(lease) => {
                    return Ok(JournalLease {
                        _file: lease,
                        _local: local,
                    })
                }
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
                        && std::time::Instant::now() < deadline =>
                {
                    // Serialize short journal transactions, including a fork's
                    // pre-exec descriptor window. Never expire or steal a lock.
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Err(error) => return Err(error),
            }
        }
    }
    pub fn execution_lease(&self) -> Result<ResourceLease> {
        ResourceLease::acquire(&self.root, "executor")
    }
    pub fn acquire_resources(&self) -> Result<Vec<ResourceLease>> {
        if worker_contained() {
            let resources: std::collections::BTreeSet<_> =
                self.context.resources.iter().cloned().collect();
            let adopted = ADOPTED_RESOURCES
                .get_or_init(|| std::sync::Mutex::new(None))
                .lock()
                .map_err(|_| anyhow::anyhow!("inherited resource registry is unavailable"))?;
            let root = self.context.lease_root.canonicalize()?;
            if !adopted
                .as_ref()
                .is_some_and(|(adopted_root, adopted_keys)| {
                    *adopted_root == root && resources.is_subset(adopted_keys)
                })
            {
                bail!("contained worker has not adopted its automation resources");
            }
            return Ok(Vec::new());
        }
        let mut resources = self.context.resources.clone();
        resources.sort();
        resources.dedup();
        resources
            .iter()
            .map(|r| ResourceLease::acquire(&self.context.lease_root, r))
            .collect()
    }
    pub fn status(&self) -> Result<ExecutionReceipt> {
        let receipt: ExecutionReceipt = read(
            &self.root.join("receipt.json"),
            self.context.output_bytes + 256 * 1024,
        )?;
        if receipt.schema != RECEIPT_SCHEMA
            || receipt.run_id != self.context.run_id
            || receipt.definition_digest != self.context.definition_digest
            || receipt.resource_binding_digest != self.context.resource_binding_digest
            || receipt.policy_digest != self.context.policy_digest
        {
            bail!("automation execution receipt identity changed");
        }
        Ok(receipt)
    }
    fn write(&self, receipt: &ExecutionReceipt) -> Result<()> {
        if serde_json::to_vec(receipt)?.len() as u64 > self.context.output_bytes + 256 * 1024 {
            bail!("aggregate automation receipt exceeds admitted output");
        }
        atomic_write(&self.root.join("receipt.json"), receipt)
    }
    pub fn start(&self) -> Result<ExecutionReceipt> {
        let _lock = self.lock()?;
        let mut r = self.status()?;
        if r.state == ExecutionState::Prepared {
            r.state = ExecutionState::Running;
            self.write(&r)?;
        }
        Ok(r)
    }
    pub fn prepare_action(&self, id: &str) -> Result<()> {
        if !valid_id(id) {
            bail!("invalid action identity");
        }
        let _lock = self.lock()?;
        let mut r = self.status()?;
        if r.state.terminal() {
            bail!("terminal execution cannot prepare an action");
        }
        if !r.actions.iter().any(|a| a.id == id) {
            if r.actions.len() >= 128 {
                bail!("too many automation actions");
            }
            r.actions.push(ActionReceipt {
                id: id.into(),
                state: ActionState::Prepared,
                result: None,
            });
            self.write(&r)?;
        }
        Ok(())
    }
    pub fn begin_action(&self, id: &str) -> Result<bool> {
        let _lock = self.lock()?;
        let mut r = self.status()?;
        if self.cancel_requested()? {
            bail!("automation cancellation requested");
        }
        if r.state != ExecutionState::Running {
            bail!("execution is not running");
        }
        let action = r
            .actions
            .iter_mut()
            .find(|a| a.id == id)
            .context("action was not prepared")?;
        match action.state {
            ActionState::Confirmed => return Ok(false),
            ActionState::Prepared => action.state = ActionState::Running,
            _ => bail!("action has an unconfirmed effect; reconcile before execution"),
        }
        self.write(&r)?;
        Ok(true)
    }
    pub fn confirm_action(&self, id: &str, result: Value) -> Result<()> {
        if serde_json::to_vec(&result)?.len() as u64 > self.context.output_bytes {
            bail!("action result exceeds admitted output");
        }
        let _lock = self.lock()?;
        let mut r = self.status()?;
        let action = r
            .actions
            .iter_mut()
            .find(|a| a.id == id)
            .context("unknown action")?;
        if action.state != ActionState::Running {
            bail!("action is not owned by this execution");
        }
        action.state = ActionState::Confirmed;
        action.result = Some(result);
        self.write(&r)
    }
    /// Adapter read-back proves a running/unknown effect's outcome. No replay here.
    pub fn reconcile_action(
        &self,
        id: &str,
        confirmed: Option<Value>,
        proven_absent: bool,
    ) -> Result<()> {
        let _lock = self.lock()?;
        let mut r = self.status()?;
        let action = r
            .actions
            .iter_mut()
            .find(|a| a.id == id)
            .context("unknown action")?;
        if !matches!(action.state, ActionState::Unknown | ActionState::Running) {
            bail!("action does not need reconciliation");
        }
        if let Some(result) = confirmed {
            action.state = ActionState::Confirmed;
            action.result = Some(result);
        } else if proven_absent {
            action.state = ActionState::Prepared;
            action.result = None;
        } else {
            bail!("adapter has not established the effect outcome");
        }
        r.state = ExecutionState::Running;
        r.reason = None;
        self.write(&r)
    }
    pub fn finish(
        &self,
        state: ExecutionState,
        result: Option<Value>,
        reason: Option<String>,
    ) -> Result<ExecutionReceipt> {
        if !state.terminal() {
            bail!("finish requires a terminal state");
        }
        if result.as_ref().is_some_and(|v| {
            serde_json::to_vec(v).map_or(true, |b| b.len() as u64 > self.context.output_bytes)
        }) {
            bail!("execution result exceeds admitted output");
        }
        let _lock = self.lock()?;
        let mut r = self.status()?;
        if r.state.terminal() {
            return Ok(r);
        }
        r.state = state;
        r.result = result;
        r.reason = reason.map(|s| s.chars().take(2048).collect());
        clear_duplicate_target(&mut r);
        self.write(&r)?;
        Ok(r)
    }
    pub fn cancel(&self) -> Result<()> {
        atomic_write(&self.root.join("cancel.json"), &true)
    }
    pub fn cancel_requested(&self) -> Result<bool> {
        let path = self.root.join("cancel.json");
        if !path.exists() {
            return Ok(false);
        }
        read(&path, 16)
    }
    pub fn reconcile(&self) -> Result<ExecutionReceipt> {
        let Ok(_executor) = self.execution_lease() else {
            return self.status();
        };
        let _lock = self.lock()?;
        let mut r = self.status()?;
        if r.state.terminal() {
            return Ok(r);
        }
        for action in &mut r.actions {
            if action.state == ActionState::Running {
                action.state = ActionState::Unknown;
            }
        }
        if r.actions.iter().any(|a| a.state == ActionState::Unknown) {
            r.state = ExecutionState::NeedsAttention;
            r.reason = Some("interrupted action needs adapter read-back before retry".into());
        } else if self.cancel_requested()? {
            r.state = ExecutionState::Cancelled;
        } else if r.state == ExecutionState::Running {
            // The target receipt captures the completed run even when it also
            // contains per-action checkpoints and the parent lost stdout.
            if let Some(target) = r
                .actions
                .iter()
                .find(|action| action.id == "target" && action.state == ActionState::Confirmed)
            {
                let value = target
                    .result
                    .clone()
                    .context("confirmed target has no receipt")?;
                if let Some(state) = value.get("execution_state") {
                    let state: ExecutionState = serde_json::from_value(state.clone())?;
                    if !state.terminal() {
                        bail!("confirmed target outcome is not terminal");
                    }
                    r.state = state;
                    r.result = value.get("result").cloned();
                } else {
                    r.state = ExecutionState::Succeeded;
                    r.result = Some(value);
                }
            } else {
                r.state = ExecutionState::NeedsAttention;
                r.reason = Some("interrupted execution requires explicit continuation".into());
            }
        }
        if r.state.terminal() {
            clear_duplicate_target(&mut r);
        }
        self.write(&r)?;
        Ok(r)
    }
    pub fn load_context(state_root: &Path, run_id: &str) -> Result<ExecutionContext> {
        if !valid_id(run_id) {
            bail!("invalid run identity");
        }
        let ctx: ExecutionContext = read(
            &state_root.join("runs").join(run_id).join("context.json"),
            256 * 1024,
        )?;
        ctx.validate().map_err(anyhow::Error::msg)?;
        if ctx.state_root != state_root || ctx.run_id != run_id {
            bail!("stored execution context changed");
        }
        Ok(ctx)
    }
}

pub fn private_dir(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!("private automation path must be absolute");
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            bail!("private path traversal");
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
            Ok(_) => bail!("private automation path contains a non-directory or symlink"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(&current) {
                Ok(()) => {
                    #[cfg(unix)]
                    {
                        let directory = OpenOptions::new()
                            .read(true)
                            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                            .open(&current)?;
                        if directory.metadata()?.uid() != unsafe { libc::geteuid() } {
                            bail!("created automation directory ownership changed");
                        }
                        directory.set_permissions(fs::Permissions::from_mode(0o700))?;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let metadata = fs::symlink_metadata(&current)?;
                    if !metadata.is_dir() || metadata.file_type().is_symlink() {
                        bail!(
                            "concurrent automation directory contains a symlink or non-directory"
                        );
                    }
                }
                Err(error) => return Err(error.into()),
            },
            Err(e) => return Err(e.into()),
        }
    }
    #[cfg(unix)]
    {
        let m = fs::metadata(path)?;
        if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
            bail!("automation state directory must be private and owner-controlled");
        }
    }
    Ok(())
}
fn owner_open(path: &Path, write: bool) -> Result<File> {
    let mut o = OpenOptions::new();
    o.read(true);
    if write {
        o.write(true).create(true);
    }
    #[cfg(unix)]
    {
        o.mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let f = o.open(path)?;
    let m = f.metadata()?;
    if !m.is_file() {
        bail!("automation state must be a regular file");
    }
    #[cfg(unix)]
    if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 || m.nlink() != 1 {
        bail!("automation state file is not owner-only and single-link");
    }
    Ok(f)
}
pub fn read<T: DeserializeOwned>(path: &Path, max: u64) -> Result<T> {
    let f = owner_open(path, false)?;
    if f.metadata()?.len() > max {
        bail!("automation state exceeds its bound");
    }
    let mut bytes = Vec::new();
    f.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        bail!("automation state exceeds its bound");
    }
    Ok(serde_json::from_slice(&bytes)?)
}
pub fn atomic_write(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("state path has no parent")?;
    private_dir(parent)?;
    let bytes = serde_json::to_vec(value)?;
    let temp = parent.join(format!(
        ".write-{}-{}",
        std::process::id(),
        TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    opts.mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let result = (|| {
        let mut file = opts.open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
static TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn clear_duplicate_target(receipt: &mut ExecutionReceipt) {
    if let Some(result) = &receipt.result {
        if let Some(target) = receipt
            .actions
            .iter_mut()
            .find(|action| action.id == "target")
        {
            if target
                .result
                .as_ref()
                .is_some_and(|value| value == result || value.get("result") == Some(result))
            {
                target.result = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context(t: &tempfile::TempDir, id: &str) -> ExecutionContext {
        ExecutionContext {
            schema: EXECUTION_SCHEMA.into(),
            run_id: id.into(),
            profile: "default".into(),
            state_root: t.path().canonicalize().unwrap().join("state"),
            lease_root: t.path().canonicalize().unwrap().join("leases"),
            definition_digest: "a".repeat(64),
            resource_binding_digest: "b".repeat(64),
            policy_digest: "c".repeat(64),
            resources: vec!["shared-target".into()],
            runtime_seconds: 60,
            output_bytes: 4096,
        }
    }
    #[test]
    fn confirmed_completion_survives_lost_parent_and_is_not_dispatched_twice() -> Result<()> {
        let t = tempfile::tempdir()?;
        let ctx = context(&t, "first");
        let s = RunStore::open(&ctx)?;
        s.start()?;
        s.prepare_action("target")?;
        assert!(s.begin_action("target")?);
        s.confirm_action("target", serde_json::json!({"effect":"complete"}))?;
        drop(s);
        let s = RunStore::open(&ctx)?;
        let r = s.reconcile()?;
        assert_eq!(r.state, ExecutionState::Succeeded);
        assert_eq!(r.result.unwrap()["effect"], "complete");
        assert_eq!(s.start()?.state, ExecutionState::Succeeded);
        assert_eq!(
            RunStore::open(&context(&t, "second"))?.status()?.state,
            ExecutionState::Prepared
        );
        Ok(())
    }
    #[test]
    fn unknown_effect_is_held_until_readback() -> Result<()> {
        let t = tempfile::tempdir()?;
        let s = RunStore::open(&context(&t, "unknown"))?;
        s.start()?;
        s.prepare_action("copy")?;
        s.begin_action("copy")?;
        assert_eq!(s.reconcile()?.state, ExecutionState::NeedsAttention);
        assert!(s.begin_action("copy").is_err());
        assert!(s.reconcile_action("copy", None, false).is_err());
        s.reconcile_action("copy", None, true)?;
        assert!(s.begin_action("copy")?);
        Ok(())
    }
    #[test]
    fn confirmed_target_reconciles_nested_actions_and_preserves_failed_outcome() -> Result<()> {
        let t = tempfile::tempdir()?;
        let s = RunStore::open(&context(&t, "nested"))?;
        s.start()?;
        for id in ["target", "workflow_query"] {
            s.prepare_action(id)?;
            s.begin_action(id)?;
        }
        s.confirm_action("workflow_query", serde_json::json!({"query":"complete"}))?;
        s.confirm_action(
            "target",
            serde_json::json!({"execution_state":"failed","result":{"status":"failed"}}),
        )?;
        let r = s.reconcile()?;
        assert_eq!(r.state, ExecutionState::Failed);
        assert_eq!(r.result.unwrap()["status"], "failed");
        Ok(())
    }
    #[test]
    fn aggregate_receipt_cannot_exceed_the_admitted_output_bound() -> Result<()> {
        let t = tempfile::tempdir()?;
        let mut ctx = context(&t, "bounded");
        ctx.output_bytes = 8 * 1024 * 1024;
        let s = RunStore::open(&ctx)?;
        s.start()?;
        let payload = serde_json::json!({"output":"x".repeat(4*1024*1024)});
        for id in ["first", "second", "third"] {
            s.prepare_action(id)?;
            s.begin_action(id)?;
        }
        s.confirm_action("first", payload.clone())?;
        s.confirm_action("second", payload.clone())?;
        assert!(s.confirm_action("third", payload).is_err());
        assert_eq!(s.status()?.actions[2].state, ActionState::Running);
        Ok(())
    }
    #[test]
    fn complete_output_near_its_bound_is_stored_once_after_confirmation() -> Result<()> {
        let t = tempfile::tempdir()?;
        let mut ctx = context(&t, "large");
        ctx.output_bytes = 1024 * 1024;
        let s = RunStore::open(&ctx)?;
        s.start()?;
        s.prepare_action("target")?;
        s.begin_action("target")?;
        let result = serde_json::json!({"output":"x".repeat(ctx.output_bytes as usize - 1024)});
        s.confirm_action(
            "target",
            serde_json::json!({"execution_state":"succeeded","result":result}),
        )?;
        let receipt = s.finish(ExecutionState::Succeeded, Some(result.clone()), None)?;
        assert_eq!(receipt.result, Some(result));
        assert!(receipt.actions[0].result.is_none());
        assert!(serde_json::to_vec(&receipt)?.len() as u64 <= ctx.output_bytes + 256 * 1024);
        Ok(())
    }
    #[test]
    fn resource_ownership_spans_profiles_and_does_not_expire() -> Result<()> {
        let t = tempfile::tempdir()?;
        let lease = ResourceLease::acquire(&t.path().canonicalize()?.join("locks"), "repo/main")?;
        assert!(
            ResourceLease::acquire(&t.path().canonicalize()?.join("locks"), "repo/main").is_err()
        );
        assert!(
            ResourceLease::acquire(&t.path().canonicalize()?.join("locks"), "repo/other").is_ok()
        );
        drop(lease);
        assert!(
            ResourceLease::acquire(&t.path().canonicalize()?.join("locks"), "repo/main").is_ok()
        );
        Ok(())
    }
    #[test]
    fn retained_resource_proof_rejects_another_key_or_replaced_binding() -> Result<()> {
        let t = tempfile::tempdir()?;
        let root = t.path().canonicalize()?.join("resource-proof");
        let claim = ResourceLease::acquire(&root, "repository/main")?;
        let other = ResourceLease::acquire(&root, "repository/other")?;
        claim.validate_binding(&root, "repository/main")?;
        assert!(claim.validate_binding(&root, "repository/other").is_err());
        other.validate_binding(&root, "repository/other")?;
        let key = format!("{:x}.lock", Sha256::digest(b"repository/main"));
        fs::rename(root.join(&key), root.join("retained-old-binding"))?;
        let replacement = ResourceLease::acquire(&root, "repository/main")?;
        assert!(claim.validate_binding(&root, "repository/main").is_err());
        replacement.validate_binding(&root, "repository/main")?;
        Ok(())
    }
    #[test]
    fn publication_resource_requires_validated_worker_adoption() -> Result<()> {
        let t = tempfile::tempdir()?;
        let ctx = context(&t, "publication-proof");
        let store = RunStore::open(&ctx)?;
        assert!(store.require_adopted_resource(&ctx.resources[0]).is_err());
        assert!(store.require_adopted_resource("unadmitted").is_err());
        Ok(())
    }
    #[test]
    fn sweep_drops_unheld_resource_locks_and_keeps_live_claims() -> Result<()> {
        let t = tempfile::tempdir()?;
        let root = t.path().canonicalize()?.join("sweep-locks");
        let held = ResourceLease::acquire(&root, "repository/main")?;
        // A released lease whose lock file is still on disk, standing in for
        // every distinct key ever used across previous runs.
        drop(ResourceLease::acquire(&root, "repository/other")?);

        assert_eq!(sweep_unheld_resource_locks(&root)?, 1);

        let held_key = format!("{:x}.lock", Sha256::digest(b"repository/main"));
        let other_key = format!("{:x}.lock", Sha256::digest(b"repository/other"));
        assert!(root.join(&held_key).exists());
        assert!(!root.join(&other_key).exists());
        held.validate_binding(&root, "repository/main")?;

        // The swept key can be taken again, and a missing root is a no-op.
        drop(ResourceLease::acquire(&root, "repository/other")?);
        assert_eq!(sweep_unheld_resource_locks(&t.path().join("absent"))?, 0);
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn inherited_resource_survives_parent_drop_until_last_descendant_exits() -> Result<()> {
        use std::io::{BufRead, BufReader};
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};
        let t = tempfile::tempdir()?;
        let root = t.path().canonicalize()?.join("inherited-locks");
        let lease = ResourceLease::acquire(&root, "repository/main")?;
        let fd = lease.raw_fd();
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "sleep 30 & printf '%s\\n' \"$!\"; wait"])
            .stdout(Stdio::piped());
        unsafe {
            command.pre_exec(move || {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn()?;
        let mut descendant = String::new();
        BufReader::new(child.stdout.take().context("worker stdout missing")?)
            .read_line(&mut descendant)?;
        let descendant_pid: i32 = descendant.trim().parse()?;
        drop(lease);
        let held_by_worker = ResourceLease::acquire(&root, "repository/main").is_err();
        child.kill()?;
        child.wait()?;
        let held_by_descendant = ResourceLease::acquire(&root, "repository/main").is_err();
        unsafe { libc::kill(descendant_pid, libc::SIGKILL) };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let released = loop {
            if ResourceLease::acquire(&root, "repository/main").is_ok() {
                break true;
            }
            if std::time::Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert!(
            held_by_worker,
            "host close released the live worker's resource"
        );
        assert!(
            held_by_descendant,
            "worker exit released its live descendant's resource"
        );
        assert!(released, "the last descendant did not release its resource");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn adoption_refuses_an_unrelated_descriptor() -> Result<()> {
        let t = tempfile::tempdir()?;
        let ctx = context(&t, "wrong-descriptor");
        let expected = ResourceLease::acquire(&ctx.lease_root, &ctx.resources[0])?;
        let unrelated = ResourceLease::acquire(&ctx.lease_root, "different-resource")?;
        assert!(adopt_resource_leases(&ctx, &[unrelated.raw_fd()]).is_err());
        assert!(ResourceLease::acquire(&ctx.lease_root, &ctx.resources[0]).is_err());
        drop(expected);
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn adopted_executor_retains_liveness_after_dispatcher_closes_its_copy() -> Result<()> {
        let t = tempfile::tempdir()?;
        let ctx = context(&t, "adopted-executor");
        let store = RunStore::open(&ctx)?;
        let original = store.execution_lease()?;
        let adopted = adopt_execution_lease(&ctx, original.raw_fd())?;
        let unrelated = ResourceLease::acquire(&ctx.lease_root, "not-executor")?;
        assert!(adopt_execution_lease(&ctx, unrelated.raw_fd()).is_err());
        drop(original);
        assert!(store.execution_lease().is_err());
        drop(adopted);
        assert!(store.execution_lease().is_ok());
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn adopted_authority_is_not_inherited_by_ordinary_physical_commands() -> Result<()> {
        const FIXTURE: &str = "CA_AUTOMATION_ADOPTED_AUTHORITY_FIXTURE";
        if std::env::var_os(FIXTURE).is_none() {
            // This fixture deliberately opens an inheritance window before
            // adoption restores CLOEXEC. Other tests must not fork through
            // that window and retain its unrelated execution claim.
            let output = std::process::Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "runtime::tests::adopted_authority_is_not_inherited_by_ordinary_physical_commands",
                    "--nocapture",
                ])
                .env(FIXTURE, "1")
                .output()?;
            assert!(
                output.status.success(),
                "isolated adopted authority fixture failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return Ok(());
        }
        let t = tempfile::tempdir()?;
        let ctx = context(&t, "adopted-private-descriptor");
        let store = RunStore::open(&ctx)?;
        let original = store.execution_lease()?;
        let fd = original.raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0);
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) },
            0
        );
        let adopted = adopt_execution_lease(&ctx, fd)?;
        assert_ne!(
            unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        let child = std::process::Command::new("/bin/sh")
            .env_clear()
            .args(["-c", &format!("if [ -e /dev/fd/{fd} ]; then exit 9; fi")])
            .status()?;
        assert!(
            child.success(),
            "physical command inherited execution authority"
        );
        drop(original);
        assert!(store.execution_lease().is_err());
        drop(adopted);
        assert!(store.execution_lease().is_ok());
        Ok(())
    }
    #[test]
    fn concurrent_journal_writers_preserve_all_actions_and_cancellation() -> Result<()> {
        let t = tempfile::tempdir()?;
        let store = RunStore::open(&context(&t, "journal-writers"))?;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(9));
        let writer_stores = (0..8)
            .map(|index| {
                if index % 2 == 0 {
                    Ok(store.clone())
                } else {
                    RunStore::open(store.context())
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let writers = writer_stores
            .into_iter()
            .enumerate()
            .map(|(index, store)| {
                let barrier = barrier.clone();
                std::thread::spawn(move || -> Result<()> {
                    barrier.wait();
                    for action in 0..8 {
                        store.prepare_action(&format!("writer_{index}_{action}"))?;
                    }
                    Ok(())
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        store.cancel()?;
        for writer in writers {
            writer.join().expect("journal writer panicked")?;
        }
        assert_eq!(store.status()?.actions.len(), 64);
        assert!(store.cancel_requested()?);
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn journal_waits_for_a_forks_transient_descriptor_without_stealing_ownership() -> Result<()> {
        let t = tempfile::tempdir()?;
        let ctx = context(&t, "journal-fork");
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "runtime::tests::isolated_journal_fork_fixture",
                "--nocapture",
            ])
            .env_clear()
            .env(
                "CA_AUTOMATION_JOURNAL_FORK_FIXTURE",
                serde_json::to_string(&ctx)?,
            )
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "isolated journal fork fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let store = RunStore::open(&ctx)?;
        assert!(store.cancel_requested()?);
        assert!(store
            .status()?
            .actions
            .iter()
            .any(|action| action.id == "after_fork"));
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn isolated_journal_fork_fixture() -> Result<()> {
        let Some(context) = std::env::var_os("CA_AUTOMATION_JOURNAL_FORK_FIXTURE") else {
            return Ok(());
        };
        let context: ExecutionContext = serde_json::from_slice(context.as_encoded_bytes())?;
        let store = RunStore::open(&context)?;
        let journal = store.lock()?;
        let started = std::time::Instant::now();
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if pid == 0 {
            // Only async-signal-safe calls after fork in this multithreaded
            // fixture. This models the inherited descriptor before exec.
            unsafe {
                libc::nanosleep(
                    &libc::timespec {
                        tv_sec: 0,
                        tv_nsec: 150_000_000,
                    },
                    std::ptr::null_mut(),
                );
                libc::_exit(0);
            }
        }
        drop(journal);
        assert!(ResourceLease::acquire(&store.root, "journal").is_err());
        let updated = store.prepare_action("after_fork");
        let waited = started.elapsed();
        let cancelled = store.cancel();
        let mut status = 0;
        unsafe { libc::waitpid(pid, &mut status, 0) };
        updated?;
        cancelled?;
        assert!(waited >= std::time::Duration::from_millis(100));
        assert!(store.cancel_requested()?);
        assert!(store.lock().is_ok());
        Ok(())
    }
    #[test]
    fn live_owner_is_not_reconciled_or_cancelled_by_inspection() -> Result<()> {
        let t = tempfile::tempdir()?;
        let s = RunStore::open(&context(&t, "live"))?;
        let _lease = s.execution_lease()?;
        s.start()?;
        s.prepare_action("observe")?;
        s.begin_action("observe")?;
        assert_eq!(s.reconcile()?.state, ExecutionState::Running);
        s.cancel()?;
        assert!(s.cancel_requested()?);
        Ok(())
    }
}
