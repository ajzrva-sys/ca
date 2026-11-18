//! Priced prompt-cache keep-alives.
//!
//! A route with a known TTL and a known cache price may replay its last request
//! at five sixths of that TTL, with a tiny output cap, and only when that read
//! is cheaper than rewriting the prefix. An unknown TTL or an unknown price
//! sends nothing. The first keep-alive that comes back as a write ends warming
//! for that route. A person gets three idle keep-alives per message. A machine
//! session warms only while a tool call is in flight, and those keep-alives
//! together cost no more than one rewrite.

use crate::config::CaConfig;
use crate::usage::DecimalUsd;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub const WARM_BUDGET: u32 = 3;
pub const MIN_CACHEABLE_PREFIX_TOKENS: u32 = 1024;
pub const KEEPALIVE_OUTPUT_CAP: u32 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    Human,
    Machine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Temperature {
    Warm,
    Cold,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Replay { delay: Duration, output_cap: u32 },
    Skip { reason: &'static str },
}

#[derive(Clone, Copy, Debug)]
struct Rates {
    read_micros: u128,
    rewrite_micros: u128,
    input_micros: u128,
    output_micros: u128,
}

#[derive(Clone, Debug)]
struct Route {
    ttl: Duration,
    rates: Option<Rates>,
    disabled: bool,
    anchor: Duration,
    cached_tokens: u32,
    uncached_tokens: u32,
    idle_used: u32,
    tool_spent_micros: u128,
    generation: u64,
    logged: Option<&'static str>,
}

#[derive(Debug)]
pub struct Controller {
    now: Duration,
    generation: u64,
    routes: HashMap<String, Route>,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    pub fn new() -> Self {
        Self {
            now: Duration::ZERO,
            generation: 1,
            routes: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub fn advance(&mut self, by: Duration) {
        self.now = self.now.saturating_add(by);
    }

    /// A new person-typed message restarts the idle budget. It does not revive
    /// a route whose keep-alive already came back as a write.
    pub fn begin_user_message(&mut self) {
        self.generation = self.generation.saturating_add(1);
        for route in self.routes.values_mut() {
            route.idle_used = 0;
            route.tool_spent_micros = 0;
            route.generation = self.generation;
        }
    }

    pub fn set_route(&mut self, route: &str, ttl: Option<Duration>, rates: Option<WarmRates>) {
        let entry = self
            .routes
            .entry(route.to_string())
            .or_insert_with(|| Route {
                ttl: Duration::ZERO,
                rates: None,
                disabled: false,
                anchor: self.now,
                cached_tokens: 0,
                uncached_tokens: 0,
                idle_used: 0,
                tool_spent_micros: 0,
                generation: self.generation,
                logged: None,
            });
        entry.ttl = ttl.unwrap_or(Duration::ZERO);
        entry.rates = rates.and_then(WarmRates::as_micros);
        if ttl.is_none() {
            entry.ttl = Duration::ZERO;
        }
    }

    pub fn note_usage(&mut self, route: &str, cached_tokens: u32, uncached_tokens: u32) {
        let generation = self.generation;
        let now = self.now;
        let entry = self
            .routes
            .entry(route.to_string())
            .or_insert_with(|| Route {
                ttl: Duration::ZERO,
                rates: None,
                disabled: false,
                anchor: now,
                cached_tokens: 0,
                uncached_tokens: 0,
                idle_used: 0,
                tool_spent_micros: 0,
                generation,
                logged: None,
            });
        if cached_tokens > 0 {
            entry.anchor = now;
        }
        entry.cached_tokens = cached_tokens;
        entry.uncached_tokens = uncached_tokens;
        entry.generation = generation;
    }

    pub fn decide(&mut self, route: &str, pace: Pace, tool_in_flight: bool) -> Decision {
        let Some(entry) = self.routes.get_mut(route) else {
            return self.skip(route, "unknown ttl");
        };
        if entry.disabled {
            return Decision::Skip {
                reason: "prompt cache warming disabled for session: keep-alive missed",
            };
        }
        if entry.ttl.is_zero() {
            return Decision::Skip {
                reason: "unknown ttl",
            };
        }
        let Some(rates) = entry.rates else {
            return Decision::Skip {
                reason: "unknown price",
            };
        };
        if entry.cached_tokens < MIN_CACHEABLE_PREFIX_TOKENS {
            return Decision::Skip {
                reason: "prefix is below the cache minimum",
            };
        }
        let Some(delay) = warming_delay(entry.ttl) else {
            return Decision::Skip {
                reason: "ttl is too short to warm",
            };
        };
        let Some(warm_cost) = refresh_cost(
            rates,
            entry.cached_tokens,
            entry.uncached_tokens,
            KEEPALIVE_OUTPUT_CAP,
        ) else {
            return Decision::Skip {
                reason: "unknown price",
            };
        };
        let Some(rewrite) = token_cost(entry.cached_tokens, rates.rewrite_micros) else {
            return Decision::Skip {
                reason: "unknown price",
            };
        };
        if warm_cost >= rewrite {
            return Decision::Skip {
                reason: "a refresh would cost more than the cache write it prevents",
            };
        }
        if matches!(pace, Pace::Machine) && !tool_in_flight {
            return Decision::Skip {
                reason: "a machine session warms only while a tool runs",
            };
        }
        if matches!(pace, Pace::Human) && !tool_in_flight && entry.idle_used >= WARM_BUDGET {
            return Decision::Skip {
                reason: "idle keep-alive budget is spent",
            };
        }
        if tool_in_flight && entry.tool_spent_micros.saturating_add(warm_cost) > rewrite {
            return Decision::Skip {
                reason: "tool keep-alives would cost more than one rewrite",
            };
        }
        Decision::Replay {
            delay,
            output_cap: KEEPALIVE_OUTPUT_CAP,
        }
    }

    /// Count the keep-alive before it is sent. A cancelled or stale plan reserves nothing.
    pub fn reserve(
        &mut self,
        route: &str,
        pace: Pace,
        tool_in_flight: bool,
        generation: u64,
    ) -> bool {
        let Some(entry) = self.routes.get_mut(route) else {
            return false;
        };
        if entry.disabled || entry.generation != generation {
            return false;
        }
        let Some(rates) = entry.rates else {
            return false;
        };
        let Some(warm_cost) = refresh_cost(
            rates,
            entry.cached_tokens,
            entry.uncached_tokens,
            KEEPALIVE_OUTPUT_CAP,
        ) else {
            return false;
        };
        if tool_in_flight {
            let Some(rewrite) = token_cost(entry.cached_tokens, rates.rewrite_micros) else {
                return false;
            };
            if entry.tool_spent_micros.saturating_add(warm_cost) > rewrite {
                return false;
            }
            entry.tool_spent_micros = entry.tool_spent_micros.saturating_add(warm_cost);
            return true;
        }
        if pace == Pace::Machine || entry.idle_used >= WARM_BUDGET {
            return false;
        }
        entry.idle_used = entry.idle_used.saturating_add(1);
        true
    }

    pub fn observe_keepalive(&mut self, route: &str, cache_write_tokens: Option<u32>) {
        let Some(entry) = self.routes.get_mut(route) else {
            return;
        };
        if cache_write_tokens.is_some_and(|tokens| tokens > 0) {
            entry.disabled = true;
            entry.logged = Some("prompt cache warming disabled for session: keep-alive missed");
        } else {
            entry.anchor = self.now;
        }
    }

    pub fn temperature(&self, route: &str) -> Temperature {
        let Some(entry) = self.routes.get(route) else {
            return Temperature::Unknown;
        };
        if entry.ttl.is_zero() || entry.cached_tokens == 0 {
            return Temperature::Unknown;
        }
        if self.now.saturating_sub(entry.anchor) >= entry.ttl {
            Temperature::Cold
        } else {
            Temperature::Warm
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn sync_real_clock(&mut self) {
        self.now = real_now();
    }

    fn skip(&mut self, route: &str, reason: &'static str) -> Decision {
        if let Some(entry) = self.routes.get_mut(route) {
            entry.logged = Some(reason);
        }
        Decision::Skip { reason }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WarmRates {
    pub read_per_million_usd: DecimalUsd,
    pub rewrite_per_million_usd: DecimalUsd,
    pub input_per_million_usd: DecimalUsd,
    pub output_per_million_usd: DecimalUsd,
}

impl WarmRates {
    fn as_micros(self) -> Option<Rates> {
        Some(Rates {
            read_micros: micros(self.read_per_million_usd)?,
            rewrite_micros: micros(self.rewrite_per_million_usd)?,
            input_micros: micros(self.input_per_million_usd)?,
            output_micros: micros(self.output_per_million_usd)?,
        })
    }
}

/// Refresh at five sixths of the TTL, leaving at least ten seconds of margin.
pub fn warming_delay(ttl: Duration) -> Option<Duration> {
    let ttl_ms = ttl.as_millis();
    if ttl_ms <= 10_000 {
        return None;
    }
    let five_sixths = ttl_ms.saturating_mul(5) / 6;
    let delay = five_sixths.clamp(1_000, ttl_ms - 10_000);
    u64::try_from(delay).ok().map(Duration::from_millis)
}

fn micros(rate: DecimalUsd) -> Option<u128> {
    let text = rate.to_string();
    let (whole, fraction) = text.split_once('.').unwrap_or((text.as_str(), ""));
    let whole: u128 = whole.parse().ok()?;
    let mut fraction = fraction.to_string();
    if fraction.len() > 6 {
        fraction.truncate(6);
    }
    while fraction.len() < 6 {
        fraction.push('0');
    }
    let fraction: u128 = fraction.parse().ok()?;
    whole.checked_mul(1_000_000)?.checked_add(fraction)
}

fn token_cost(tokens: u32, per_million_micros: u128) -> Option<u128> {
    u128::from(tokens)
        .checked_mul(per_million_micros)?
        .checked_div(1_000_000)
}

fn refresh_cost(rates: Rates, cached: u32, uncached: u32, output_tokens: u32) -> Option<u128> {
    token_cost(cached, rates.read_micros)?
        .checked_add(token_cost(uncached, rates.input_micros)?)?
        .checked_add(token_cost(output_tokens, rates.output_micros)?)
}

fn real_now() -> Duration {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed()
}

fn registry() -> &'static Mutex<Controller> {
    static REGISTRY: OnceLock<Mutex<Controller>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Controller::new()))
}

fn lock() -> std::sync::MutexGuard<'static, Controller> {
    registry()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

pub fn install(config: &CaConfig) {
    let mut controller = lock();
    for (key, pricing) in &config.pricing {
        let Some(read) = pricing.cached_input_per_million_usd else {
            continue;
        };
        let rewrite = pricing
            .cache_write_input_per_million_usd
            .unwrap_or(pricing.input_per_million_usd);
        let ttl = config
            .model_prompt_cache
            .get(key)
            .and_then(|cache| cache.ttl_seconds)
            .map(Duration::from_secs);
        controller.set_route(
            key,
            ttl,
            Some(WarmRates {
                read_per_million_usd: read,
                rewrite_per_million_usd: rewrite,
                input_per_million_usd: pricing.input_per_million_usd,
                output_per_million_usd: pricing.output_per_million_usd,
            }),
        );
    }
}

pub fn begin_user_message() {
    lock().begin_user_message();
}

pub fn note_usage(route: &str, cached_tokens: u32, prompt_tokens: u32) {
    let uncached = prompt_tokens.saturating_sub(cached_tokens);
    let mut controller = lock();
    controller.sync_real_clock();
    controller.note_usage(route, cached_tokens, uncached);
}

pub fn decide(route: &str, pace: Pace, tool_in_flight: bool) -> Decision {
    lock().decide(route, pace, tool_in_flight)
}

pub fn reserve(route: &str, pace: Pace, tool_in_flight: bool, generation: u64) -> bool {
    lock().reserve(route, pace, tool_in_flight, generation)
}

pub fn observe_keepalive(route: &str, cache_write_tokens: Option<u32>) {
    let mut controller = lock();
    controller.sync_real_clock();
    controller.observe_keepalive(route, cache_write_tokens);
}

pub fn generation() -> u64 {
    lock().generation()
}

pub fn temperature(provider: &str, model: &str) -> Temperature {
    let mut controller = lock();
    controller.sync_real_clock();
    let exact = format!("{provider}/{model}");
    match controller.temperature(&exact) {
        Temperature::Unknown => controller.temperature(model),
        temperature => temperature,
    }
}

/// Replay the last request after the priced delay. The replay is not a transcript
/// turn: tools are stripped, output is capped, and the stream is drained.
pub fn spawn_keepalive(
    client: crate::agent::AgentClient,
    mut prepared: crate::request::PreparedModelRequest,
    pace: Pace,
    tool_in_flight: bool,
    mut stop: Option<tokio::sync::watch::Receiver<bool>>,
) {
    let route = format!("{}/{}", prepared.provider, prepared.model);
    let Decision::Replay { delay, output_cap } = decide(&route, pace, tool_in_flight) else {
        return;
    };
    let planned = generation();
    tokio::spawn(async move {
        let sleep = tokio::time::sleep(delay);
        tokio::pin!(sleep);
        if let Some(stop) = stop.as_mut() {
            tokio::select! {
                _ = &mut sleep => {}
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() {
                        return;
                    }
                }
            }
            if *stop.borrow() {
                return;
            }
        } else {
            sleep.await;
        }
        if generation() != planned {
            return;
        }
        if !reserve(&route, pace, tool_in_flight, planned) {
            return;
        }
        prepared.tools.clear();
        let Ok(client) = client.with_output_token_cap(output_cap) else {
            return;
        };
        let Ok(mut stream) = client.stream_prepared(&prepared).await else {
            return;
        };
        let mut writes = None;
        let _ = crate::agent::stream::consume_stream(&mut stream, |event| {
            if let crate::agent::stream::ProviderEvent::Usage(usage) = event {
                writes = usage.cache_write_tokens;
            }
        })
        .await;
        observe_keepalive(&route, writes);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rates(read: &str, rewrite: &str) -> WarmRates {
        WarmRates {
            read_per_million_usd: read.parse().unwrap(),
            rewrite_per_million_usd: rewrite.parse().unwrap(),
            input_per_million_usd: "3".parse().unwrap(),
            output_per_million_usd: "15".parse().unwrap(),
        }
    }

    fn ready(controller: &mut Controller) {
        controller.set_route(
            "anthropic/claude",
            Some(Duration::from_secs(300)),
            Some(rates("0.30", "3.75")),
        );
        controller.note_usage("anthropic/claude", 4_000, 100);
    }

    #[test]
    fn priced_prompt_cache_warmer_replays_at_five_sixths_and_stops_on_the_first_miss() {
        let mut controller = Controller::new();
        ready(&mut controller);
        let decision = controller.decide("anthropic/claude", Pace::Human, false);
        assert_eq!(
            decision,
            Decision::Replay {
                delay: Duration::from_secs(250),
                output_cap: KEEPALIVE_OUTPUT_CAP,
            }
        );
        let generation = controller.generation();
        assert!(controller.reserve("anthropic/claude", Pace::Human, false, generation));
        controller.observe_keepalive("anthropic/claude", Some(4_000));
        assert_eq!(
            controller.decide("anthropic/claude", Pace::Human, false),
            Decision::Skip {
                reason: "prompt cache warming disabled for session: keep-alive missed",
            }
        );
        controller.begin_user_message();
        assert!(matches!(
            controller.decide("anthropic/claude", Pace::Human, false),
            Decision::Skip {
                reason: "prompt cache warming disabled for session: keep-alive missed"
            }
        ));
    }

    #[test]
    fn unknown_ttl_or_price_sends_nothing_and_budgets_are_separate() {
        let mut controller = Controller::new();
        controller.set_route("plain/model", None, Some(rates("0.30", "3.75")));
        controller.note_usage("plain/model", 4_000, 0);
        assert_eq!(
            controller.decide("plain/model", Pace::Human, false),
            Decision::Skip {
                reason: "unknown ttl"
            }
        );
        controller.set_route("priced/model", Some(Duration::from_secs(60)), None);
        controller.note_usage("priced/model", 4_000, 0);
        assert_eq!(
            controller.decide("priced/model", Pace::Human, false),
            Decision::Skip {
                reason: "unknown price"
            }
        );

        ready(&mut controller);
        let generation = controller.generation();
        for _ in 0..WARM_BUDGET {
            assert!(controller.reserve("anthropic/claude", Pace::Human, false, generation));
        }
        assert!(!controller.reserve("anthropic/claude", Pace::Human, false, generation));
        assert_eq!(
            controller.decide("anthropic/claude", Pace::Human, false),
            Decision::Skip {
                reason: "idle keep-alive budget is spent"
            }
        );
        assert!(matches!(
            controller.decide("anthropic/claude", Pace::Machine, true),
            Decision::Replay { .. }
        ));
        assert_eq!(
            controller.decide("anthropic/claude", Pace::Machine, false),
            Decision::Skip {
                reason: "a machine session warms only while a tool runs"
            }
        );
        controller.begin_user_message();
        assert!(matches!(
            controller.decide("anthropic/claude", Pace::Human, false),
            Decision::Replay { .. }
        ));
    }

    #[test]
    fn a_read_that_is_not_cheaper_than_a_rewrite_does_not_warm() {
        let mut controller = Controller::new();
        controller.set_route(
            "even/model",
            Some(Duration::from_secs(300)),
            Some(rates("3", "3")),
        );
        controller.note_usage("even/model", 4_000, 0);
        assert_eq!(
            controller.decide("even/model", Pace::Human, false),
            Decision::Skip {
                reason: "a refresh would cost more than the cache write it prevents"
            }
        );
        assert_eq!(controller.temperature("missing"), Temperature::Unknown);
        controller.advance(Duration::from_secs(299));
        assert_eq!(controller.temperature("even/model"), Temperature::Warm);
        controller.advance(Duration::from_secs(1));
        assert_eq!(controller.temperature("even/model"), Temperature::Cold);
    }

    #[test]
    fn five_sixths_of_an_hour_leaves_ten_seconds_of_margin() {
        assert_eq!(
            warming_delay(Duration::from_secs(3_600)),
            Some(Duration::from_secs(3_000))
        );
        assert_eq!(warming_delay(Duration::from_secs(10)), None);
    }
}
