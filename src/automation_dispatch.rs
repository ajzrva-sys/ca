//! Host-owned limits for one scheduled model run. Reservations precede every
//! physical dispatch and stay charged when usage or completion is unavailable.
use crate::agent::stream::{ProviderEvent, UsageInfo};
use crate::automation::{AutomationBudgetV1, AutomationUsageCountersV1};
use crate::harness_budget::FrozenHarnessProvider;
use crate::pricing::{actual_cost_for, ActualCost, ExactRational};
use crate::request::PreparedModelRequest;
use crate::usage::DecimalUsd;
use std::time::{Duration, Instant};

const MIN_RETAINED_OUTPUT_BYTES: u64 = 256;
const MAX_TOOL_RETAINED_OUTPUT_BYTES: u64 = 64 * 1024;

#[derive(Clone)]
pub struct AutomationDispatchPolicy {
    pub budget: AutomationBudgetV1,
    frozen: FrozenHarnessProvider,
    maximum_microusd: u64,
    local_zero: bool,
}
impl std::fmt::Debug for AutomationDispatchPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AutomationDispatchPolicy")
            .field("budget", &self.budget)
            .field("route", &self.frozen.source)
            .field("model", &self.frozen.model)
            .finish_non_exhaustive()
    }
}
impl AutomationDispatchPolicy {
    pub fn new(
        budget: AutomationBudgetV1,
        mut frozen: FrozenHarnessProvider,
    ) -> Result<Self, String> {
        if budget.provider_requests == 0
            || budget.input_tokens == 0
            || budget.output_tokens == 0
            || budget.runtime_secs == 0
            || budget.output_bytes == 0
            || budget.max_cost_usd == DecimalUsd::ZERO
            || frozen.context_tokens == 0
            || frozen.source.provider != frozen.price.provider
            || frozen.model != frozen.price.model
        {
            return Err(
                "Automation dispatch requires positive limits and an exact frozen tariff".into(),
            );
        }
        frozen.output_tokens = frozen
            .output_tokens
            .min(budget.output_tokens.min(u64::from(u32::MAX)) as u32);
        frozen.request_seconds = frozen.request_seconds.min(budget.runtime_secs);
        let price = ExactRational::from_decimal(budget.max_cost_usd)
            .ok_or("Automation cost limit overflow")?;
        let maximum_microusd = u64::try_from(
            price
                .numerator()
                .checked_mul(1_000_000)
                .ok_or("Automation cost limit overflow")?
                / price.denominator(),
        )
        .map_err(|_| "Automation cost limit overflow")?;
        if maximum_microusd == 0 || frozen.output_tokens == 0 {
            return Err("Automation cost or output cap is too small".into());
        }
        // Validate the tariff now; the request's narrower text ceiling is used
        // for each actual reservation instead of charging an empty full context.
        frozen
            .maximum_microusd()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            budget,
            frozen,
            maximum_microusd,
            local_zero: false,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub fn for_run(
        config: &crate::config::CaConfig,
        paths: &crate::runtime::RuntimePaths,
        profile: &crate::runtime::ActiveProfile,
        provider: &str,
        model: &str,
        options: crate::agent::client::ChatRequestOptions,
        budget: AutomationBudgetV1,
        binding: &crate::automation::PricingBindingV1,
    ) -> Result<Self, String> {
        use crate::automation::PricingBindingV1;
        match binding {
            PricingBindingV1::Metered { price_card_sha256 } => {
                let mut selected = config.clone();
                selected.defaults.provider = provider.into();
                selected.defaults.model = model.into();
                let mut frozen =
                    crate::harness_control::configured_provider(&selected, paths, profile)
                        .map_err(|error| error.to_string())?;
                let digest = format!(
                    "{:x}",
                    <sha2::Sha256 as sha2::Digest>::digest(
                        serde_json::to_vec(&frozen.price)
                            .map_err(|_| "Automation tariff serialization failed")?
                    )
                );
                if &digest != price_card_sha256 {
                    return Err("Automation frozen tariff changed since approval".into());
                }
                frozen.options = options;
                Self::new(budget, frozen)
            }
            PricingBindingV1::MeteredRoutes { .. } => Err(
                "multi-route learning pricing requires the native bounded role-run worker".into(),
            ),
            PricingBindingV1::LocalZero => {
                let route = config
                    .providers
                    .get(provider)
                    .ok_or("Automation local provider is unavailable")?;
                let url = reqwest::Url::parse(&route.base_url)
                    .map_err(|_| "Automation local endpoint is invalid")?;
                if !url
                    .host_str()
                    .and_then(|host| host.parse::<std::net::IpAddr>().ok())
                    .is_some_and(|ip| ip.is_loopback())
                    || !matches!(url.scheme(), "http" | "https")
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
                {
                    return Err(
                        "Automation local-zero binding requires a literal loopback endpoint".into(),
                    );
                }
                // This private zero tariff is explicitly authorized by the
                // local binding; it never enters the official price catalog.
                let mut price = crate::pricing::OfficialPriceCatalog::seeded()
                    .cards
                    .remove(0);
                price.provider = provider.into();
                price.model = model.into();
                price.pricing = crate::pricing::CardPricing::MeteredUsd {
                    input_per_million_usd: DecimalUsd::ZERO,
                    cached_input_per_million_usd: DecimalUsd::ZERO,
                    cache_write_billing: crate::usage::CacheWriteBilling::Included,
                    output_per_million_usd: DecimalUsd::ZERO,
                    reasoning_output_per_million_usd: DecimalUsd::ZERO,
                    long_context_threshold: u64::MAX,
                    long_input_multiplier_numerator: 1,
                    long_input_multiplier_denominator: 1,
                    long_output_multiplier_numerator: 1,
                    long_output_multiplier_denominator: 1,
                };
                let context = config
                    .context_for_model(provider, model)
                    .and_then(|context| context.context_window_tokens)
                    .unwrap_or(64 * 1024)
                    .min(u64::from(u32::MAX)) as u32;
                let frozen = FrozenHarnessProvider {
                    source: crate::agent::stream::ProviderStateSourceV1::from_route(
                        provider,
                        route.protocol,
                        &route.base_url,
                    )
                    .map_err(|error| error.to_string())?,
                    model: model.into(),
                    options,
                    context_tokens: context,
                    input_byte_limit: Some(context),
                    output_tokens: context.min(8192),
                    request_seconds: 120,
                    price,
                    billing: None,
                };
                let mut policy = Self::new(budget, frozen)?;
                policy.local_zero = true;
                Ok(policy)
            }
            PricingBindingV1::ModelFree => {
                Err("Model-free automation cannot dispatch a provider request".into())
            }
        }
    }
    pub fn output_token_cap(&self) -> u32 {
        self.frozen.output_tokens
    }
    pub fn input_byte_cap(&self) -> usize {
        self.frozen
            .input_byte_limit
            .unwrap_or(self.frozen.context_tokens) as usize
    }
    pub fn request_seconds(&self) -> u64 {
        self.frozen.request_seconds
    }
    pub fn start(&self) -> AutomationDispatch {
        AutomationDispatch {
            policy: self.clone(),
            started: Instant::now(),
            counters: Default::default(),
            microusd: 0,
            tool_output_cap: 0,
        }
    }
}

pub struct AutomationDispatch {
    policy: AutomationDispatchPolicy,
    started: Instant,
    counters: AutomationUsageCountersV1,
    microusd: u64,
    tool_output_cap: usize,
}
pub struct AutomationReservation {
    input_tokens: u64,
    output_tokens: u64,
    microusd: u64,
    started_at: chrono::DateTime<chrono::Utc>,
}
impl AutomationDispatch {
    pub fn remaining(&self) -> Duration {
        Duration::from_secs(self.policy.budget.runtime_secs).saturating_sub(self.started.elapsed())
    }
    fn check_time(&self) -> Result<(), String> {
        if self.remaining().is_zero() {
            Err("Automation runtime budget exhausted".into())
        } else {
            Ok(())
        }
    }
    pub fn reserve_request(
        &mut self,
        request: &PreparedModelRequest,
    ) -> Result<AutomationReservation, String> {
        self.check_time()?;
        if self
            .policy
            .budget
            .output_bytes
            .saturating_sub(self.counters.output_bytes)
            < MIN_RETAINED_OUTPUT_BYTES
        {
            return Err(
                "Automation retained-output capacity exhausted before provider dispatch".into(),
            );
        }
        if request.provider != self.policy.frozen.source.provider
            || request.model != self.policy.frozen.model
        {
            return Err("Automation request changed its frozen route".into());
        }
        if request
            .messages
            .iter()
            .any(|message| message.content.has_images())
        {
            return Err("Automation token reservation supports bounded text only".into());
        }
        // A UTF-8 byte can require one token; count serialized tool schemas and
        // options too, with additional allowance for provider message framing.
        struct Counter {
            bytes: usize,
            maximum: usize,
        }
        impl std::io::Write for Counter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.bytes = self
                    .bytes
                    .checked_add(bytes.len())
                    .filter(|bytes| *bytes <= self.maximum)
                    .ok_or_else(|| {
                        std::io::Error::other("Automation input frame exceeds its bound")
                    })?;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut input = Counter {
            bytes: 0,
            maximum: self.policy.input_byte_cap(),
        };
        serde_json::to_writer(
            &mut input,
            &(&request.messages, &request.tools, &request.options),
        )
        .map_err(|_| "Automation input frame exceeds its bound")?;
        let input_tokens = input
            .bytes
            .saturating_add(1024)
            .max(self.policy.output_token_cap() as usize) as u64;
        if input_tokens > u64::from(self.policy.frozen.context_tokens) {
            return Err("Automation conservative input ceiling exceeds provider context".into());
        }
        let output_tokens = u64::from(self.policy.output_token_cap());
        let mut tariff = self.policy.frozen.clone();
        tariff.context_tokens = input_tokens as u32;
        tariff.input_byte_limit = None;
        let microusd = if self.policy.local_zero {
            0
        } else {
            tariff
                .maximum_microusd()
                .map_err(|error| error.to_string())?
        };
        let requests = self
            .counters
            .provider_requests
            .checked_add(1)
            .ok_or("Automation request counter overflow")?;
        let inputs = self
            .counters
            .input_tokens
            .checked_add(input_tokens)
            .ok_or("Automation input counter overflow")?;
        let outputs = self
            .counters
            .output_tokens
            .checked_add(output_tokens)
            .ok_or("Automation output counter overflow")?;
        let cost = self
            .microusd
            .checked_add(microusd)
            .ok_or("Automation cost counter overflow")?;
        let budget = &self.policy.budget;
        if requests > budget.provider_requests
            || inputs > budget.input_tokens
            || outputs > budget.output_tokens
            || cost > self.policy.maximum_microusd
        {
            return Err(
                "Automation provider request, token, or cost budget exhausted before dispatch"
                    .into(),
            );
        }
        self.counters.provider_requests = requests;
        self.counters.input_tokens = inputs;
        self.counters.output_tokens = outputs;
        self.microusd = cost;
        Ok(AutomationReservation {
            input_tokens,
            output_tokens,
            microusd,
            started_at: chrono::Utc::now(),
        })
    }
    pub fn settle_request(
        &mut self,
        reservation: AutomationReservation,
        usage: Option<&UsageInfo>,
    ) -> Result<(), String> {
        let Some(usage) = usage else {
            return Ok(());
        };
        let cost = if self.policy.local_zero {
            0
        } else {
            actual_cost_for(
                &self.policy.frozen.price,
                self.policy.frozen.billing.as_ref(),
                usage,
                reservation.started_at,
                chrono::Utc::now(),
            )
            .and_then(cost_microusd)
            .ok_or("Automation usage cannot be priced with its frozen tariff")?
        };
        if u64::from(usage.prompt_tokens) > reservation.input_tokens
            || u64::from(usage.completion_tokens) > reservation.output_tokens
            || cost > reservation.microusd
        {
            return Err("Automation provider usage exceeded its conservative reservation".into());
        }
        self.counters.input_tokens =
            self.counters.input_tokens - reservation.input_tokens + u64::from(usage.prompt_tokens);
        self.counters.output_tokens = self.counters.output_tokens - reservation.output_tokens
            + u64::from(usage.completion_tokens);
        self.microusd = self.microusd - reservation.microusd + cost;
        Ok(())
    }
    pub fn reserve_tools(&mut self, count: usize) -> Result<(), String> {
        self.check_time()?;
        if count == 0 {
            self.tool_output_cap = 0;
            return Ok(());
        }
        let count =
            u32::try_from(count).map_err(|_| "Automation tool batch exceeds counter range")?;
        let cap = (self
            .policy
            .budget
            .output_bytes
            .saturating_sub(self.counters.output_bytes)
            / u64::from(count))
        .min(MAX_TOOL_RETAINED_OUTPUT_BYTES);
        if cap < MIN_RETAINED_OUTPUT_BYTES {
            return Err(
                "Automation retained-output capacity insufficient before tool dispatch".into(),
            );
        }
        let next = self
            .counters
            .tool_calls
            .checked_add(count)
            .ok_or("Automation tool counter overflow")?;
        if next > self.policy.budget.tool_calls {
            return Err("Automation tool-call budget exhausted before dispatch".into());
        }
        // Charge the frozen whole-batch ceiling before any physical tool starts.
        // Background commands may produce output later, after their start result.
        self.counters.output_bytes = self
            .counters
            .output_bytes
            .checked_add(
                cap.checked_mul(u64::from(count))
                    .ok_or("Automation tool output reservation overflow")?,
            )
            .ok_or("Automation output counter overflow")?;
        self.tool_output_cap = cap as usize;
        self.counters.tool_calls = next;
        Ok(())
    }
    pub fn tool_output_cap(&self) -> usize {
        self.tool_output_cap
    }
    pub fn account_output(&mut self, bytes: usize) -> Result<(), String> {
        self.check_time()?;
        let total = self
            .counters
            .output_bytes
            .checked_add(bytes as u64)
            .ok_or("Automation output counter overflow")?;
        if total > self.policy.budget.output_bytes {
            return Err("Automation retained-output budget exhausted".into());
        }
        self.counters.output_bytes = total;
        Ok(())
    }
    pub fn account_provider_event(&mut self, event: &ProviderEvent) -> Result<(), String> {
        let bytes = match event {
            ProviderEvent::Text(text) | ProviderEvent::Reasoning(text) => text.len(),
            ProviderEvent::ToolCallStart { id, name, .. } => id.len().saturating_add(name.len()),
            ProviderEvent::ToolCallArg { arg, .. } => arg.len(),
            ProviderEvent::ResponseItem(item) => serde_json::to_vec(item)
                .map_err(|_| "Automation provider item cannot be bounded")?
                .len(),
            _ => 0,
        };
        self.account_output(bytes)
    }
    pub fn snapshot(&self) -> AutomationUsageCountersV1 {
        let mut counters = self.counters.clone();
        counters.runtime_secs =
            charged_runtime_secs(self.started.elapsed()).min(self.policy.budget.runtime_secs);
        counters.cost_usd = format!(
            "{}.{:06}",
            self.microusd / 1_000_000,
            self.microusd % 1_000_000
        )
        .parse()
        .expect("bounded microdollar amount");
        counters
    }
}

/// Apply the ceiling admitted before this tool batch without discarding typed
/// mutation receipts or private/public result separation.
pub fn bound_tool_result(
    mut result: crate::agent::registry::ToolExecutionResult,
    cap: usize,
) -> crate::agent::registry::ToolExecutionResult {
    use crate::agent::registry::ToolExecutionResult;
    if result.image().is_some_and(|image| {
        image
            .encoded_len()
            .saturating_add(result.model_text().len())
            > cap
    }) {
        return ToolExecutionResult::Failure(
            "Image output exceeds the admitted automation output capacity".into(),
        );
    }
    let bound = |text: &mut String| {
        if text.len() <= cap {
            return;
        }
        let marker = "\n[output truncated by automation budget]";
        let suffix = &marker[..marker.len().min(cap)];
        let mut end = cap.saturating_sub(suffix.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str(suffix);
    };
    match &mut result {
        ToolExecutionResult::Success(text)
        | ToolExecutionResult::Failure(text)
        | ToolExecutionResult::Denied(text)
        | ToolExecutionResult::Cancelled(text) => bound(text),
        ToolExecutionResult::Command { model_text, .. } => bound(model_text),
        ToolExecutionResult::SuccessMutation { marker, .. }
        | ToolExecutionResult::SuccessImage { marker, .. }
        | ToolExecutionResult::MemoryProposal { marker, .. } => bound(marker),
        ToolExecutionResult::ExternalPrivateSuccess { live, persistent } => {
            bound(live);
            bound(persistent);
        }
    }
    result
}

pub fn bound_retained_text(text: String, cap: usize) -> String {
    bound_tool_result(
        crate::agent::registry::ToolExecutionResult::Success(text),
        cap,
    )
    .model_text()
    .to_string()
}
/// Counters use whole seconds; round upward so short runs cannot evade a
/// persisted daily allowance. Dispatch deadlines retain exact durations.
pub fn charged_runtime_secs(elapsed: Duration) -> u64 {
    elapsed
        .as_secs()
        .saturating_add(u64::from(elapsed.subsec_nanos() != 0))
}
fn cost_microusd(cost: ActualCost) -> Option<u64> {
    let value = match cost {
        ActualCost::IncrementalUsd(value) | ActualCost::UpperBoundIncrementalUsd(value) => value,
        ActualCost::SubscriptionAllocation { allocated_usd, .. } => allocated_usd,
    };
    value
        .numerator()
        .checked_mul(1_000_000)?
        .div_ceil(value.denominator())
        .try_into()
        .ok()
}

pub fn remaining_budget(
    budget: &AutomationBudgetV1,
    usage: &AutomationUsageCountersV1,
) -> Result<AutomationBudgetV1, String> {
    let maximum =
        ExactRational::from_decimal(budget.max_cost_usd).ok_or("Automation cost limit overflow")?;
    let spent =
        ExactRational::from_decimal(usage.cost_usd).ok_or("Automation charged cost overflow")?;
    let numerator = maximum
        .numerator()
        .checked_mul(spent.denominator())
        .and_then(|left| {
            spent
                .numerator()
                .checked_mul(maximum.denominator())
                .and_then(|right| left.checked_sub(right))
        })
        .ok_or("Automation workflow cost budget exhausted")?;
    let denominator = maximum
        .denominator()
        .checked_mul(spent.denominator())
        .ok_or("Automation workflow cost overflow")?;
    let micros = numerator
        .checked_mul(1_000_000)
        .ok_or("Automation workflow cost overflow")?
        / denominator;
    let subtract = || "Automation workflow budget exhausted".to_string();
    Ok(AutomationBudgetV1 {
        provider_requests: budget
            .provider_requests
            .checked_sub(usage.provider_requests)
            .ok_or_else(subtract)?,
        input_tokens: budget
            .input_tokens
            .checked_sub(usage.input_tokens)
            .ok_or_else(subtract)?,
        output_tokens: budget
            .output_tokens
            .checked_sub(usage.output_tokens)
            .ok_or_else(subtract)?,
        tool_calls: budget
            .tool_calls
            .checked_sub(usage.tool_calls)
            .ok_or_else(subtract)?,
        runtime_secs: budget
            .runtime_secs
            .checked_sub(usage.runtime_secs)
            .ok_or_else(subtract)?,
        output_bytes: budget
            .output_bytes
            .checked_sub(usage.output_bytes)
            .ok_or_else(subtract)?,
        max_cost_usd: format!("{}.{:06}", micros / 1_000_000, micros % 1_000_000)
            .parse()
            .map_err(|_| "Automation remaining cost overflow")?,
    })
}
pub fn add_usage(
    total: &mut AutomationUsageCountersV1,
    value: &AutomationUsageCountersV1,
) -> Result<(), String> {
    let overflow = || "Automation workflow accounting overflow".to_string();
    total.provider_requests = total
        .provider_requests
        .checked_add(value.provider_requests)
        .ok_or_else(overflow)?;
    total.input_tokens = total
        .input_tokens
        .checked_add(value.input_tokens)
        .ok_or_else(overflow)?;
    total.output_tokens = total
        .output_tokens
        .checked_add(value.output_tokens)
        .ok_or_else(overflow)?;
    total.tool_calls = total
        .tool_calls
        .checked_add(value.tool_calls)
        .ok_or_else(overflow)?;
    total.runtime_secs = total
        .runtime_secs
        .checked_add(value.runtime_secs)
        .ok_or_else(overflow)?;
    total.output_bytes = total
        .output_bytes
        .checked_add(value.output_bytes)
        .ok_or_else(overflow)?;
    total.cost_usd = total
        .cost_usd
        .checked_add(value.cost_usd)
        .ok_or_else(overflow)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_output_admission_precedes_requests_and_tool_batches() {
        let mut budget = AutomationBudgetV1::per_run_default();
        budget.output_bytes = 1;
        let mut frozen = crate::harness_budget::tests::frozen();
        frozen.context_tokens = 4096;
        frozen.input_byte_limit = Some(4096);
        let policy = AutomationDispatchPolicy::new(budget, frozen).unwrap();
        let mut dispatch = policy.start();
        assert!(dispatch.reserve_request(&request()).is_err());
        assert!(dispatch.reserve_tools(1).is_err());
        assert_eq!(dispatch.snapshot().provider_requests, 0);
        assert_eq!(dispatch.snapshot().tool_calls, 0);
    }

    #[test]
    fn admitted_tool_batch_keeps_its_output_ceiling_and_exhausts_later_dispatch() {
        let mut policy = policy();
        policy.budget.output_bytes = 8192;
        let mut dispatch = policy.start();
        dispatch.reserve_tools(2).unwrap();
        assert_eq!(dispatch.tool_output_cap(), 4096);
        assert_eq!(dispatch.snapshot().output_bytes, 8192);
        assert_eq!(dispatch.snapshot().tool_calls, 2);
        assert!(dispatch.reserve_request(&request()).is_err());
        assert!(dispatch.reserve_tools(1).is_err());
        assert_eq!(dispatch.tool_output_cap(), 4096);
        assert_eq!(dispatch.snapshot().provider_requests, 0);
        assert_eq!(dispatch.snapshot().tool_calls, 2);
    }

    #[test]
    fn bounded_tool_output_preserves_typed_receipts_private_views_and_utf8() {
        use crate::agent::registry::ToolExecutionResult;
        let receipt =
            crate::undo::FileMutationBatchReceipt::single(crate::undo::FileMutationReceipt::new(
                std::path::PathBuf::from("/fixture/edited.txt"),
                "write_file",
                crate::undo::FileState::Missing,
                crate::undo::FileState::Utf8("new".into()),
            ));
        let result = bound_tool_result(
            ToolExecutionResult::SuccessMutation {
                marker: "文字".repeat(1000),
                mutation: receipt.clone(),
            },
            257,
        );
        assert!(result.is_success());
        assert!(result.model_text().len() <= 257);
        assert!(result
            .model_text()
            .ends_with("[output truncated by automation budget]"));
        assert_eq!(result.mutation_batch(), Some(&receipt));
        let private = bound_tool_result(
            ToolExecutionResult::ExternalPrivateSuccess {
                live: "private文字".repeat(1000),
                persistent: "[external content withheld]".into(),
            },
            256,
        );
        assert!(private.is_external_private());
        assert!(private.model_text().len() <= 256);
        assert_eq!(private.persistent_text(), "[external content withheld]");
        let cancelled = bound_tool_result(ToolExecutionResult::Cancelled("x".repeat(1000)), 256);
        assert!(matches!(cancelled, ToolExecutionResult::Cancelled(_)));
        assert!(!cancelled.is_success());
    }

    #[test]
    fn local_zero_preserves_authenticated_exact_loopback_bindings() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(temporary.path());
        let mut config = crate::config::CaConfig::default();
        config.providers.insert(
            "fixture-local".into(),
            crate::config::ProviderConfig {
                google: None,
                tool_schema_policy: Default::default(),
                base_url: "http://127.0.0.1:12345/v1".into(),
                api_key: String::new(),
                protocol: Default::default(),
                prompt_cache_mode: Default::default(),
                models: vec!["fixture".into()],
                timeout: Default::default(),
            },
        );
        let create = |config: &crate::config::CaConfig| {
            AutomationDispatchPolicy::for_run(
                config,
                &paths,
                &crate::runtime::ActiveProfile::Default,
                "fixture-local",
                "fixture",
                Default::default(),
                AutomationBudgetV1::per_run_default(),
                &crate::automation::PricingBindingV1::LocalZero,
            )
        };
        assert!(create(&config).is_ok());
        config.providers.get_mut("fixture-local").unwrap().api_key =
            "fixture-credential-never-echo".into();
        assert!(create(&config).is_ok());
        config.providers.get_mut("fixture-local").unwrap().base_url =
            "http://localhost:12345/v1".into();
        assert!(create(&config).is_err());
        assert!(!paths
            .profile_data_dir(&crate::runtime::ActiveProfile::Default)
            .exists());
    }
    fn policy() -> AutomationDispatchPolicy {
        AutomationDispatchPolicy::new(
            AutomationBudgetV1::per_run_default(),
            crate::harness_budget::tests::frozen(),
        )
        .unwrap()
    }
    fn request() -> PreparedModelRequest {
        PreparedModelRequest::new(
            crate::request::RequestKind::Scheduled,
            "fake",
            "fake-model",
            vec![],
            vec![],
            Default::default(),
        )
    }
    #[test]
    fn reservations_precede_dispatch_and_missing_usage_stays_charged() {
        let mut policy = policy();
        policy.frozen.context_tokens = 2048;
        policy.frozen.input_byte_limit = Some(2048);
        policy.budget.provider_requests = 1;
        let mut dispatch = policy.start();
        let reservation = dispatch.reserve_request(&request()).unwrap();
        assert_eq!(dispatch.snapshot().provider_requests, 1);
        assert!(dispatch.reserve_request(&request()).is_err());
        dispatch.settle_request(reservation, None).unwrap();
        assert!(dispatch.snapshot().input_tokens >= 1024);
        assert_eq!(dispatch.snapshot().output_tokens, 100);
        assert_ne!(dispatch.snapshot().cost_usd, DecimalUsd::ZERO);
    }
    #[test]
    fn subsecond_runs_charge_a_second_instead_of_evading_daily_runtime() {
        let mut dispatch = policy().start();
        dispatch.started = Instant::now() - Duration::from_millis(1);
        assert_eq!(dispatch.snapshot().runtime_secs, 1);
    }
    #[test]
    fn tool_batches_and_output_are_refused_before_any_admission() {
        let mut policy = policy();
        policy.budget.tool_calls = 1;
        policy.budget.output_bytes = 4;
        let mut dispatch = policy.start();
        assert!(dispatch.reserve_tools(2).is_err());
        assert_eq!(dispatch.snapshot().tool_calls, 0);
        dispatch.account_output(4).unwrap();
        assert!(dispatch.account_output(1).is_err());
        assert_eq!(dispatch.snapshot().output_bytes, 4);
    }
}
