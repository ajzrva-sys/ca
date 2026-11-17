//! Process-wide fetch for pages the web tool can inline or hand to the caller.
//!
//! Returned text is decoded response bytes. It is not HTML-stripped. The caller
//! scans it before a provider sees it, and the caller writes artifact files.

#![allow(dead_code)] // fetch_bounded is the web tool's next entry; not wired yet.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use reqwest::Url;

const WEB_FETCH_MAX_DECODED_BYTES: usize = 1024 * 1024;
/// Inline budget is this percent of the caller-supplied context window.
pub const INLINE_CONTEXT_SHARE_PERCENT: usize = 3;
const ARTIFACT_PREVIEW_BYTES: usize = 512;
/// Complete inline text only. Truncated and artifact bodies are not stored.
pub const CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const CACHE_MAX_ENTRIES: usize = 32;
/// Redirects followed after the first response. Each next hop is checked first.
const MAX_REDIRECTS: usize = 5;
const METADATA_IPV4: Ipv4Addr = Ipv4Addr::new(169, 254, 169, 254);
const REDIRECT_LOCATION_MAX_BYTES: usize = 8 * 1024;
const UNREAD_MARKER: &str = "\n[… remaining response was not read …]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchedPage {
    Inline(String),
    /// `body` is for the caller to write. This module does not keep it.
    Artifact {
        preview: String,
        format: String,
        suggested_relative_name: String,
        body: String,
        truncated: bool,
    },
}

struct RawPage {
    text: String,
    truncated: bool,
    format: String,
}

#[derive(Clone)]
struct CachedPage {
    text: String,
    format: String,
    stored: Instant,
}

struct PageCache {
    entries: HashMap<String, CachedPage>,
    order: VecDeque<String>,
}

/// One client for the process. Redirects stay off so each hop can be refused
/// before its body is read. The per-call timeout is set on the request.
fn shared_client() -> Result<&'static reqwest::Client, String> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "could not initialize the HTTP client".to_string())?;
    Ok(CLIENT.get_or_init(|| client))
}

/// Fetch `url` within `timeout`. A page larger than
/// [`INLINE_CONTEXT_SHARE_PERCENT`] of `context_window` is an artifact.
pub async fn fetch_bounded(
    url: &str,
    timeout: Duration,
    context_window: usize,
) -> Result<FetchedPage, String> {
    let parsed = parse_fetch_url(url)?;
    ensure_allowed(&parsed, false).await?;
    if let Some(hit) = cache_get(url) {
        return Ok(materialize(url, context_window, hit));
    }
    let raw = fetch_remote(parsed, timeout).await?;
    Ok(admit(url, context_window, raw))
}

/// Loopback, private, link-local, unspecified, and the metadata address.
pub fn is_blocked_target(address: SocketAddr) -> bool {
    ip_blocked(address.ip())
}

fn blocked_hop(ip: IpAddr, redirect: bool) -> bool {
    if !redirect && ip.is_loopback() {
        return false;
    }
    ip_blocked(ip)
}

fn ip_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ipv4_blocked(ip),
        IpAddr::V6(ip) => ipv6_blocked(ip),
    }
}

fn ipv4_blocked(ip: Ipv4Addr) -> bool {
    ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip == METADATA_IPV4
}

fn ipv6_blocked(ip: Ipv6Addr) -> bool {
    if ip.is_unspecified() || ip.is_loopback() {
        return true;
    }
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return ipv4_blocked(mapped);
    }
    ip.is_unique_local() || ip.is_unicast_link_local()
}

fn parse_fetch_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| "invalid URL".to_string())?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("invalid URL".into());
    }
    if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err("invalid URL".into());
    }
    Ok(url)
}

/// Open the final response without reading its body.
///
/// The first hop may be loopback so the local fetch tests can use a temporary
/// server. A redirect onto loopback, a private network, or the metadata
/// address is refused before that body is read.
pub async fn open_checked(url: &str, timeout: Duration) -> Result<reqwest::Response, String> {
    let client = shared_client()?;
    let mut current = parse_fetch_url(url)?;
    let mut followed = 0usize;
    loop {
        ensure_allowed(&current, followed > 0).await?;
        let response = client
            .get(current.as_str())
            .timeout(timeout)
            .send()
            .await
            .map_err(safe_fetch_error)?;
        if !response.status().is_redirection() {
            return Ok(response);
        }
        if followed >= MAX_REDIRECTS {
            return Err("too many redirects".into());
        }
        followed += 1;
        let location = redirect_location(&response)?;
        drop(response);
        current = join_redirect(&current, &location)?;
    }
}

pub fn cached_text(url: &str) -> Option<String> {
    cache_get(url).map(|hit| hit.text)
}

pub fn remember_complete(url: &str, text: &str) {
    cache_insert(url, "text", text.to_string());
}

async fn ensure_allowed(url: &Url, redirect: bool) -> Result<(), String> {
    let host = url
        .host_str()
        .ok_or_else(|| "invalid URL".to_string())?
        .trim_matches(|ch| ch == '[' || ch == ']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        return if blocked_hop(ip, redirect) {
            Err("refusing private or metadata address".into())
        } else {
            Ok(())
        };
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "invalid URL".to_string())?;
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| "could not resolve host".to_string())?;
    let mut resolved = false;
    for address in addresses {
        resolved = true;
        let blocked = if redirect {
            is_blocked_target(address)
        } else {
            blocked_hop(address.ip(), false)
        };
        if blocked {
            return Err("refusing private or metadata address".into());
        }
    }
    if !resolved {
        return Err("could not resolve host".into());
    }
    Ok(())
}

async fn fetch_remote(start: Url, timeout: Duration) -> Result<RawPage, String> {
    let client = shared_client()?;
    let mut current = start;
    let mut followed = 0usize;
    loop {
        ensure_allowed(&current, followed > 0).await?;
        let response = client
            .get(current.as_str())
            .timeout(timeout)
            .send()
            .await
            .map_err(safe_fetch_error)?;
        if response.status().is_redirection() {
            if followed >= MAX_REDIRECTS {
                return Err("too many redirects".into());
            }
            followed += 1;
            let location = redirect_location(&response)?;
            drop(response);
            current = join_redirect(&current, &location)?;
            continue;
        }
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status().as_u16()));
        }
        let mut response = response;
        return read_page(&mut response).await;
    }
}

fn redirect_location(response: &reqwest::Response) -> Result<String, String> {
    let value = response
        .headers()
        .get(reqwest::header::LOCATION)
        .ok_or_else(|| "redirect missing location".to_string())?;
    value
        .to_str()
        .map(str::to_string)
        .map_err(|_| "invalid redirect".to_string())
}

fn join_redirect(current: &Url, location: &str) -> Result<Url, String> {
    if location.len() > REDIRECT_LOCATION_MAX_BYTES || location.chars().any(char::is_control) {
        return Err("invalid redirect".into());
    }
    let next = current
        .join(location)
        .map_err(|_| "invalid redirect".to_string())?;
    if (next.scheme() != "http" && next.scheme() != "https")
        || next.host_str().is_none()
        || !next.username().is_empty()
        || next.password().is_some()
    {
        return Err("invalid redirect".into());
    }
    Ok(next)
}

async fn read_page(response: &mut reqwest::Response) -> Result<RawPage, String> {
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(media_type);
    if content_type
        .as_deref()
        .is_some_and(|kind| !safe_text_content_type(kind))
    {
        return Err(format!(
            "unsupported content type {}",
            content_type.as_deref().unwrap_or("unknown")
        ));
    }
    let format = body_format(content_type.as_deref()).to_string();
    let declared_oversize = response
        .content_length()
        .is_some_and(|length| length > WEB_FETCH_MAX_DECODED_BYTES as u64);
    let mut body = Vec::with_capacity(WEB_FETCH_MAX_DECODED_BYTES.min(64 * 1024));
    let mut unread = declared_oversize;
    while let Some(chunk) = response.chunk().await.map_err(safe_fetch_error)? {
        let remaining = WEB_FETCH_MAX_DECODED_BYTES.saturating_sub(body.len());
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            unread = true;
            break;
        }
        body.extend_from_slice(&chunk);
        if body.len() == WEB_FETCH_MAX_DECODED_BYTES {
            if response.chunk().await.map_err(safe_fetch_error)?.is_some() {
                unread = true;
            }
            break;
        }
    }
    let text = decode_body(&body, unread)?;
    Ok(RawPage {
        text,
        truncated: unread,
        format,
    })
}

fn media_type(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn safe_text_content_type(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || matches!(
            content_type,
            "application/json"
                | "application/ld+json"
                | "application/xml"
                | "application/xhtml+xml"
                | "application/javascript"
                | "application/x-javascript"
        )
        || content_type.ends_with("+json")
        || content_type.ends_with("+xml")
}

fn body_format(content_type: Option<&str>) -> &'static str {
    let Some(kind) = content_type else {
        return "text";
    };
    if kind.contains("html") {
        "html"
    } else if kind.contains("json") {
        "json"
    } else if kind.contains("xml") {
        "xml"
    } else if kind.contains("javascript") {
        "javascript"
    } else {
        "text"
    }
}

fn decode_body(body: &[u8], unread: bool) -> Result<String, String> {
    match std::str::from_utf8(body) {
        Ok(value) => Ok(value.to_string()),
        Err(error) if unread && error.error_len().is_none() => {
            let valid = std::str::from_utf8(&body[..error.valid_up_to()])
                .map_err(|_| "response is not valid UTF-8".to_string())?;
            Ok(valid.to_string())
        }
        Err(_) => Err("response is not valid UTF-8".into()),
    }
}

fn safe_fetch_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "request timed out".into()
    } else if error.is_connect() {
        "could not connect".into()
    } else if error.is_decode() {
        "response decoding failed".into()
    } else {
        "request failed".into()
    }
}

fn inline_byte_budget(context_window: usize) -> usize {
    let budget =
        (context_window as u128).saturating_mul(INLINE_CONTEXT_SHARE_PERCENT as u128) / 100;
    usize::try_from(budget).unwrap_or(usize::MAX)
}

fn prefix(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn suggested_relative_name(url: &str, format: &str) -> String {
    let ext = match format {
        "html" => "html",
        "json" => "json",
        "xml" => "xml",
        "javascript" => "js",
        _ => "txt",
    };
    let parsed = Url::parse(url).ok();
    let host = parsed.as_ref().and_then(Url::host_str).unwrap_or("page");
    let path = parsed.as_ref().map(Url::path).unwrap_or("");
    let mut slug = String::new();
    for byte in host.bytes().chain(path.bytes()) {
        let ch = byte as char;
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 64 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    let slug = if slug.is_empty() { "page" } else { slug };
    format!("web-fetch/{slug}-{:08x}.{ext}", short_hash(url))
}

fn short_hash(text: &str) -> u32 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    u32::try_from(hash & 0xffff_ffff).unwrap_or(0)
}

fn artifact_page(url: &str, format: &str, body: String, truncated: bool) -> FetchedPage {
    let preview = prefix(&body, ARTIFACT_PREVIEW_BYTES);
    FetchedPage::Artifact {
        preview,
        format: format.to_string(),
        suggested_relative_name: suggested_relative_name(url, format),
        body,
        truncated,
    }
}

pub fn spill_at(url: &str, text: &str, dir: &std::path::Path, window: usize) -> Option<String> {
    let budget = inline_byte_budget(window);
    if text.len() <= budget {
        return None;
    }
    let relative = suggested_relative_name(url, "text");
    let path = dir.join(&relative);
    let parent = path.parent()?;
    std::fs::create_dir_all(parent).ok()?;
    std::fs::write(&path, text).ok()?;
    let preview = prefix(text, ARTIFACT_PREVIEW_BYTES);
    Some(format!(
        "format: text\npath: {}\npreview:\n{preview}",
        path.display()
    ))
}

pub fn spill_if_oversized(url: &str, text: &str) -> Option<String> {
    let destination = ARTIFACT.with(|slot| slot.borrow().clone())?;
    spill_at(url, text, &destination.dir, destination.window)
}

pub fn set_artifact_destination(dir: Option<std::path::PathBuf>, window: usize) {
    ARTIFACT.with(|slot| {
        *slot.borrow_mut() = dir.map(|dir| ArtifactDestination { dir, window });
    });
}

#[derive(Clone)]
struct ArtifactDestination {
    dir: std::path::PathBuf,
    window: usize,
}

thread_local! {
    static ARTIFACT: std::cell::RefCell<Option<ArtifactDestination>> =
        const { std::cell::RefCell::new(None) };
}

fn admit(url: &str, context_window: usize, raw: RawPage) -> FetchedPage {
    if raw.text.len() > inline_byte_budget(context_window) {
        return artifact_page(url, &raw.format, raw.text, raw.truncated);
    }
    if raw.truncated {
        let mut text = raw.text;
        text.push_str(UNREAD_MARKER);
        return FetchedPage::Inline(text);
    }
    cache_insert(url, &raw.format, raw.text.clone());
    FetchedPage::Inline(raw.text)
}

fn materialize(url: &str, context_window: usize, hit: CachedPage) -> FetchedPage {
    if hit.text.len() <= inline_byte_budget(context_window) {
        return FetchedPage::Inline(hit.text);
    }
    artifact_page(url, &hit.format, hit.text, false)
}

/// Cache lookup for a body the refusal checks already allowed.
/// The getter runs only on a miss.
fn load_cached<F>(url: &str, context_window: usize, getter: F) -> Result<FetchedPage, String>
where
    F: FnOnce() -> Result<RawPage, String>,
{
    if let Some(hit) = cache_get(url) {
        return Ok(materialize(url, context_window, hit));
    }
    let raw = getter()?;
    Ok(admit(url, context_window, raw))
}

fn cache() -> &'static Mutex<PageCache> {
    static CACHE: OnceLock<Mutex<PageCache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(PageCache {
            entries: HashMap::new(),
            order: VecDeque::new(),
        })
    })
}

fn lock_cache() -> std::sync::MutexGuard<'static, PageCache> {
    cache().lock().unwrap_or_else(|poison| poison.into_inner())
}

fn fresh(stored: Instant) -> bool {
    Instant::now()
        .checked_duration_since(stored)
        .is_some_and(|age| age < CACHE_TTL)
}

fn cache_get(url: &str) -> Option<CachedPage> {
    let mut guard = lock_cache();
    let fresh_entry = match guard.entries.get(url) {
        Some(entry) if fresh(entry.stored) => Some(entry.clone()),
        Some(_) => None,
        None => return None,
    };
    if let Some(entry) = fresh_entry {
        return Some(entry);
    }
    guard.entries.remove(url);
    guard.order.retain(|key| key != url);
    None
}

fn cache_insert(url: &str, format: &str, text: String) {
    let mut guard = lock_cache();
    guard.order.retain(|key| key != url);
    guard.entries.insert(
        url.to_string(),
        CachedPage {
            text,
            format: format.to_string(),
            stored: Instant::now(),
        },
    );
    guard.order.push_back(url.to_string());
    let PageCache { entries, order } = &mut *guard;
    evict(entries, order);
}

fn evict(entries: &mut HashMap<String, CachedPage>, order: &mut VecDeque<String>) {
    let mut kept = VecDeque::new();
    while let Some(key) = order.pop_front() {
        let keep = entries.get(&key).is_some_and(|entry| fresh(entry.stored));
        if keep {
            kept.push_back(key);
        } else {
            entries.remove(&key);
        }
    }
    while kept.len() > CACHE_MAX_ENTRIES {
        if let Some(key) = kept.pop_front() {
            entries.remove(&key);
        }
    }
    *order = kept;
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::*;

    fn raw(text: &str, truncated: bool, format: &str) -> RawPage {
        RawPage {
            text: text.to_string(),
            truncated,
            format: format.to_string(),
        }
    }

    #[test]
    fn ssrf_classifier_rejects_private_and_metadata_targets() {
        let blocked = [
            "127.0.0.1:80",
            "10.1.2.3:443",
            "172.16.5.5:80",
            "192.168.1.1:80",
            "169.254.1.1:80",
            "169.254.169.254:80",
            "0.0.0.0:80",
            "[::]:80",
            "[::1]:80",
            "[fc00::1]:443",
            "[fd00::1]:443",
            "[fe80::1]:80",
            "[::ffff:127.0.0.1]:80",
            "[::ffff:169.254.169.254]:80",
            "[::ffff:10.1.2.3]:80",
        ];
        for text in blocked {
            let address: SocketAddr = text.parse().expect(text);
            assert!(is_blocked_target(address), "{text}");
        }
        let allowed = ["8.8.8.8:53", "[2001:4860:4860::8888]:53"];
        for text in allowed {
            let address: SocketAddr = text.parse().expect(text);
            assert!(!is_blocked_target(address), "{text}");
        }
    }

    #[test]
    fn cached_inline_text_skips_the_second_getter() {
        let url = "https://example.com/ca-web-fetch-cache-hit";
        let context_window = 10_000;
        let first = load_cached(url, context_window, || Ok(raw("hello", false, "text")))
            .expect("first fetch");
        assert_eq!(first, FetchedPage::Inline("hello".into()));
        let second = load_cached(url, context_window, || {
            Err("getter should not run on a cache hit".to_string())
        })
        .expect("cache hit");
        assert_eq!(second, first);
    }

    #[test]
    fn truncated_body_is_not_cached() {
        let url = "https://example.com/ca-web-fetch-truncated";
        let context_window = 10_000;
        let first = load_cached(url, context_window, || Ok(raw("partial", true, "text")))
            .expect("truncated fetch");
        assert_eq!(
            first,
            FetchedPage::Inline(format!("partial{UNREAD_MARKER}"))
        );
        let second = load_cached(url, context_window, || Ok(raw("fresh", true, "text")))
            .expect("second truncated fetch");
        assert_eq!(second, FetchedPage::Inline(format!("fresh{UNREAD_MARKER}")));
    }

    #[test]
    fn body_over_context_share_is_an_uncached_artifact() {
        assert_eq!(INLINE_CONTEXT_SHARE_PERCENT, 3);
        let context_window = 20_000;
        let limit = inline_byte_budget(context_window);
        assert_eq!(limit, 600);

        let url = "https://example.com/docs/page";
        let over = "x".repeat(limit + 1);
        let first = load_cached(url, context_window, || Ok(raw(&over, false, "html")))
            .expect("over-share fetch");
        match &first {
            FetchedPage::Artifact {
                preview,
                format,
                suggested_relative_name,
                body,
                truncated,
            } => {
                assert_eq!(body, &over);
                assert!(!truncated);
                assert_eq!(format, "html");
                assert_eq!(preview, &prefix(&over, ARTIFACT_PREVIEW_BYTES));
                assert!(preview.len() < body.len());
                assert!(body.starts_with(preview.as_str()));
                assert!(suggested_relative_name.starts_with("web-fetch/"));
                assert!(suggested_relative_name.ends_with(".html"));
                assert!(!suggested_relative_name.contains(".."));
                assert!(!suggested_relative_name.starts_with('/'));
            }
            FetchedPage::Inline(_) => panic!("over-share body was returned inline"),
        }

        let replacement = "y".repeat(limit + 1);
        let second = load_cached(url, context_window, || Ok(raw(&replacement, false, "html")))
            .expect("artifact must miss the cache");
        match second {
            FetchedPage::Artifact { body, .. } => assert_eq!(body, replacement),
            FetchedPage::Inline(_) => panic!("artifact body was cached"),
        }

        let exact_url = "https://example.com/docs/exact-share";
        let exact = "z".repeat(limit);
        let cached = load_cached(exact_url, context_window, || Ok(raw(&exact, false, "text")))
            .expect("exact share");
        assert_eq!(cached, FetchedPage::Inline(exact));
        let again = load_cached(exact_url, context_window, || {
            Err("exact inline body should be cached".to_string())
        })
        .expect("exact share cache hit");
        assert_eq!(again, cached);
    }
}
