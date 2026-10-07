use crate::config::CaConfig;
use crate::pricing::{ExactRational, OfficialPriceCatalog};
use crate::usage::{DecimalUsd, UsageSnapshot};
use chrono::{DateTime, Datelike as _, TimeZone as _, Utc};
use futures::{stream, StreamExt as _};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::str::FromStr as _;
use std::time::{Duration, Instant};

pub(crate) const CACHE_TTL: Duration = Duration::from_secs(60);
const ADAPTER_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_PAGE_BUCKETS: usize = 31;
const MAX_RESULTS: usize = 4096;
const MAX_BALANCES: usize = 8;
const MAX_CREDENTIAL_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ProviderAccountService {
    OpenAi,
    DeepSeek,
    Mimo,
    Unsupported,
}

impl ProviderAccountService {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::DeepSeek => "DeepSeek",
            Self::Mimo => "MiMo",
            Self::Unsupported => "Unsupported provider",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurrencyAmount {
    pub currency: String,
    pub value: DecimalUsd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeepSeekBalance {
    pub currency: String,
    pub total: DecimalUsd,
    pub granted: DecimalUsd,
    pub topped_up: DecimalUsd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProviderAccountEntry {
    OpenAi {
        provider: String,
        month: String,
        requests: u64,
        input_tokens: u64,
        output_tokens: u64,
        cached_input_tokens: u64,
        costs: Vec<CurrencyAmount>,
    },
    DeepSeek {
        provider: String,
        is_available: bool,
        balances: Vec<DeepSeekBalance>,
    },
    Mimo {
        provider: String,
        allocation_credits: Option<u128>,
        allocation_usd: Option<DecimalUsd>,
        observed_credits: Option<ExactRational>,
        observed_requests: usize,
        observed_at_least: bool,
    },
    Unavailable {
        provider: String,
        service: ProviderAccountService,
        reason: String,
    },
}

impl ProviderAccountEntry {
    fn sort_key(&self) -> (&str, ProviderAccountService) {
        match self {
            Self::OpenAi { provider, .. } => (provider, ProviderAccountService::OpenAi),
            Self::DeepSeek { provider, .. } => (provider, ProviderAccountService::DeepSeek),
            Self::Mimo { provider, .. } => (provider, ProviderAccountService::Mimo),
            Self::Unavailable {
                provider, service, ..
            } => (provider, *service),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderAccountReport {
    pub queried_at: DateTime<Utc>,
    pub entries: Vec<ProviderAccountEntry>,
}

#[derive(Clone)]
struct CachedReport {
    key: [u8; 32],
    fetched_at: Instant,
    report: ProviderAccountReport,
}

#[derive(Clone, Default)]
pub(crate) struct ProviderAccountUsageCache {
    cached: Option<CachedReport>,
}

impl std::fmt::Debug for ProviderAccountUsageCache {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderAccountUsageCache")
            .field("populated", &self.cached.is_some())
            .finish()
    }
}

impl ProviderAccountUsageCache {
    pub(crate) fn fresh(&self, key: &[u8; 32], now: Instant) -> Option<ProviderAccountReport> {
        self.cached.as_ref().and_then(|cached| {
            (cached.key == *key && now.saturating_duration_since(cached.fetched_at) < CACHE_TTL)
                .then(|| cached.report.clone())
        })
    }

    pub(crate) fn store(
        &mut self,
        key: [u8; 32],
        fetched_at: Instant,
        report: ProviderAccountReport,
    ) {
        self.cached = Some(CachedReport {
            key,
            fetched_at,
            report,
        });
    }
}

pub(crate) fn query_key(config: &CaConfig, official: Option<&OfficialPriceCatalog>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"ca-provider-account-usage-v1\0");
    let providers = config.providers.iter().collect::<BTreeMap<_, _>>();
    for (id, provider) in providers {
        hash_field(&mut hasher, id.as_bytes());
        hash_field(&mut hasher, provider.base_url.as_bytes());
        hash_field(&mut hasher, provider.api_key.as_bytes());
        hash_field(&mut hasher, provider.protocol.label().as_bytes());
    }
    let billing = config.provider_billing.iter().collect::<BTreeMap<_, _>>();
    for (id, value) in billing {
        hash_field(&mut hasher, id.as_bytes());
        if let Ok(encoded) = serde_json::to_vec(value) {
            hash_field(&mut hasher, &encoded);
        }
    }
    let pricing = config.pricing.iter().collect::<BTreeMap<_, _>>();
    for (route, value) in pricing {
        hash_field(&mut hasher, route.as_bytes());
        if let Ok(encoded) = serde_json::to_vec(value) {
            hash_field(&mut hasher, &encoded);
        }
    }
    if let Some(official) = official.and_then(|value| serde_json::to_vec(value).ok()) {
        hash_field(&mut hasher, &official);
    }
    hasher.finalize().into()
}

pub(crate) async fn query(
    config: CaConfig,
    snapshot: UsageSnapshot,
    official: Option<OfficialPriceCatalog>,
    now: DateTime<Utc>,
) -> ProviderAccountReport {
    query_with_targets(config, snapshot, official, now, QueryTargets::production()).await
}

pub(crate) fn render(report: &ProviderAccountReport) -> String {
    if report.entries.is_empty() {
        return "  No configured providers".into();
    }
    let mut lines = Vec::new();
    for entry in &report.entries {
        match entry {
            ProviderAccountEntry::OpenAi {
                provider,
                month,
                requests,
                input_tokens,
                output_tokens,
                cached_input_tokens,
                costs,
            } => {
                lines.push(format!(
                    "  {} · OpenAI month-to-date completions usage ({month})",
                    safe_label(provider)
                ));
                lines.push(format!(
                    "    {requests} requests · {input_tokens} input · {output_tokens} output · {cached_input_tokens} cached input"
                ));
                if costs.is_empty() {
                    lines.push("    Organization cost: unavailable".into());
                } else {
                    for amount in costs {
                        lines.push(format!(
                            "    Organization cost: {} {}",
                            amount.currency, amount.value
                        ));
                    }
                }
            }
            ProviderAccountEntry::DeepSeek {
                provider,
                is_available,
                balances,
            } => {
                lines.push(format!(
                    "  {} · DeepSeek live balance · API available: {}",
                    safe_label(provider),
                    if *is_available { "yes" } else { "no" }
                ));
                if balances.is_empty() {
                    lines.push("    Balance details unavailable".into());
                }
                for balance in balances {
                    lines.push(format!(
                        "    {} {} total · {} granted · {} topped up",
                        balance.currency, balance.total, balance.granted, balance.topped_up
                    ));
                }
            }
            ProviderAccountEntry::Mimo {
                provider,
                allocation_credits,
                allocation_usd,
                observed_credits,
                observed_requests,
                observed_at_least,
            } => {
                lines.push(format!("  {} · MiMo", safe_label(provider)));
                match (allocation_credits, allocation_usd) {
                    (Some(credits), Some(usd)) => lines.push(format!(
                        "    MiMo configured Token Plan: {credits} credits · USD {usd}"
                    )),
                    _ => lines.push("    Configured Token Plan: unavailable".into()),
                }
                match observed_credits {
                    Some(credits) => lines.push(format!(
                        "    Locally observed consumption estimate: {}{credits} credits · {observed_requests} request(s)",
                        if *observed_at_least { "at least " } else { "" }
                    )),
                    None => lines.push(
                        "    Locally observed consumption estimate: unavailable".into(),
                    ),
                }
                lines.push(
                    "    Live quota: unavailable (no documented public balance endpoint)".into(),
                );
            }
            ProviderAccountEntry::Unavailable {
                provider,
                service,
                reason,
            } => lines.push(format!(
                "  {} · {}: unavailable ({})",
                safe_label(provider),
                service.label(),
                safe_reason(reason)
            )),
        }
    }
    lines.join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UsageLimitGraph {
    pub title: String,
    pub percent: u8,
    pub caption: Option<String>,
    pub details: String,
}

pub(crate) fn usage_limit_graph(
    report: Option<&ProviderAccountReport>,
    current_provider: Option<&str>,
    loading: bool,
) -> UsageLimitGraph {
    if loading && report.is_none() {
        return UsageLimitGraph {
            title: "Usage limit".into(),
            percent: 0,
            caption: Some("Loading configured provider accounts…".into()),
            details: String::new(),
        };
    }
    let Some(report) = report else {
        return UsageLimitGraph {
            title: "Usage limit".into(),
            percent: 0,
            caption: Some("No configured providers".into()),
            details: String::new(),
        };
    };
    if report.entries.is_empty() {
        return UsageLimitGraph {
            title: "Usage limit".into(),
            percent: 0,
            caption: Some("No configured providers".into()),
            details: String::new(),
        };
    }
    let details = compact_limit_details(&report.entries);
    match pick_limit_entry(&report.entries, current_provider) {
        Some(entry) => graph_from_entry(entry, details),
        None => UsageLimitGraph {
            title: "Usage limit".into(),
            percent: 0,
            caption: Some("No provider quota available".into()),
            details,
        },
    }
}

fn pick_limit_entry<'a>(
    entries: &'a [ProviderAccountEntry],
    current_provider: Option<&str>,
) -> Option<&'a ProviderAccountEntry> {
    if let Some(current) = current_provider {
        if let Some(entry) = entries.iter().find(|entry| {
            entry_provider(entry) == current
                && !matches!(entry, ProviderAccountEntry::Unavailable { .. })
        }) {
            return Some(entry);
        }
    }
    entries
        .iter()
        .find(|entry| {
            matches!(
                entry,
                ProviderAccountEntry::Mimo {
                    allocation_credits: Some(_),
                    ..
                }
            )
        })
        .or_else(|| {
            entries
                .iter()
                .find(|entry| matches!(entry, ProviderAccountEntry::DeepSeek { .. }))
        })
        .or_else(|| {
            entries
                .iter()
                .find(|entry| matches!(entry, ProviderAccountEntry::OpenAi { .. }))
        })
}

fn entry_provider(entry: &ProviderAccountEntry) -> &str {
    match entry {
        ProviderAccountEntry::OpenAi { provider, .. }
        | ProviderAccountEntry::DeepSeek { provider, .. }
        | ProviderAccountEntry::Mimo { provider, .. }
        | ProviderAccountEntry::Unavailable { provider, .. } => provider,
    }
}

fn graph_from_entry(entry: &ProviderAccountEntry, details: String) -> UsageLimitGraph {
    match entry {
        ProviderAccountEntry::Mimo {
            provider,
            allocation_credits,
            observed_credits,
            observed_at_least,
            ..
        } => {
            let percent = match (observed_credits, allocation_credits) {
                (Some(observed), Some(allocation)) => allocation_percent(*observed, *allocation),
                _ => 0,
            };
            let caption = match (observed_credits, allocation_credits) {
                (Some(observed), Some(allocation)) => Some(format!(
                    "{}{observed} of {allocation} credits used",
                    if *observed_at_least { "at least " } else { "" }
                )),
                (None, Some(_)) => Some("Live quota unavailable".into()),
                _ => Some("Configured Token Plan unavailable".into()),
            };
            UsageLimitGraph {
                title: format!("Token plan ({})", safe_label(provider)),
                percent,
                caption,
                details,
            }
        }
        ProviderAccountEntry::DeepSeek {
            provider, balances, ..
        } => {
            let caption = balances
                .first()
                .map(|balance| format!("{} {} remaining", balance.currency, balance.total));
            UsageLimitGraph {
                title: format!("Account balance ({})", safe_label(provider)),
                percent: 0,
                caption: caption.or_else(|| Some("Balance unavailable".into())),
                details,
            }
        }
        ProviderAccountEntry::OpenAi {
            provider,
            month,
            requests,
            costs,
            ..
        } => {
            let cost = costs
                .first()
                .map(|amount| format!(" · {} {}", amount.currency, amount.value));
            UsageLimitGraph {
                title: format!("Month-to-date ({})", safe_label(provider)),
                percent: 0,
                caption: Some(format!(
                    "{month} · {requests} requests{}",
                    cost.unwrap_or_default()
                )),
                details,
            }
        }
        ProviderAccountEntry::Unavailable { .. } => UsageLimitGraph {
            title: "Usage limit".into(),
            percent: 0,
            caption: Some("No provider quota available".into()),
            details,
        },
    }
}

fn compact_limit_details(entries: &[ProviderAccountEntry]) -> String {
    let mut lines = Vec::new();
    for entry in entries {
        match entry {
            ProviderAccountEntry::Mimo {
                allocation_credits,
                allocation_usd,
                ..
            } => {
                let plan = match (allocation_credits, allocation_usd) {
                    (Some(credits), Some(usd)) => {
                        format!("{credits} credits · USD {usd}")
                    }
                    (Some(credits), None) => format!("{credits} credits"),
                    _ => "plan unavailable".into(),
                };
                lines.push(format!("MiMo  {plan}"));
            }
            ProviderAccountEntry::DeepSeek { balances, .. } => {
                if let Some(balance) = balances.first() {
                    lines.push(format!(
                        "DeepSeek  {} {} remaining",
                        balance.currency, balance.total
                    ));
                } else {
                    lines.push("DeepSeek  balance unavailable".into());
                }
            }
            ProviderAccountEntry::OpenAi {
                month,
                requests,
                costs,
                ..
            } => {
                let cost = costs
                    .first()
                    .map(|amount| format!(" · {} {}", amount.currency, amount.value))
                    .unwrap_or_default();
                lines.push(format!("OpenAI  {month} · {requests} requests{cost}"));
            }
            ProviderAccountEntry::Unavailable { .. } => {}
        }
    }
    lines.join("\n")
}

fn allocation_percent(observed: ExactRational, allocation: u128) -> u8 {
    if allocation == 0 {
        return 0;
    }
    let Some(numer) = observed.numerator().checked_mul(100) else {
        return 100;
    };
    let Some(denom) = observed.denominator().checked_mul(allocation) else {
        return 0;
    };
    if denom == 0 {
        return 0;
    }
    let pct = numer / denom;
    u8::try_from(pct.min(100)).unwrap_or(100)
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

#[derive(Clone)]
struct QueryTargets {
    openai_base: String,
    deepseek_base: String,
    timeout: Duration,
}

impl QueryTargets {
    fn production() -> Self {
        Self {
            openai_base: crate::pricing::OPENAI_API_BASE_URL.into(),
            deepseek_base: crate::pricing::DEEPSEEK_API_BASE_URL.into(),
            timeout: ADAPTER_TIMEOUT,
        }
    }
}

enum QuerySpec {
    OpenAi {
        provider: String,
        credential: String,
    },
    DeepSeek {
        provider: String,
        credential: String,
    },
    Mimo {
        provider: String,
    },
    Unavailable {
        provider: String,
        service: ProviderAccountService,
        reason: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdapterError {
    Unauthorized,
    Timeout,
    Network,
    Http(u16),
    Oversized,
    Invalid,
}

async fn query_with_targets(
    config: CaConfig,
    snapshot: UsageSnapshot,
    official: Option<OfficialPriceCatalog>,
    now: DateTime<Utc>,
    targets: QueryTargets,
) -> ProviderAccountReport {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(targets.timeout)
        .timeout(targets.timeout)
        .user_agent(concat!("ca/", env!("CARGO_PKG_VERSION")))
        .build();
    let client = client.ok();
    let mut providers = config.providers.iter().collect::<Vec<_>>();
    providers.sort_by(|left, right| left.0.cmp(right.0));
    let mut specs = Vec::with_capacity(providers.len());
    for (id, provider) in providers {
        let spec = match id.as_str() {
            "openai" if !official_openai_route(&provider.base_url) => QuerySpec::Unavailable {
                provider: id.clone(),
                service: ProviderAccountService::OpenAi,
                reason: "official host required",
            },
            _ if official_openai_route(&provider.base_url)
                && !credential_ready(&provider.api_key) =>
            {
                QuerySpec::Unavailable {
                    provider: id.clone(),
                    service: ProviderAccountService::OpenAi,
                    reason: "credential unavailable",
                }
            }
            _ if official_openai_route(&provider.base_url) => QuerySpec::OpenAi {
                provider: id.clone(),
                credential: provider.api_key.clone(),
            },
            "deepseek" if !official_deepseek_route(&provider.base_url) => QuerySpec::Unavailable {
                provider: id.clone(),
                service: ProviderAccountService::DeepSeek,
                reason: "official host required",
            },
            _ if official_deepseek_route(&provider.base_url)
                && !credential_ready(&provider.api_key) =>
            {
                QuerySpec::Unavailable {
                    provider: id.clone(),
                    service: ProviderAccountService::DeepSeek,
                    reason: "credential unavailable",
                }
            }
            _ if official_deepseek_route(&provider.base_url) => QuerySpec::DeepSeek {
                provider: id.clone(),
                credential: provider.api_key.clone(),
            },
            _ if config
                .provider_billing
                .get(id)
                .and_then(crate::pricing::ProviderBillingConfig::mimo_allocation)
                .is_some()
                || crate::pricing::MimoRegion::from_token_plan_url(&provider.base_url)
                    .is_some() =>
            {
                QuerySpec::Mimo {
                    provider: id.clone(),
                }
            }
            "mimo" => QuerySpec::Mimo {
                provider: id.clone(),
            },
            _ => QuerySpec::Unavailable {
                provider: id.clone(),
                service: ProviderAccountService::Unsupported,
                reason: "no supported account adapter",
            },
        };
        specs.push(spec);
    }

    let concurrency = specs.len().clamp(1, 16);
    let mut entries = stream::iter(specs.into_iter().map(|spec| {
        let client = client.clone();
        let config = config.clone();
        let snapshot = snapshot.clone();
        let official = official.clone();
        let targets = targets.clone();
        async move {
            query_one(
                spec,
                client,
                &config,
                &snapshot,
                official.as_ref(),
                now,
                &targets,
            )
            .await
        }
    }))
    .buffer_unordered(concurrency)
    .collect::<Vec<_>>()
    .await;
    entries.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
    ProviderAccountReport {
        queried_at: now,
        entries,
    }
}

async fn query_one(
    spec: QuerySpec,
    client: Option<reqwest::Client>,
    config: &CaConfig,
    snapshot: &UsageSnapshot,
    official: Option<&OfficialPriceCatalog>,
    now: DateTime<Utc>,
    targets: &QueryTargets,
) -> ProviderAccountEntry {
    match spec {
        QuerySpec::OpenAi {
            provider,
            credential,
        } => {
            let Some(client) = client else {
                return unavailable(
                    provider,
                    ProviderAccountService::OpenAi,
                    AdapterError::Network,
                );
            };
            match tokio::time::timeout(
                targets.timeout,
                query_openai(&client, &targets.openai_base, &credential, now),
            )
            .await
            {
                Ok(Ok(entry)) => ProviderAccountEntry::OpenAi {
                    provider,
                    month: entry.month,
                    requests: entry.requests,
                    input_tokens: entry.input_tokens,
                    output_tokens: entry.output_tokens,
                    cached_input_tokens: entry.cached_input_tokens,
                    costs: entry.costs,
                },
                Ok(Err(error)) => unavailable(provider, ProviderAccountService::OpenAi, error),
                Err(_) => unavailable(
                    provider,
                    ProviderAccountService::OpenAi,
                    AdapterError::Timeout,
                ),
            }
        }
        QuerySpec::DeepSeek {
            provider,
            credential,
        } => {
            let Some(client) = client else {
                return unavailable(
                    provider,
                    ProviderAccountService::DeepSeek,
                    AdapterError::Network,
                );
            };
            match tokio::time::timeout(
                targets.timeout,
                query_deepseek(&client, &targets.deepseek_base, &credential),
            )
            .await
            {
                Ok(Ok((is_available, balances))) => ProviderAccountEntry::DeepSeek {
                    provider,
                    is_available,
                    balances,
                },
                Ok(Err(error)) => unavailable(provider, ProviderAccountService::DeepSeek, error),
                Err(_) => unavailable(
                    provider,
                    ProviderAccountService::DeepSeek,
                    AdapterError::Timeout,
                ),
            }
        }
        QuerySpec::Mimo { provider } => {
            let allocation = config
                .provider_billing
                .get(&provider)
                .and_then(crate::pricing::ProviderBillingConfig::mimo_allocation);
            let totals = crate::usage::summarize_usage(
                snapshot,
                &config.pricing,
                official,
                &config.provider_billing,
                now,
            );
            ProviderAccountEntry::Mimo {
                provider,
                allocation_credits: allocation.map(|(_, credits)| credits),
                allocation_usd: allocation.map(|(usd, _)| usd),
                observed_credits: (totals.allocation_requests > 0)
                    .then_some(totals.allocation_credits),
                observed_requests: totals.allocation_requests,
                observed_at_least: snapshot.evicted_records > 0,
            }
        }
        QuerySpec::Unavailable {
            provider,
            service,
            reason,
        } => ProviderAccountEntry::Unavailable {
            provider,
            service,
            reason: reason.into(),
        },
    }
}

struct OpenAiValues {
    month: String,
    requests: u64,
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    costs: Vec<CurrencyAmount>,
}

async fn query_openai(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
    now: DateTime<Utc>,
) -> Result<OpenAiValues, AdapterError> {
    let start = Utc
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .ok_or(AdapterError::Invalid)?;
    let mut usage_url = reqwest::Url::parse(&format!(
        "{}/organization/usage/completions",
        base.trim_end_matches('/')
    ))
    .map_err(|_| AdapterError::Invalid)?;
    usage_url
        .query_pairs_mut()
        .append_pair("start_time", &start.timestamp().to_string())
        .append_pair("end_time", &now.timestamp().to_string())
        .append_pair("bucket_width", "1d")
        .append_pair("limit", "31");
    let mut costs_url = reqwest::Url::parse(&format!(
        "{}/organization/costs",
        base.trim_end_matches('/')
    ))
    .map_err(|_| AdapterError::Invalid)?;
    costs_url
        .query_pairs_mut()
        .append_pair("start_time", &start.timestamp().to_string())
        .append_pair("end_time", &now.timestamp().to_string())
        .append_pair("bucket_width", "1d")
        .append_pair("limit", "31");
    let (usage, costs) = tokio::join!(
        request_json(client, usage_url, credential),
        request_json(client, costs_url, credential)
    );
    let (usage, costs) = match (usage, costs) {
        (Err(AdapterError::Unauthorized), _) | (_, Err(AdapterError::Unauthorized)) => {
            return Err(AdapterError::Unauthorized);
        }
        (Err(error), _) | (_, Err(error)) => return Err(error),
        (Ok(usage), Ok(costs)) => (usage, costs),
    };
    let (requests, input_tokens, output_tokens, cached_input_tokens) = parse_openai_usage(&usage)?;
    let costs = parse_openai_costs(&costs)?;
    Ok(OpenAiValues {
        month: format!("{:04}-{:02}", now.year(), now.month()),
        requests,
        input_tokens,
        output_tokens,
        cached_input_tokens,
        costs,
    })
}

async fn query_deepseek(
    client: &reqwest::Client,
    base: &str,
    credential: &str,
) -> Result<(bool, Vec<DeepSeekBalance>), AdapterError> {
    let url = reqwest::Url::parse(&format!("{}/user/balance", base.trim_end_matches('/')))
        .map_err(|_| AdapterError::Invalid)?;
    let value = request_json(client, url, credential).await?;
    parse_deepseek_balance(&value)
}

async fn request_json(
    client: &reqwest::Client,
    url: reqwest::Url,
    credential: &str,
) -> Result<serde_json::Value, AdapterError> {
    let response = client
        .get(url)
        .bearer_auth(credential)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                AdapterError::Timeout
            } else {
                AdapterError::Network
            }
        })?;
    let status = response.status();
    if matches!(status.as_u16(), 401 | 403) {
        return Err(AdapterError::Unauthorized);
    }
    if !status.is_success() {
        return Err(AdapterError::Http(status.as_u16()));
    }
    let mut body = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| AdapterError::Network)?;
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(AdapterError::Oversized);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| AdapterError::Invalid)
}

fn parse_openai_usage(value: &serde_json::Value) -> Result<(u64, u64, u64, u64), AdapterError> {
    let buckets = page_buckets(value)?;
    let mut requests = 0_u64;
    let mut input = 0_u64;
    let mut output = 0_u64;
    let mut cached = 0_u64;
    let mut result_count = 0_usize;
    for bucket in buckets {
        let results = bucket
            .get("results")
            .and_then(serde_json::Value::as_array)
            .ok_or(AdapterError::Invalid)?;
        result_count = result_count
            .checked_add(results.len())
            .filter(|count| *count <= MAX_RESULTS)
            .ok_or(AdapterError::Invalid)?;
        for result in results {
            if result.get("object").and_then(serde_json::Value::as_str)
                != Some("organization.usage.completions.result")
            {
                return Err(AdapterError::Invalid);
            }
            requests = checked_json_sum(requests, result, "num_model_requests")?;
            input = checked_json_sum(input, result, "input_tokens")?;
            output = checked_json_sum(output, result, "output_tokens")?;
            cached = checked_optional_json_sum(cached, result, "input_cached_tokens")?;
        }
    }
    Ok((requests, input, output, cached))
}

fn parse_openai_costs(value: &serde_json::Value) -> Result<Vec<CurrencyAmount>, AdapterError> {
    let buckets = page_buckets(value)?;
    let mut costs = BTreeMap::<String, ExactRational>::new();
    let mut result_count = 0_usize;
    for bucket in buckets {
        let results = bucket
            .get("results")
            .and_then(serde_json::Value::as_array)
            .ok_or(AdapterError::Invalid)?;
        result_count = result_count
            .checked_add(results.len())
            .filter(|count| *count <= MAX_RESULTS)
            .ok_or(AdapterError::Invalid)?;
        for result in results {
            if result.get("object").and_then(serde_json::Value::as_str)
                != Some("organization.costs.result")
            {
                return Err(AdapterError::Invalid);
            }
            let Some(amount) = result.get("amount") else {
                continue;
            };
            if amount.is_null() {
                continue;
            }
            let currency = amount
                .get("currency")
                .and_then(serde_json::Value::as_str)
                .filter(|value| {
                    value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_alphabetic())
                })
                .ok_or(AdapterError::Invalid)?
                .to_ascii_uppercase();
            let decimal = amount
                .get("value")
                .and_then(serde_json::Value::as_number)
                .and_then(|value| DecimalUsd::from_str(&value.to_string()).ok())
                .ok_or(AdapterError::Invalid)?;
            let rational = ExactRational::from_decimal(decimal).ok_or(AdapterError::Invalid)?;
            let current = costs
                .get(&currency)
                .copied()
                .unwrap_or_else(|| ExactRational::new(0, 1).expect("nonzero denominator"));
            costs.insert(
                currency,
                current.checked_add(rational).ok_or(AdapterError::Invalid)?,
            );
        }
    }
    costs
        .into_iter()
        .map(|(currency, value)| {
            value
                .rounded_decimal(6)
                .map(|value| CurrencyAmount { currency, value })
                .ok_or(AdapterError::Invalid)
        })
        .collect()
}

fn page_buckets(value: &serde_json::Value) -> Result<&[serde_json::Value], AdapterError> {
    if value.get("object").and_then(serde_json::Value::as_str) != Some("page")
        || value.get("has_more").and_then(serde_json::Value::as_bool) != Some(false)
        || value.get("next_page").is_some_and(|value| !value.is_null())
    {
        return Err(AdapterError::Invalid);
    }
    value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .filter(|buckets| buckets.len() <= MAX_PAGE_BUCKETS)
        .map(Vec::as_slice)
        .ok_or(AdapterError::Invalid)
}

fn parse_deepseek_balance(
    value: &serde_json::Value,
) -> Result<(bool, Vec<DeepSeekBalance>), AdapterError> {
    let is_available = value
        .get("is_available")
        .and_then(serde_json::Value::as_bool)
        .ok_or(AdapterError::Invalid)?;
    let values = value
        .get("balance_infos")
        .and_then(serde_json::Value::as_array)
        .filter(|values| values.len() <= MAX_BALANCES)
        .ok_or(AdapterError::Invalid)?;
    let mut balances = Vec::with_capacity(values.len());
    for value in values {
        let currency = value
            .get("currency")
            .and_then(serde_json::Value::as_str)
            .filter(|currency| matches!(*currency, "CNY" | "USD"))
            .ok_or(AdapterError::Invalid)?
            .to_string();
        let total = decimal_string(value, "total_balance")?;
        let granted = decimal_string(value, "granted_balance")?;
        let topped_up = decimal_string(value, "topped_up_balance")?;
        let components = ExactRational::from_decimal(granted)
            .and_then(|value| value.checked_add(ExactRational::from_decimal(topped_up)?))
            .ok_or(AdapterError::Invalid)?;
        if components != ExactRational::from_decimal(total).ok_or(AdapterError::Invalid)? {
            return Err(AdapterError::Invalid);
        }
        balances.push(DeepSeekBalance {
            currency,
            total,
            granted,
            topped_up,
        });
    }
    balances.sort_by(|left, right| left.currency.cmp(&right.currency));
    Ok((is_available, balances))
}

fn decimal_string(value: &serde_json::Value, field: &str) -> Result<DecimalUsd, AdapterError> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .and_then(|value| DecimalUsd::from_str(value).ok())
        .ok_or(AdapterError::Invalid)
}

fn checked_json_sum(
    current: u64,
    value: &serde_json::Value,
    field: &str,
) -> Result<u64, AdapterError> {
    current
        .checked_add(
            value
                .get(field)
                .and_then(serde_json::Value::as_u64)
                .ok_or(AdapterError::Invalid)?,
        )
        .ok_or(AdapterError::Invalid)
}

fn checked_optional_json_sum(
    current: u64,
    value: &serde_json::Value,
    field: &str,
) -> Result<u64, AdapterError> {
    match value.get(field) {
        None | Some(serde_json::Value::Null) => Ok(current),
        Some(value) => current
            .checked_add(value.as_u64().ok_or(AdapterError::Invalid)?)
            .ok_or(AdapterError::Invalid),
    }
}

fn unavailable(
    provider: String,
    service: ProviderAccountService,
    error: AdapterError,
) -> ProviderAccountEntry {
    ProviderAccountEntry::Unavailable {
        provider,
        service,
        reason: match (service, error) {
            (ProviderAccountService::OpenAi, AdapterError::Unauthorized) => {
                "organization-admin access unavailable".into()
            }
            (_, AdapterError::Unauthorized) => "credential rejected".into(),
            (_, AdapterError::Timeout) => "request timed out".into(),
            (_, AdapterError::Network) => "connection failed".into(),
            (_, AdapterError::Http(status)) => format!("HTTP {status}"),
            (_, AdapterError::Oversized) => "response exceeded 256 KiB".into(),
            (_, AdapterError::Invalid) => "invalid bounded response".into(),
        },
    }
}

fn official_openai_route(value: &str) -> bool {
    value.trim_end_matches('/') == crate::pricing::OPENAI_API_BASE_URL
}

fn official_deepseek_route(value: &str) -> bool {
    matches!(
        value.trim_end_matches('/'),
        crate::pricing::DEEPSEEK_API_BASE_URL | crate::pricing::DEEPSEEK_API_V1_BASE_URL
    )
}

fn credential_ready(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= MAX_CREDENTIAL_BYTES
        && !value.contains("${")
        && !value.chars().any(char::is_control)
}

fn safe_label(value: &str) -> String {
    let safe = crate::terminal_text::sanitize_terminal_text(value);
    let mut characters = safe.chars();
    let mut value = characters.by_ref().take(64).collect::<String>();
    if characters.next().is_some() {
        value.push('…');
    }
    if value.is_empty() {
        "[unnamed provider]".into()
    } else {
        value
    }
}

fn safe_reason(value: &str) -> String {
    crate::terminal_text::sanitize_terminal_text(value)
        .chars()
        .take(160)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ProviderProtocol;
    use crate::config::{ProviderConfig, ProviderTimeoutConfig};
    use crate::pricing::{MimoPlanTerm, MimoPlanTier, MimoRegion, ProviderBillingConfig};
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::sync::Notify;

    fn provider(base_url: &str, key: &str) -> ProviderConfig {
        ProviderConfig {
            base_url: base_url.into(),
            api_key: key.into(),
            protocol: ProviderProtocol::ChatCompletions,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: vec!["model".into()],
            timeout: ProviderTimeoutConfig::default(),
        }
    }

    async fn loopback_provider_server(
        expected: usize,
    ) -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::new(AtomicUsize::new(0));
        let all_seen = Arc::new(Notify::new());
        let captured = requests.clone();
        let handle = tokio::spawn(async move {
            let mut tasks = Vec::new();
            for _ in 0..expected {
                let (mut socket, _) = listener.accept().await.unwrap();
                let captured = captured.clone();
                let seen = seen.clone();
                let all_seen = all_seen.clone();
                tasks.push(tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut chunk = [0_u8; 1024];
                    loop {
                        let read = socket.read(&mut chunk).await.unwrap();
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&chunk[..read]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                        assert!(request.len() < 32 * 1024);
                    }
                    let request = String::from_utf8(request).unwrap();
                    captured.lock().unwrap().push(request.clone());
                    if seen.fetch_add(1, Ordering::SeqCst) + 1 == expected {
                        all_seen.notify_waiters();
                    } else {
                        loop {
                            let notified = all_seen.notified();
                            if seen.load(Ordering::SeqCst) == expected {
                                break;
                            }
                            tokio::time::timeout(Duration::from_secs(2), notified)
                                .await
                                .expect("all provider requests must start concurrently");
                        }
                    }
                    let first_line = request.lines().next().unwrap_or_default();
                    let body = if first_line.contains("/organization/usage/completions?") {
                        r#"{"object":"page","data":[{"object":"bucket","start_time":1785542400,"end_time":1785628800,"results":[{"object":"organization.usage.completions.result","num_model_requests":4,"input_tokens":120,"output_tokens":30,"input_cached_tokens":20}]}],"has_more":false,"next_page":null}"#
                    } else if first_line.contains("/organization/costs?") {
                        r#"{"object":"page","data":[{"object":"bucket","start_time":1785542400,"end_time":1785628800,"results":[{"object":"organization.costs.result","amount":{"value":1.25,"currency":"usd"}}]}],"has_more":false,"next_page":null}"#
                    } else if first_line.starts_with("GET /user/balance ") {
                        r#"{"is_available":true,"balance_infos":[{"currency":"USD","total_balance":"12.50","granted_balance":"2.50","topped_up_balance":"10.00"}]}"#
                    } else {
                        panic!("unexpected provider account request: {first_line}");
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    socket.write_all(response.as_bytes()).await.unwrap();
                }));
            }
            for task in tasks {
                task.await.unwrap();
            }
        });
        (format!("http://{address}"), requests, handle)
    }

    #[test]
    fn provider_account_cache_is_keyed_and_expires_at_sixty_seconds() {
        let report = ProviderAccountReport {
            queried_at: Utc::now(),
            entries: Vec::new(),
        };
        let key = [7; 32];
        let started = Instant::now();
        let mut cache = ProviderAccountUsageCache::default();
        cache.store(key, started, report.clone());

        assert_eq!(
            cache.fresh(&key, started + CACHE_TTL - Duration::from_nanos(1)),
            Some(report)
        );
        assert!(cache.fresh(&key, started + CACHE_TTL).is_none());
        assert!(cache.fresh(&[8; 32], started).is_none());
        assert!(!format!("{cache:?}").contains('7'));
    }

    #[test]
    fn query_key_changes_for_routes_credentials_and_billing_without_exposing_them() {
        let mut one = CaConfig::default();
        one.providers.insert(
            "openai".into(),
            provider(crate::pricing::OPENAI_API_BASE_URL, "first-secret"),
        );
        let mut two = one.clone();
        two.providers.get_mut("openai").unwrap().api_key = "second-secret".into();
        assert_ne!(query_key(&one, None), query_key(&two, None));

        two = one.clone();
        two.providers.get_mut("openai").unwrap().base_url = "https://example.invalid/v1".into();
        assert_ne!(query_key(&one, None), query_key(&two, None));

        let billing = ProviderBillingConfig::mimo_token_plan(
            MimoRegion::Singapore,
            MimoPlanTier::Pro,
            MimoPlanTerm::Monthly,
            DecimalUsd::from_str("50").unwrap(),
            true,
        )
        .unwrap();
        two = one.clone();
        two.provider_billing.insert("mimo".into(), billing);
        assert_ne!(query_key(&one, None), query_key(&two, None));
    }

    #[tokio::test]
    async fn unsupported_and_overridden_hosts_are_visible_without_inference() {
        let mut config = CaConfig::default();
        config.providers.clear();
        config.providers.insert(
            "openai".into(),
            provider("https://proxy.invalid/v1", "openai-secret"),
        );
        config.providers.insert(
            "deepseek".into(),
            provider(
                "https://api.deepseek.com.evil.invalid/v1",
                "deepseek-secret",
            ),
        );
        config.providers.insert(
            "custom".into(),
            provider("https://example.invalid/v1", "custom-secret"),
        );

        let report = query(
            config,
            UsageSnapshot {
                records: Vec::new(),
                evicted_records: 0,
            },
            None,
            Utc::now(),
        )
        .await;
        let text = render(&report);

        assert_eq!(report.entries.len(), 3);
        assert!(text.contains("openai · OpenAI: unavailable (official host required)"));
        assert!(text.contains("deepseek · DeepSeek: unavailable (official host required)"));
        assert!(text.contains("custom · Unsupported provider: unavailable"));
        for secret in ["openai-secret", "deepseek-secret", "custom-secret"] {
            assert!(!text.contains(secret));
            assert!(!format!("{report:?}").contains(secret));
        }
    }

    #[tokio::test]
    async fn supported_adapters_query_exact_bounded_routes_concurrently() {
        let (base, requests, server) = loopback_provider_server(3).await;
        let mut config = CaConfig::default();
        config.providers.insert(
            "work-openai".into(),
            provider(
                crate::pricing::OPENAI_API_BASE_URL,
                "synthetic-openai-secret",
            ),
        );
        config.providers.insert(
            "team-deepseek".into(),
            provider(
                crate::pricing::DEEPSEEK_API_BASE_URL,
                "synthetic-deepseek-secret",
            ),
        );
        let snapshot = UsageSnapshot {
            records: Vec::new(),
            evicted_records: 0,
        };
        let report = query_with_targets(
            config,
            snapshot.clone(),
            None,
            DateTime::parse_from_rfc3339("2026-08-16T12:34:56Z")
                .unwrap()
                .to_utc(),
            QueryTargets {
                openai_base: base.clone(),
                deepseek_base: base,
                timeout: Duration::from_secs(2),
            },
        )
        .await;
        server.await.unwrap();

        assert_eq!(
            snapshot.records.len(),
            0,
            "account calls do not touch the ledger"
        );
        assert_eq!(report.entries.len(), 2);
        let text = render(&report);
        assert!(text.contains("4 requests · 120 input · 30 output · 20 cached input"));
        assert!(text.contains("Organization cost: USD 1.25"));
        assert!(text.contains("USD 12.5 total · 2.5 granted · 10 topped up"));

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests.iter().any(|request| {
            request.starts_with("GET /organization/usage/completions?start_time=1785542400&end_time=1786883696&bucket_width=1d&limit=31 HTTP/1.1")
                && request.contains("authorization: Bearer synthetic-openai-secret")
        }));
        assert!(requests.iter().any(|request| {
            request.starts_with("GET /organization/costs?start_time=1785542400&end_time=1786883696&bucket_width=1d&limit=31 HTTP/1.1")
                && request.contains("authorization: Bearer synthetic-openai-secret")
        }));
        assert!(requests.iter().any(|request| {
            request.starts_with("GET /user/balance HTTP/1.1")
                && request.contains("authorization: Bearer synthetic-deepseek-secret")
        }));
        assert!(!format!("{report:?}").contains("secret"));
    }

    #[tokio::test]
    async fn malformed_and_admin_denied_responses_are_sanitized() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 4096];
                let read = socket.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..read]);
                let (status, body) = if request.contains("/organization/costs?") {
                    ("403 Forbidden", "admin-secret-from-server")
                } else {
                    ("200 OK", r#"{"has_more":true,"data":[]}"#)
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let mut config = CaConfig::default();
        config.providers.insert(
            "openai".into(),
            provider(crate::pricing::OPENAI_API_BASE_URL, "client-secret"),
        );
        let report = query_with_targets(
            config,
            UsageSnapshot {
                records: Vec::new(),
                evicted_records: 0,
            },
            None,
            Utc::now(),
            QueryTargets {
                openai_base: format!("http://{address}"),
                deepseek_base: "http://127.0.0.1:1".into(),
                timeout: Duration::from_secs(2),
            },
        )
        .await;
        server.await.unwrap();
        let text = render(&report);
        assert!(text.contains("organization-admin access unavailable"));
        assert!(!text.contains("client-secret"));
        assert!(!text.contains("admin-secret-from-server"));
    }

    #[tokio::test]
    async fn bounded_http_reader_reports_size_and_timeout_without_body_content() {
        let oversized_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let oversized_address = oversized_listener.local_addr().unwrap();
        let oversized_server = tokio::spawn(async move {
            let (mut socket, _) = oversized_listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request).await.unwrap();
            let body = vec![b'x'; MAX_RESPONSE_BYTES + 1];
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(header.as_bytes()).await.unwrap();
            let _ = socket.write_all(&body).await;
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let oversized = request_json(
            &client,
            reqwest::Url::parse(&format!("http://{oversized_address}/bounded")).unwrap(),
            "synthetic-secret",
        )
        .await;
        oversized_server.await.unwrap();
        assert_eq!(oversized, Err(AdapterError::Oversized));

        let timeout_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let timeout_address = timeout_listener.local_addr().unwrap();
        let timeout_server = tokio::spawn(async move {
            let (mut socket, _) = timeout_listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(20))
            .build()
            .unwrap();
        let timed_out = request_json(
            &client,
            reqwest::Url::parse(&format!("http://{timeout_address}/timeout")).unwrap(),
            "synthetic-secret",
        )
        .await;
        timeout_server.await.unwrap();
        assert_eq!(timed_out, Err(AdapterError::Timeout));
    }

    #[test]
    fn response_parsers_reject_pagination_overflow_and_inconsistent_balances() {
        assert_eq!(
            parse_openai_usage(&serde_json::json!({
                "object": "page", "data": [], "has_more": true, "next_page": "secret"
            })),
            Err(AdapterError::Invalid)
        );
        assert_eq!(
            parse_openai_costs(&serde_json::json!({
                "object": "page",
                "data": [{"results": [{
                    "object": "organization.costs.result",
                    "amount": {"value": -1, "currency": "usd"}
                }]}],
                "has_more": false,
                "next_page": null
            })),
            Err(AdapterError::Invalid)
        );
        assert_eq!(
            parse_deepseek_balance(&serde_json::json!({
                "is_available": true,
                "balance_infos": [{
                    "currency": "USD",
                    "total_balance": "100",
                    "granted_balance": "10",
                    "topped_up_balance": "20"
                }]
            })),
            Err(AdapterError::Invalid)
        );
    }

    #[tokio::test]
    async fn mimo_uses_only_configured_plan_and_local_observations() {
        let mut config = CaConfig::default();
        config.providers.insert(
            "mimo".into(),
            provider(MimoRegion::Singapore.token_plan_url(), "unused-mimo-secret"),
        );
        config.provider_billing.insert(
            "mimo".into(),
            ProviderBillingConfig::mimo_token_plan(
                MimoRegion::Singapore,
                MimoPlanTier::Pro,
                MimoPlanTerm::Monthly,
                DecimalUsd::from_str("50").unwrap(),
                true,
            )
            .unwrap(),
        );
        let report = query_with_targets(
            config,
            UsageSnapshot {
                records: Vec::new(),
                evicted_records: 7,
            },
            None,
            Utc::now(),
            QueryTargets {
                openai_base: "http://127.0.0.1:1".into(),
                deepseek_base: "http://127.0.0.1:1".into(),
                timeout: Duration::from_millis(10),
            },
        )
        .await;
        let text = render(&report);
        assert!(text.contains("MiMo configured Token Plan"));
        assert!(text.contains("Live quota: unavailable"));
        assert!(!text.contains("unused-mimo-secret"));
    }

    #[test]
    fn provider_account_report_separates_incompatible_scopes() {
        let report = ProviderAccountReport {
            queried_at: DateTime::parse_from_rfc3339("2026-08-16T12:00:00Z")
                .unwrap()
                .to_utc(),
            entries: vec![
                ProviderAccountEntry::OpenAi {
                    provider: "openai".into(),
                    month: "2026-08".into(),
                    requests: 4,
                    input_tokens: 120,
                    output_tokens: 30,
                    cached_input_tokens: 20,
                    costs: vec![CurrencyAmount {
                        currency: "USD".into(),
                        value: DecimalUsd::from_str("1.25").unwrap(),
                    }],
                },
                ProviderAccountEntry::DeepSeek {
                    provider: "deepseek".into(),
                    is_available: true,
                    balances: vec![DeepSeekBalance {
                        currency: "CNY".into(),
                        total: DecimalUsd::from_str("110").unwrap(),
                        granted: DecimalUsd::from_str("10").unwrap(),
                        topped_up: DecimalUsd::from_str("100").unwrap(),
                    }],
                },
                ProviderAccountEntry::Mimo {
                    provider: "mimo".into(),
                    allocation_credits: Some(8_000_000),
                    allocation_usd: Some(DecimalUsd::from_str("50").unwrap()),
                    observed_credits: Some(ExactRational::new(3010, 1).unwrap()),
                    observed_requests: 2,
                    observed_at_least: false,
                },
            ],
        };

        let text = render(&report);
        assert!(text.contains("OpenAI month-to-date completions usage"));
        assert!(text.contains("Organization cost: USD 1.25"));
        assert!(text.contains("DeepSeek live balance"));
        assert!(text.contains("CNY 110 total · 10 granted · 100 topped up"));
        assert!(text.contains("MiMo configured Token Plan: 8000000 credits · USD 50"));
        assert!(text.contains("Locally observed consumption estimate: 3010 credits · 2 request(s)"));
        assert!(text.contains("Live quota: unavailable"));
    }

    #[test]
    fn entries_have_deterministic_provider_service_sort_keys() {
        let mut entries = [
            ProviderAccountEntry::Unavailable {
                provider: "zeta".into(),
                service: ProviderAccountService::Unsupported,
                reason: "unsupported".into(),
            },
            ProviderAccountEntry::Unavailable {
                provider: "alpha".into(),
                service: ProviderAccountService::OpenAi,
                reason: "unavailable".into(),
            },
        ];
        entries.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
        assert_eq!(entries[0].sort_key().0, "alpha");
    }

    fn sample_entries() -> Vec<ProviderAccountEntry> {
        vec![
            ProviderAccountEntry::DeepSeek {
                provider: "deepseek".into(),
                is_available: true,
                balances: vec![DeepSeekBalance {
                    currency: "CNY".into(),
                    total: DecimalUsd::from_str("1294.37").unwrap(),
                    granted: DecimalUsd::from_str("0").unwrap(),
                    topped_up: DecimalUsd::from_str("1294.37").unwrap(),
                }],
            },
            ProviderAccountEntry::Mimo {
                provider: "mimo".into(),
                allocation_credits: Some(8_000_000),
                allocation_usd: Some(DecimalUsd::from_str("50").unwrap()),
                observed_credits: Some(ExactRational::new(4_000_000, 1).unwrap()),
                observed_requests: 2,
                observed_at_least: false,
            },
            ProviderAccountEntry::Unavailable {
                provider: "openai-codex".into(),
                service: ProviderAccountService::Unsupported,
                reason: "no supported account adapter".into(),
            },
        ]
    }

    #[test]
    fn usage_limit_graph_uses_mimo_plan_ratio() {
        let report = ProviderAccountReport {
            queried_at: Utc::now(),
            entries: sample_entries(),
        };
        let graph = usage_limit_graph(Some(&report), None, false);
        assert_eq!(graph.title, "Token plan (mimo)");
        assert_eq!(graph.percent, 50);
        assert_eq!(
            graph.caption.as_deref(),
            Some("4000000 of 8000000 credits used")
        );
        assert!(graph.details.contains("MiMo  8000000 credits · USD 50"));
        assert!(graph.details.contains("DeepSeek  CNY 1294.37 remaining"));
        assert!(!graph.details.contains("openai-codex"));
        assert!(!graph.details.contains("unsupported"));
    }

    #[test]
    fn usage_limit_graph_prefers_the_current_provider() {
        let report = ProviderAccountReport {
            queried_at: Utc::now(),
            entries: sample_entries(),
        };
        let graph = usage_limit_graph(Some(&report), Some("deepseek"), false);
        assert_eq!(graph.title, "Account balance (deepseek)");
        assert_eq!(graph.percent, 0);
        assert_eq!(graph.caption.as_deref(), Some("CNY 1294.37 remaining"));
    }

    #[test]
    fn usage_limit_graph_skips_unavailable_current_provider() {
        let report = ProviderAccountReport {
            queried_at: Utc::now(),
            entries: sample_entries(),
        };
        let graph = usage_limit_graph(Some(&report), Some("openai-codex"), false);
        assert_eq!(graph.title, "Token plan (mimo)");
        assert_eq!(graph.percent, 50);
    }

    #[test]
    fn usage_limit_graph_loading_without_cache_shows_empty_bar() {
        let graph = usage_limit_graph(None, None, true);
        assert_eq!(graph.title, "Usage limit");
        assert_eq!(graph.percent, 0);
        assert_eq!(
            graph.caption.as_deref(),
            Some("Loading configured provider accounts…")
        );
        assert!(graph.details.is_empty());
    }

    #[test]
    fn usage_limit_graph_empty_report_is_honest() {
        let report = ProviderAccountReport {
            queried_at: Utc::now(),
            entries: Vec::new(),
        };
        let graph = usage_limit_graph(Some(&report), None, false);
        assert_eq!(graph.title, "Usage limit");
        assert_eq!(graph.percent, 0);
        assert_eq!(graph.caption.as_deref(), Some("No configured providers"));
    }

    #[test]
    fn usage_limit_graph_mimo_without_observed_stays_at_zero() {
        let report = ProviderAccountReport {
            queried_at: Utc::now(),
            entries: vec![ProviderAccountEntry::Mimo {
                provider: "mimo".into(),
                allocation_credits: Some(456_000_000_000),
                allocation_usd: Some(DecimalUsd::from_str("528").unwrap()),
                observed_credits: None,
                observed_requests: 0,
                observed_at_least: false,
            }],
        };
        let graph = usage_limit_graph(Some(&report), Some("mimo"), false);
        assert_eq!(graph.title, "Token plan (mimo)");
        assert_eq!(graph.percent, 0);
        assert_eq!(graph.caption.as_deref(), Some("Live quota unavailable"));
        assert!(graph.details.contains("456000000000 credits · USD 528"));
    }
}
