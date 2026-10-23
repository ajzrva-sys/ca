//! Process-owned cooldowns. Admission chooses routes; clients only record evidence.
use crate::agent::stream::{ProviderStateSourceV1, ProviderTransportFailure};
use crate::fallback::ProviderFallbackCause;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

const MAX_ENTRIES: usize = 2048;
const MAX_DELAY: Duration = Duration::from_secs(86400);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RouteKey(String);
impl RouteKey {
    pub(crate) fn new(
        provider: &str,
        model: &str,
        protocol: crate::agent::ProviderProtocol,
        endpoint: &str,
        credential: &str,
    ) -> Option<Self> {
        let route = ProviderStateSourceV1::from_route(provider, protocol, endpoint).ok()?;
        Some(Self(format!(
            "{:x}",
            Sha256::digest(
                format!(
                    "ca-cooldown-v1\0{provider}\0{model}\0{}\0{}\0{credential}",
                    protocol.label(),
                    route.endpoint_sha256
                )
                .as_bytes()
            )
        )))
    }
    pub(crate) async fn configured(
        provider: &str,
        model: &str,
        config: &crate::config::ProviderConfig,
    ) -> Option<Self> {
        let mut endpoint = config.base_url.clone();
        let mut credential = config.api_key.clone();
        if config.protocol == crate::agent::ProviderProtocol::GoogleGenerateContent {
            let settings = config.google.clone().unwrap_or_default();
            endpoint = settings.endpoint(&endpoint).ok()?;
            if settings.mode == crate::google_auth::Mode::Vertex {
                credential = crate::google_auth::Auth::new(settings)
                    .identity()
                    .await
                    .ok()?;
            }
        }
        Self::new(provider, model, config.protocol, &endpoint, &credential)
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Cooldown {
    pub remaining: Duration,
    pub cause: ProviderFallbackCause,
}
#[derive(Default)]
pub(crate) struct Registry {
    entries: HashMap<RouteKey, (Instant, ProviderFallbackCause, Instant)>,
}
impl Registry {
    pub(crate) fn record(
        &mut self,
        key: RouteKey,
        delay: Duration,
        cause: ProviderFallbackCause,
        now: Instant,
    ) {
        self.entries.retain(|_, (until, _, _)| *until > now);
        if delay.is_zero() {
            // This failure adds no delay; it cannot cancel another failure's
            // unexpired cooldown. Structured refinement removes its own
            // provisional entry before reaching this branch.
            return;
        }
        if self.entries.len() >= MAX_ENTRIES && !self.entries.contains_key(&key) {
            if let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, v)| v.0)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest);
            }
        }
        let expiry = now + delay.min(MAX_DELAY);
        match self.entries.get_mut(&key) {
            Some(entry) if entry.0 > expiry => {
                entry.2 = now;
            }
            _ => {
                self.entries.insert(key, (expiry, cause, now));
            }
        }
    }
    pub(crate) fn clear_success(&mut self, key: &RouteKey, started: Instant) {
        if self
            .entries
            .get(key)
            .is_some_and(|entry| entry.2 <= started)
        {
            self.entries.remove(key);
        }
    }
    pub(crate) fn get(&mut self, key: &RouteKey, now: Instant) -> Option<Cooldown> {
        self.entries.retain(|_, (until, _, _)| *until > now);
        self.entries.get(key).map(|(until, cause, _)| Cooldown {
            remaining: until.saturating_duration_since(now),
            cause: *cause,
        })
    }
}
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}
pub(crate) fn get(key: &RouteKey) -> Option<Cooldown> {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(key, Instant::now())
}
pub(crate) fn clear_success(key: &RouteKey, started: Instant) {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear_success(key, started);
}
type Entry = (Instant, ProviderFallbackCause, Instant);
#[derive(Clone, Copy)]
pub(crate) struct Recorded {
    at: Instant,
    previous: Option<Entry>,
}
pub(crate) fn record_http(
    key: RouteKey,
    status: u16,
    headers: &reqwest::header::HeaderMap,
) -> Option<Recorded> {
    let cause = match status {
        429 => ProviderFallbackCause::RateLimited,
        500..=599 => ProviderFallbackCause::ServerError,
        _ => return None,
    };
    let delay = header_delay(headers, SystemTime::now())
        .unwrap_or(Duration::from_secs(if status == 429 { 60 } else { 15 }));
    let recorded = Instant::now();
    let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    let previous = registry.entries.get(&key).copied();
    registry.record(key, delay, cause, recorded);
    Some(Recorded {
        at: recorded,
        previous,
    })
}
pub(crate) fn record_structured(
    key: RouteKey,
    status: u16,
    headers: &reqwest::header::HeaderMap,
    body: &[u8],
    recorded: Option<Recorded>,
) {
    if !matches!(status, 429 | 500..=599) || header_delay(headers, SystemTime::now()).is_some() {
        return;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return;
    };
    let Some(details) = value
        .pointer("/error/details")
        .and_then(serde_json::Value::as_array)
    else {
        return;
    };
    let delay = details
        .iter()
        .find(|v| v["@type"] == "type.googleapis.com/google.rpc.RetryInfo")
        .and_then(|v| v["retryDelay"].as_str())
        .and_then(|v| v.strip_suffix('s'))
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| Duration::from_secs_f64(v.min(MAX_DELAY.as_secs_f64())));
    if let (Some(delay), Some(recorded)) = (delay, recorded) {
        let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
        if registry
            .entries
            .get(&key)
            .is_some_and(|entry| entry.2 == recorded.at)
        {
            registry.entries.remove(&key);
            if let Some(previous) = recorded.previous {
                registry.entries.insert(key.clone(), previous);
            }
            registry.record(
                key,
                delay,
                if status == 429 {
                    ProviderFallbackCause::RateLimited
                } else {
                    ProviderFallbackCause::ServerError
                },
                recorded.at,
            );
        }
    }
}
pub(crate) fn record_transport(key: RouteKey, failure: ProviderTransportFailure) {
    let cause = match failure {
        ProviderTransportFailure::Connect => ProviderFallbackCause::Connect,
        ProviderTransportFailure::Timeout | ProviderTransportFailure::FirstTokenTimeout => {
            ProviderFallbackCause::Timeout
        }
        _ => return,
    };
    registry().lock().unwrap_or_else(|e| e.into_inner()).record(
        key,
        Duration::from_secs(15),
        cause,
        Instant::now(),
    );
}
// Official Claude reset headers use RFC3339; OpenAI reset headers use durations.
// Only exhausted buckets influence a 429. Retry-After takes precedence.
fn header_delay(headers: &reqwest::header::HeaderMap, now: SystemTime) -> Option<Duration> {
    let value = |key: &str| headers.get(key).and_then(|v| v.to_str().ok());
    if let Some(delay) = value("retry-after").and_then(|v| parse_retry_after(v, now)) {
        return Some(delay);
    }
    let mut delays = Vec::new();
    for bucket in ["requests", "tokens", "input-tokens", "output-tokens"] {
        let prefix = format!("anthropic-ratelimit-{bucket}");
        if value(&format!("{prefix}-remaining")).and_then(|v| v.parse::<u64>().ok()) == Some(0) {
            if let Some(date) = value(&format!("{prefix}-reset"))
                .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
            {
                let date: SystemTime = date.with_timezone(&chrono::Utc).into();
                delays.push(date.duration_since(now).unwrap_or_default().min(MAX_DELAY));
            }
        }
    }
    for bucket in ["requests", "tokens", "project-tokens"] {
        if value(&format!("x-ratelimit-remaining-{bucket}")).and_then(|v| v.parse::<u64>().ok())
            == Some(0)
        {
            if let Some(delay) =
                value(&format!("x-ratelimit-reset-{bucket}")).and_then(parse_reset_duration)
            {
                delays.push(delay);
            }
        }
    }
    delays.into_iter().max()
}
fn parse_reset_duration(mut input: &str) -> Option<Duration> {
    if input.len() > 64 || input.is_empty() {
        return None;
    }
    let mut seconds = 0.0;
    while !input.is_empty() {
        let end = input.find(|ch: char| !(ch.is_ascii_digit() || ch == '.'))?;
        let amount: f64 = input[..end].parse().ok()?;
        if !amount.is_finite() || amount < 0.0 {
            return None;
        }
        input = &input[end..];
        let (unit, multiplier) = if input.starts_with("ms") {
            (2, 0.001)
        } else if input.starts_with('s') {
            (1, 1.0)
        } else if input.starts_with('m') {
            (1, 60.0)
        } else if input.starts_with('h') {
            (1, 3600.0)
        } else if input.starts_with('d') {
            (1, 86400.0)
        } else {
            return None;
        };
        seconds = (seconds + amount * multiplier).min(MAX_DELAY.as_secs_f64());
        input = &input[unit..];
    }
    Some(Duration::from_secs_f64(seconds))
}
pub(crate) fn parse_retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    if let Ok(seconds) = value.trim().parse::<u64>() {
        return Some(Duration::from_secs(seconds).min(MAX_DELAY));
    }
    let date = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let date: SystemTime = date.with_timezone(&chrono::Utc).into();
    Some(date.duration_since(now).unwrap_or_default().min(MAX_DELAY))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cooldown_fake_clock_expiry_and_identity_are_exact() {
        let now = Instant::now();
        let mut r = Registry::default();
        let key = RouteKey::new(
            "p",
            "m",
            crate::agent::ProviderProtocol::ChatCompletions,
            "https://example.test/v1/",
            "key1",
        )
        .unwrap();
        let other = RouteKey::new(
            "p",
            "m",
            crate::agent::ProviderProtocol::ChatCompletions,
            "https://example.test/v1",
            "key2",
        )
        .unwrap();
        r.record(
            key.clone(),
            Duration::from_secs(60),
            ProviderFallbackCause::RateLimited,
            now,
        );
        assert!(r.get(&other, now).is_none());
        assert_eq!(
            r.get(&key, now + Duration::from_secs(59))
                .unwrap()
                .remaining,
            Duration::from_secs(1)
        );
        assert!(r.get(&key, now + Duration::from_secs(60)).is_none());
    }
    #[test]
    fn cooldown_retry_after_accepts_dates_zero_and_caps() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(0);
        assert_eq!(parse_retry_after("0", now), Some(Duration::ZERO));
        assert_eq!(parse_retry_after("999999", now), Some(MAX_DELAY));
        assert_eq!(
            parse_retry_after("Thu, 01 Jan 1970 00:01:00 GMT", now),
            Some(Duration::from_secs(60))
        );
        assert!(parse_retry_after("later", now).is_none());
    }
    #[test]
    fn cooldown_bound_and_replacement_do_not_grow_forever() {
        let now = Instant::now();
        let mut r = Registry::default();
        for i in 0..MAX_ENTRIES + 3 {
            r.record(
                RouteKey(i.to_string()),
                MAX_DELAY,
                ProviderFallbackCause::ServerError,
                now,
            );
        }
        assert_eq!(r.entries.len(), MAX_ENTRIES);
        r.record(
            RouteKey("last".into()),
            Duration::ZERO,
            ProviderFallbackCause::ServerError,
            now,
        );
        assert_eq!(r.entries.len(), MAX_ENTRIES);
    }
    #[test]
    fn cooldown_concurrent_failures_do_not_shorten_and_older_success_cannot_clear() {
        let mut registry = Registry::default();
        let now = Instant::now();
        let key = RouteKey("concurrent".into());
        registry.record(
            key.clone(),
            Duration::from_secs(60),
            ProviderFallbackCause::RateLimited,
            now,
        );
        registry.record(
            key.clone(),
            Duration::from_secs(15),
            ProviderFallbackCause::ServerError,
            now + Duration::from_secs(1),
        );
        registry.clear_success(&key, now);
        assert_eq!(
            registry.get(&key, now).unwrap().remaining,
            Duration::from_secs(60)
        );
        registry.clear_success(&key, now + Duration::from_secs(2));
        assert!(registry.get(&key, now).is_none());
    }
    #[test]
    fn cooldown_documented_reset_metadata_and_precedence() {
        let now = SystemTime::UNIX_EPOCH;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("anthropic-ratelimit-tokens-remaining", "0".parse().unwrap());
        headers.insert(
            "anthropic-ratelimit-tokens-reset",
            "1970-01-01T00:02:00Z".parse().unwrap(),
        );
        headers.insert("x-ratelimit-remaining-requests", "0".parse().unwrap());
        headers.insert("x-ratelimit-reset-requests", "1m30s".parse().unwrap());
        assert_eq!(header_delay(&headers, now), Some(Duration::from_secs(120)));
        headers.insert("retry-after", "3".parse().unwrap());
        assert_eq!(header_delay(&headers, now), Some(Duration::from_secs(3)));
        assert_eq!(
            parse_reset_duration("1h2m3.5s"),
            Some(Duration::from_millis(3723500))
        );
        assert_eq!(
            parse_reset_duration("250ms"),
            Some(Duration::from_millis(250))
        );
        for invalid in ["NaNs", "-1s", "2", "", "1sbad", "1e6s"] {
            assert!(parse_reset_duration(invalid).is_none());
        }
    }
    #[test]
    fn cooldown_structured_retry_refines_default_without_erasing_concurrent_failure() {
        let key = RouteKey(format!("structured-{}", uuid::Uuid::new_v4()));
        let headers = reqwest::header::HeaderMap::new();
        let recorded = record_http(key.clone(), 429, &headers);
        let body = br#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"2.5s"}]}}"#;
        record_structured(key.clone(), 429, &headers, body, recorded);
        let remaining = get(&key).unwrap().remaining;
        assert!(remaining <= Duration::from_millis(2500) && remaining > Duration::from_secs(2));
        let mut later = headers.clone();
        later.insert("retry-after", "100".parse().unwrap());
        let _ = record_http(key.clone(), 503, &later);
        record_structured(key.clone(), 429, &headers, body, recorded);
        assert!(get(&key).unwrap().remaining > Duration::from_secs(99));
    }
    #[test]
    fn cooldown_structured_refinement_keeps_an_earlier_longer_failure() {
        let key = RouteKey(format!("prior-{}", uuid::Uuid::new_v4()));
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("retry-after", "100".parse().unwrap());
        let _ = record_http(key.clone(), 503, &headers);
        let headers = reqwest::header::HeaderMap::new();
        let recorded = record_http(key.clone(), 429, &headers);
        record_structured(key.clone(),429,&headers,br#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"2s"}]}}"#,recorded);
        assert!(get(&key).unwrap().remaining > Duration::from_secs(99));
    }
    #[test]
    fn cooldown_zero_delay_keeps_independent_unexpired_failure() {
        let mut registry = Registry::default();
        let now = Instant::now();
        let key = RouteKey("zero-independent".into());
        registry.record(
            key.clone(),
            Duration::from_secs(100),
            ProviderFallbackCause::ServerError,
            now,
        );
        registry.record(
            key.clone(),
            Duration::ZERO,
            ProviderFallbackCause::RateLimited,
            now + Duration::from_secs(1),
        );
        let cooldown = registry.get(&key, now + Duration::from_secs(1)).unwrap();
        assert_eq!(cooldown.remaining, Duration::from_secs(99));
        assert_eq!(cooldown.cause, ProviderFallbackCause::ServerError);
        assert!(registry.get(&key, now + Duration::from_secs(100)).is_none());
    }
    #[test]
    fn cooldown_structured_zero_removes_only_its_own_provisional_delay() {
        for earlier_failure in [false, true] {
            let key = RouteKey(format!("zero-structured-{}", uuid::Uuid::new_v4()));
            if earlier_failure {
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert("retry-after", "100".parse().unwrap());
                let _ = record_http(key.clone(), 503, &headers);
            }
            let headers = reqwest::header::HeaderMap::new();
            let recorded = record_http(key.clone(), 429, &headers);
            record_structured(key.clone(), 429, &headers, br#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"0s"}]}}"#, recorded);
            if earlier_failure {
                assert!(get(&key).unwrap().remaining > Duration::from_secs(99));
            } else {
                assert!(get(&key).is_none());
            }
        }
    }
}
