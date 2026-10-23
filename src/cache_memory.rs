//! Shared budget for reproducible cache payloads. Revocation never deletes history.
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

const DEFAULT_BUDGET: usize = 256 * 1024 * 1024;
static LIMIT: AtomicUsize = AtomicUsize::new(DEFAULT_BUDGET);
static CLOCK: AtomicU64 = AtomicU64::new(1);
struct Lease {
    component: &'static str,
    bytes: usize,
    last: AtomicU64,
    revoked: AtomicBool,
}
#[derive(Clone)]
pub(crate) struct Ticket(Arc<Lease>);
impl std::fmt::Debug for Ticket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CacheTicket")
            .field("bytes", &self.0.bytes)
            .field("valid", &self.valid())
            .finish()
    }
}
impl Ticket {
    pub(crate) fn valid(&self) -> bool {
        !self.0.revoked.load(Ordering::Relaxed)
    }
    #[cfg(test)]
    pub(crate) fn revoke_for_test(&self) {
        self.0.revoked.store(true, Ordering::Relaxed);
    }
    pub(crate) fn touch(&self) {
        self.0
            .last
            .store(CLOCK.fetch_add(1, Ordering::Relaxed), Ordering::Relaxed);
    }
}
#[derive(Debug, Default, Clone, Serialize)]
pub(crate) struct Component {
    pub bytes: usize,
    pub high_water: usize,
    pub reclaimable: usize,
    pub pending_reclaim: usize,
    pub evictions: u64,
    pub released_bytes: u64,
    pub essential: bool,
}
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Snapshot {
    pub budget_bytes: usize,
    pub retained_cache_bytes: usize,
    pub bypasses: u64,
    pub components: BTreeMap<&'static str, Component>,
}
pub(crate) struct Budget {
    limit: usize,
    entries: Vec<Weak<Lease>>,
    components: BTreeMap<&'static str, Component>,
    bypasses: u64,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            limit: DEFAULT_BUDGET,
            entries: vec![],
            components: BTreeMap::new(),
            bypasses: 0,
        }
    }
}
impl Budget {
    fn sweep(&mut self) {
        // Registered leases are weak, so accounting never extends the lifetime of cache data.
        let old = self
            .components
            .iter()
            .map(|(name, c)| (*name, c.bytes))
            .collect::<BTreeMap<_, _>>();
        for c in self.components.values_mut() {
            c.bytes = 0;
            c.reclaimable = 0;
            c.pending_reclaim = 0;
        }
        self.entries.retain(|entry| entry.strong_count() > 0);
        for lease in self.entries.iter().filter_map(Weak::upgrade) {
            let c = self.components.entry(lease.component).or_default();
            c.bytes = c.bytes.saturating_add(lease.bytes);
            if lease.revoked.load(Ordering::Relaxed) {
                c.pending_reclaim = c.pending_reclaim.saturating_add(lease.bytes);
            } else {
                c.reclaimable = c.reclaimable.saturating_add(lease.bytes);
            }
        }
        for (name, c) in &mut self.components {
            c.high_water = c.high_water.max(c.bytes);
            c.released_bytes = c
                .released_bytes
                .saturating_add(old.get(name).copied().unwrap_or(0).saturating_sub(c.bytes) as u64);
        }
    }
    fn used(&self) -> usize {
        self.components.values().map(|v| v.bytes).sum()
    }
    fn request_reclaim(&mut self, additional: usize) {
        let used = self.used();
        let pending: usize = self.components.values().map(|c| c.pending_reclaim).sum();
        let mut remaining = used
            .saturating_add(additional)
            .saturating_sub(self.limit)
            .saturating_sub(pending);
        let mut candidates = self
            .entries
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|lease| !lease.revoked.load(Ordering::Relaxed))
            .collect::<Vec<_>>();
        candidates.sort_by_key(|lease| lease.last.load(Ordering::Relaxed));
        for lease in candidates {
            if remaining == 0 {
                break;
            }
            if !lease.revoked.swap(true, Ordering::Relaxed) {
                remaining = remaining.saturating_sub(lease.bytes);
                self.components
                    .entry(lease.component)
                    .or_default()
                    .evictions += 1;
            }
        }
    }
    fn reserve(&mut self, component: &'static str, bytes: usize) -> Option<Ticket> {
        self.sweep();
        if bytes > self.limit
            || self.used().saturating_add(bytes) > self.limit
            || self.entries.len() >= 16384
        {
            if bytes <= self.limit {
                self.request_reclaim(bytes);
            }
            self.bypasses = self.bypasses.saturating_add(1);
            return None;
        }
        let lease = Arc::new(Lease {
            component,
            bytes,
            last: AtomicU64::new(CLOCK.fetch_add(1, Ordering::Relaxed)),
            revoked: AtomicBool::new(false),
        });
        self.entries.push(Arc::downgrade(&lease));
        let c = self.components.entry(component).or_default();
        c.bytes = c.bytes.saturating_add(bytes);
        c.reclaimable = c.reclaimable.saturating_add(bytes);
        c.high_water = c.high_water.max(c.bytes);
        Some(Ticket(lease))
    }
    fn snapshot(&mut self) -> Snapshot {
        self.sweep();
        Snapshot {
            budget_bytes: self.limit,
            retained_cache_bytes: self.used(),
            bypasses: self.bypasses,
            components: self.components.clone(),
        }
    }
}
fn budget() -> &'static Mutex<Budget> {
    static BUDGET: OnceLock<Mutex<Budget>> = OnceLock::new();
    BUDGET.get_or_init(Default::default)
}
/// Configuration publication never waits for cache owners or workers.
pub(crate) fn configure(mib: u64) {
    let limit = usize::try_from(mib)
        .unwrap_or(usize::MAX)
        .saturating_mul(1024 * 1024);
    if LIMIT.swap(limit, Ordering::Relaxed) != limit {
        if let Ok(mut b) = budget().try_lock() {
            b.limit = limit;
            b.sweep();
            b.request_reclaim(0);
        }
    }
}

pub(crate) fn reserve(component: &'static str, bytes: usize) -> Option<Ticket> {
    let mut b = budget().try_lock().ok()?;
    b.limit = LIMIT.load(Ordering::Relaxed);
    b.reserve(component, bytes)
}

#[derive(Debug, Default)]
struct OwnedComponent {
    bytes: AtomicUsize,
    high_water: AtomicUsize,
}
#[derive(Debug)]
struct Owned {
    bytes: AtomicUsize,
    component: Arc<OwnedComponent>,
}
impl Drop for Owned {
    fn drop(&mut self) {
        self.component
            .bytes
            .fetch_sub(self.bytes.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}
/// Shared owners clone this handle, so an Arc-backed allocation is counted once.
#[derive(Debug, Clone)]
pub(crate) struct Essential(Arc<Owned>);
fn owned() -> &'static Mutex<BTreeMap<&'static str, Arc<OwnedComponent>>> {
    static OWNED: OnceLock<Mutex<BTreeMap<&'static str, Arc<OwnedComponent>>>> = OnceLock::new();
    OWNED.get_or_init(Default::default)
}
impl Essential {
    pub(crate) fn new(component: &'static str) -> Self {
        let counter = owned()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(component)
            .or_default()
            .clone();
        Self(Arc::new(Owned {
            bytes: AtomicUsize::new(0),
            component: counter,
        }))
    }
    pub(crate) fn set(&self, bytes: usize) {
        let previous = self.0.bytes.swap(bytes, Ordering::Relaxed);
        let current = if bytes >= previous {
            self.0
                .component
                .bytes
                .fetch_add(bytes - previous, Ordering::Relaxed)
                .saturating_add(bytes - previous)
        } else {
            self.0
                .component
                .bytes
                .fetch_sub(previous - bytes, Ordering::Relaxed)
                .saturating_sub(previous - bytes)
        };
        self.0
            .component
            .high_water
            .fetch_max(current, Ordering::Relaxed);
    }
}
pub(crate) fn snapshot() -> Option<Snapshot> {
    let mut b = budget().try_lock().ok()?;
    b.limit = LIMIT.load(Ordering::Relaxed);
    b.sweep();
    b.request_reclaim(0);
    let mut result = b.snapshot();
    for (name, counter) in owned().try_lock().ok()?.iter() {
        let c = result.components.entry(name).or_default();
        c.essential = true;
        c.bytes = counter.bytes.load(Ordering::Relaxed);
        c.high_water = counter.high_water.load(Ordering::Relaxed);
    }
    Some(result)
}
pub(crate) fn render(peak_rss: u64, observation: &str) -> String {
    let Some(snapshot) = snapshot() else {
        return "Memory diagnostics busy; try /debug memory again.".into();
    };
    let mut lines = vec![
        "Memory diagnostics".to_string(),
        format!(
            "Cache budget: {} MiB; retained: {} bytes",
            snapshot.budget_bytes / 1024 / 1024,
            snapshot.retained_cache_bytes
        ),
        format!("Cache insertions bypassed: {}", snapshot.bypasses),
        format!("Process peak RSS: {peak_rss} bytes"),
        "Owned payload estimates exclude allocator overhead; the cache budget is not an RSS limit."
            .into(),
        observation.into(),
        String::new(),
    ];
    for (name, c) in snapshot.components {
        if c.essential {
            lines.push(format!(
                "{name}: {} bytes; peak {}; retained",
                c.bytes, c.high_water
            ));
        } else {
            lines.push(format!("{name}: {} bytes; peak {}; reclaimable {}; pending {}; eviction requests {}; released {}",c.bytes,c.high_water,c.reclaimable,c.pending_reclaim,c.evictions,c.released_bytes));
        }
    }
    lines.join("\n")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_budget_revokes_lru_without_reclaiming_shared_live_data() {
        let mut b = Budget {
            limit: 10,
            ..Default::default()
        };
        let a = b.reserve("a", 6).unwrap();
        let copy = a.clone();
        let c = b.reserve("b", 4).unwrap();
        a.touch();
        assert!(b.reserve("new", 4).is_none());
        assert!(!c.valid());
        assert!(a.valid());
        assert_eq!(b.snapshot().retained_cache_bytes, 10);
        drop(c);
        let next = b.reserve("new", 4).unwrap();
        assert_eq!(b.snapshot().retained_cache_bytes, 10);
        drop(a);
        assert_eq!(b.snapshot().retained_cache_bytes, 10);
        drop(copy);
        assert_eq!(b.snapshot().retained_cache_bytes, 4);
        assert!(next.valid());
    }
    #[test]
    fn cache_budget_high_water_and_oversized_bypass_are_bounded() {
        let mut b = Budget {
            limit: 8,
            ..Default::default()
        };
        let a = b.reserve("a", 8).unwrap();
        assert!(b.reserve("oversized", 9).is_none());
        assert_eq!(b.snapshot().components["a"].high_water, 8);
        drop(a);
        assert_eq!(b.snapshot().retained_cache_bytes, 0);
        assert_eq!(b.snapshot().components["a"].high_water, 8);
    }
    #[test]
    fn ocx_memory_shared_essential_owner_counts_once_and_survives_cache_pressure() {
        let owner = Essential::new("OCX essential accounting fixture");
        owner.set(1024);
        let shared = owner.clone();
        assert_eq!(owner.0.component.bytes.load(Ordering::Relaxed), 1024);
        let mut b = Budget {
            limit: 0,
            ..Default::default()
        };
        assert!(b.reserve("cache fixture", 1).is_none());
        assert_eq!(shared.0.component.bytes.load(Ordering::Relaxed), 1024);
        drop(owner);
        assert_eq!(shared.0.component.bytes.load(Ordering::Relaxed), 1024);
        let counter = shared.0.component.clone();
        drop(shared);
        assert_eq!(counter.bytes.load(Ordering::Relaxed), 0);
        assert_eq!(counter.high_water.load(Ordering::Relaxed), 1024);
    }
}
