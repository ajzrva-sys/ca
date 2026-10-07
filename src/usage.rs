use crate::agent::stream::UsageInfo;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_DECIMAL_SCALE: usize = 12;
const MAX_USAGE_RECORDS: usize = 2048;
const REPORT_RECORDS: usize = 50;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DecimalUsd {
    coefficient: u128,
    scale: u8,
}

impl Ord for DecimalUsd {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let scale = self.scale.max(other.scale);
        let mut left = self.coefficient.to_string();
        left.extend(std::iter::repeat_n(
            '0',
            usize::from(scale.saturating_sub(self.scale)),
        ));
        let mut right = other.coefficient.to_string();
        right.extend(std::iter::repeat_n(
            '0',
            usize::from(scale.saturating_sub(other.scale)),
        ));
        left.len()
            .cmp(&right.len())
            .then_with(|| left.as_bytes().cmp(right.as_bytes()))
    }
}

impl PartialOrd for DecimalUsd {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl DecimalUsd {
    pub const ZERO: Self = Self {
        coefficient: 0,
        scale: 0,
    };

    fn normalize(mut self) -> Self {
        while self.scale > 0 && self.coefficient.is_multiple_of(10) {
            self.coefficient /= 10;
            self.scale -= 1;
        }
        self
    }

    fn checked_component(self, tokens: u32) -> Option<Self> {
        Some(
            Self {
                coefficient: self.coefficient.checked_mul(u128::from(tokens))?,
                scale: self.scale.checked_add(6)?,
            }
            .normalize(),
        )
    }

    pub(crate) fn checked_add(self, other: Self) -> Option<Self> {
        let scale = self.scale.max(other.scale);
        let left = self
            .coefficient
            .checked_mul(pow10(u32::from(scale - self.scale))?)?;
        let right = other
            .coefficient
            .checked_mul(pow10(u32::from(scale - other.scale))?)?;
        Some(
            Self {
                coefficient: left.checked_add(right)?,
                scale,
            }
            .normalize(),
        )
    }

    pub(crate) const fn coefficient(self) -> u128 {
        self.coefficient
    }

    pub(crate) const fn scale(self) -> u8 {
        self.scale
    }

    pub(crate) fn from_parts(coefficient: u128, scale: u8) -> Option<Self> {
        (usize::from(scale) <= MAX_DECIMAL_SCALE).then(|| Self { coefficient, scale }.normalize())
    }
}

fn pow10(exponent: u32) -> Option<u128> {
    10_u128.checked_pow(exponent)
}

impl FromStr for DecimalUsd {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() || value.trim() != value {
            return Err("price must be a quoted non-negative decimal".into());
        }
        let (integer, fraction) = value.split_once('.').map_or((value, ""), |parts| parts);
        if integer.is_empty()
            || !integer.bytes().all(|byte| byte.is_ascii_digit())
            || (!fraction.is_empty() && !fraction.bytes().all(|byte| byte.is_ascii_digit()))
            || (value.contains('.') && fraction.is_empty())
        {
            return Err("price must use plain decimal digits without a sign or exponent".into());
        }
        if fraction.len() > MAX_DECIMAL_SCALE {
            return Err(format!(
                "price supports at most {MAX_DECIMAL_SCALE} decimal places"
            ));
        }
        let digits = format!("{integer}{fraction}");
        let coefficient = digits
            .parse::<u128>()
            .map_err(|_| "price is too large".to_string())?;
        Ok(Self {
            coefficient,
            scale: fraction.len() as u8,
        }
        .normalize())
    }
}

impl fmt::Display for DecimalUsd {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.scale == 0 {
            return write!(formatter, "{}", self.coefficient);
        }
        let scale = usize::from(self.scale);
        let mut digits = self.coefficient.to_string();
        if digits.len() <= scale {
            digits.insert_str(0, &"0".repeat(scale + 1 - digits.len()));
        }
        let split = digits.len() - scale;
        write!(formatter, "{}.{}", &digits[..split], &digits[split..])
    }
}

impl fmt::Debug for DecimalUsd {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("DecimalUsd")
            .field(&self.to_string())
            .finish()
    }
}

impl Serialize for DecimalUsd {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for DecimalUsd {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(serde::de::Error::custom)
    }
}

/// How cache-write tokens affect the input bill.
///
/// A write is always a subset of cache misses. `Included` therefore means the
/// write operation has no incremental charge; the tokens still pay the miss
/// rate. A genuinely free written token is represented by `Replacement` with a
/// zero rate.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum CacheWriteBilling<R> {
    Replacement { rate: R },
    Surcharge { rate: R },
    Included,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UsagePricing {
    pub input_per_million_usd: DecimalUsd,
    pub output_per_million_usd: DecimalUsd,
    #[serde(default)]
    pub cached_input_per_million_usd: Option<DecimalUsd>,
    #[serde(default)]
    pub cache_write_input_per_million_usd: Option<DecimalUsd>,
    #[serde(default)]
    pub reasoning_output_per_million_usd: Option<DecimalUsd>,
}

impl UsagePricing {
    /// Legacy profile pricing has always treated a cache-write rate as a
    /// replacement rate. An omitted rate is the ordinary miss rate, preserving
    /// the legacy configuration shape and totals.
    pub fn cache_write_billing(&self) -> CacheWriteBilling<DecimalUsd> {
        CacheWriteBilling::Replacement {
            rate: self
                .cache_write_input_per_million_usd
                .unwrap_or(self.input_per_million_usd),
        }
    }
}

pub fn usage_details_valid(usage: &UsageInfo) -> bool {
    usage
        .prompt_tokens
        .checked_add(usage.completion_tokens)
        .is_some_and(|total| total == usage.total_tokens)
        && usage.cache_usage().valid_for_prompt(usage.prompt_tokens)
        && usage
            .reasoning_tokens
            .is_none_or(|reasoning| reasoning <= usage.completion_tokens)
}

pub fn cost_for(pricing: &UsagePricing, usage: &UsageInfo) -> Option<DecimalUsd> {
    cost_for_with_cache_write_billing(pricing, pricing.cache_write_billing(), usage)
}

pub fn cost_for_with_cache_write_billing(
    pricing: &UsagePricing,
    cache_write_billing: CacheWriteBilling<DecimalUsd>,
    usage: &UsageInfo,
) -> Option<DecimalUsd> {
    if !usage_details_valid(usage) {
        return None;
    }
    let reasoning = usage.reasoning_tokens.unwrap_or(0);
    let regular_output = usage.completion_tokens - reasoning;
    let cached_rate = pricing
        .cached_input_per_million_usd
        .unwrap_or(pricing.input_per_million_usd);
    let reasoning_rate = pricing
        .reasoning_output_per_million_usd
        .unwrap_or(pricing.output_per_million_usd);
    [
        cache_input_cost(
            pricing.input_per_million_usd,
            cached_rate,
            cache_write_billing,
            usage,
        )?,
        pricing
            .output_per_million_usd
            .checked_component(regular_output)?,
        reasoning_rate.checked_component(reasoning)?,
    ]
    .into_iter()
    .try_fold(DecimalUsd::ZERO, DecimalUsd::checked_add)
}

fn cache_input_cost(
    miss_rate: DecimalUsd,
    read_rate: DecimalUsd,
    write_billing: CacheWriteBilling<DecimalUsd>,
    usage: &UsageInfo,
) -> Option<DecimalUsd> {
    let cache = usage.cache_usage();
    let prompt = usage.prompt_tokens;

    let base = if read_rate == miss_rate {
        miss_rate.checked_component(prompt)?
    } else {
        let read = cache.read.tokens()?;
        let miss = cache.miss.tokens()?;
        read_rate
            .checked_component(read)?
            .checked_add(miss_rate.checked_component(miss)?)?
    };

    match write_billing {
        CacheWriteBilling::Included => Some(base),
        CacheWriteBilling::Surcharge { rate } if rate == DecimalUsd::ZERO => Some(base),
        CacheWriteBilling::Surcharge { rate } => {
            base.checked_add(rate.checked_component(cache.write.tokens()?)?)
        }
        CacheWriteBilling::Replacement { rate } if rate == miss_rate => Some(base),
        CacheWriteBilling::Replacement { rate } => {
            let write = cache.write.tokens()?;
            let miss = if read_rate == miss_rate {
                prompt
            } else {
                cache.miss.tokens()?
            };
            let non_write = miss.checked_sub(write)?;
            let read = if read_rate == miss_rate {
                DecimalUsd::ZERO
            } else {
                read_rate.checked_component(cache.read.tokens()?)?
            };
            read.checked_add(miss_rate.checked_component(non_write)?)?
                .checked_add(rate.checked_component(write)?)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UsageKind {
    MainTurn,
    ToolContinuation,
    Check,
    ContextCheckpoint,
    BranchHandoff,
    ProjectInit,
    Review,
    Subagent,
    ProviderProbe,
    Btw,
    SessionTitle,
    GoalEvaluation,
    WorkflowStep,
    MultiRunCandidate,
    MultiRunJudge,
    MultiRunFusion,
    ChangeWalkthrough,
    VoiceTranscription,
    Dream,
    Distill,
    Analyze,
}

impl UsageKind {
    pub const ALL: [Self; 21] = [
        Self::MainTurn,
        Self::ToolContinuation,
        Self::Check,
        Self::ContextCheckpoint,
        Self::BranchHandoff,
        Self::ProjectInit,
        Self::Review,
        Self::Subagent,
        Self::ProviderProbe,
        Self::Btw,
        Self::SessionTitle,
        Self::GoalEvaluation,
        Self::WorkflowStep,
        Self::MultiRunCandidate,
        Self::MultiRunJudge,
        Self::MultiRunFusion,
        Self::ChangeWalkthrough,
        Self::VoiceTranscription,
        Self::Dream,
        Self::Distill,
        Self::Analyze,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::MainTurn => "main turn",
            Self::ToolContinuation => "tool continuation",
            Self::Check => "check",
            Self::ContextCheckpoint => "context checkpoint",
            Self::BranchHandoff => "branch handoff",
            Self::ProjectInit => "project initialization",
            Self::Review => "review",
            Self::Subagent => "subagent",
            Self::ProviderProbe => "provider probe",
            Self::Btw => "btw",
            Self::SessionTitle => "session title",
            Self::GoalEvaluation => "goal evaluation",
            Self::WorkflowStep => "workflow step",
            Self::MultiRunCandidate => "multi-run candidate",
            Self::MultiRunJudge => "multi-run judge",
            Self::MultiRunFusion => "multi-run fusion",
            Self::ChangeWalkthrough => "change walkthrough",
            Self::VoiceTranscription => "voice transcription",
            Self::Dream => "dream maintenance",
            Self::Distill => "distill maintenance",
            Self::Analyze => "analyze maintenance",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UsageOutcome {
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl UsageOutcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageHandle(u64);

impl UsageHandle {
    #[cfg(test)]
    pub fn test_handle() -> Self {
        Self(0)
    }

    #[cfg(test)]
    pub fn id(self) -> u64 {
        self.0
    }
}

#[derive(Debug)]
struct ActiveUsageRecord {
    id: u64,
    kind: UsageKind,
    provider: String,
    model: String,
    started: Instant,
    started_at: chrono::DateTime<chrono::Utc>,
    finished_at: Option<chrono::DateTime<chrono::Utc>>,
    duration: Option<Duration>,
    generation_duration: Option<Duration>,
    usage: Option<UsageInfo>,
    outcome: UsageOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRecord {
    pub id: u64,
    pub kind: UsageKind,
    pub provider: String,
    pub model: String,
    pub duration: Duration,
    pub generation_duration: Option<Duration>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub usage: Option<UsageInfo>,
    pub outcome: UsageOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSnapshot {
    pub records: Vec<UsageRecord>,
    pub evicted_records: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UsageCursor {
    after_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageWindow {
    pub snapshot: UsageSnapshot,
    pub dropped_since_cursor: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageTotals {
    pub requests: usize,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub total_latency: Duration,
    pub first_output_latency: Duration,
    pub first_output_latency_samples: usize,
    pub known_usage: usize,
    pub cache_read_unknown_requests: usize,
    pub cache_write_unknown_requests: usize,
    pub cache_miss_unknown_requests: usize,
    pub cache_detail_unknown_requests: usize,
    pub priced_requests: usize,
    pub incremental_cost: crate::pricing::ExactRational,
    pub upper_bound_requests: usize,
    pub allocation_cost: crate::pricing::ExactRational,
    pub allocation_credits: crate::pricing::ExactRational,
    pub allocation_requests: usize,
    pub by_kind: std::collections::BTreeMap<UsageKind, usize>,
    pub by_outcome: std::collections::BTreeMap<UsageOutcome, usize>,
}

impl UsageTotals {
    pub fn new_input_tokens(&self) -> Option<u64> {
        (self.known_usage > 0 && self.cache_read_unknown_requests == 0)
            .then(|| self.prompt_tokens.saturating_sub(self.cached_input_tokens))
    }

    pub fn cache_hit_ratio_tenths_percent(&self) -> Option<u64> {
        if self.known_usage == 0 || self.cache_read_unknown_requests > 0 || self.prompt_tokens == 0
        {
            return None;
        }
        u64::try_from(
            u128::from(self.cached_input_tokens)
                .saturating_mul(1_000)
                .checked_div(u128::from(self.prompt_tokens))?,
        )
        .ok()
    }

    pub fn tool_round_trips(&self) -> usize {
        self.by_kind
            .get(&UsageKind::ToolContinuation)
            .copied()
            .unwrap_or(0)
    }

    pub fn average_total_latency(&self) -> Option<Duration> {
        average_duration(self.total_latency, self.requests)
    }

    pub fn average_first_output_latency(&self) -> Option<Duration> {
        average_duration(self.first_output_latency, self.first_output_latency_samples)
    }

    pub fn unknown_usage(&self) -> usize {
        self.requests.saturating_sub(self.known_usage)
    }

    pub fn unknown_cost(&self) -> usize {
        self.known_usage.saturating_sub(self.priced_requests)
    }
}

fn average_duration(total: Duration, samples: usize) -> Option<Duration> {
    let samples = u32::try_from(samples).ok().filter(|samples| *samples > 0)?;
    Some(total / samples)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SittingCostPresentation {
    #[default]
    Unknown,
    Incremental {
        usd: String,
    },
    UpperBound {
        usd: String,
        allocation_credits: Option<String>,
        unknown_requests: usize,
    },
    SubscriptionAllocation {
        credits: String,
    },
    Mixed {
        incremental_usd: Option<String>,
        allocation_credits: Option<String>,
        unknown_requests: usize,
    },
}

impl SittingCostPresentation {
    pub fn compact_label(&self) -> String {
        match self {
            Self::Unknown => "cost unknown".into(),
            Self::Incremental { usd } => format!("${usd} cost"),
            Self::UpperBound {
                usd,
                allocation_credits,
                unknown_requests,
            } => {
                let mut parts = vec![format!("≤${usd} cost (peak-rate upper bound)")];
                if let Some(credits) = allocation_credits {
                    parts.push(format!("{credits} credits allocation"));
                }
                if *unknown_requests > 0 {
                    parts.push(format!("{unknown_requests} unknown"));
                }
                parts.join(" · ")
            }
            Self::SubscriptionAllocation { credits } => {
                format!("{credits} credits allocation")
            }
            Self::Mixed {
                incremental_usd,
                allocation_credits,
                unknown_requests,
            } => {
                let mut parts = Vec::new();
                if let Some(usd) = incremental_usd {
                    parts.push(format!("${usd}"));
                }
                if let Some(credits) = allocation_credits {
                    parts.push(format!("{credits} credits"));
                }
                if *unknown_requests > 0 {
                    parts.push(format!("{unknown_requests} unknown"));
                }
                format!("mixed cost {}", parts.join(" + "))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SittingUsageSummary {
    pub totals: UsageTotals,
    pub latest_generation_speed_tenths: Option<u64>,
    pub cost: SittingCostPresentation,
}

#[derive(Debug, Default)]
pub struct UsageLedger {
    next_id: u64,
    records: VecDeque<ActiveUsageRecord>,
    evicted_records: u64,
}

impl UsageLedger {
    pub fn cursor(&self) -> UsageCursor {
        UsageCursor {
            after_id: self.next_id,
        }
    }

    pub fn snapshot_since(&self, cursor: UsageCursor) -> UsageWindow {
        let records = self
            .snapshot()
            .records
            .into_iter()
            .filter(|record| record.id > cursor.after_id)
            .collect::<Vec<_>>();
        let total_since = self.next_id.saturating_sub(cursor.after_id);
        let dropped_since_cursor = total_since.saturating_sub(records.len() as u64);
        UsageWindow {
            snapshot: UsageSnapshot {
                records,
                evicted_records: dropped_since_cursor,
            },
            dropped_since_cursor,
        }
    }

    pub fn begin(&mut self, kind: UsageKind, provider: &str, model: &str) -> UsageHandle {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        if self.records.len() == MAX_USAGE_RECORDS {
            self.records.pop_front();
            self.evicted_records = self.evicted_records.saturating_add(1);
        }
        let handle = UsageHandle(self.next_id);
        self.records.push_back(ActiveUsageRecord {
            id: handle.0,
            kind,
            provider: provider.into(),
            model: model.into(),
            started: Instant::now(),
            started_at: chrono::Utc::now(),
            finished_at: None,
            duration: None,
            generation_duration: None,
            usage: None,
            outcome: UsageOutcome::Running,
        });
        handle
    }

    pub fn finish(
        &mut self,
        handle: UsageHandle,
        usage: Option<UsageInfo>,
        outcome: UsageOutcome,
    ) -> bool {
        self.finish_with_generation_duration(handle, usage, outcome, None)
    }

    pub fn finish_with_generation_duration(
        &mut self,
        handle: UsageHandle,
        usage: Option<UsageInfo>,
        outcome: UsageOutcome,
        generation_duration: Option<Duration>,
    ) -> bool {
        if outcome == UsageOutcome::Running {
            return false;
        }
        let Some(record) = self
            .records
            .iter_mut()
            .find(|record| record.id == handle.0 && record.outcome == UsageOutcome::Running)
        else {
            return false;
        };
        record.duration = Some(record.started.elapsed());
        record.generation_duration = generation_duration;
        record.finished_at = Some(chrono::Utc::now());
        record.usage = usage;
        record.outcome = outcome;
        true
    }

    pub fn snapshot(&self) -> UsageSnapshot {
        UsageSnapshot {
            records: self
                .records
                .iter()
                .map(|record| UsageRecord {
                    id: record.id,
                    kind: record.kind,
                    provider: record.provider.clone(),
                    model: record.model.clone(),
                    duration: record.duration.unwrap_or_else(|| record.started.elapsed()),
                    generation_duration: record.generation_duration,
                    started_at: record.started_at,
                    finished_at: record.finished_at,
                    usage: record.usage.clone(),
                    outcome: record.outcome,
                })
                .collect(),
            evicted_records: self.evicted_records,
        }
    }

    #[cfg(test)]
    pub fn push_test_record(
        &mut self,
        kind: UsageKind,
        provider: &str,
        model: &str,
        duration: Duration,
        usage: Option<UsageInfo>,
        outcome: UsageOutcome,
    ) {
        let handle = self.begin(kind, provider, model);
        let record = self
            .records
            .back_mut()
            .expect("begin inserts one usage record");
        record.started = Instant::now()
            .checked_sub(duration)
            .unwrap_or_else(Instant::now);
        record.started_at = chrono::Utc::now()
            - chrono::Duration::from_std(duration).unwrap_or(chrono::Duration::zero());
        assert!(self.finish(handle, usage, outcome));
        self.records
            .back_mut()
            .expect("record remains present")
            .duration = Some(duration);
    }
}

#[derive(Clone)]
pub struct UsageRecorder {
    ledger: Arc<Mutex<UsageLedger>>,
    kind: UsageKind,
    provider: String,
    model: String,
}

impl UsageRecorder {
    pub fn new(
        ledger: Arc<Mutex<UsageLedger>>,
        kind: UsageKind,
        provider: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            ledger,
            kind,
            provider: provider.into(),
            model: model.into(),
        }
    }

    pub fn begin(&self) -> UsageSpan {
        let handle = self
            .ledger
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .begin(self.kind, &self.provider, &self.model);
        UsageSpan {
            ledger: self.ledger.clone(),
            handle,
            finished: false,
            last_usage: None,
        }
    }

    pub fn reroute(
        &self,
        kind: UsageKind,
        provider: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self::new(self.ledger.clone(), kind, provider, model)
    }
}

pub struct UsageSpan {
    ledger: Arc<Mutex<UsageLedger>>,
    handle: UsageHandle,
    finished: bool,
    last_usage: Option<UsageInfo>,
}

impl UsageSpan {
    pub fn observe(&mut self, usage: &UsageInfo) {
        self.last_usage = Some(usage.clone());
    }

    pub fn complete(mut self, usage: Option<UsageInfo>) {
        self.finish(usage, UsageOutcome::Completed);
    }

    pub fn fail(mut self, usage: Option<UsageInfo>) {
        self.finish(usage, UsageOutcome::Failed);
    }

    fn finish(&mut self, usage: Option<UsageInfo>, outcome: UsageOutcome) {
        if self.finished {
            return;
        }
        self.ledger
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .finish(
                self.handle,
                usage.or_else(|| self.last_usage.clone()),
                outcome,
            );
        self.finished = true;
    }
}

impl Drop for UsageSpan {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(None, UsageOutcome::Cancelled);
        }
    }
}

#[cfg(test)]
pub fn render_report(snapshot: &UsageSnapshot, pricing: &HashMap<String, UsagePricing>) -> String {
    render_report_with_official(snapshot, pricing, None, &HashMap::new(), chrono::Utc::now())
}

#[cfg(test)]
pub fn render_report_with_official(
    snapshot: &UsageSnapshot,
    pricing: &HashMap<String, UsagePricing>,
    official: Option<&crate::pricing::OfficialPriceCatalog>,
    billing: &HashMap<String, crate::pricing::ProviderBillingConfig>,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    render_report_with_routes(snapshot, pricing, official, billing, None, now)
}

fn render_report_with_routes(
    snapshot: &UsageSnapshot,
    pricing: &HashMap<String, UsagePricing>,
    official: Option<&crate::pricing::OfficialPriceCatalog>,
    billing: &HashMap<String, crate::pricing::ProviderBillingConfig>,
    providers: Option<&HashMap<String, crate::config::ProviderConfig>>,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    let totals = summarize_usage_with_routes(snapshot, pricing, official, billing, providers, now);
    let prompt = totals.prompt_tokens;
    let completion = totals.completion_tokens;
    let cached = totals.cached_input_tokens;
    let cache_write = totals.cache_write_tokens;
    let reasoning = totals.reasoning_output_tokens;
    let tool_round_trips = totals.tool_round_trips();
    let incremental_cost = totals.incremental_cost;
    let allocation_cost = totals.allocation_cost;
    let allocation_credits = totals.allocation_credits;
    let allocation_requests = totals.allocation_requests;
    let priced = totals.priced_requests;
    let unknown_usage = totals.unknown_usage();
    let unknown_cost = totals.unknown_cost();
    let mut lines = vec![
        "Usage · current ca process".to_string(),
        String::new(),
        format!("Requests: {}", snapshot.records.len()),
        format!("Tokens: {prompt} input · {completion} output"),
        format!(
            "Subsets: {cached} cached input · {cache_write} cache write · {reasoning} reasoning output"
        ),
        format!(
            "Cost: {}",
            if priced == 0 {
                "unknown".into()
            } else if incremental_cost.numerator() == 0 && allocation_requests > 0 {
                "no incremental charge".into()
            } else {
                format!(
                    "{}${} incremental{}",
                    if totals.upper_bound_requests > 0 { "≤" } else { "" },
                    (if totals.upper_bound_requests > 0 {
                        incremental_cost.ceiling_decimal(6)
                    } else {
                        incremental_cost.rounded_decimal(6)
                    })
                        .unwrap_or(DecimalUsd::ZERO),
                    if totals.upper_bound_requests > 0 { " · peak-rate upper bound" } else { "" },
                )
            }
        ),
    ];
    if let (Some(new_input), Some(hit_ratio)) = (
        totals.new_input_tokens(),
        totals.cache_hit_ratio_tenths_percent(),
    ) {
        lines.push(format!(
            "Input cache: {cached} cached · {new_input} new input · {}.{}% cache hit",
            hit_ratio / 10,
            hit_ratio % 10
        ));
    } else {
        let reason = if totals.known_usage == 0 {
            "no provider token usage is available".into()
        } else if totals.prompt_tokens == 0 {
            "reported input is zero".into()
        } else {
            format!(
                "{} request(s) missing cache-read detail",
                totals.cache_read_unknown_requests
            )
        };
        lines.push(format!(
            "Input cache: {cached} cached where reported · new input/cache hit unavailable ({reason})"
        ));
    }
    lines.push(format!(
        "Tool round trips: {tool_round_trips} tool round {}",
        if tool_round_trips == 1 {
            "trip"
        } else {
            "trips"
        }
    ));
    let average_total = totals
        .average_total_latency()
        .map_or_else(|| "unavailable".into(), format_latency);
    let average_first = totals
        .average_first_output_latency()
        .map_or_else(|| "unavailable".into(), format_latency);
    lines.push(format!(
        "Latency: {average_total} average total · {average_first} average to first output ({} of {} request(s) measured)",
        totals.first_output_latency_samples, totals.requests
    ));
    if allocation_requests > 0 {
        lines.push(format!(
            "Subscription allocation: {allocation_credits} credits · ~${}",
            allocation_cost
                .rounded_decimal(6)
                .unwrap_or(DecimalUsd::ZERO)
        ));
        lines.push(format!(
            "MiMo Token Plan: {}",
            crate::pricing::MIMO_OFF_PEAK_COPY
        ));
    }
    if unknown_cost > 0 {
        lines.push(format!("Cost unknown for {unknown_cost} request(s)"));
    }
    if unknown_usage > 0 {
        lines.push(format!(
            "Token usage unavailable for {unknown_usage} request(s)"
        ));
    }
    if snapshot.evicted_records > 0 {
        lines.push(format!(
            "{} older request(s) rolled out of the process view",
            snapshot.evicted_records
        ));
    }
    lines.push(String::new());
    lines.push("Requests by kind".into());
    for kind in UsageKind::ALL {
        if matches!(
            kind,
            UsageKind::VoiceTranscription | UsageKind::SessionTitle | UsageKind::BranchHandoff
        ) {
            continue;
        }
        let count = totals.by_kind.get(&kind).copied().unwrap_or(0);
        if kind == UsageKind::ContextCheckpoint {
            let handoffs = totals
                .by_kind
                .get(&UsageKind::BranchHandoff)
                .copied()
                .unwrap_or(0);
            lines.push(format!(
                "  {}: {count} · {}: {handoffs}",
                kind.label(),
                UsageKind::BranchHandoff.label()
            ));
        } else if kind == UsageKind::ProviderProbe {
            let voice = totals
                .by_kind
                .get(&UsageKind::VoiceTranscription)
                .copied()
                .unwrap_or(0);
            lines.push(format!(
                "  {}: {count} · {}: {voice}",
                kind.label(),
                UsageKind::VoiceTranscription.label()
            ));
        } else if kind == UsageKind::Btw {
            let titles = totals
                .by_kind
                .get(&UsageKind::SessionTitle)
                .copied()
                .unwrap_or(0);
            lines.push(format!(
                "  {}: {count} · {}: {titles}",
                kind.label(),
                UsageKind::SessionTitle.label()
            ));
        } else {
            lines.push(format!("  {}: {count}", kind.label()));
        }
    }
    lines.push(String::new());
    lines.push("Newest requests".into());
    if snapshot.records.is_empty() {
        lines.push("  No provider requests recorded yet".into());
    }
    for record in snapshot.records.iter().rev().take(REPORT_RECORDS) {
        let duration = format_latency(record.duration);
        let first_output = record
            .generation_duration
            .and_then(|generation| record.duration.checked_sub(generation))
            .map_or_else(
                || "first output unknown".into(),
                |latency| format!("first output {}", format_latency(latency)),
            );
        let usage = record.usage.as_ref().map_or_else(
            || "tokens unknown".into(),
            |usage| {
                let mut label = format!(
                    "{} in · {} out",
                    usage.prompt_tokens, usage.completion_tokens
                );
                if let Some(cached) = usage.cached_prompt_tokens {
                    label.push_str(&format!(" · {cached} cached"));
                }
                if let Some(cache_write) = usage.cache_write_tokens {
                    label.push_str(&format!(" · {cache_write} cache write"));
                }
                if let Some(reasoning) = usage.reasoning_tokens {
                    label.push_str(&format!(" · {reasoning} reasoning"));
                }
                if !usage_details_valid(usage) {
                    label.push_str(" · invalid provider subsets");
                }
                label
            },
        );
        let cost = reported_cost(record, pricing, official, billing, providers, now)
            .map_or_else(|| "cost unknown".into(), crate::pricing::ActualCost::render);
        lines.push(format!(
            "  #{} {} · {}/{} · {} · total {duration} · {first_output}",
            record.id,
            record.kind.label(),
            record.provider,
            record.model,
            record.outcome.label(),
        ));
        lines.push(format!("     {usage} · {cost}"));
    }
    lines.push(String::new());
    lines.push(
        "Official fresh cards are preferred; legacy profile prices are display-only fallbacks for unknown routes."
            .into(),
    );
    lines.join("\n")
}

pub fn format_latency(duration: Duration) -> String {
    if duration < Duration::from_secs(1) {
        format!("{} ms", duration.as_millis())
    } else {
        format!("{:.1} s", duration.as_secs_f64())
    }
}

pub fn render_report_with_provider_account(
    snapshot: &UsageSnapshot,
    pricing: &HashMap<String, UsagePricing>,
    official: Option<&crate::pricing::OfficialPriceCatalog>,
    billing: &HashMap<String, crate::pricing::ProviderBillingConfig>,
    providers: &HashMap<String, crate::config::ProviderConfig>,
    now: chrono::DateTime<chrono::Utc>,
    provider_account: &str,
) -> String {
    let local =
        render_report_with_routes(snapshot, pricing, official, billing, Some(providers), now);
    let local = local
        .strip_prefix("Usage · current ca process\n\n")
        .unwrap_or(&local);
    format!(
        "Usage\n\nProvider account\n{provider_account}\n\nLocal ca ledger · current process\n{local}"
    )
}

pub fn summarize_usage(
    snapshot: &UsageSnapshot,
    pricing: &HashMap<String, UsagePricing>,
    official: Option<&crate::pricing::OfficialPriceCatalog>,
    billing: &HashMap<String, crate::pricing::ProviderBillingConfig>,
    now: chrono::DateTime<chrono::Utc>,
) -> UsageTotals {
    summarize_usage_with_routes(snapshot, pricing, official, billing, None, now)
}

fn summarize_usage_with_routes(
    snapshot: &UsageSnapshot,
    pricing: &HashMap<String, UsagePricing>,
    official: Option<&crate::pricing::OfficialPriceCatalog>,
    billing: &HashMap<String, crate::pricing::ProviderBillingConfig>,
    providers: Option<&HashMap<String, crate::config::ProviderConfig>>,
    now: chrono::DateTime<chrono::Utc>,
) -> UsageTotals {
    let mut prompt_tokens = 0_u64;
    let mut completion_tokens = 0_u64;
    let mut cached_input_tokens = 0_u64;
    let mut cache_write_tokens = 0_u64;
    let mut reasoning_output_tokens = 0_u64;
    let mut total_latency = Duration::ZERO;
    let mut first_output_latency = Duration::ZERO;
    let mut first_output_latency_samples = 0_usize;
    let mut known_usage = 0_usize;
    let mut cache_read_unknown_requests = 0_usize;
    let mut cache_write_unknown_requests = 0_usize;
    let mut cache_miss_unknown_requests = 0_usize;
    let mut cache_detail_unknown_requests = 0_usize;
    let mut incremental_cost = crate::pricing::ExactRational::new(0, 1).unwrap();
    let mut upper_bound_requests = 0_usize;
    let mut allocation_cost = crate::pricing::ExactRational::new(0, 1).unwrap();
    let mut allocation_credits = crate::pricing::ExactRational::new(0, 1).unwrap();
    let mut allocation_requests = 0_usize;
    let mut priced_requests = 0_usize;
    let mut by_kind = std::collections::BTreeMap::new();
    let mut by_outcome = std::collections::BTreeMap::new();
    for record in &snapshot.records {
        *by_kind.entry(record.kind).or_insert(0) += 1;
        *by_outcome.entry(record.outcome).or_insert(0) += 1;
        total_latency = total_latency.saturating_add(record.duration);
        if let Some(latency) = record
            .generation_duration
            .and_then(|generation| record.duration.checked_sub(generation))
        {
            first_output_latency = first_output_latency.saturating_add(latency);
            first_output_latency_samples = first_output_latency_samples.saturating_add(1);
        }
        let Some(usage) = &record.usage else {
            continue;
        };
        known_usage += 1;
        let cache = usage.cache_usage();
        cache_read_unknown_requests += usize::from(cache.read.is_unknown());
        cache_write_unknown_requests += usize::from(cache.write.is_unknown());
        cache_miss_unknown_requests += usize::from(cache.miss.is_unknown());
        cache_detail_unknown_requests += usize::from(cache.has_unknown_detail());
        prompt_tokens = prompt_tokens.saturating_add(u64::from(usage.prompt_tokens));
        completion_tokens = completion_tokens.saturating_add(u64::from(usage.completion_tokens));
        if let Some(tokens) = cache.read.tokens() {
            cached_input_tokens = cached_input_tokens.saturating_add(u64::from(tokens));
        }
        if let Some(tokens) = cache.write.tokens() {
            cache_write_tokens = cache_write_tokens.saturating_add(u64::from(tokens));
        }
        reasoning_output_tokens =
            reasoning_output_tokens.saturating_add(u64::from(usage.reasoning_tokens.unwrap_or(0)));
        if let Some(cost) = reported_cost(record, pricing, official, billing, providers, now) {
            let accumulated = match cost {
                crate::pricing::ActualCost::IncrementalUsd(value) => incremental_cost
                    .checked_add(value)
                    .map(|total| incremental_cost = total),
                crate::pricing::ActualCost::UpperBoundIncrementalUsd(value) => {
                    incremental_cost.checked_add(value).map(|total| {
                        incremental_cost = total;
                        upper_bound_requests += 1;
                    })
                }
                crate::pricing::ActualCost::SubscriptionAllocation {
                    credits,
                    allocated_usd,
                    ..
                } => allocation_credits
                    .checked_add(credits)
                    .zip(allocation_cost.checked_add(allocated_usd))
                    .map(|(credit_total, usd_total)| {
                        allocation_credits = credit_total;
                        allocation_cost = usd_total;
                        allocation_requests += 1;
                    }),
            };
            if accumulated.is_some() {
                priced_requests += 1;
            }
        }
    }
    UsageTotals {
        requests: snapshot.records.len(),
        prompt_tokens,
        completion_tokens,
        cached_input_tokens,
        cache_write_tokens,
        reasoning_output_tokens,
        total_latency,
        first_output_latency,
        first_output_latency_samples,
        known_usage,
        cache_read_unknown_requests,
        cache_write_unknown_requests,
        cache_miss_unknown_requests,
        cache_detail_unknown_requests,
        priced_requests,
        incremental_cost,
        upper_bound_requests,
        allocation_cost,
        allocation_credits,
        allocation_requests,
        by_kind,
        by_outcome,
    }
}

#[cfg(test)]
pub fn summarize_sitting_usage(
    snapshot: &UsageSnapshot,
    pricing: &HashMap<String, UsagePricing>,
    official: Option<&crate::pricing::OfficialPriceCatalog>,
    billing: &HashMap<String, crate::pricing::ProviderBillingConfig>,
    now: chrono::DateTime<chrono::Utc>,
) -> SittingUsageSummary {
    let totals = summarize_usage(snapshot, pricing, official, billing, now);
    summarize_sitting_usage_from_totals(snapshot, totals)
}

pub fn summarize_sitting_usage_with_providers(
    snapshot: &UsageSnapshot,
    pricing: &HashMap<String, UsagePricing>,
    official: Option<&crate::pricing::OfficialPriceCatalog>,
    billing: &HashMap<String, crate::pricing::ProviderBillingConfig>,
    providers: &HashMap<String, crate::config::ProviderConfig>,
    now: chrono::DateTime<chrono::Utc>,
) -> SittingUsageSummary {
    let totals =
        summarize_usage_with_routes(snapshot, pricing, official, billing, Some(providers), now);
    summarize_sitting_usage_from_totals(snapshot, totals)
}

fn summarize_sitting_usage_from_totals(
    snapshot: &UsageSnapshot,
    totals: UsageTotals,
) -> SittingUsageSummary {
    let latest_generation_speed_tenths = snapshot
        .records
        .iter()
        .rev()
        .find(|record| {
            matches!(
                record.kind,
                UsageKind::MainTurn | UsageKind::ToolContinuation
            ) && record.outcome == UsageOutcome::Completed
        })
        .and_then(|record| {
            let usage = record.usage.as_ref()?;
            let duration = record.generation_duration?;
            if usage.completion_tokens == 0 || duration.is_zero() {
                return None;
            }
            let numerator = u128::from(usage.completion_tokens)
                .saturating_mul(10)
                .saturating_mul(1_000_000_000);
            let denominator = duration.as_nanos();
            u64::try_from(
                numerator
                    .saturating_add(denominator / 2)
                    .checked_div(denominator)?,
            )
            .ok()
        });
    let incremental_requests = totals
        .priced_requests
        .saturating_sub(totals.allocation_requests);
    let incremental_usd = (incremental_requests > 0).then(|| {
        (if totals.upper_bound_requests > 0 {
            totals.incremental_cost.ceiling_decimal(6)
        } else {
            totals.incremental_cost.rounded_decimal(6)
        })
        .unwrap_or(DecimalUsd::ZERO)
        .to_string()
    });
    let allocation_credits =
        (totals.allocation_requests > 0).then(|| totals.allocation_credits.to_string());
    let unknown_requests = totals.requests.saturating_sub(totals.priced_requests);
    let component_count = usize::from(incremental_usd.is_some())
        + usize::from(allocation_credits.is_some())
        + usize::from(unknown_requests > 0);
    let cost = match component_count {
        _ if totals.upper_bound_requests > 0 => SittingCostPresentation::UpperBound {
            usd: incremental_usd.expect("upper bound requests have metered amounts"),
            allocation_credits,
            unknown_requests,
        },
        0 => SittingCostPresentation::Unknown,
        1 if incremental_usd.is_some() => SittingCostPresentation::Incremental {
            usd: incremental_usd.expect("checked"),
        },
        1 if allocation_credits.is_some() => SittingCostPresentation::SubscriptionAllocation {
            credits: allocation_credits.expect("checked"),
        },
        1 => SittingCostPresentation::Unknown,
        _ => SittingCostPresentation::Mixed {
            incremental_usd,
            allocation_credits,
            unknown_requests,
        },
    };
    SittingUsageSummary {
        totals,
        latest_generation_speed_tenths,
        cost,
    }
}

fn reported_cost(
    record: &UsageRecord,
    legacy: &HashMap<String, UsagePricing>,
    official: Option<&crate::pricing::OfficialPriceCatalog>,
    billing: &HashMap<String, crate::pricing::ProviderBillingConfig>,
    providers: Option<&HashMap<String, crate::config::ProviderConfig>>,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<crate::pricing::ActualCost> {
    let usage = record.usage.as_ref()?;
    let card = official.and_then(|catalog| {
        if let Some(providers) = providers {
            let provider = providers.get(&record.provider)?;
            let provider_billing = billing.get(&record.provider);
            if !crate::pricing::supported_billing_route(
                &record.provider,
                &provider.base_url,
                provider_billing,
            ) {
                return None;
            }
            let mode = if record.provider == "mimo" && provider_billing.is_some() {
                "mimo_token_plan"
            } else {
                "metered_usd"
            };
            catalog.find_for_billing_mode(&record.provider, &record.model, mode)
        } else {
            catalog.find(&record.provider, &record.model)
        }
    });
    if let Some(card) = card {
        if !card.fresh_at(now) {
            return None;
        }
        return crate::pricing::actual_cost_for(
            card,
            billing.get(&record.provider),
            usage,
            record.started_at,
            record.finished_at?,
        );
    }
    let cost = legacy
        .get(&format!("{}/{}", record.provider, record.model))
        .and_then(|pricing| cost_for(pricing, usage))?;
    Some(crate::pricing::ActualCost::IncrementalUsd(
        crate::pricing::ExactRational::from_decimal(cost)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::stream::UsageInfo;
    use std::str::FromStr;

    fn sitting_record(
        id: u64,
        provider: &str,
        model: &str,
        completion_tokens: u32,
        generation_duration: Option<Duration>,
        usage_known: bool,
    ) -> UsageRecord {
        let started_at = chrono::DateTime::parse_from_rfc3339("2026-08-15T12:00:00Z")
            .unwrap()
            .to_utc();
        UsageRecord {
            id,
            kind: UsageKind::MainTurn,
            provider: provider.into(),
            model: model.into(),
            duration: Duration::from_secs(2),
            generation_duration,
            started_at,
            finished_at: Some(started_at + chrono::Duration::seconds(2)),
            usage: usage_known.then_some(UsageInfo {
                prompt_tokens: 4,
                completion_tokens,
                total_tokens: 4 + completion_tokens,
                cached_prompt_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
                response_metadata: None,
                cache_usage_override: None,
            }),
            outcome: UsageOutcome::Completed,
        }
    }

    #[test]
    fn decimal_ordering_compares_numeric_value_across_scales() {
        let larger: DecimalUsd = "0.000001".parse().unwrap();
        let smaller: DecimalUsd = "0.0000005".parse().unwrap();
        assert!(larger > smaller);
        assert_eq!("1.0".parse::<DecimalUsd>().unwrap(), "1".parse().unwrap());
    }

    #[test]
    fn exact_decimal_pricing_uses_token_subsets_without_floats() {
        let pricing = UsagePricing {
            input_per_million_usd: DecimalUsd::from_str("2").unwrap(),
            output_per_million_usd: DecimalUsd::from_str("4").unwrap(),
            cached_input_per_million_usd: Some(DecimalUsd::from_str("1").unwrap()),
            cache_write_input_per_million_usd: Some(DecimalUsd::from_str("2.5").unwrap()),
            reasoning_output_per_million_usd: Some(DecimalUsd::from_str("8").unwrap()),
        };
        let usage = UsageInfo {
            prompt_tokens: 1_000,
            completion_tokens: 200,
            total_tokens: 1_200,
            cached_prompt_tokens: Some(400),
            cache_write_tokens: Some(100),
            reasoning_tokens: Some(50),
            response_metadata: None,
            cache_usage_override: None,
        };

        assert_eq!(cost_for(&pricing, &usage).unwrap().to_string(), "0.00265");
        assert!(usage_details_valid(&usage));
    }

    #[test]
    fn cache_write_billing_distinguishes_replacement_surcharge_and_included() {
        let pricing = UsagePricing {
            input_per_million_usd: DecimalUsd::from_str("2").unwrap(),
            output_per_million_usd: DecimalUsd::ZERO,
            cached_input_per_million_usd: Some(DecimalUsd::from_str("1").unwrap()),
            cache_write_input_per_million_usd: None,
            reasoning_output_per_million_usd: None,
        };
        let usage = UsageInfo {
            prompt_tokens: 100,
            completion_tokens: 0,
            total_tokens: 100,
            cached_prompt_tokens: Some(40),
            cache_write_tokens: Some(20),
            reasoning_tokens: None,
            response_metadata: None,
            cache_usage_override: None,
        };

        let cost = |billing| {
            cost_for_with_cache_write_billing(&pricing, billing, &usage)
                .unwrap()
                .to_string()
        };
        assert_eq!(
            cost(CacheWriteBilling::Replacement {
                rate: DecimalUsd::from_str("3").unwrap(),
            }),
            "0.00018"
        );
        assert_eq!(
            cost(CacheWriteBilling::Surcharge {
                rate: DecimalUsd::from_str("0.5").unwrap(),
            }),
            "0.00017"
        );
        assert_eq!(cost(CacheWriteBilling::Included), "0.00016");
        assert_eq!(
            cost(CacheWriteBilling::Replacement {
                rate: DecimalUsd::ZERO,
            }),
            "0.00012"
        );
    }

    #[test]
    fn cache_pricing_requires_unknown_details_only_when_they_change_cost() {
        let pricing = UsagePricing {
            input_per_million_usd: DecimalUsd::from_str("2").unwrap(),
            output_per_million_usd: DecimalUsd::ZERO,
            cached_input_per_million_usd: Some(DecimalUsd::from_str("1").unwrap()),
            cache_write_input_per_million_usd: None,
            reasoning_output_per_million_usd: None,
        };
        let unknown_write = UsageInfo {
            prompt_tokens: 100,
            completion_tokens: 0,
            total_tokens: 100,
            cached_prompt_tokens: Some(40),
            cache_write_tokens: None,
            reasoning_tokens: None,
            response_metadata: None,
            cache_usage_override: None,
        };
        assert!(cost_for_with_cache_write_billing(
            &pricing,
            CacheWriteBilling::Included,
            &unknown_write,
        )
        .is_some());
        assert!(cost_for_with_cache_write_billing(
            &pricing,
            CacheWriteBilling::Replacement {
                rate: pricing.input_per_million_usd,
            },
            &unknown_write,
        )
        .is_some());
        assert!(cost_for_with_cache_write_billing(
            &pricing,
            CacheWriteBilling::Surcharge {
                rate: DecimalUsd::ZERO,
            },
            &unknown_write,
        )
        .is_some());
        assert!(cost_for_with_cache_write_billing(
            &pricing,
            CacheWriteBilling::Surcharge {
                rate: DecimalUsd::from_str("0.5").unwrap(),
            },
            &unknown_write,
        )
        .is_none());

        let unknown_read = UsageInfo {
            cached_prompt_tokens: None,
            ..unknown_write
        };
        assert!(cost_for_with_cache_write_billing(
            &pricing,
            CacheWriteBilling::Included,
            &unknown_read,
        )
        .is_none());
        let equal_rates = UsagePricing {
            cached_input_per_million_usd: None,
            ..pricing
        };
        assert_eq!(
            cost_for_with_cache_write_billing(
                &equal_rates,
                CacheWriteBilling::Included,
                &unknown_read,
            )
            .unwrap()
            .to_string(),
            "0.0002"
        );
    }

    #[test]
    fn usage_totals_track_unknown_cache_detail_without_treating_writes_as_uncached_partition() {
        let started_at = chrono::Utc::now();
        let records = vec![
            UsageRecord {
                id: 1,
                kind: UsageKind::MainTurn,
                provider: "local".into(),
                model: "one".into(),
                duration: Duration::from_millis(1),
                generation_duration: None,
                started_at,
                finished_at: Some(started_at),
                usage: Some(UsageInfo {
                    prompt_tokens: 100,
                    completion_tokens: 0,
                    total_tokens: 100,
                    cached_prompt_tokens: Some(40),
                    cache_write_tokens: Some(20),
                    reasoning_tokens: None,
                    response_metadata: None,
                    cache_usage_override: None,
                }),
                outcome: UsageOutcome::Completed,
            },
            UsageRecord {
                id: 2,
                kind: UsageKind::MainTurn,
                provider: "local".into(),
                model: "two".into(),
                duration: Duration::from_millis(1),
                generation_duration: None,
                started_at,
                finished_at: Some(started_at),
                usage: Some(UsageInfo {
                    prompt_tokens: 50,
                    completion_tokens: 0,
                    total_tokens: 50,
                    cached_prompt_tokens: None,
                    cache_write_tokens: None,
                    reasoning_tokens: None,
                    response_metadata: None,
                    cache_usage_override: None,
                }),
                outcome: UsageOutcome::Completed,
            },
        ];
        let totals = summarize_usage(
            &UsageSnapshot {
                records,
                evicted_records: 0,
            },
            &HashMap::new(),
            None,
            &HashMap::new(),
            started_at,
        );
        assert_eq!(totals.cached_input_tokens, 40);
        assert_eq!(totals.cache_write_tokens, 20);
        assert_eq!(
            totals
                .prompt_tokens
                .saturating_sub(totals.cached_input_tokens),
            110
        );
        assert_eq!(totals.cache_read_unknown_requests, 1);
        assert_eq!(totals.cache_write_unknown_requests, 1);
        assert_eq!(totals.cache_miss_unknown_requests, 1);
        assert_eq!(totals.cache_detail_unknown_requests, 1);
        assert_eq!(totals.new_input_tokens(), None);
        assert_eq!(totals.cache_hit_ratio_tenths_percent(), None);
    }

    #[test]
    fn efficiency_totals_are_exact_only_when_source_detail_is_known() {
        let started_at = chrono::Utc::now();
        let snapshot = UsageSnapshot {
            records: vec![
                UsageRecord {
                    id: 1,
                    kind: UsageKind::MainTurn,
                    provider: "local".into(),
                    model: "model".into(),
                    duration: Duration::from_millis(1_000),
                    generation_duration: Some(Duration::from_millis(600)),
                    started_at,
                    finished_at: Some(started_at),
                    usage: Some(UsageInfo {
                        prompt_tokens: 100,
                        completion_tokens: 20,
                        total_tokens: 120,
                        cached_prompt_tokens: Some(40),
                        cache_write_tokens: Some(0),
                        reasoning_tokens: Some(0),
                        response_metadata: None,
                        cache_usage_override: None,
                    }),
                    outcome: UsageOutcome::Completed,
                },
                UsageRecord {
                    id: 2,
                    kind: UsageKind::ToolContinuation,
                    provider: "local".into(),
                    model: "model".into(),
                    duration: Duration::from_millis(500),
                    generation_duration: Some(Duration::from_millis(300)),
                    started_at,
                    finished_at: Some(started_at),
                    usage: Some(UsageInfo {
                        prompt_tokens: 50,
                        completion_tokens: 10,
                        total_tokens: 60,
                        cached_prompt_tokens: Some(20),
                        cache_write_tokens: Some(0),
                        reasoning_tokens: Some(0),
                        response_metadata: None,
                        cache_usage_override: None,
                    }),
                    outcome: UsageOutcome::Completed,
                },
            ],
            evicted_records: 0,
        };
        let totals = summarize_usage(
            &snapshot,
            &HashMap::new(),
            None,
            &HashMap::new(),
            started_at,
        );

        assert_eq!(totals.new_input_tokens(), Some(90));
        assert_eq!(totals.cache_hit_ratio_tenths_percent(), Some(400));
        assert_eq!(totals.tool_round_trips(), 1);
        assert_eq!(
            totals.average_total_latency(),
            Some(Duration::from_millis(750))
        );
        assert_eq!(
            totals.average_first_output_latency(),
            Some(Duration::from_millis(300))
        );

        let report = render_report(&snapshot, &HashMap::new());
        assert!(report.contains("90 new input"));
        assert!(report.contains("40.0% cache hit"));
        assert!(report.contains("1 tool round trip"));
        assert!(report.contains("750 ms average total"));
        assert!(report.contains("300 ms average to first output"));
    }

    #[test]
    fn decimal_prices_reject_non_exact_or_unbounded_forms() {
        for invalid in ["", "-1", "+1", "1e-3", ".5", "1.", "1.1234567890123"] {
            assert!(DecimalUsd::from_str(invalid).is_err(), "accepted {invalid}");
        }
        assert_eq!(DecimalUsd::from_str("0.1500").unwrap().to_string(), "0.15");
    }

    #[test]
    fn ledger_lifecycle_is_monotonic_and_finishes_once() {
        let mut ledger = UsageLedger::default();
        let first = ledger.begin(UsageKind::MainTurn, "mimo", "mimo-v2.5");
        let second = ledger.begin(UsageKind::ToolContinuation, "mimo", "mimo-v2.5");
        assert!(second.id() > first.id());
        assert!(ledger.finish(
            first,
            Some(UsageInfo {
                prompt_tokens: 10,
                completion_tokens: 2,
                total_tokens: 12,
                cached_prompt_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
                response_metadata: None,
                cache_usage_override: None,
            }),
            UsageOutcome::Completed,
        ));
        assert!(!ledger.finish(first, None, UsageOutcome::Failed));
        assert!(ledger.finish(second, None, UsageOutcome::Cancelled));

        let records = ledger.snapshot().records;
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].outcome, UsageOutcome::Completed);
        assert_eq!(records[1].outcome, UsageOutcome::Cancelled);
        assert!(records[1].usage.is_none());
    }

    #[test]
    fn foreground_generation_duration_is_recorded_separately_from_wall_time() {
        let mut ledger = UsageLedger::default();
        let handle = ledger.begin(UsageKind::MainTurn, "local", "model");
        assert!(ledger.finish_with_generation_duration(
            handle,
            Some(UsageInfo {
                prompt_tokens: 4,
                completion_tokens: 20,
                total_tokens: 24,
                cached_prompt_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
                response_metadata: None,
                cache_usage_override: None,
            }),
            UsageOutcome::Completed,
            Some(Duration::from_secs(2)),
        ));
        let record = ledger.snapshot().records.pop().unwrap();
        assert_eq!(record.generation_duration, Some(Duration::from_secs(2)));
        assert!(record.duration <= Duration::from_secs(1));
    }

    #[test]
    fn sitting_summary_keeps_speed_and_all_cost_states_honest() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-15T12:00:02Z")
            .unwrap()
            .to_utc();
        let pricing = HashMap::from([(
            "local/model".into(),
            UsagePricing {
                input_per_million_usd: DecimalUsd::from_str("2").unwrap(),
                output_per_million_usd: DecimalUsd::from_str("4").unwrap(),
                cached_input_per_million_usd: None,
                cache_write_input_per_million_usd: None,
                reasoning_output_per_million_usd: None,
            },
        )]);
        let incremental_snapshot = UsageSnapshot {
            records: vec![sitting_record(
                1,
                "local",
                "model",
                20,
                Some(Duration::from_millis(1_600)),
                true,
            )],
            evicted_records: 0,
        };
        let incremental =
            summarize_sitting_usage(&incremental_snapshot, &pricing, None, &HashMap::new(), now);
        assert_eq!(incremental.latest_generation_speed_tenths, Some(125));
        assert!(matches!(
            incremental.cost,
            SittingCostPresentation::Incremental { .. }
        ));

        let mut records = incremental_snapshot.records.clone();
        records.push(sitting_record(2, "local", "unknown", 1, None, false));
        let mixed = summarize_sitting_usage(
            &UsageSnapshot {
                records,
                evicted_records: 0,
            },
            &pricing,
            None,
            &HashMap::new(),
            now,
        );
        assert!(matches!(
            mixed.cost,
            SittingCostPresentation::Mixed {
                unknown_requests: 1,
                ..
            }
        ));
        assert!(mixed.cost.compact_label().contains("unknown"));

        let empty = summarize_sitting_usage(
            &UsageSnapshot {
                records: Vec::new(),
                evicted_records: 0,
            },
            &HashMap::new(),
            None,
            &HashMap::new(),
            now,
        );
        assert_eq!(empty.latest_generation_speed_tenths, None);
        assert_eq!(empty.cost, SittingCostPresentation::Unknown);

        let missing_usage = summarize_sitting_usage(
            &UsageSnapshot {
                records: vec![sitting_record(
                    3,
                    "local",
                    "model",
                    0,
                    Some(Duration::from_secs(1)),
                    false,
                )],
                evicted_records: 0,
            },
            &pricing,
            None,
            &HashMap::new(),
            now,
        );
        assert_eq!(missing_usage.latest_generation_speed_tenths, None);
        assert_eq!(missing_usage.cost, SittingCostPresentation::Unknown);

        let missing_timing = summarize_sitting_usage(
            &UsageSnapshot {
                records: vec![sitting_record(4, "local", "model", 20, None, true)],
                evicted_records: 0,
            },
            &pricing,
            None,
            &HashMap::new(),
            now,
        );
        assert_eq!(missing_timing.latest_generation_speed_tenths, None);
        assert!(matches!(
            missing_timing.cost,
            SittingCostPresentation::Incremental { .. }
        ));

        let missing_pricing = summarize_sitting_usage(
            &incremental_snapshot,
            &HashMap::new(),
            None,
            &HashMap::new(),
            now,
        );
        assert_eq!(missing_pricing.latest_generation_speed_tenths, Some(125));
        assert_eq!(missing_pricing.cost, SittingCostPresentation::Unknown);
    }

    #[test]
    fn sitting_summary_labels_subscription_allocation_without_incremental_cost() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-15T12:00:02Z")
            .unwrap()
            .to_utc();
        let mut record = sitting_record(
            1,
            "mimo",
            "mimo-v2.5",
            2,
            Some(Duration::from_secs(1)),
            true,
        );
        let usage = record.usage.as_mut().unwrap();
        usage.cached_prompt_tokens = Some(0);
        usage.cache_write_tokens = Some(0);
        let snapshot = UsageSnapshot {
            records: vec![record],
            evicted_records: 0,
        };
        let billing = HashMap::from([(
            "mimo".into(),
            crate::pricing::ProviderBillingConfig::mimo_token_plan(
                crate::pricing::MimoRegion::Singapore,
                crate::pricing::MimoPlanTier::Lite,
                crate::pricing::MimoPlanTerm::Monthly,
                "6".parse().unwrap(),
                true,
            )
            .unwrap(),
        )]);
        let summary = summarize_sitting_usage(
            &snapshot,
            &HashMap::new(),
            Some(&crate::pricing::OfficialPriceCatalog::seeded()),
            &billing,
            now,
        );
        assert!(matches!(
            summary.cost,
            SittingCostPresentation::SubscriptionAllocation { .. }
        ));
        assert!(summary.cost.compact_label().contains("allocation"));
    }

    #[test]
    fn session_usage_window_ignores_pre_epoch_eviction_and_marks_own_truncation() {
        let mut ledger = UsageLedger::default();
        for _ in 0..MAX_USAGE_RECORDS {
            ledger.push_test_record(
                UsageKind::MainTurn,
                "local",
                "old",
                Duration::from_millis(1),
                None,
                UsageOutcome::Completed,
            );
        }
        let cursor = ledger.cursor();
        ledger.push_test_record(
            UsageKind::MainTurn,
            "local",
            "new",
            Duration::from_millis(1),
            None,
            UsageOutcome::Completed,
        );
        let fresh = ledger.snapshot_since(cursor);
        assert_eq!(fresh.snapshot.records.len(), 1);
        assert_eq!(fresh.dropped_since_cursor, 0);

        for _ in 0..MAX_USAGE_RECORDS {
            ledger.push_test_record(
                UsageKind::ToolContinuation,
                "local",
                "new",
                Duration::from_millis(1),
                None,
                UsageOutcome::Completed,
            );
        }
        let truncated = ledger.snapshot_since(cursor);
        assert_eq!(truncated.snapshot.records.len(), MAX_USAGE_RECORDS);
        assert_eq!(truncated.dropped_since_cursor, 1);
        assert_eq!(truncated.snapshot.evicted_records, 1);
    }

    #[test]
    fn missing_pricing_is_visibly_unknown_in_report() {
        let mut ledger = UsageLedger::default();
        let request = ledger.begin(UsageKind::ProviderProbe, "local", "model");
        ledger.finish(request, None, UsageOutcome::Failed);

        let report = render_report(&ledger.snapshot(), &std::collections::HashMap::new());

        assert!(report.contains("Cost: unknown"));
        assert!(report.contains("provider probe"));
        assert!(report.contains("tokens unknown"));
    }

    #[test]
    fn peak_rate_usage_and_sitting_totals_are_labeled_upper_bounds() {
        let now = chrono::Utc::now();
        let mut catalog = crate::pricing::OfficialPriceCatalog::seeded();
        let card = catalog
            .cards
            .iter_mut()
            .find(|card| card.provider == "deepseek" && card.model == "deepseek-v4-pro")
            .unwrap();
        card.verified_at = now;
        card.cost_basis = crate::pricing::CostBasis::PeakUpperBound;
        let mut ledger = UsageLedger::default();
        let request = ledger.begin(UsageKind::MainTurn, "deepseek", "deepseek-v4-pro");
        ledger.finish(
            request,
            Some(UsageInfo {
                prompt_tokens: 100,
                completion_tokens: 10,
                total_tokens: 110,
                cached_prompt_tokens: Some(0),
                cache_write_tokens: None,
                reasoning_tokens: None,
                response_metadata: None,
                cache_usage_override: None,
            }),
            UsageOutcome::Completed,
        );
        let snapshot = ledger.snapshot();
        let summary = summarize_sitting_usage(
            &snapshot,
            &HashMap::new(),
            Some(&catalog),
            &HashMap::new(),
            now,
        );
        assert_eq!(summary.totals.upper_bound_requests, 1);
        assert_eq!(summary.totals.unknown_cost(), 0);
        assert!(matches!(
            summary.cost,
            SittingCostPresentation::UpperBound { .. }
        ));
        assert!(summary
            .cost
            .compact_label()
            .contains("peak-rate upper bound"));
        let report = render_report_with_official(
            &snapshot,
            &HashMap::new(),
            Some(&catalog),
            &HashMap::new(),
            now,
        );
        assert!(report.contains("Cost: ≤$"));
        assert!(report.contains("peak-rate upper bound"));
    }

    #[test]
    fn recorder_drop_marks_cancelled_and_exact_pricing_is_reported() {
        let ledger = Arc::new(Mutex::new(UsageLedger::default()));
        let recorder = UsageRecorder::new(
            ledger.clone(),
            UsageKind::ContextCheckpoint,
            "safe",
            "model",
        );
        {
            let mut span = recorder.begin();
            span.observe(&UsageInfo {
                prompt_tokens: 1_000,
                completion_tokens: 250,
                total_tokens: 1_250,
                cached_prompt_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
                response_metadata: None,
                cache_usage_override: None,
            });
        }
        let snapshot = ledger.lock().unwrap().snapshot();
        assert_eq!(snapshot.records[0].outcome, UsageOutcome::Cancelled);
        assert_eq!(
            snapshot.records[0].usage.as_ref().unwrap().total_tokens,
            1_250
        );

        let pricing = HashMap::from([(
            "safe/model".into(),
            UsagePricing {
                input_per_million_usd: DecimalUsd::from_str("2").unwrap(),
                output_per_million_usd: DecimalUsd::from_str("4").unwrap(),
                cached_input_per_million_usd: None,
                cache_write_input_per_million_usd: None,
                reasoning_output_per_million_usd: None,
            },
        )]);
        let report = render_report(&snapshot, &pricing);
        assert!(report.contains("Cost: $0.003"));
        assert!(report.contains("context checkpoint"));
        assert!(report.contains("cancelled"));
    }

    #[test]
    fn official_token_plan_report_labels_credits_as_subscription_allocation() {
        let started_at = chrono::DateTime::parse_from_rfc3339("2026-08-15T12:00:00Z")
            .unwrap()
            .to_utc();
        let snapshot = UsageSnapshot {
            records: vec![UsageRecord {
                id: 1,
                kind: UsageKind::Subagent,
                provider: "mimo".into(),
                model: "mimo-v2.5".into(),
                duration: Duration::from_secs(1),
                generation_duration: Some(Duration::from_secs(1)),
                started_at,
                finished_at: Some(started_at + chrono::Duration::seconds(1)),
                usage: Some(UsageInfo {
                    prompt_tokens: 10,
                    completion_tokens: 2,
                    total_tokens: 12,
                    cached_prompt_tokens: Some(4),
                    cache_write_tokens: Some(0),
                    reasoning_tokens: None,
                    response_metadata: None,
                    cache_usage_override: None,
                }),
                outcome: UsageOutcome::Completed,
            }],
            evicted_records: 0,
        };
        let billing = HashMap::from([(
            "mimo".into(),
            crate::pricing::ProviderBillingConfig::mimo_token_plan(
                crate::pricing::MimoRegion::Singapore,
                crate::pricing::MimoPlanTier::Lite,
                crate::pricing::MimoPlanTerm::Monthly,
                "6".parse().unwrap(),
                true,
            )
            .unwrap(),
        )]);
        let report = render_report_with_official(
            &snapshot,
            &HashMap::new(),
            Some(&crate::pricing::OfficialPriceCatalog::seeded()),
            &billing,
            started_at,
        );

        assert!(report.contains("Cost: no incremental charge"));
        assert!(report.contains("1008 credits"));
        assert!(report.contains("subscription allocation"));
        assert!(report.contains(crate::pricing::MIMO_OFF_PEAK_COPY));
    }

    #[test]
    fn official_mimo_cost_selects_payg_or_token_plan_by_endpoint_and_billing() {
        let started_at = chrono::DateTime::parse_from_rfc3339("2026-08-15T12:00:00Z")
            .unwrap()
            .to_utc();
        let record = UsageRecord {
            id: 1,
            kind: UsageKind::MainTurn,
            provider: "mimo".into(),
            model: "mimo-v2.5-pro".into(),
            duration: Duration::from_secs(1),
            generation_duration: Some(Duration::from_secs(1)),
            started_at,
            finished_at: Some(started_at + chrono::Duration::seconds(1)),
            usage: Some(UsageInfo {
                prompt_tokens: 10,
                completion_tokens: 2,
                total_tokens: 12,
                cached_prompt_tokens: Some(4),
                cache_write_tokens: Some(0),
                reasoning_tokens: None,
                response_metadata: None,
                cache_usage_override: None,
            }),
            outcome: UsageOutcome::Completed,
        };
        let catalog = crate::pricing::OfficialPriceCatalog::seeded();
        let provider = |base_url: &str| crate::config::ProviderConfig {
            base_url: base_url.into(),
            api_key: "unused".into(),
            protocol: crate::agent::ProviderProtocol::OpenAiResponses,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["mimo-v2.5-pro".into()],
            timeout: crate::config::ProviderTimeoutConfig::default(),
        };
        let payg_providers =
            HashMap::from([("mimo".into(), provider(crate::pricing::MIMO_PAYG_BASE_URL))]);
        assert!(matches!(
            reported_cost(
                &record,
                &HashMap::new(),
                Some(&catalog),
                &HashMap::new(),
                Some(&payg_providers),
                started_at,
            ),
            Some(crate::pricing::ActualCost::IncrementalUsd(_))
        ));

        let proxy_providers =
            HashMap::from([("mimo".into(), provider("https://proxy.example/v1"))]);
        assert!(reported_cost(
            &record,
            &HashMap::new(),
            Some(&catalog),
            &HashMap::new(),
            Some(&proxy_providers),
            started_at,
        )
        .is_none());

        let token_billing = HashMap::from([(
            "mimo".into(),
            crate::pricing::ProviderBillingConfig::mimo_token_plan(
                crate::pricing::MimoRegion::Singapore,
                crate::pricing::MimoPlanTier::Lite,
                crate::pricing::MimoPlanTerm::Monthly,
                "6".parse().unwrap(),
                true,
            )
            .unwrap(),
        )]);
        let token_providers = HashMap::from([(
            "mimo".into(),
            provider(crate::pricing::MimoRegion::Singapore.token_plan_url()),
        )]);
        assert!(matches!(
            reported_cost(
                &record,
                &HashMap::new(),
                Some(&catalog),
                &token_billing,
                Some(&token_providers),
                started_at,
            ),
            Some(crate::pricing::ActualCost::SubscriptionAllocation { .. })
        ));
    }

    #[test]
    fn invalid_provider_totals_and_subsets_fail_closed_for_cost() {
        let pricing = UsagePricing {
            input_per_million_usd: DecimalUsd::from_str("1").unwrap(),
            output_per_million_usd: DecimalUsd::from_str("1").unwrap(),
            cached_input_per_million_usd: Some(DecimalUsd::from_str("0.5").unwrap()),
            cache_write_input_per_million_usd: Some(DecimalUsd::from_str("1.25").unwrap()),
            reasoning_output_per_million_usd: Some(DecimalUsd::from_str("0.5").unwrap()),
        };
        let usage = UsageInfo {
            prompt_tokens: 10,
            completion_tokens: 10,
            total_tokens: 20,
            cached_prompt_tokens: Some(99),
            cache_write_tokens: Some(1),
            reasoning_tokens: Some(99),
            response_metadata: None,
            cache_usage_override: None,
        };
        assert!(!usage_details_valid(&usage));
        assert!(cost_for(&pricing, &usage).is_none());

        let invalid_total = UsageInfo {
            prompt_tokens: 10,
            completion_tokens: 10,
            total_tokens: 19,
            cached_prompt_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            response_metadata: None,
            cache_usage_override: None,
        };
        assert!(!usage_details_valid(&invalid_total));
        assert!(cost_for(&pricing, &invalid_total).is_none());

        let overlapping_input_subsets = UsageInfo {
            prompt_tokens: 10,
            completion_tokens: 0,
            total_tokens: 10,
            cached_prompt_tokens: Some(6),
            cache_write_tokens: Some(5),
            reasoning_tokens: None,
            response_metadata: None,
            cache_usage_override: None,
        };
        assert!(!usage_details_valid(&overlapping_input_subsets));
        assert!(cost_for(&pricing, &overlapping_input_subsets).is_none());
    }

    #[test]
    fn pricing_without_cache_write_rate_is_backward_compatible() {
        let pricing: UsagePricing =
            serde_json::from_str(r#"{"input_per_million_usd":"2","output_per_million_usd":"4"}"#)
                .unwrap();
        assert_eq!(pricing.cache_write_input_per_million_usd, None);

        let usage = UsageInfo {
            prompt_tokens: 100,
            completion_tokens: 0,
            total_tokens: 100,
            cached_prompt_tokens: None,
            cache_write_tokens: Some(100),
            reasoning_tokens: None,
            response_metadata: None,
            cache_usage_override: None,
        };
        assert_eq!(cost_for(&pricing, &usage).unwrap().to_string(), "0.0002");
    }
}
