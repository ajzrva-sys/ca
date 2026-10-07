use mmdflux::{OutputFormat, RenderConfig, TextColorMode};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::{mpsc, Arc, LazyLock, Mutex};

pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 64;
const MAX_PENDING_JOBS: usize = 16;

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

#[derive(Debug, Clone)]
enum CacheEntry {
    Pending,
    Ready(String),
    Failed,
}

#[derive(Debug)]
struct RenderJob {
    key: CacheKey,
    source: String,
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

static RENDERER: LazyLock<Renderer> = LazyLock::new(|| {
    let cache = Arc::new(Mutex::new(RenderCache::default()));
    let (sender, receiver) = mpsc::sync_channel::<RenderJob>(MAX_PENDING_JOBS);
    let worker_cache = Arc::clone(&cache);
    std::thread::Builder::new()
        .name("ca-mermaid-renderer".into())
        .spawn(move || render_worker(receiver, worker_cache))
        .expect("spawn bounded Mermaid renderer");
    Renderer { cache, sender }
});

pub fn render_or_queue(source: &str, width: u16) -> MermaidRender {
    if !eligible(source, width) {
        return MermaidRender::Fallback;
    }
    let key = CacheKey {
        digest: Sha256::digest(source.as_bytes()).into(),
        width,
    };
    {
        let cache = RENDERER
            .cache
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = cache.entries.get(&key) {
            return match entry {
                CacheEntry::Pending => MermaidRender::Pending,
                CacheEntry::Ready(output) => MermaidRender::Ready(output.clone()),
                CacheEntry::Failed => MermaidRender::Fallback,
            };
        }
    }

    {
        let mut cache = RENDERER
            .cache
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        prune_cache(&mut cache);
        cache.entries.insert(key.clone(), CacheEntry::Pending);
        cache.order.push_back(key.clone());
    }
    let job = RenderJob {
        key: key.clone(),
        source: source.to_string(),
    };
    if RENDERER.sender.try_send(job).is_err() {
        let mut cache = RENDERER
            .cache
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        cache.entries.remove(&key);
        cache.order.retain(|candidate| candidate != &key);
        return MermaidRender::Fallback;
    }
    MermaidRender::Pending
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
        if matches!(cache.entries.get(&candidate), Some(CacheEntry::Pending)) {
            cache.order.push_back(candidate);
            if cache
                .order
                .iter()
                .all(|key| matches!(cache.entries.get(key), Some(CacheEntry::Pending)))
            {
                break;
            }
        } else {
            cache.entries.remove(&candidate);
        }
    }
}

fn render_worker(receiver: mpsc::Receiver<RenderJob>, cache: Arc<Mutex<RenderCache>>) {
    while let Ok(job) = receiver.recv() {
        let rendered = render_bounded(&job.source, job.key.width)
            .map(CacheEntry::Ready)
            .unwrap_or(CacheEntry::Failed);
        cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entries
            .insert(job.key, rendered);
    }
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
}
