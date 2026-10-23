//! Host-owned inference accounting. No candidate-supplied price, usage, route,
//! or budget can authorize a provider request.
use crate::agent::client::{
    AgentClient, ChatBackend, ChatMessage, ChatRequestOptions, ChatStreamItem, ToolDef,
};
use crate::agent::stream::{ProviderError, ProviderEvent, ProviderStateSourceV1, UsageInfo};
use crate::pricing::{
    actual_cost_for, ActualCost, CardPricing, ExactRational, OfficialPriceCard,
    ProviderBillingConfig,
};
use anyhow::{bail, Context, Result};
use chrono::{TimeZone, Utc};
use codex_team::self_improvement::{
    canonical_digest, update_harness_cycle, HarnessPlan, LearningVault,
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenHarnessProvider {
    pub source: ProviderStateSourceV1,
    pub model: String,
    pub options: ChatRequestOptions,
    /// Provider contract context ceiling used for conservative reservations.
    pub context_tokens: u32,
    /// Configured payload cap, kept separate from the provider's billable bound.
    /// Older saved experiments omit this and retain their original byte cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_byte_limit: Option<u32>,
    pub output_tokens: u32,
    pub request_seconds: u64,
    pub price: OfficialPriceCard,
    pub billing: Option<ProviderBillingConfig>,
}

impl FrozenHarnessProvider {
    fn input_byte_cap(&self) -> usize {
        self.input_byte_limit
            .unwrap_or(self.context_tokens)
            .min(self.context_tokens) as usize
    }
    pub fn route_digest(&self) -> Result<String> {
        let mut route = serde_json::json!({
            "source": self.source, "model": self.model, "options": self.options,
            "context_tokens": self.context_tokens, "output_tokens": self.output_tokens,
            "request_seconds": self.request_seconds,
        });
        if let Some(limit) = self.input_byte_limit {
            route["input_byte_limit"] = limit.into();
        }
        canonical_digest(&route)
    }
    pub fn price_digest(&self) -> Result<String> {
        canonical_digest(&(&self.price, &self.billing))
    }

    /// Evaluate the corners of the piecewise-linear tariff, including both
    /// context tiers, all cache categories, reasoning output and every UTC hour.
    /// Round upward; even sub-microdollar requests require a reservation.
    pub(crate) fn maximum_microusd(&self) -> Result<u64> {
        if self.context_tokens == 0
            || self.input_byte_limit == Some(0)
            || self.output_tokens == 0
            || self.output_tokens > self.context_tokens
            || self.request_seconds == 0
            || self.request_seconds > 7200
        {
            bail!("invalid frozen provider limits");
        }
        let mut prompts = vec![self.context_tokens];
        if let CardPricing::MeteredUsd {
            long_context_threshold,
            ..
        } = self.price.pricing
        {
            if let Ok(threshold) = u32::try_from(long_context_threshold) {
                if threshold < self.context_tokens {
                    prompts.push(threshold);
                }
            }
        }
        let mut maximum = 0;
        for prompt in prompts {
            for (cached, written) in [(0, 0), (prompt, 0), (0, prompt)] {
                for reasoning in [0, self.output_tokens] {
                    let usage = UsageInfo {
                        prompt_tokens: prompt,
                        completion_tokens: self.output_tokens,
                        total_tokens: prompt
                            .checked_add(self.output_tokens)
                            .context("token limit overflow")?,
                        cached_prompt_tokens: Some(cached),
                        cache_write_tokens: Some(written),
                        reasoning_tokens: Some(reasoning),
                        response_metadata: None,
                        cache_usage_override: None,
                    };
                    for hour in 0..24 {
                        let at = Utc
                            .with_ymd_and_hms(2026, 1, 1, hour, 0, 0)
                            .single()
                            .unwrap();
                        let cost =
                            actual_cost_for(&self.price, self.billing.as_ref(), &usage, at, at)
                                .context("frozen tariff cannot bound provider charges")?;
                        maximum = maximum.max(cost_microusd(cost)?);
                    }
                }
            }
        }
        Ok(maximum.max(1))
    }
}

/// A bounded serializer avoids allocating a second, arbitrarily large copy of
/// tool results. One byte per configured context token is deliberately strict;
/// this does not establish the provider's actual tokenizer or context limit.
fn validate_input_bytes(
    messages: &[ChatMessage],
    tools: &[ToolDef],
    options: &ChatRequestOptions,
    maximum: usize,
) -> Result<()> {
    if messages.iter().any(|message| message.content.has_images()) {
        bail!("harness input accounting supports text only");
    }
    struct Limit(usize);
    impl std::io::Write for Limit {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.checked_sub(bytes.len()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "harness input byte cap exceeded",
                )
            })?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Limit(maximum), &(messages, tools, options))
        .context("harness input byte cap exceeded")?;
    Ok(())
}

fn cost_microusd(cost: ActualCost) -> Result<u64> {
    let value: ExactRational = match cost {
        ActualCost::IncrementalUsd(value) | ActualCost::UpperBoundIncrementalUsd(value) => value,
        ActualCost::SubscriptionAllocation { allocated_usd, .. } => allocated_usd,
    };
    let numerator = value
        .numerator()
        .checked_mul(1_000_000)
        .context("price overflow")?;
    u64::try_from(numerator.div_ceil(value.denominator())).context("price exceeds budget range")
}

pub struct BudgetedBackend<B> {
    backend: B,
    vault: LearningVault,
    plan: HarnessPlan,
    frozen: FrozenHarnessProvider,
    maximum_microusd: u64,
    activity: Option<std::sync::Arc<crate::harness_activity::HarnessActivity>>,
    // Serialize calls so active time is not counted twice for concurrent work.
    gate: tokio::sync::Mutex<()>,
}

impl BudgetedBackend<AgentClient> {
    pub fn new(
        client: AgentClient,
        vault: LearningVault,
        plan: HarnessPlan,
        frozen: FrozenHarnessProvider,
    ) -> Result<Self> {
        if client.route_identity()? != frozen.source
            || client.model_id() != frozen.model
            || client.provider_id() != frozen.price.provider
            || client.model_id() != frozen.price.model
        {
            bail!("provider route differs from frozen experiment");
        }
        let client = client
            .with_one_attempt_policy()
            .without_trace()
            .with_output_token_cap(frozen.output_tokens)?
            .with_input_byte_cap(frozen.input_byte_cap())?;
        Self::bind(client, vault, plan, frozen)
    }
}

impl<B: ChatBackend> BudgetedBackend<B> {
    fn bind(
        backend: B,
        vault: LearningVault,
        plan: HarnessPlan,
        frozen: FrozenHarnessProvider,
    ) -> Result<Self> {
        plan.validate()?;
        if frozen.route_digest()? != plan.route_digest
            || frozen.price_digest()? != plan.price_digest
        {
            bail!("frozen route or pricing changed");
        }
        if !frozen.price.fresh_at(Utc::now()) {
            bail!("verified provider pricing is stale");
        }
        let maximum_microusd = frozen.maximum_microusd()?;
        Ok(Self {
            backend,
            vault,
            plan,
            frozen,
            maximum_microusd,
            activity: None,
            gate: tokio::sync::Mutex::new(()),
        })
    }

    pub fn in_activity(
        mut self,
        activity: std::sync::Arc<crate::harness_activity::HarnessActivity>,
    ) -> Result<Self> {
        if !activity.matches(&self.vault, &self.plan) {
            bail!("activity belongs to a different experiment");
        }
        self.activity = Some(activity);
        Ok(self)
    }

    async fn request(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>> {
        let _gate = self.gate.lock().await;
        if !self.frozen.price.fresh_at(Utc::now()) {
            bail!("verified provider pricing is stale");
        }
        // Admit only bounded text inputs before reserving spend. The real client
        // separately checks the exact adapter body, including protocol overhead.
        validate_input_bytes(
            messages,
            tools,
            &self.frozen.options,
            self.frozen.input_byte_cap(),
        )?;
        let request_id = uuid::Uuid::new_v4().to_string();
        let (vault, plan, id) = (self.vault.clone(), self.plan.clone(), request_id.clone());
        let maximum = self.maximum_microusd;
        let seconds = self.frozen.request_seconds;
        let timeout = match &self.activity {
            Some(activity) => activity.remaining()?.min(Duration::from_secs(seconds)),
            None => Duration::from_secs(seconds),
        };
        let active = self.activity.as_ref().map(|activity| {
            let (id, maximum) = activity.reservation();
            (id.to_owned(), maximum)
        });
        let own_time = active.is_none();
        let common = crate::harness_automation::reserve_request(
            &self.vault,
            &self.plan.digest,
            &request_id,
            (
                u64::from(self.frozen.context_tokens),
                u64::from(self.frozen.output_tokens),
                maximum,
                if tools.is_empty() { 0 } else { 512 },
            ),
        )
        .await?;
        let admission = tokio::task::spawn_blocking(move || {
            update_harness_cycle(&vault, &plan.id, |state| {
                let mut admitted = state.clone();
                let time_result = match &active {
                    Some((work_id, duration))
                        if admitted.active_reservations.get(work_id) == Some(duration) =>
                    {
                        Ok(())
                    }
                    Some(_) => Err(anyhow::anyhow!(
                        "activity reservation is missing or changed"
                    )),
                    None => admitted.reserve_active_time(&plan, &id, seconds),
                };
                let result = time_result.and_then(|()| admitted.reserve(&plan, &id, maximum));
                if result.is_ok() {
                    *state = admitted;
                } else {
                    // Retain a terminal budget decision, but no phantom
                    // reservation for an attempt that never reached the wire.
                    state.stage = admitted.stage;
                }
                result
            })
        })
        .await;
        if !matches!(&admission, Ok(Ok(()))) {
            // No provider operation ran, so releasing this reservation is safe.
            crate::harness_automation::finish_request(&self.vault, common, None, true).await?;
            admission??;
            unreachable!("successful admission returned above");
        }
        // Both reservations are now on disk. No fallible provider work precedes them.
        let clock = Instant::now();
        let started = Utc::now();
        let operation = async {
            let mut stream = self
                .backend
                .stream_chat_with_options(messages, tools, &self.frozen.options)
                .await?;
            let mut events = Vec::new();
            let mut bytes = 0usize;
            let mut done = false;
            let mut finished = false;
            let mut usage = Vec::new();
            while let Some(event) = stream.recv().await {
                let event = event?;
                if done {
                    bail!("provider emitted data after completion");
                }
                bytes = bytes
                    .checked_add(serde_json::to_vec(&event)?.len())
                    .context("response size overflow")?;
                if bytes > crate::harness_worker::MAX_FRAME || events.len() >= 65536 {
                    bail!("budgeted provider response exceeded its bounded frame");
                }
                match &event {
                    ProviderEvent::Finish {
                        usage: reported, ..
                    } => {
                        finished = true;
                        if let Some(reported) = reported {
                            usage.push(reported.clone());
                        }
                    }
                    ProviderEvent::Usage(reported) => usage.push(reported.clone()),
                    ProviderEvent::Done => done = true,
                    _ => {}
                }
                events.push(event);
            }
            if !done || !finished || usage.is_empty() {
                bail!("provider completion or usage is unknown");
            }
            let mut actual = 0;
            let mut input_tokens = 0;
            let mut output_tokens = 0;
            for usage in usage {
                if usage.prompt_tokens > self.frozen.context_tokens
                    || usage.completion_tokens > self.frozen.output_tokens
                {
                    bail!("provider usage exceeded frozen token limits");
                }
                input_tokens = input_tokens.max(u64::from(usage.prompt_tokens));
                output_tokens = output_tokens.max(u64::from(usage.completion_tokens));
                actual = actual.max(cost_microusd(
                    actual_cost_for(
                        &self.frozen.price,
                        self.frozen.billing.as_ref(),
                        &usage,
                        started,
                        Utc::now(),
                    )
                    .context("provider charge is unknown")?,
                )?);
            }
            let tool_calls = events
                .iter()
                .filter_map(|event| match event {
                    ProviderEvent::ToolCallStart { index, .. } => Some(*index),
                    _ => None,
                })
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            Ok::<_, anyhow::Error>((
                events,
                actual,
                input_tokens,
                output_tokens,
                u32::try_from(tool_calls).context("provider tool call overflow")?,
            ))
        };
        let timeout = match &self.activity {
            Some(activity) => activity.remaining()?.min(timeout),
            None => timeout,
        };
        let result = tokio::time::timeout(timeout, operation).await;
        let elapsed = if result.is_err() {
            seconds
        } else {
            clock
                .elapsed()
                .as_secs()
                .saturating_add(u64::from(clock.elapsed().subsec_nanos() != 0))
                .min(seconds)
        };
        let result = result
            .context("budgeted provider deadline expired")
            .and_then(|value| value);
        let actual = result.as_ref().ok().map(|(_, cost, ..)| *cost);
        let common_actual = result
            .as_ref()
            .ok()
            .map(|(_, cost, input, output, tools)| (*input, *output, *cost, *tools));
        let (vault, plan) = (self.vault.clone(), self.plan.clone());
        tokio::task::spawn_blocking(move || {
            update_harness_cycle(&vault, &plan.id, |state| {
                if own_time {
                    state.settle_active_time(&plan, &request_id, elapsed)?;
                }
                if let Some(actual) = actual {
                    state.settle(&request_id, actual)?;
                }
                Ok(())
            })
        })
        .await??;
        // Evidence is settled before any model-supplied tool call reaches the
        // isolated trial. Unknown completion keeps the conservative reservation.
        crate::harness_automation::finish_request(&self.vault, common, common_actual, false)
            .await?;
        let (events, ..) = result?;
        let (sender, receiver) = mpsc::unbounded_channel();
        for event in events {
            sender
                .send(Ok(event))
                .map_err(|_| anyhow::anyhow!("provider receiver closed"))?;
        }
        Ok(receiver)
    }
}

#[async_trait::async_trait]
impl<B: ChatBackend> ChatBackend for BudgetedBackend<B> {
    async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        self.request(messages, tools).await.map_err(|_| {
            ProviderError::Protocol(
                "host evaluation budget or provider evidence check failed".into(),
            )
        })
    }
    async fn stream_chat_with_options(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
        options: &ChatRequestOptions,
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
        if options != &self.frozen.options {
            return Err(ProviderError::Protocol(
                "frozen request options changed".into(),
            ));
        }
        self.stream_chat(messages, tools).await
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agent::provider::ProviderProtocol;
    use crate::agent::stream::FinishReason;
    use crate::usage::CacheWriteBilling;
    use codex_team::self_improvement::{
        load_harness_cycle, start_harness_cycle, HarnessCase, HarnessSettings,
        SourceRepositorySnapshotV1, HARNESS_SCHEMA,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct FakeProvider {
        vault: LearningVault,
        calls: Arc<AtomicUsize>,
        report_usage: bool,
        pause_forever: bool,
    }
    #[async_trait::async_trait]
    impl ChatBackend for FakeProvider {
        async fn stream_chat(
            &self,
            _: &[ChatMessage],
            _: &[ToolDef],
        ) -> Result<mpsc::UnboundedReceiver<ChatStreamItem>, ProviderError> {
            // This assertion executes exactly at the physical-request boundary.
            let (_, state) = load_harness_cycle(&self.vault, "budget-test").unwrap();
            assert!(!state.reservations.is_empty());
            assert!(!state.active_reservations.is_empty());
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.pause_forever {
                std::future::pending::<()>().await;
            }
            let (sender, receiver) = mpsc::unbounded_channel();
            sender
                .send(Ok(ProviderEvent::Finish {
                    reason: FinishReason::Stop,
                    usage: self.report_usage.then(|| usage(20, 10)),
                }))
                .unwrap();
            sender.send(Ok(ProviderEvent::Done)).unwrap();
            Ok(receiver)
        }
    }
    fn usage(prompt: u32, output: u32) -> UsageInfo {
        UsageInfo {
            prompt_tokens: prompt,
            completion_tokens: output,
            total_tokens: prompt + output,
            cached_prompt_tokens: Some(0),
            cache_write_tokens: Some(0),
            reasoning_tokens: Some(0),
            response_metadata: None,
            cache_usage_override: None,
        }
    }
    pub(crate) fn frozen() -> FrozenHarnessProvider {
        let mut price = crate::pricing::OfficialPriceCatalog::seeded()
            .cards
            .remove(0);
        price.provider = "fake".into();
        price.model = "fake-model".into();
        price.verified_at = Utc::now();
        price.pricing = CardPricing::MeteredUsd {
            input_per_million_usd: "1".parse().unwrap(),
            cached_input_per_million_usd: "0.5".parse().unwrap(),
            cache_write_billing: CacheWriteBilling::Surcharge {
                rate: "0.5".parse().unwrap(),
            },
            output_per_million_usd: "2".parse().unwrap(),
            reasoning_output_per_million_usd: "3".parse().unwrap(),
            long_context_threshold: 500,
            long_input_multiplier_numerator: 2,
            long_input_multiplier_denominator: 1,
            long_output_multiplier_numerator: 2,
            long_output_multiplier_denominator: 1,
        };
        FrozenHarnessProvider {
            source: ProviderStateSourceV1::from_route(
                "fake",
                ProviderProtocol::ChatCompletions,
                "http://127.0.0.1:1/v1",
            )
            .unwrap(),
            model: "fake-model".into(),
            options: ChatRequestOptions::default(),
            context_tokens: 1000,
            input_byte_limit: None,
            output_tokens: 100,
            request_seconds: 30,
            price,
            billing: None,
        }
    }
    fn plan(frozen: &FrozenHarnessProvider) -> HarnessPlan {
        let mut plan = HarnessPlan {
            vm: None,
            preparation_seconds: None,
            guidance_source: None,
            activation_executable: None,
            source_files: Default::default(),
            schema: HARNESS_SCHEMA.into(),
            id: "budget-test".into(),
            baseline: SourceRepositorySnapshotV1 {
                schema: "test".into(),
                branch: "main".into(),
                head: "a".repeat(40),
                tracked_tree_digest: "a".repeat(64),
                submodule_digest: "b".repeat(64),
                repository_fingerprint: "c".repeat(64),
            },
            baseline_binary_digest: "a".repeat(64),
            route_digest: frozen.route_digest().unwrap(),
            price_digest: frozen.price_digest().unwrap(),
            evidence_ids: vec!["task:event".into()],
            cases: (0..3)
                .map(|i| HarnessCase {
                    id: format!("case-{i}"),
                    input_digest: "a".repeat(64),
                    verifier_digest: "b".repeat(64),
                    grants_digest: "c".repeat(64),
                    held_out: i > 0,
                    predicted_fix: i == 0,
                })
                .collect(),
            repetitions: 3,
            settings: HarnessSettings::default(),
            digest: String::new(),
        };
        plan.seal().unwrap();
        plan
    }
    fn broker(
        report_usage: bool,
        budget: Option<u64>,
        pause_forever: bool,
    ) -> (
        tempfile::TempDir,
        BudgetedBackend<FakeProvider>,
        Arc<AtomicUsize>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(directory.path().join("vault")).unwrap();
        let frozen = frozen();
        let mut plan = plan(&frozen);
        if let Some(budget) = budget {
            plan.settings.maximum_microusd = budget;
            plan.seal().unwrap();
        }
        start_harness_cycle(&vault, &plan, 0).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let backend = FakeProvider {
            vault: vault.clone(),
            calls: calls.clone(),
            report_usage,
            pause_forever,
        };
        let broker = BudgetedBackend::bind(backend, vault, plan, frozen).unwrap();
        (directory, broker, calls)
    }

    #[test]
    fn hrs253_input_limit_counts_utf8_escaping_and_tool_schemas() {
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text("é\n\""),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        }];
        let tools = vec![ToolDef {
            def_type: "function".into(),
            function: crate::agent::client::ToolFunction {
                name: "test".into(),
                description: "schema".repeat(200),
                parameters: serde_json::json!({"type": "object"}),
            },
        }];
        let options = ChatRequestOptions::default();
        let exact = serde_json::to_vec(&(&messages, &tools, &options))
            .unwrap()
            .len();
        assert!(validate_input_bytes(&messages, &tools, &options, exact).is_ok());
        assert!(validate_input_bytes(&messages, &tools, &options, exact - 1).is_err());
        let image = crate::image_content::RawClipboardImage::new(1, 1, vec![0, 0, 0, 255])
            .unwrap()
            .normalize()
            .unwrap();
        let mut image_messages = messages;
        image_messages[0].content =
            crate::image_content::ModelContent::multimodal("image", &[image]);
        assert!(
            validate_input_bytes(&image_messages, &[], &options, usize::MAX)
                .unwrap_err()
                .to_string()
                .contains("text only")
        );
    }

    #[tokio::test]
    async fn hrs253_oversized_input_is_rejected_without_spend_or_provider_call() {
        let (_directory, broker, calls) = broker(true, None, false);
        let message = ChatMessage {
            role: "user".into(),
            content: crate::image_content::ModelContent::text("é".repeat(600)),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        };
        assert!(broker.stream_chat(&[message], &[]).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let (_, state) = load_harness_cycle(&broker.vault, "budget-test").unwrap();
        assert!(state.reservations.is_empty());
        assert!(state.active_reservations.is_empty());
        assert!(state.charged_requests.is_empty());
    }

    #[tokio::test]
    async fn hrs253_budget_reserves_before_provider_and_settles_exact_usage() {
        let (_directory, broker, calls) = broker(true, None, false);
        let mut stream = broker.stream_chat(&[], &[]).await.unwrap();
        assert!(stream.recv().await.is_some());
        let (_, state) = load_harness_cycle(&broker.vault, "budget-test").unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(state.settled_microusd, 40);
        assert!(state.reservations.is_empty());
        assert!(state.active_reservations.is_empty());
        assert_eq!(state.charged_requests.len(), 1);
    }

    #[tokio::test]
    async fn hrs253_unknown_usage_retains_full_reservation_across_reload() {
        let (_directory, broker, calls) = broker(false, None, false);
        assert!(broker.stream_chat(&[], &[]).await.is_err());
        let (_, state) = load_harness_cycle(&broker.vault, "budget-test").unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            state.reservations.values().sum::<u64>(),
            broker.maximum_microusd
        );
        assert_eq!(state.settled_microusd, 0);
        assert!(state.active_reservations.is_empty());
        assert!(state.active_seconds > 0);
    }

    #[tokio::test]
    async fn hrs253_over_budget_never_calls_provider_or_adds_phantom_reservations() {
        let (_directory, broker, calls) = broker(true, Some(1), false);
        assert!(broker.stream_chat(&[], &[]).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let (_, state) = load_harness_cycle(&broker.vault, "budget-test").unwrap();
        assert!(state.reservations.is_empty());
        assert!(state.active_reservations.is_empty());
        assert_eq!(
            state.stage,
            codex_team::self_improvement::HarnessStage::BudgetExhausted
        );
    }

    #[tokio::test]
    async fn hrs253_cancelled_provider_retains_both_durable_reservations() {
        let (_directory, broker, calls) = broker(true, None, true);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), broker.stream_chat(&[], &[]))
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let (_, state) = load_harness_cycle(&broker.vault, "budget-test").unwrap();
        assert_eq!(state.reservations.len(), 1);
        assert_eq!(state.active_reservations.len(), 1);
    }

    #[tokio::test]
    async fn hrs253_physical_posts_are_capped_and_each_retry_is_reserved() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let directory = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(directory.path().join("vault")).unwrap();
        let mut frozen = frozen();
        frozen.source =
            ProviderStateSourceV1::from_route("fake", ProviderProtocol::ChatCompletions, &url)
                .unwrap();
        let plan = plan(&frozen);
        start_harness_cycle(&vault, &plan, 0).unwrap();
        let provider = crate::config::ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: "synthetic-test-token".into(),
            protocol: ProviderProtocol::ChatCompletions,
            prompt_cache_mode: Default::default(),
            models: vec!["fake-model".into()],
            timeout: Default::default(),
        };
        let client = AgentClient::with_call_policy(
            "fake",
            &provider,
            "fake-model",
            crate::agent::client::ProviderCallPolicy::default(),
            crate::request::RequestStore::default(),
        );
        let broker = BudgetedBackend::new(client, vault.clone(), plan, frozen).unwrap();
        let server = tokio::spawn(async move {
            for attempt in 1..=2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut chunk = [0; 4096];
                let body = loop {
                    let count = socket.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                    assert!(request.len() < 65536);
                    if let Some(boundary) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&request[..boundary]).unwrap();
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse().unwrap())
                            })
                            .unwrap();
                        if request.len() >= boundary + 4 + length {
                            break serde_json::from_slice::<serde_json::Value>(
                                &request[boundary + 4..boundary + 4 + length],
                            )
                            .unwrap();
                        }
                    }
                };
                assert_eq!(body["max_tokens"], 100);
                let (_, state) = load_harness_cycle(&vault, "budget-test").unwrap();
                assert_eq!(state.reservations.len(), attempt);
                socket.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
            }
        });
        for _ in 0..2 {
            assert!(
                tokio::time::timeout(Duration::from_secs(5), broker.stream_chat(&[], &[]))
                    .await
                    .unwrap()
                    .is_err()
            );
        }
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
        let (_, state) = load_harness_cycle(&broker.vault, "budget-test").unwrap();
        assert_eq!(state.reservations.len(), 2);
        assert_eq!(
            state.reservations.values().sum::<u64>(),
            2 * broker.maximum_microusd
        );
    }

    #[tokio::test]
    async fn hrs253_whole_activity_counts_tools_without_double_charging_requests() {
        let (_directory, broker, calls) = broker(true, None, false);
        let activity = crate::harness_activity::HarnessActivity::begin(
            broker.vault.clone(),
            broker.plan.clone(),
            30,
        )
        .await
        .unwrap();
        let broker = broker.in_activity(activity.clone()).unwrap();
        assert!(broker.stream_chat(&[], &[]).await.is_err());
        activity
            .run(async {
                let _ = broker.stream_chat(&[], &[]).await?;
                let (_, state) = load_harness_cycle(&broker.vault, "budget-test")?;
                assert_eq!(state.active_reservations.len(), 1);
                assert_eq!(state.active_seconds, 0);
                assert!(state.completed_work.is_empty());
                tokio::time::sleep(Duration::from_millis(50)).await;
                let _ = broker.stream_chat(&[], &[]).await?;
                Ok(())
            })
            .await
            .unwrap();
        let (_, state) = load_harness_cycle(&broker.vault, "budget-test").unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(state.settled_microusd, 80);
        assert!(state.active_reservations.is_empty());
        assert_eq!(state.completed_work.len(), 1);
        assert_eq!(state.active_seconds, 1);
        assert!(broker.stream_chat(&[], &[]).await.is_err());
        assert!(activity.run(async { Ok(()) }).await.is_err());
    }

    #[tokio::test]
    async fn hrs253_aborted_activity_closes_admission_and_retains_unknown_time() {
        let (_directory, broker, calls) = broker(true, None, false);
        let activity = crate::harness_activity::HarnessActivity::begin(
            broker.vault.clone(),
            broker.plan.clone(),
            30,
        )
        .await
        .unwrap();
        let broker = broker.in_activity(activity.clone()).unwrap();
        let operation = activity.run(async {
            let _ = broker.stream_chat(&[], &[]).await?;
            std::future::pending::<()>().await;
            Ok(())
        });
        assert!(tokio::time::timeout(Duration::from_millis(100), operation)
            .await
            .is_err());
        assert!(activity.remaining().is_err());
        assert!(broker.stream_chat(&[], &[]).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let (_, state) = load_harness_cycle(&broker.vault, "budget-test").unwrap();
        assert_eq!(state.active_reservations.values().sum::<u64>(), 30);
        assert_eq!(state.active_seconds, 0);
    }

    #[tokio::test]
    async fn hrs253_activity_deadline_consumes_reserved_time() {
        let (_directory, broker, _) = broker(true, None, false);
        let activity = crate::harness_activity::HarnessActivity::begin(
            broker.vault.clone(),
            broker.plan.clone(),
            1,
        )
        .await
        .unwrap();
        assert!(activity
            .run(async {
                std::future::pending::<()>().await;
                Ok(())
            })
            .await
            .is_err());
        let (_, state) = load_harness_cycle(&broker.vault, "budget-test").unwrap();
        assert_eq!(state.active_seconds, 1);
        assert!(state.active_reservations.is_empty());
        assert!(activity.remaining().is_err());
    }

    #[test]
    fn hrs253_smaller_payload_does_not_underreserve_provider_context() {
        let legacy = frozen();
        let serialized = serde_json::to_value(&legacy).unwrap();
        assert!(serialized.get("input_byte_limit").is_none());
        let restored: FrozenHarnessProvider = serde_json::from_value(serialized).unwrap();
        assert_eq!(
            legacy.route_digest().unwrap(),
            restored.route_digest().unwrap()
        );
        let mut smaller = legacy.clone();
        smaller.input_byte_limit = Some(100);
        assert_eq!(smaller.input_byte_cap(), 100);
        assert_eq!(
            smaller.maximum_microusd().unwrap(),
            legacy.maximum_microusd().unwrap()
        );
        assert_ne!(
            smaller.route_digest().unwrap(),
            legacy.route_digest().unwrap()
        );
        assert_eq!(
            cost_microusd(ActualCost::UpperBoundIncrementalUsd(
                ExactRational::new(1, 10_000_000).unwrap()
            ))
            .unwrap(),
            1
        );
    }

    #[test]
    fn hrs253_price_bound_covers_cache_reasoning_and_long_context() {
        let frozen = frozen();
        // 1000 input at $1.50/M + 100 reasoning at $3/M, both doubled.
        assert_eq!(frozen.maximum_microusd().unwrap(), 3600);
        assert_eq!(
            cost_microusd(ActualCost::IncrementalUsd(
                ExactRational::new(1, 10_000_000).unwrap()
            ))
            .unwrap(),
            1
        );
        let mut changed = frozen.clone();
        changed.context_tokens += 1;
        assert_ne!(
            changed.route_digest().unwrap(),
            frozen.route_digest().unwrap()
        );
        changed = frozen.clone();
        changed.price.verified_at -= chrono::Duration::days(1);
        assert_ne!(
            changed.price_digest().unwrap(),
            frozen.price_digest().unwrap()
        );
    }
}
