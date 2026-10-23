use mmdflux::{OutputFormat, RenderConfig, TextColorMode};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 64;
const MAX_PENDING_JOBS: usize = 16;
const RESULT_DELIVERY_TTL: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MermaidRender {
    Ready(String),
    Pending,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    digest: [u8; 32],
    width: u16,
}

#[derive(Debug)]
enum CacheEntry {
    Pending(u64),
    Ready {
        output: String,
        ticket: Option<crate::cache_memory::Ticket>,
        _delivery: Option<crate::cache_memory::Essential>,
        delivery_until: Option<std::time::Instant>,
    },
    Failed,
}

#[derive(Debug)]
struct RenderJob {
    key: CacheKey,
    source: String,
    generation: u64,
    _active: crate::cache_memory::Essential,
}

#[derive(Default)]
struct RenderCache {
    entries: HashMap<CacheKey, CacheEntry>,
    order: VecDeque<CacheKey>,
}

struct Renderer {
    cache: Arc<Mutex<RenderCache>>,
    sender: mpsc::SyncSender<RenderJob>,
}

static RENDERER: OnceLock<Renderer> = OnceLock::new();
fn renderer() -> &'static Renderer {
    RENDERER.get_or_init(|| {
        let cache = Arc::new(Mutex::new(RenderCache::default()));
        let (sender, receiver) = mpsc::sync_channel::<RenderJob>(MAX_PENDING_JOBS);
        let worker_cache = Arc::clone(&cache);
        std::thread::Builder::new()
            .name("ca-mermaid-renderer".into())
            .spawn(move || render_worker(receiver, worker_cache))
            .expect("spawn bounded Mermaid renderer");
        Renderer { cache, sender }
    })
}

pub fn render_or_queue(source: &str, width: u16) -> MermaidRender {
    if !eligible(source, width) {
        return MermaidRender::Fallback;
    }
    let key = CacheKey {
        digest: Sha256::digest(source.as_bytes()).into(),
        width,
    };
    let renderer = renderer();
    let Ok(mut cache) = renderer.cache.try_lock() else {
        return MermaidRender::Pending;
    };
    reclaim(&mut cache);
    if let Some(entry) = cache.entries.get(&key) {
        match entry {
            CacheEntry::Pending(_) => return MermaidRender::Pending,
            CacheEntry::Ready { output, ticket, .. } => {
                let output = output.clone();
                if let Some(ticket) = ticket {
                    ticket.touch();
                } else {
                    cache.entries.remove(&key);
                    cache.order.retain(|candidate| candidate != &key);
                }
                return MermaidRender::Ready(output);
            }
            CacheEntry::Failed => return MermaidRender::Fallback,
        }
    }
    prune_cache(&mut cache);
    if cache.entries.len() >= MAX_CACHE_ENTRIES {
        return MermaidRender::Fallback;
    }
    static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let generation = GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    cache
        .entries
        .insert(key.clone(), CacheEntry::Pending(generation));
    cache.order.push_back(key.clone());
    let active = crate::cache_memory::Essential::new("active Mermaid worker buffers");
    active.set(source.len());
    let job = RenderJob {
        key: key.clone(),
        source: source.to_string(),
        generation,
        _active: active,
    };
    if renderer.sender.try_send(job).is_err() {
        cache.entries.remove(&key);
        cache.order.retain(|candidate| candidate != &key);
        return MermaidRender::Fallback;
    }
    MermaidRender::Pending
}
/// Owners release revoked entries at a nonblocking, bounded safe boundary.
pub(crate) fn maintain() {
    if let Some(renderer) = RENDERER.get() {
        if let Ok(mut cache) = renderer.cache.try_lock() {
            reclaim(&mut cache);
        }
    }
}
fn reclaim(cache: &mut RenderCache) {
    reclaim_at(cache, std::time::Instant::now());
}
fn reclaim_at(cache: &mut RenderCache, now: std::time::Instant) {
    cache.entries.retain(|_, entry| match entry {
        CacheEntry::Ready {
            ticket: Some(ticket),
            ..
        } => ticket.valid(),
        CacheEntry::Ready {
            ticket: None,
            delivery_until: Some(until),
            ..
        } => now < *until,
        _ => true,
    });
    cache.order.retain(|key| cache.entries.contains_key(key));
}

fn eligible(source: &str, width: u16) -> bool {
    !source.is_empty()
        && source.len() <= MAX_SOURCE_BYTES
        && width > 0
        && source
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with("%%"))
            .and_then(|line| line.split_whitespace().next())
            .is_some_and(|kind| {
                matches!(
                    kind.to_ascii_lowercase().as_str(),
                    "graph"
                        | "flowchart"
                        | "classdiagram"
                        | "statediagram"
                        | "statediagram-v2"
                        | "sequencediagram"
                )
            })
}

fn prune_cache(cache: &mut RenderCache) {
    while cache.entries.len() >= MAX_CACHE_ENTRIES {
        let Some(candidate) = cache.order.pop_front() else {
            break;
        };
        if matches!(cache.entries.get(&candidate), Some(CacheEntry::Pending(_))) {
            cache.order.push_back(candidate);
            if cache
                .order
                .iter()
                .all(|key| matches!(cache.entries.get(key), Some(CacheEntry::Pending(_))))
            {
                break;
            }
        } else {
            cache.entries.remove(&candidate);
        }
    }
}

fn render_worker(receiver: mpsc::Receiver<RenderJob>, cache: Arc<Mutex<RenderCache>>) {
    loop {
        match receiver.recv_timeout(RESULT_DELIVERY_TTL) {
            Ok(job) => {
                let output = render_bounded(&job.source, job.key.width);
                let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
                reclaim(&mut cache);
                complete_job(&mut cache, &job, output);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                reclaim(&mut cache.lock().unwrap_or_else(|error| error.into_inner()));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn complete_job(cache: &mut RenderCache, job: &RenderJob, output: Option<String>) {
    // An obsolete completion cannot recreate a removed/replaced entry.
    if !matches!(cache.entries.get(&job.key), Some(CacheEntry::Pending(generation)) if *generation == job.generation)
    {
        return;
    }
    let entry = match output {
        Some(output) => {
            let ticket =
                crate::cache_memory::reserve("completed Mermaid renders", output.capacity());
            let delivery = if ticket.is_none() {
                let owner = crate::cache_memory::Essential::new("pending Mermaid result delivery");
                owner.set(output.capacity());
                Some(owner)
            } else {
                None
            };
            CacheEntry::Ready {
                output,
                ticket,
                delivery_until: delivery
                    .as_ref()
                    .map(|_| std::time::Instant::now() + RESULT_DELIVERY_TTL),
                _delivery: delivery,
            }
        }
        None => CacheEntry::Failed,
    };
    cache.entries.insert(job.key.clone(), entry);
}

fn render_bounded(source: &str, width: u16) -> Option<String> {
    const FITS: [(f64, f64, f64); 3] = [(30.0, 30.0, 12.0), (20.0, 25.0, 8.0), (12.0, 25.0, 5.0)];
    let mut best = render_once(source, None)?;
    let budget = usize::from(width);
    for spacing in FITS {
        if display_width(&best) <= budget {
            break;
        }
        if let Some(candidate) = render_once(source, Some(spacing)) {
            if display_width(&candidate) < display_width(&best) {
                best = candidate;
            }
        }
    }
    if display_width(&best) > budget {
        return None;
    }
    sanitize_output(best)
}

fn render_once(source: &str, spacing: Option<(f64, f64, f64)>) -> Option<String> {
    let mut config = RenderConfig {
        text_color_mode: TextColorMode::Plain,
        ..RenderConfig::default()
    };
    if let Some((node, rank, edge)) = spacing {
        config.layout.node_sep = node;
        config.layout.rank_sep = rank;
        config.layout.edge_sep = edge;
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        mmdflux::render_diagram(source, OutputFormat::Text, &config)
    }))
    .ok()?
    .ok()
}

fn sanitize_output(output: String) -> Option<String> {
    if output.is_empty() || output.len() > MAX_OUTPUT_BYTES {
        return None;
    }
    let mut safe = String::with_capacity(output.len());
    for character in output.chars() {
        match character {
            '\n' => safe.push('\n'),
            '\t' => safe.push_str("    "),
            character if !character.is_control() => safe.push(character),
            _ => {}
        }
        if safe.len() > MAX_OUTPUT_BYTES {
            return None;
        }
    }
    (!safe.trim().is_empty()).then_some(safe)
}

fn display_width(output: &str) -> usize {
    output
        .lines()
        .map(unicode_width::UnicodeWidthStr::width)
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
pub fn wait_for_render(source: &str, width: u16) -> MermaidRender {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let result = render_or_queue(source, width);
        if result != MermaidRender::Pending || std::time::Instant::now() >= deadline {
            return result;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_diagrams_render_off_thread_and_cache_by_width() {
        for source in [
            "flowchart LR\nA[Start] --> B[Done]",
            "classDiagram\nclass Animal",
            "stateDiagram-v2\n[*] --> Ready",
            "sequenceDiagram\nAlice->>Bob: Hello",
        ] {
            let MermaidRender::Ready(output) = wait_for_render(source, 80) else {
                panic!("supported Mermaid did not render: {source}");
            };
            assert!(!output.trim().is_empty());
            assert!(!output.contains('\u{1b}'));
            assert!(matches!(
                render_or_queue(source, 80),
                MermaidRender::Ready(_)
            ));
        }
    }

    #[test]
    fn invalid_unsupported_and_oversized_sources_fall_back() {
        assert_eq!(
            render_or_queue("pie\ntitle Unsupported", 80),
            MermaidRender::Fallback
        );
        assert!(matches!(
            wait_for_render("flowchart LR\nA -->", 80),
            MermaidRender::Fallback
        ));
        assert_eq!(
            render_or_queue(
                &format!("flowchart LR\nA[{}]", "x".repeat(MAX_SOURCE_BYTES)),
                80
            ),
            MermaidRender::Fallback
        );
    }
    #[test]
    fn ocx_memory_mermaid_regenerates_identical_output_after_reclamation() {
        let source = "flowchart LR\nMemory[Cache pressure fixture] --> Rebuilt[Identical output]";
        let MermaidRender::Ready(before) = wait_for_render(source, 100) else {
            panic!("fixture did not render");
        };
        {
            let cache = renderer().cache.lock().unwrap();
            for entry in cache.entries.values() {
                if let CacheEntry::Ready {
                    ticket: Some(ticket),
                    ..
                } = entry
                {
                    ticket.revoke_for_test();
                }
            }
        }
        maintain();
        let MermaidRender::Ready(after) = wait_for_render(source, 100) else {
            panic!("fixture did not regenerate");
        };
        assert_eq!(before, after);
    }
    #[test]
    fn ocx_memory_mermaid_late_completion_cannot_restore_invalidated_entry() {
        let key = CacheKey {
            digest: [42; 32],
            width: 80,
        };
        let job = RenderJob {
            key: key.clone(),
            source: "old".into(),
            generation: 1,
            _active: crate::cache_memory::Essential::new("test Mermaid worker"),
        };
        let mut cache = RenderCache::default();
        cache.entries.insert(key.clone(), CacheEntry::Pending(2));
        complete_job(&mut cache, &job, Some("stale".into()));
        assert!(matches!(
            cache.entries.get(&key),
            Some(CacheEntry::Pending(2))
        ));
        cache.entries.remove(&key);
        complete_job(&mut cache, &job, Some("stale".into()));
        assert!(!cache.entries.contains_key(&key));
    }
    #[test]
    fn ocx_memory_mermaid_abandoned_delivery_expires_without_a_viewport() {
        let key = CacheKey {
            digest: [43; 32],
            width: 80,
        };
        let now = std::time::Instant::now();
        let owner = crate::cache_memory::Essential::new("test abandoned Mermaid delivery");
        owner.set(128);
        let mut cache = RenderCache::default();
        cache.entries.insert(
            key.clone(),
            CacheEntry::Ready {
                output: "transient".into(),
                ticket: None,
                _delivery: Some(owner),
                delivery_until: Some(now + RESULT_DELIVERY_TTL),
            },
        );
        cache.order.push_back(key.clone());
        reclaim_at(&mut cache, now);
        assert!(cache.entries.contains_key(&key));
        reclaim_at(&mut cache, now + RESULT_DELIVERY_TTL);
        assert!(cache.entries.is_empty());
        assert!(cache.order.is_empty());
    }
}
