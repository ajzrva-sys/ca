use crate::agent::stream::UsageInfo;
use crate::runtime::{ActiveProfile, RuntimePaths};
use crate::usage::{CacheWriteBilling, DecimalUsd};
use chrono::{DateTime, NaiveTime, Utc};
use clap::{CommandFactory as _, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::str::FromStr as _;
use std::time::Duration;
use uuid::Uuid;

pub const OPENAI_SOL_SOURCE: &str = "https://developers.openai.com/api/docs/models/gpt-5.6-sol";
pub const OPENAI_TERRA_SOURCE: &str = "https://developers.openai.com/api/docs/models/gpt-5.6-terra";
pub const OPENAI_LUNA_SOURCE: &str = "https://developers.openai.com/api/docs/models/gpt-5.6-luna";
pub const MIMO_PAYG_SOURCE: &str = "https://mimo.mi.com/docs/en-US/price/pay-as-you-go";
pub const MIMO_TOKEN_PLAN_SOURCE: &str =
    "https://mimo.mi.com/docs/en-US/tokenplan/Token%20Plan/subscription";
const LEGACY_MIMO_TOKEN_PLAN_SOURCE: &str = "https://mimo.mi.com/docs/en-US/tokenplan/subscription";
pub const DEEPSEEK_PRICING_SOURCE: &str = "https://api-docs.deepseek.com/quick_start/pricing/";
pub const OPENAI_API_BASE_URL: &str = "https://api.openai.com/v1";
pub const MIMO_PAYG_BASE_URL: &str = "https://api.xiaomimimo.com/v1";
pub const DEEPSEEK_API_BASE_URL: &str = "https://api.deepseek.com";
pub const DEEPSEEK_API_V1_BASE_URL: &str = "https://api.deepseek.com/v1";
pub const MIMO_OFF_PEAK_COPY: &str =
    "20% off during off-peak hours: 16:00–24:00 UTC (9:00 AM–5:00 PM PDT / 8:00 AM–4:00 PM PST).";
const MODELS_DEV_SOURCE: &str = "https://models.dev/api.json";
const CATALOG_SCHEMA: &str = "ca.pricing.catalog.v3";
const LEGACY_CATALOG_SCHEMA_V2: &str = "ca.pricing.catalog.v2";
const LEGACY_CATALOG_SCHEMA_V1: &str = "ca.pricing.catalog.v1";
const STATUS_SCHEMA: &str = "ca.pricing.status.v1";
const MAX_CATALOG_BYTES: u64 = 512 * 1024;
const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const MAX_CARD_AGE_DAYS: i64 = 30;
const SEEDED_VERIFIED_AT: &str = "2026-08-15T00:00:00Z";
const LONG_CONTEXT_THRESHOLD: u64 = 272_000;

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Complexity {
    Simple,
    Standard,
    Complex,
}

impl Complexity {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Standard => "standard",
            Self::Complex => "complex",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MimoRegion {
    Cn,
    Singapore,
    Europe,
}

impl MimoRegion {
    pub const ALL: [Self; 3] = [Self::Cn, Self::Singapore, Self::Europe];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Cn => "CN",
            Self::Singapore => "Singapore",
            Self::Europe => "Europe",
        }
    }

    pub const fn token_plan_url(self) -> &'static str {
        match self {
            Self::Cn => "https://token-plan-cn.xiaomimimo.com/v1",
            Self::Singapore => "https://token-plan-sgp.xiaomimimo.com/v1",
            Self::Europe => "https://token-plan-ams.xiaomimimo.com/v1",
        }
    }

    pub fn from_token_plan_url(value: &str) -> Option<Self> {
        let value = value.trim_end_matches('/');
        Self::ALL
            .into_iter()
            .find(|region| value == region.token_plan_url())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MimoPlanTier {
    Lite,
    Standard,
    Pro,
    Max,
}

impl MimoPlanTier {
    pub const ALL: [Self; 4] = [Self::Lite, Self::Standard, Self::Pro, Self::Max];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Lite => "Lite",
            Self::Standard => "Standard",
            Self::Pro => "Pro",
            Self::Max => "Max",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MimoPlanTerm {
    Monthly,
    Annual,
}

impl MimoPlanTerm {
    pub const ALL: [Self; 2] = [Self::Monthly, Self::Annual];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Monthly => "monthly",
            Self::Annual => "annual",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderBillingConfig {
    MimoTokenPlan {
        region: MimoRegion,
        tier: MimoPlanTier,
        term: MimoPlanTerm,
        actual_usd: DecimalUsd,
        official_list_usd: DecimalUsd,
        price_difference_confirmed: bool,
    },
}

impl ProviderBillingConfig {
    pub fn mimo_token_plan(
        region: MimoRegion,
        tier: MimoPlanTier,
        term: MimoPlanTerm,
        actual_usd: DecimalUsd,
        confirmed: bool,
    ) -> Result<Self, String> {
        let (official_list_usd, _) = mimo_plan_terms(tier, term);
        if actual_usd != official_list_usd && !confirmed {
            return Err(format!(
                "actual USD ${actual_usd} differs from official list ${official_list_usd}; confirmation is required"
            ));
        }
        Ok(Self::MimoTokenPlan {
            region,
            tier,
            term,
            actual_usd,
            official_list_usd,
            price_difference_confirmed: confirmed || actual_usd == official_list_usd,
        })
    }

    pub fn delegation_ready(&self) -> bool {
        match self {
            Self::MimoTokenPlan {
                actual_usd,
                official_list_usd,
                price_difference_confirmed,
                ..
            } => {
                *actual_usd != DecimalUsd::ZERO
                    && *official_list_usd != DecimalUsd::ZERO
                    && *price_difference_confirmed
            }
        }
    }

    pub fn mimo_allocation(&self) -> Option<(DecimalUsd, u128)> {
        match self {
            Self::MimoTokenPlan {
                tier,
                term,
                actual_usd,
                ..
            } => Some((*actual_usd, mimo_plan_terms(*tier, *term).1)),
        }
    }

    pub fn validate_for_provider(
        &self,
        provider: &str,
        provider_base_url: &str,
    ) -> Result<(), String> {
        match self {
            Self::MimoTokenPlan {
                region,
                tier,
                term,
                actual_usd,
                official_list_usd,
                price_difference_confirmed,
            } => {
                let (expected_list, _) = mimo_plan_terms(*tier, *term);
                if provider != "mimo"
                    || provider_base_url.trim_end_matches('/') != region.token_plan_url()
                    || *actual_usd == DecimalUsd::ZERO
                    || *official_list_usd != expected_list
                    || (!*price_difference_confirmed && actual_usd != official_list_usd)
                {
                    return Err("provider billing metadata is invalid or inconsistent".into());
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "billing_mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum CardPricing {
    MeteredUsd {
        input_per_million_usd: DecimalUsd,
        cached_input_per_million_usd: DecimalUsd,
        #[serde(
            rename = "cache_write_billing",
            alias = "cache_write_input_per_million_usd",
            deserialize_with = "deserialize_cache_write_billing"
        )]
        cache_write_billing: CacheWriteBilling<DecimalUsd>,
        output_per_million_usd: DecimalUsd,
        reasoning_output_per_million_usd: DecimalUsd,
        long_context_threshold: u64,
        long_input_multiplier_numerator: u32,
        long_input_multiplier_denominator: u32,
        long_output_multiplier_numerator: u32,
        long_output_multiplier_denominator: u32,
    },
    MimoTokenPlan {
        cache_hit_credits_per_token: ExactRational,
        cache_miss_credits_per_token: ExactRational,
        #[serde(default = "included_exact_cache_write_billing")]
        cache_write_billing: CacheWriteBilling<ExactRational>,
        output_credits_per_token: ExactRational,
        off_peak_start_utc_hour: u8,
        off_peak_end_utc_hour: u8,
        off_peak_multiplier: ExactRational,
    },
}

fn deserialize_cache_write_billing<'de, D>(
    deserializer: D,
) -> Result<CacheWriteBilling<DecimalUsd>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Wire {
        Current(CacheWriteBilling<DecimalUsd>),
        Legacy(DecimalUsd),
    }

    Ok(match Wire::deserialize(deserializer)? {
        Wire::Current(billing) => billing,
        Wire::Legacy(rate) => CacheWriteBilling::Replacement { rate },
    })
}

fn included_exact_cache_write_billing() -> CacheWriteBilling<ExactRational> {
    CacheWriteBilling::Included
}

impl CardPricing {
    pub const fn billing_mode_label(&self) -> &'static str {
        match self {
            Self::MeteredUsd { .. } => "metered_usd",
            Self::MimoTokenPlan { .. } => "mimo_token_plan",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CostBasis {
    #[default]
    Exact,
    /// Published peak tariff, without assuming eligibility for a time discount.
    PeakUpperBound,
}

impl CostBasis {
    fn is_exact(&self) -> bool {
        *self == Self::Exact
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OfficialPriceCard {
    pub provider: String,
    pub model: String,
    pub capability: Complexity,
    pub source_url: String,
    pub verified_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "CostBasis::is_exact")]
    pub cost_basis: CostBasis,
    pub pricing: CardPricing,
}

impl OfficialPriceCard {
    pub fn route_label(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }

    pub fn fresh_at(&self, now: DateTime<Utc>) -> bool {
        self.verified_at <= now + chrono::Duration::days(1)
            && now.signed_duration_since(self.verified_at)
                <= chrono::Duration::days(MAX_CARD_AGE_DAYS)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OfficialPriceCatalog {
    schema: String,
    pub cards: Vec<OfficialPriceCard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advisory: Option<String>,
}

impl OfficialPriceCatalog {
    pub fn seeded() -> Self {
        seeded_catalog_at(
            DateTime::parse_from_rfc3339(SEEDED_VERIFIED_AT)
                .unwrap()
                .to_utc(),
        )
    }

    pub fn find(&self, provider: &str, model: &str) -> Option<&OfficialPriceCard> {
        self.cards
            .iter()
            .find(|card| card.provider == provider && card.model == model)
    }

    pub fn find_for_billing_mode(
        &self,
        provider: &str,
        model: &str,
        billing_mode: &str,
    ) -> Option<&OfficialPriceCard> {
        self.cards.iter().find(|card| {
            card.provider == provider
                && card.model == model
                && card.pricing.billing_mode_label() == billing_mode
        })
    }

    pub fn validate(&self) -> Result<(), PricingError> {
        if self.schema != CATALOG_SCHEMA {
            return Err(PricingError::InvalidCatalog);
        }
        let mut labels = self
            .cards
            .iter()
            .map(|card| {
                format!(
                    "{}@{}",
                    card.route_label(),
                    card.pricing.billing_mode_label()
                )
            })
            .collect::<Vec<_>>();
        labels.sort();
        let legacy_labels = [
            "mimo/mimo-v2.5-pro@mimo_token_plan",
            "mimo/mimo-v2.5@mimo_token_plan",
            "openai/gpt-5.6-luna@metered_usd",
            "openai/gpt-5.6-sol@metered_usd",
        ];
        let current_labels = [
            "deepseek/deepseek-v4-flash@metered_usd",
            "deepseek/deepseek-v4-pro@metered_usd",
            "mimo/mimo-v2.5-pro@mimo_token_plan",
            "mimo/mimo-v2.5@mimo_token_plan",
            "openai/gpt-5.6-luna@metered_usd",
            "openai/gpt-5.6-sol@metered_usd",
        ];
        let first_party_labels = [
            "deepseek/deepseek-v4-flash@metered_usd",
            "deepseek/deepseek-v4-pro@metered_usd",
            "mimo/mimo-v2.5-pro@metered_usd",
            "mimo/mimo-v2.5-pro@mimo_token_plan",
            "mimo/mimo-v2.5@metered_usd",
            "mimo/mimo-v2.5@mimo_token_plan",
            "openai/gpt-5.6-luna@metered_usd",
            "openai/gpt-5.6-sol@metered_usd",
            "openai/gpt-5.6-terra@metered_usd",
        ];
        if labels != legacy_labels && labels != current_labels && labels != first_party_labels {
            return Err(PricingError::InvalidCatalog);
        }
        for card in &self.cards {
            let valid_source = match card.provider.as_str() {
                "openai" => {
                    card.source_url == OPENAI_SOL_SOURCE
                        || card.source_url == OPENAI_TERRA_SOURCE
                        || card.source_url == OPENAI_LUNA_SOURCE
                }
                "mimo" => match card.pricing {
                    CardPricing::MeteredUsd { .. } => card.source_url == MIMO_PAYG_SOURCE,
                    CardPricing::MimoTokenPlan { .. } => {
                        card.source_url == MIMO_TOKEN_PLAN_SOURCE
                            || card.source_url == LEGACY_MIMO_TOKEN_PLAN_SOURCE
                    }
                },
                "deepseek" => card.source_url == DEEPSEEK_PRICING_SOURCE,
                _ => false,
            };
            let valid_cache_write_billing = matches!(
                (&card.pricing, card.provider.as_str()),
                (
                    CardPricing::MeteredUsd {
                        cache_write_billing: CacheWriteBilling::Replacement { .. },
                        ..
                    },
                    "openai",
                ) | (
                    CardPricing::MeteredUsd {
                        cache_write_billing: CacheWriteBilling::Included,
                        ..
                    },
                    "mimo" | "deepseek",
                ) | (
                    CardPricing::MimoTokenPlan {
                        cache_write_billing: CacheWriteBilling::Included,
                        ..
                    },
                    "mimo",
                )
            );
            if !valid_source
                || !valid_cache_write_billing
                || card.verified_at.timestamp() <= 0
                || (card.cost_basis == CostBasis::PeakUpperBound
                    && (card.provider != "deepseek"
                        || !matches!(card.pricing, CardPricing::MeteredUsd { .. })))
            {
                return Err(PricingError::InvalidCatalog);
            }
        }
        Ok(())
    }
}

fn migrate_catalog_in_memory(catalog: &mut OfficialPriceCatalog) {
    if !matches!(
        catalog.schema.as_str(),
        LEGACY_CATALOG_SCHEMA_V1 | LEGACY_CATALOG_SCHEMA_V2
    ) {
        return;
    }

    for card in &mut catalog.cards {
        match (&mut card.pricing, card.provider.as_str()) {
            (
                CardPricing::MeteredUsd {
                    cache_write_billing,
                    ..
                },
                "mimo" | "deepseek",
            ) => *cache_write_billing = CacheWriteBilling::Included,
            (
                CardPricing::MimoTokenPlan {
                    cache_write_billing,
                    ..
                },
                "mimo",
            ) => *cache_write_billing = CacheWriteBilling::Included,
            _ => {}
        }
    }
    catalog.schema = CATALOG_SCHEMA.into();
}

#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExactRational {
    numerator: u128,
    denominator: u128,
}

impl<'de> Deserialize<'de> for ExactRational {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            #[serde(deserialize_with = "deserialize_json_u128")]
            numerator: u128,
            #[serde(deserialize_with = "deserialize_json_u128")]
            denominator: u128,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.numerator, wire.denominator)
            .ok_or_else(|| serde::de::Error::custom("rational denominator must be nonzero"))
    }
}

fn deserialize_json_u128<'de, D>(deserializer: D) -> Result<u128, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;

    impl serde::de::Visitor<'_> for Visitor {
        type Value = u128;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a nonnegative integer")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
            Ok(u128::from(value))
        }

        fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E> {
            Ok(value)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            u128::try_from(value).map_err(|_| E::custom("integer must be nonnegative"))
        }
    }

    deserializer.deserialize_any(Visitor)
}

impl ExactRational {
    pub fn new(numerator: u128, denominator: u128) -> Option<Self> {
        if denominator == 0 {
            return None;
        }
        let divisor = gcd(numerator, denominator);
        Some(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    pub const fn numerator(self) -> u128 {
        self.numerator
    }

    pub const fn denominator(self) -> u128 {
        self.denominator
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        let numerator = self
            .numerator
            .checked_mul(other.denominator)?
            .checked_add(other.numerator.checked_mul(self.denominator)?)?;
        let denominator = self.denominator.checked_mul(other.denominator)?;
        Self::new(numerator, denominator)
    }

    pub fn checked_mul(self, other: Self) -> Option<Self> {
        Self::new(
            self.numerator.checked_mul(other.numerator)?,
            self.denominator.checked_mul(other.denominator)?,
        )
    }

    pub fn checked_mul_integer(self, value: u64) -> Option<Self> {
        Self::new(
            self.numerator.checked_mul(u128::from(value))?,
            self.denominator,
        )
    }

    pub fn from_decimal(value: DecimalUsd) -> Option<Self> {
        Self::new(
            value.coefficient(),
            10_u128.checked_pow(u32::from(value.scale()))?,
        )
    }

    pub fn rounded_decimal(self, places: u8) -> Option<DecimalUsd> {
        let scale = 10_u128.checked_pow(u32::from(places))?;
        let scaled = self.numerator.checked_mul(scale)?;
        let rounded = scaled
            .checked_add(self.denominator / 2)?
            .checked_div(self.denominator)?;
        DecimalUsd::from_parts(rounded, places)
    }

    pub fn ceiling_decimal(self, places: u8) -> Option<DecimalUsd> {
        let scaled = self
            .numerator
            .checked_mul(10_u128.checked_pow(u32::from(places))?)?;
        let coefficient = scaled
            .checked_div(self.denominator)?
            .checked_add(u128::from(!scaled.is_multiple_of(self.denominator)))?;
        DecimalUsd::from_parts(coefficient, places)
    }
}

impl std::fmt::Debug for ExactRational {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self}")
    }
}

impl std::fmt::Display for ExactRational {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.denominator == 1 {
            write!(formatter, "{}", self.numerator)
        } else if let Some(decimal) = terminating_decimal(*self) {
            write!(formatter, "{decimal}")
        } else {
            write!(formatter, "{}/{}", self.numerator, self.denominator)
        }
    }
}

impl Ord for ExactRational {
    fn cmp(&self, other: &Self) -> Ordering {
        self.numerator
            .checked_mul(other.denominator)
            .zip(other.numerator.checked_mul(self.denominator))
            .map_or_else(
                || self.to_string().cmp(&other.to_string()),
                |(left, right)| left.cmp(&right),
            )
    }
}

impl PartialOrd for ExactRational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forecast {
    IncrementalUsd(ExactRational),
    SubscriptionAllocation {
        credits: ExactRational,
        allocated_usd: ExactRational,
    },
}

impl Forecast {
    pub fn score(&self) -> ExactRational {
        match self {
            Self::IncrementalUsd(value) => *value,
            Self::SubscriptionAllocation { allocated_usd, .. } => *allocated_usd,
        }
    }

    pub fn render(self) -> String {
        match self {
            Self::IncrementalUsd(value) => format!(
                "~${} incremental",
                value.rounded_decimal(6).unwrap_or(DecimalUsd::ZERO)
            ),
            Self::SubscriptionAllocation {
                credits,
                allocated_usd,
            } => format!(
                "{credits} credits · ~${} subscription allocation · peak forecast; 20% off-peak not included",
                allocated_usd.rounded_decimal(6).unwrap_or(DecimalUsd::ZERO)
            ),
        }
    }

    pub fn render_scaled(self, calls: usize) -> String {
        let calls = u64::try_from(calls).unwrap_or(u64::MAX);
        match self {
            Self::IncrementalUsd(value) => value.checked_mul_integer(calls).map_or_else(
                || "cost estimate overflowed".into(),
                |total| {
                    format!(
                        "~${} incremental for {calls} bounded calls",
                        total.rounded_decimal(6).unwrap_or(DecimalUsd::ZERO)
                    )
                },
            ),
            Self::SubscriptionAllocation {
                credits,
                allocated_usd,
            } => credits
                .checked_mul_integer(calls)
                .zip(allocated_usd.checked_mul_integer(calls))
                .map_or_else(
                    || "cost estimate overflowed".into(),
                    |(credits, allocated_usd)| {
                        format!(
                            "{credits} credits · ~${} subscription allocation for {calls} bounded calls · peak forecast; 20% off-peak not included",
                            allocated_usd
                                .rounded_decimal(6)
                                .unwrap_or(DecimalUsd::ZERO)
                        )
                    },
                ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActualCost {
    IncrementalUsd(ExactRational),
    UpperBoundIncrementalUsd(ExactRational),
    SubscriptionAllocation {
        credits: ExactRational,
        allocated_usd: ExactRational,
        off_peak: bool,
    },
}

impl ActualCost {
    /// A budget debit, not evidence of an invoiced charge. Subscription credit
    /// allocation is intentionally not treated as incremental USD.
    pub fn upper_bound_usd(self) -> Option<ExactRational> {
        match self {
            Self::IncrementalUsd(value) | Self::UpperBoundIncrementalUsd(value) => Some(value),
            Self::SubscriptionAllocation { .. } => None,
        }
    }

    pub fn render(self) -> String {
        match self {
            Self::IncrementalUsd(value) => format!(
                "${} incremental",
                value.rounded_decimal(6).unwrap_or(DecimalUsd::ZERO)
            ),
            Self::UpperBoundIncrementalUsd(value) => format!(
                "≤${} incremental · peak-rate upper bound",
                value
                    .ceiling_decimal(6)
                    .map_or_else(|| value.to_string(), |value| value.to_string())
            ),
            Self::SubscriptionAllocation {
                credits,
                allocated_usd,
                off_peak,
            } => format!(
                "{credits} credits{} · ~${} subscription allocation",
                if off_peak { " (20% off-peak)" } else { "" },
                allocated_usd.rounded_decimal(6).unwrap_or(DecimalUsd::ZERO)
            ),
        }
    }
}

pub fn forecast_for(
    card: &OfficialPriceCard,
    billing: Option<&ProviderBillingConfig>,
    input_tokens: u64,
    output_tokens: u64,
) -> Option<Forecast> {
    match &card.pricing {
        CardPricing::MeteredUsd {
            input_per_million_usd,
            cached_input_per_million_usd,
            cache_write_billing,
            output_per_million_usd,
            long_context_threshold,
            long_input_multiplier_numerator,
            long_input_multiplier_denominator,
            long_output_multiplier_numerator,
            long_output_multiplier_denominator,
            ..
        } => {
            let long = input_tokens > *long_context_threshold;
            let input_rate = worst_cache_input_rate(
                decimal_per_token(*input_per_million_usd)?,
                decimal_per_token(*cached_input_per_million_usd)?,
                decimal_cache_write_per_token(*cache_write_billing)?,
            )?;
            let input = input_rate.checked_mul_integer(input_tokens)?;
            let output = decimal_token_component(*output_per_million_usd, output_tokens)?;
            let input = if long {
                input.checked_mul(ExactRational::new(
                    u128::from(*long_input_multiplier_numerator),
                    u128::from(*long_input_multiplier_denominator),
                )?)?
            } else {
                input
            };
            let output = if long {
                output.checked_mul(ExactRational::new(
                    u128::from(*long_output_multiplier_numerator),
                    u128::from(*long_output_multiplier_denominator),
                )?)?
            } else {
                output
            };
            Some(Forecast::IncrementalUsd(input.checked_add(output)?))
        }
        CardPricing::MimoTokenPlan {
            cache_hit_credits_per_token,
            cache_miss_credits_per_token,
            cache_write_billing,
            output_credits_per_token,
            ..
        } => {
            let (actual_usd, pool) = billing?.mimo_allocation()?;
            if !billing?.delegation_ready() {
                return None;
            }
            let input_rate = worst_cache_input_rate(
                *cache_miss_credits_per_token,
                *cache_hit_credits_per_token,
                *cache_write_billing,
            )?;
            let credits = input_rate
                .checked_mul_integer(input_tokens)?
                .checked_add(output_credits_per_token.checked_mul_integer(output_tokens)?)?;
            let allocated_usd = credits
                .checked_mul(ExactRational::from_decimal(actual_usd)?)?
                .checked_mul(ExactRational::new(1, pool)?)?;
            Some(Forecast::SubscriptionAllocation {
                credits,
                allocated_usd,
            })
        }
    }
}

pub fn actual_cost_for(
    card: &OfficialPriceCard,
    billing: Option<&ProviderBillingConfig>,
    usage: &UsageInfo,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
) -> Option<ActualCost> {
    if !crate::usage::usage_details_valid(usage) || finished_at < started_at {
        return None;
    }
    match &card.pricing {
        CardPricing::MeteredUsd {
            input_per_million_usd,
            cached_input_per_million_usd,
            cache_write_billing,
            output_per_million_usd,
            reasoning_output_per_million_usd,
            long_context_threshold,
            long_input_multiplier_numerator,
            long_input_multiplier_denominator,
            long_output_multiplier_numerator,
            long_output_multiplier_denominator,
        } => {
            let reasoning = match usage.reasoning_tokens {
                Some(reasoning) => u64::from(reasoning),
                None if reasoning_output_per_million_usd == output_per_million_usd => 0,
                None => return None,
            };
            let prompt = u64::from(usage.prompt_tokens);
            let completion = u64::from(usage.completion_tokens);
            let regular_output = completion.checked_sub(reasoning)?;
            let input_multiplier = if prompt > *long_context_threshold {
                ExactRational::new(
                    u128::from(*long_input_multiplier_numerator),
                    u128::from(*long_input_multiplier_denominator),
                )?
            } else {
                ExactRational::new(1, 1)?
            };
            let output_multiplier = if prompt > *long_context_threshold {
                ExactRational::new(
                    u128::from(*long_output_multiplier_numerator),
                    u128::from(*long_output_multiplier_denominator),
                )?
            } else {
                ExactRational::new(1, 1)?
            };
            let input = exact_cache_input_cost(
                decimal_per_token(*input_per_million_usd)?,
                decimal_per_token(*cached_input_per_million_usd)?,
                decimal_cache_write_per_token(*cache_write_billing)?,
                usage,
            )?
            .checked_mul(input_multiplier)?;
            let output = decimal_token_component(*output_per_million_usd, regular_output)?
                .checked_add(decimal_token_component(
                    *reasoning_output_per_million_usd,
                    reasoning,
                )?)?
                .checked_mul(output_multiplier)?;
            let amount = input.checked_add(output)?;
            Some(match card.cost_basis {
                CostBasis::Exact => ActualCost::IncrementalUsd(amount),
                CostBasis::PeakUpperBound => ActualCost::UpperBoundIncrementalUsd(amount),
            })
        }
        CardPricing::MimoTokenPlan {
            cache_hit_credits_per_token,
            cache_miss_credits_per_token,
            cache_write_billing,
            output_credits_per_token,
            off_peak_multiplier,
            ..
        } => {
            let mut credits = exact_cache_input_cost(
                *cache_miss_credits_per_token,
                *cache_hit_credits_per_token,
                *cache_write_billing,
                usage,
            )?
            .checked_add(
                output_credits_per_token.checked_mul_integer(u64::from(usage.completion_tokens))?,
            )?;
            let off_peak = request_window(started_at, finished_at)?;
            if off_peak {
                credits = credits.checked_mul(*off_peak_multiplier)?;
            }
            let (actual_usd, pool) = billing?.mimo_allocation()?;
            if !billing?.delegation_ready() {
                return None;
            }
            let allocated_usd = credits
                .checked_mul(ExactRational::from_decimal(actual_usd)?)?
                .checked_mul(ExactRational::new(1, pool)?)?;
            Some(ActualCost::SubscriptionAllocation {
                credits,
                allocated_usd,
                off_peak,
            })
        }
    }
}

fn decimal_per_token(rate: DecimalUsd) -> Option<ExactRational> {
    ExactRational::from_decimal(rate)?.checked_mul(ExactRational::new(1, 1_000_000)?)
}

fn decimal_cache_write_per_token(
    billing: CacheWriteBilling<DecimalUsd>,
) -> Option<CacheWriteBilling<ExactRational>> {
    Some(match billing {
        CacheWriteBilling::Replacement { rate } => CacheWriteBilling::Replacement {
            rate: decimal_per_token(rate)?,
        },
        CacheWriteBilling::Surcharge { rate } => CacheWriteBilling::Surcharge {
            rate: decimal_per_token(rate)?,
        },
        CacheWriteBilling::Included => CacheWriteBilling::Included,
    })
}

fn worst_cache_input_rate(
    miss_rate: ExactRational,
    read_rate: ExactRational,
    write_billing: CacheWriteBilling<ExactRational>,
) -> Option<ExactRational> {
    Some(match write_billing {
        CacheWriteBilling::Included => miss_rate.max(read_rate),
        CacheWriteBilling::Replacement { rate } => miss_rate.max(read_rate).max(rate),
        CacheWriteBilling::Surcharge { rate } => read_rate.max(miss_rate.checked_add(rate)?),
    })
}

fn exact_cache_input_cost(
    miss_rate: ExactRational,
    read_rate: ExactRational,
    write_billing: CacheWriteBilling<ExactRational>,
    usage: &UsageInfo,
) -> Option<ExactRational> {
    let cache = usage.cache_usage();
    let prompt = u64::from(usage.prompt_tokens);
    let zero = ExactRational::new(0, 1)?;
    let base = if read_rate == miss_rate {
        miss_rate.checked_mul_integer(prompt)?
    } else {
        read_rate
            .checked_mul_integer(u64::from(cache.read.tokens()?))?
            .checked_add(miss_rate.checked_mul_integer(u64::from(cache.miss.tokens()?))?)?
    };

    match write_billing {
        CacheWriteBilling::Included => Some(base),
        CacheWriteBilling::Surcharge { rate } if rate == zero => Some(base),
        CacheWriteBilling::Surcharge { rate } => {
            base.checked_add(rate.checked_mul_integer(u64::from(cache.write.tokens()?))?)
        }
        CacheWriteBilling::Replacement { rate } if rate == miss_rate => Some(base),
        CacheWriteBilling::Replacement { rate } => {
            let write = u64::from(cache.write.tokens()?);
            let miss = if read_rate == miss_rate {
                prompt
            } else {
                u64::from(cache.miss.tokens()?)
            };
            let non_write = miss.checked_sub(write)?;
            let read = if read_rate == miss_rate {
                zero
            } else {
                read_rate.checked_mul_integer(u64::from(cache.read.tokens()?))?
            };
            read.checked_add(miss_rate.checked_mul_integer(non_write)?)?
                .checked_add(rate.checked_mul_integer(write)?)
        }
    }
}

fn request_window(started_at: DateTime<Utc>, finished_at: DateTime<Utc>) -> Option<bool> {
    if started_at.date_naive() != finished_at.date_naive() {
        return None;
    }
    let boundary = NaiveTime::from_hms_opt(16, 0, 0)?;
    if started_at.time() >= boundary && finished_at.time() >= boundary {
        Some(true)
    } else if started_at.time() < boundary && finished_at.time() < boundary {
        Some(false)
    } else {
        None
    }
}

fn decimal_token_component(rate: DecimalUsd, tokens: u64) -> Option<ExactRational> {
    ExactRational::from_decimal(rate)?
        .checked_mul_integer(tokens)?
        .checked_mul(ExactRational::new(1, 1_000_000)?)
}

fn mimo_plan_terms(tier: MimoPlanTier, term: MimoPlanTerm) -> (DecimalUsd, u128) {
    let (price, credits) = match (tier, term) {
        (MimoPlanTier::Lite, MimoPlanTerm::Monthly) => ("6", 4_100_000_000),
        (MimoPlanTier::Standard, MimoPlanTerm::Monthly) => ("16", 11_000_000_000),
        (MimoPlanTier::Pro, MimoPlanTerm::Monthly) => ("50", 38_000_000_000),
        (MimoPlanTier::Max, MimoPlanTerm::Monthly) => ("100", 82_000_000_000),
        (MimoPlanTier::Lite, MimoPlanTerm::Annual) => ("63.36", 49_200_000_000),
        (MimoPlanTier::Standard, MimoPlanTerm::Annual) => ("168.96", 132_000_000_000),
        (MimoPlanTier::Pro, MimoPlanTerm::Annual) => ("528", 456_000_000_000),
        (MimoPlanTier::Max, MimoPlanTerm::Annual) => ("1056", 984_000_000_000),
    };
    (
        DecimalUsd::from_str(price).expect("seeded MiMo list price is exact"),
        credits,
    )
}

pub fn mimo_plan_list_price(tier: MimoPlanTier, term: MimoPlanTerm) -> DecimalUsd {
    mimo_plan_terms(tier, term).0
}

pub fn valid_token_plan_key(bytes: &[u8]) -> bool {
    bytes.len() > 3
        && bytes.len() <= 64 * 1024
        && bytes.starts_with(b"tp-")
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() && !byte.is_ascii_whitespace())
}

fn seeded_catalog_at(verified_at: DateTime<Utc>) -> OfficialPriceCatalog {
    let openai =
        |input: &str, cached: &str, cache_write: &str, output: &str| CardPricing::MeteredUsd {
            input_per_million_usd: input.parse().unwrap(),
            cached_input_per_million_usd: cached.parse().unwrap(),
            cache_write_billing: CacheWriteBilling::Replacement {
                rate: cache_write.parse().unwrap(),
            },
            output_per_million_usd: output.parse().unwrap(),
            reasoning_output_per_million_usd: output.parse().unwrap(),
            long_context_threshold: LONG_CONTEXT_THRESHOLD,
            long_input_multiplier_numerator: 2,
            long_input_multiplier_denominator: 1,
            long_output_multiplier_numerator: 3,
            long_output_multiplier_denominator: 2,
        };
    let payg = |cached: &str, input: &str, output: &str| CardPricing::MeteredUsd {
        input_per_million_usd: input.parse().unwrap(),
        cached_input_per_million_usd: cached.parse().unwrap(),
        cache_write_billing: CacheWriteBilling::Included,
        output_per_million_usd: output.parse().unwrap(),
        reasoning_output_per_million_usd: output.parse().unwrap(),
        long_context_threshold: u64::MAX,
        long_input_multiplier_numerator: 1,
        long_input_multiplier_denominator: 1,
        long_output_multiplier_numerator: 1,
        long_output_multiplier_denominator: 1,
    };
    let mimo = |hit: (u128, u128), miss: u128, output: u128| CardPricing::MimoTokenPlan {
        cache_hit_credits_per_token: ExactRational::new(hit.0, hit.1).unwrap(),
        cache_miss_credits_per_token: ExactRational::new(miss, 1).unwrap(),
        cache_write_billing: CacheWriteBilling::Included,
        output_credits_per_token: ExactRational::new(output, 1).unwrap(),
        off_peak_start_utc_hour: 16,
        off_peak_end_utc_hour: 24,
        off_peak_multiplier: ExactRational::new(4, 5).unwrap(),
    };
    OfficialPriceCatalog {
        schema: CATALOG_SCHEMA.into(),
        cards: vec![
            OfficialPriceCard {
                provider: "openai".into(),
                model: "gpt-5.6-sol".into(),
                capability: Complexity::Complex,
                source_url: OPENAI_SOL_SOURCE.into(),
                verified_at,
                cost_basis: CostBasis::Exact,
                pricing: openai("5", "0.50", "6.25", "30"),
            },
            OfficialPriceCard {
                provider: "openai".into(),
                model: "gpt-5.6-terra".into(),
                capability: Complexity::Standard,
                source_url: OPENAI_TERRA_SOURCE.into(),
                verified_at,
                cost_basis: CostBasis::Exact,
                pricing: openai("2", "0.20", "2.50", "12"),
            },
            OfficialPriceCard {
                provider: "openai".into(),
                model: "gpt-5.6-luna".into(),
                capability: Complexity::Simple,
                source_url: OPENAI_LUNA_SOURCE.into(),
                verified_at,
                cost_basis: CostBasis::Exact,
                pricing: openai("0.20", "0.02", "0.25", "1.20"),
            },
            OfficialPriceCard {
                provider: "mimo".into(),
                model: "mimo-v2.5".into(),
                capability: Complexity::Simple,
                source_url: MIMO_TOKEN_PLAN_SOURCE.into(),
                verified_at,
                cost_basis: CostBasis::Exact,
                pricing: mimo((2, 1), 100, 200),
            },
            OfficialPriceCard {
                provider: "mimo".into(),
                model: "mimo-v2.5-pro".into(),
                capability: Complexity::Standard,
                source_url: MIMO_TOKEN_PLAN_SOURCE.into(),
                verified_at,
                cost_basis: CostBasis::Exact,
                pricing: mimo((5, 2), 300, 600),
            },
            OfficialPriceCard {
                provider: "mimo".into(),
                model: "mimo-v2.5".into(),
                capability: Complexity::Simple,
                source_url: MIMO_PAYG_SOURCE.into(),
                verified_at,
                cost_basis: CostBasis::Exact,
                pricing: payg("0.0028", "0.14", "0.28"),
            },
            OfficialPriceCard {
                provider: "mimo".into(),
                model: "mimo-v2.5-pro".into(),
                capability: Complexity::Standard,
                source_url: MIMO_PAYG_SOURCE.into(),
                verified_at,
                cost_basis: CostBasis::Exact,
                pricing: payg("0.0036", "0.435", "0.87"),
            },
            OfficialPriceCard {
                provider: "deepseek".into(),
                model: "deepseek-v4-flash".into(),
                capability: Complexity::Simple,
                source_url: DEEPSEEK_PRICING_SOURCE.into(),
                verified_at,
                cost_basis: CostBasis::Exact,
                pricing: payg("0.0028", "0.14", "0.28"),
            },
            OfficialPriceCard {
                provider: "deepseek".into(),
                model: "deepseek-v4-pro".into(),
                capability: Complexity::Standard,
                source_url: DEEPSEEK_PRICING_SOURCE.into(),
                verified_at,
                cost_basis: CostBasis::Exact,
                pricing: payg("0.003625", "0.435", "0.87"),
            },
        ],
        advisory: None,
    }
}

// Call only after the selected official source has validated. Static seeds keep
// their historical timestamp and interpretation instead of inventing freshness.
fn refreshed_catalog_at(verified_at: DateTime<Utc>) -> OfficialPriceCatalog {
    let mut catalog = seeded_catalog_at(verified_at);
    for card in &mut catalog.cards {
        if card.provider != "deepseek" {
            continue;
        }
        let (cached, input, output) = match card.model.as_str() {
            "deepseek-v4-flash" => ("0.006", "0.30", "1.20"),
            "deepseek-v4-pro" => ("0.044", "1.32", "3.96"),
            _ => unreachable!("fixed DeepSeek catalog"),
        };
        if let CardPricing::MeteredUsd {
            input_per_million_usd,
            cached_input_per_million_usd,
            output_per_million_usd,
            reasoning_output_per_million_usd,
            ..
        } = &mut card.pricing
        {
            *input_per_million_usd = input.parse().unwrap();
            *cached_input_per_million_usd = cached.parse().unwrap();
            *output_per_million_usd = output.parse().unwrap();
            *reasoning_output_per_million_usd = output.parse().unwrap();
        }
        card.cost_basis = CostBasis::PeakUpperBound;
    }
    catalog
}

fn gcd(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

fn terminating_decimal(value: ExactRational) -> Option<String> {
    let mut denominator = value.denominator;
    let mut twos = 0_u32;
    let mut fives = 0_u32;
    while denominator.is_multiple_of(2) {
        denominator /= 2;
        twos += 1;
    }
    while denominator.is_multiple_of(5) {
        denominator /= 5;
        fives += 1;
    }
    if denominator != 1 {
        return None;
    }
    let scale = twos.max(fives);
    if scale > 12 {
        return None;
    }
    let scaled = value
        .numerator
        .checked_mul(2_u128.checked_pow(scale - twos)?)?
        .checked_mul(5_u128.checked_pow(scale - fives)?)?;
    DecimalUsd::from_parts(scaled, scale as u8).map(|decimal| decimal.to_string())
}

#[derive(Debug, thiserror::Error)]
pub enum PricingError {
    #[error("pricing catalog is invalid")]
    InvalidCatalog,
    #[error("official pricing source was incomplete or changed")]
    InvalidOfficialSource,
    #[error("official pricing request failed")]
    Request,
    #[error("official pricing response was too large")]
    TooLarge,
    #[error("pricing I/O failed: {0}")]
    Io(String),
}

pub struct PricingStore {
    path: PathBuf,
}

impl PricingStore {
    pub(crate) fn admission_revision(&self) -> Result<String, PricingError> {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        match std::fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok("seeded".into()),
            Err(error) => Err(PricingError::Io(error.to_string())),
            Ok(metadata)
                if metadata.file_type().is_file() && metadata.len() <= MAX_CATALOG_BYTES =>
            {
                let file = std::fs::File::open(&self.path)
                    .map_err(|error| PricingError::Io(error.to_string()))?;
                let mut bytes = Vec::new();
                file.take(MAX_CATALOG_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| PricingError::Io(error.to_string()))?;
                if bytes.len() as u64 > MAX_CATALOG_BYTES {
                    return Err(PricingError::InvalidCatalog);
                }
                Ok(format!("{:x}", Sha256::digest(bytes)))
            }
            _ => Err(PricingError::InvalidCatalog),
        }
    }

    pub fn for_paths(paths: &RuntimePaths, profile: &ActiveProfile) -> Self {
        Self {
            path: paths.profile_data_dir(profile).join("pricing/catalog.json"),
        }
    }

    pub fn for_profile_label(paths: &RuntimePaths, profile: &str) -> Self {
        let data = match profile {
            "default" | "bare" => paths.data_dir.clone(),
            name => paths.data_dir.join("profiles").join(name),
        };
        Self {
            path: data.join("pricing/catalog.json"),
        }
    }

    #[cfg(test)]
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<OfficialPriceCatalog, PricingError> {
        if !self.path.exists() {
            return Ok(OfficialPriceCatalog::seeded());
        }
        let metadata = std::fs::symlink_metadata(&self.path)
            .map_err(|error| PricingError::Io(error.to_string()))?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_CATALOG_BYTES {
            return Err(PricingError::InvalidCatalog);
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(&self.path)
            .and_then(|file| file.take(MAX_CATALOG_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|error| PricingError::Io(error.to_string()))?;
        if bytes.len() as u64 > MAX_CATALOG_BYTES {
            return Err(PricingError::InvalidCatalog);
        }
        let mut catalog = serde_json::from_slice::<OfficialPriceCatalog>(&bytes)
            .map_err(|_| PricingError::InvalidCatalog)?;
        migrate_catalog_in_memory(&mut catalog);
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn save(&self, catalog: &OfficialPriceCatalog) -> Result<(), PricingError> {
        catalog.validate()?;
        let bytes = serde_json::to_vec_pretty(catalog).map_err(|_| PricingError::InvalidCatalog)?;
        if bytes.len() as u64 > MAX_CATALOG_BYTES {
            return Err(PricingError::InvalidCatalog);
        }
        let parent = self.path.parent().ok_or(PricingError::InvalidCatalog)?;
        std::fs::create_dir_all(parent).map_err(|error| PricingError::Io(error.to_string()))?;
        let temporary = parent.join(format!(".catalog.{}.tmp", Uuid::new_v4()));
        let result = (|| -> Result<(), PricingError> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.mode(0o600);
            }
            let mut file = options
                .open(&temporary)
                .map_err(|error| PricingError::Io(error.to_string()))?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|error| PricingError::Io(error.to_string()))?;
            std::fs::rename(&temporary, &self.path)
                .map_err(|error| PricingError::Io(error.to_string()))?;
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| PricingError::Io(error.to_string()))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

#[derive(Parser)]
#[command(
    name = "ca pricing",
    bin_name = "ca pricing",
    about = "Inspect or refresh the fail-closed official delegation price catalog"
)]
struct RawPricingArgs {
    #[command(subcommand)]
    command: PricingCommand,
    #[arg(long, value_name = "NAME", global = true)]
    profile: Option<String>,
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum PricingCommand {
    /// Show source freshness and delegation price-card readiness without network
    Status,
    /// Refresh from allowlisted official pages and advisory-check models.dev
    Refresh {
        /// Refresh only this provider; preserve other providers' saved prices.
        #[arg(long, value_parser = ["openai", "mimo", "deepseek"])]
        provider: Option<String>,
    },
}

pub struct PricingCliArgs {
    command: PricingCommand,
    profile: ActiveProfile,
    json: bool,
}

impl std::fmt::Debug for PricingCliArgs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PricingCliArgs")
            .field("command", &self.command)
            .field("profile", &self.profile)
            .field("json", &self.json)
            .finish()
    }
}

pub fn parse_args<I, T>(args: I) -> Result<PricingCliArgs, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let raw = RawPricingArgs::try_parse_from(args)?;
    let profile = ActiveProfile::parse(raw.profile.as_deref()).map_err(|error| {
        RawPricingArgs::command().error(clap::error::ErrorKind::InvalidValue, error.to_string())
    })?;
    Ok(PricingCliArgs {
        command: raw.command,
        profile,
        json: raw.json,
    })
}

#[derive(Debug, Serialize)]
struct PricingStatus {
    schema: &'static str,
    profile: String,
    refreshed: bool,
    ready_cards: usize,
    stale_cards: usize,
    advisory: Option<String>,
    cards: Vec<PricingCardStatus>,
}

#[derive(Debug, Serialize)]
struct PricingCardStatus {
    provider: String,
    model: String,
    capability: Complexity,
    billing_mode: &'static str,
    cost_basis: CostBasis,
    source_url: String,
    verified_at: DateTime<Utc>,
    fresh: bool,
}

pub async fn run_cli(cli: PricingCliArgs) -> i32 {
    let paths = match RuntimePaths::discover() {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("ca pricing: {error}");
            return 1;
        }
    };
    let store = PricingStore::for_paths(&paths, &cli.profile);
    let selected = match &cli.command {
        PricingCommand::Refresh { provider } => provider.clone(),
        PricingCommand::Status => None,
    };
    let (catalog, refreshed) = match cli.command {
        PricingCommand::Status => match store.load() {
            Ok(catalog) => (catalog, false),
            Err(error) => {
                eprintln!("ca pricing: {error}");
                return 1;
            }
        },
        PricingCommand::Refresh { provider } => {
            match refresh_official_catalog(&store, provider.as_deref()).await {
                Ok(catalog) => {
                    if let Err(error) = store.save(&catalog) {
                        eprintln!("ca pricing: {error}");
                        return 1;
                    }
                    (catalog, true)
                }
                Err(error) => {
                    eprintln!("ca pricing: {error}");
                    return 1;
                }
            }
        }
    };
    let mut status = status_for(&catalog, &cli.profile, refreshed, Utc::now());
    if let Some(provider) = selected {
        status.cards.retain(|card| card.provider == provider);
        status.ready_cards = status.cards.iter().filter(|card| card.fresh).count();
        status.stale_cards = status.cards.len().saturating_sub(status.ready_cards);
    }
    let output = if cli.json {
        serde_json::to_string_pretty(&status).expect("pricing status is serializable")
    } else {
        render_status(&status)
    };
    println!("{output}");
    i32::from(status.stale_cards != 0)
}

fn status_for(
    catalog: &OfficialPriceCatalog,
    profile: &ActiveProfile,
    refreshed: bool,
    now: DateTime<Utc>,
) -> PricingStatus {
    let cards = catalog
        .cards
        .iter()
        .map(|card| PricingCardStatus {
            provider: card.provider.clone(),
            model: card.model.clone(),
            capability: card.capability,
            billing_mode: card.pricing.billing_mode_label(),
            cost_basis: card.cost_basis,
            source_url: card.source_url.clone(),
            verified_at: card.verified_at,
            fresh: card.fresh_at(now),
        })
        .collect::<Vec<_>>();
    let ready_cards = cards.iter().filter(|card| card.fresh).count();
    PricingStatus {
        schema: STATUS_SCHEMA,
        profile: profile.label().into(),
        refreshed,
        ready_cards,
        stale_cards: cards.len().saturating_sub(ready_cards),
        advisory: catalog.advisory.clone(),
        cards,
    }
}

fn render_status(status: &PricingStatus) -> String {
    let mut lines = vec![
        format!("ca pricing · profile {}", status.profile),
        String::new(),
    ];
    for card in &status.cards {
        lines.push(format!(
            "- {}/{}: {} · {} · verified {} · {}",
            card.provider,
            card.model,
            card.capability.label(),
            card.billing_mode,
            card.verified_at.to_rfc3339(),
            if card.fresh { "fresh" } else { "STALE" }
        ));
        lines.push(format!("  source: {}", card.source_url));
    }
    if status.cards.iter().any(|card| card.provider == "mimo") {
        lines.push(String::new());
        lines.push(format!("MiMo Token Plan: {MIMO_OFF_PEAK_COPY}"));
        lines.push("Routing forecasts use peak credits and do not apply this discount.".into());
    }
    if status.cards.iter().any(|card| card.provider == "deepseek") {
        lines.push(String::new());
        lines.push("New DeepSeek cards use peak-rate upper bounds; time discounts and invoiced charges are not inferred.".into());
    }
    if let Some(advisory) = status.advisory.as_deref() {
        lines.push(String::new());
        lines.push(format!("Advisory: {advisory}"));
    }
    lines.push(String::new());
    lines.push(format!(
        "{} ready · {} stale{}",
        status.ready_cards,
        status.stale_cards,
        if status.refreshed {
            " · refreshed"
        } else {
            ""
        }
    ));
    lines.join("\n")
}

async fn refresh_official_catalog(
    store: &PricingStore,
    selected: Option<&str>,
) -> Result<OfficialPriceCatalog, PricingError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| PricingError::Request)?;
    let catalog = if selected.is_some() {
        store.load()?
    } else {
        OfficialPriceCatalog::seeded()
    };
    refresh_with_fetch(catalog, selected, Utc::now(), |url| {
        fetch_bounded(&client, url)
    })
    .await
}

async fn refresh_with_fetch<F, Fut>(
    mut catalog: OfficialPriceCatalog,
    selected: Option<&str>,
    verified_at: DateTime<Utc>,
    mut fetch: F,
) -> Result<OfficialPriceCatalog, PricingError>
where
    F: FnMut(&'static str) -> Fut,
    Fut: std::future::Future<Output = Result<String, PricingError>>,
{
    if selected.is_some_and(|name| !matches!(name, "openai" | "mimo" | "deepseek")) {
        return Err(PricingError::InvalidOfficialSource);
    }
    catalog.validate()?;
    if selected.is_none_or(|name| name == "openai") {
        for (url, model, required) in [
            (
                OPENAI_SOL_SOURCE,
                "gpt-5.6-sol",
                ["$5", "$0.50", "$30", "272", "1.25"],
            ),
            (
                OPENAI_TERRA_SOURCE,
                "gpt-5.6-terra",
                ["$2", "$0.20", "$12", "272", "1.25"],
            ),
            (
                OPENAI_LUNA_SOURCE,
                "gpt-5.6-luna",
                ["$0.20", "$0.02", "$1.20", "272", "1.25"],
            ),
        ] {
            validate_openai_source(&fetch(url).await?, model, &required)?;
        }
    }
    if selected.is_none_or(|name| name == "mimo") {
        validate_mimo_payg_source(&fetch(MIMO_PAYG_SOURCE).await?)?;
        validate_mimo_source(&fetch(MIMO_TOKEN_PLAN_SOURCE).await?)?;
        catalog.advisory = Some(match fetch(MODELS_DEV_SOURCE).await {
            Ok(body) if body.contains("mimo-v2.5") => "models.dev MiMo PAYG data was present and was not used for Token Plan eligibility",
            Ok(_) => "models.dev did not contain the expected MiMo advisory entries",
            Err(_) => "models.dev advisory cross-check was unavailable",
        }.into());
    }
    if selected.is_none_or(|name| name == "deepseek") {
        validate_deepseek_source(&fetch(DEEPSEEK_PRICING_SOURCE).await?)?;
    }
    // Upgrade the shape of legacy catalogs without making their missing or
    // unrelated cards fresh. Only successfully checked providers get new rates.
    for card in OfficialPriceCatalog::seeded().cards {
        if catalog
            .find_for_billing_mode(
                &card.provider,
                &card.model,
                card.pricing.billing_mode_label(),
            )
            .is_none()
        {
            catalog.cards.push(card);
        }
    }
    for card in refreshed_catalog_at(verified_at).cards {
        if selected.is_none_or(|name| name == card.provider) {
            let saved = catalog
                .cards
                .iter_mut()
                .find(|saved| {
                    saved.provider == card.provider
                        && saved.model == card.model
                        && saved.pricing.billing_mode_label() == card.pricing.billing_mode_label()
                })
                .ok_or(PricingError::InvalidCatalog)?;
            *saved = card;
        }
    }
    catalog.validate()?;
    Ok(catalog)
}

async fn fetch_bounded(client: &reqwest::Client, url: &str) -> Result<String, PricingError> {
    if !matches!(
        url,
        OPENAI_SOL_SOURCE
            | OPENAI_TERRA_SOURCE
            | OPENAI_LUNA_SOURCE
            | MIMO_PAYG_SOURCE
            | MIMO_TOKEN_PLAN_SOURCE
            | DEEPSEEK_PRICING_SOURCE
            | MODELS_DEV_SOURCE
    ) {
        return Err(PricingError::Request);
    }
    let parsed = reqwest::Url::parse(url).map_err(|_| PricingError::Request)?;
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(PricingError::Request);
    }
    let response = client
        .get(parsed)
        .send()
        .await
        .map_err(|_| PricingError::Request)?;
    if !response.status().is_success() {
        return Err(PricingError::Request);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_SOURCE_BYTES as u64)
    {
        return Err(PricingError::TooLarge);
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    use futures::StreamExt as _;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| PricingError::Request)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_SOURCE_BYTES {
            return Err(PricingError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| PricingError::InvalidOfficialSource)
}

fn validate_openai_source(body: &str, model: &str, required: &[&str]) -> Result<(), PricingError> {
    if body.len() > MAX_SOURCE_BYTES
        || !body.contains(model)
        || required.iter().any(|item| !body.contains(item))
    {
        return Err(PricingError::InvalidOfficialSource);
    }
    Ok(())
}

fn validate_mimo_source(body: &str) -> Result<(), PricingError> {
    let required = [
        "mimo-v2.5-pro",
        "mimo-v2.5",
        "4,100,000,000",
        "11,000,000,000",
        "38,000,000,000",
        "82,000,000,000",
        "$6",
        "$16",
        "$50",
        "$100",
        "63.36",
        "168.96",
        "528",
        "1056",
        "0.8",
        "16:00",
        "24:00",
    ];
    if body.len() > MAX_SOURCE_BYTES {
        return Err(PricingError::InvalidOfficialSource);
    }
    if required.iter().all(|item| body.contains(item)) {
        return Ok(());
    }
    // The current official page expresses quotas in billions and uses a comma
    // in the annual Max price. Bind both existing model conversion rows and
    // every price/quota to the same checked page before refreshing old routes.
    let tags = regex::Regex::new("<[^>]*>").expect("constant HTML tag regex");
    let stripped = tags.replace_all(body, " ");
    let text = stripped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(',', "");
    let current = [
        "$6/month", "$16/month", "$50/month", "$100/month",
        "Fixed Monthly Quota 4.1 billion Credits 11 billion Credits 38 billion Credits 82 billion Credits",
        "$63.36/year", "$168.96/year", "USD 528.00/year", "USD 1056.00/year",
        "Annual Fixed Quota 49.2 billion Credits 132 billion Credits 456 billion Credits 984 billion Credits",
        "mimo-v2.5-pro 2.5 Credits 300 Credits 600 Credits mimo-v2.5 2 Credits 100 Credits 200 Credits",
        "16:00", "24:00", "0.8x",
    ];
    if current.iter().all(|item| text.contains(item)) {
        Ok(())
    } else {
        Err(PricingError::InvalidOfficialSource)
    }
}

fn validate_mimo_payg_source(body: &str) -> Result<(), PricingError> {
    let required = [
        "mimo-v2.5-pro",
        "mimo-v2.5",
        "$0.0028",
        "$0.0036",
        "$0.14",
        "$0.435",
        "$0.28",
        "$0.87",
    ];
    if body.len() > MAX_SOURCE_BYTES || required.iter().any(|item| !body.contains(item)) {
        return Err(PricingError::InvalidOfficialSource);
    }
    Ok(())
}

fn validate_deepseek_source(body: &str) -> Result<(), PricingError> {
    if body.len() > MAX_SOURCE_BYTES {
        return Err(PricingError::InvalidOfficialSource);
    }
    // Bind the model columns and the ordered cache/input/output rows together;
    // finding unrelated dollar values elsewhere on a changed page is insufficient.
    let tags = regex::Regex::new("<[^>]*>").expect("constant HTML tag regex");
    let stripped = tags.replace_all(body, " ");
    let text = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    let required = [
        "MODEL deepseek-flash (1) deepseek-v4-pro",
        "1M INPUT TOKENS (CACHE HIT) OFF-PEAK $0.003 $0.022 PEAK $0.006 $0.044 1M INPUT TOKENS (CACHE MISS) OFF-PEAK $0.15 $0.66 PEAK $0.3 $1.32 1M OUTPUT TOKENS OFF-PEAK $0.6 $1.98 PEAK $1.2 $3.96",
        "deepseek-v4-flash",
        "billed at the Flash price",
        "Off-peak rates are half of the peak rates",
        "01:00 - 04:00 and 06:00 - 10:00 UTC, Monday through Friday, excluding Chinese public holidays",
    ];
    if required.iter().any(|item| !text.contains(item)) {
        return Err(PricingError::InvalidOfficialSource);
    }
    Ok(())
}

pub fn billing_readiness(
    provider: &str,
    base_url: &str,
    config: Option<&ProviderBillingConfig>,
) -> &'static str {
    match provider {
        "openai" if base_url.trim_end_matches('/') == OPENAI_API_BASE_URL => "metered USD ready",
        "openai" => "custom endpoint; delegation ineligible",
        "deepseek" if official_deepseek_base_url(base_url) => "metered USD ready",
        "deepseek" => "custom endpoint; delegation ineligible",
        "mimo" if official_mimo_payg_base_url(base_url) && config.is_none() => "metered USD ready",
        "mimo"
            if config.is_some_and(|billing| {
                billing.delegation_ready()
                    && billing.validate_for_provider(provider, base_url).is_ok()
            }) =>
        {
            "USD Token Plan ready"
        }
        "mimo" if MimoRegion::from_token_plan_url(base_url).is_some() && config.is_none() => {
            "Token Plan endpoint detected; allocation metadata incomplete"
        }
        "mimo" if config.is_none() => "custom endpoint; delegation ineligible",
        "mimo" => "Token Plan endpoint and billing metadata are inconsistent",
        "openai-codex" => "subscription/OAuth ineligible",
        _ => "unsupported billing mode",
    }
}

pub fn supported_billing_route(
    provider: &str,
    base_url: &str,
    billing: Option<&ProviderBillingConfig>,
) -> bool {
    match provider {
        "openai" => base_url.trim_end_matches('/') == OPENAI_API_BASE_URL,
        "deepseek" => official_deepseek_base_url(base_url),
        "mimo" => {
            (billing.is_none() && official_mimo_payg_base_url(base_url))
                || billing.is_some_and(|billing| {
                    billing.delegation_ready()
                        && billing.validate_for_provider(provider, base_url).is_ok()
                })
        }
        _ => false,
    }
}

fn official_deepseek_base_url(base_url: &str) -> bool {
    matches!(
        base_url.trim_end_matches('/'),
        DEEPSEEK_API_BASE_URL | DEEPSEEK_API_V1_BASE_URL
    )
}

fn official_mimo_payg_base_url(base_url: &str) -> bool {
    base_url.trim_end_matches('/') == MIMO_PAYG_BASE_URL
}

fn supported_card_billing_route(
    card: &OfficialPriceCard,
    base_url: &str,
    billing: Option<&ProviderBillingConfig>,
) -> bool {
    match (&card.pricing, card.provider.as_str()) {
        (CardPricing::MeteredUsd { .. }, "mimo") => {
            billing.is_none() && official_mimo_payg_base_url(base_url)
        }
        (CardPricing::MimoTokenPlan { .. }, "mimo") => billing.is_some_and(|billing| {
            billing.delegation_ready()
                && billing
                    .validate_for_provider(&card.provider, base_url)
                    .is_ok()
        }),
        (CardPricing::MeteredUsd { .. }, _) => {
            supported_billing_route(&card.provider, base_url, billing)
        }
        _ => false,
    }
}

#[derive(Debug, Clone)]
struct DelegationRoute {
    provider: String,
    model: String,
    capability: Complexity,
    card: OfficialPriceCard,
    billing: Option<ProviderBillingConfig>,
}

#[derive(Debug, Clone, Default)]
pub struct DelegationRouter {
    routes: Vec<DelegationRoute>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSelection {
    pub provider: String,
    pub model: String,
    pub capability: Complexity,
    pub estimated_input_tokens: u64,
    pub forecast: Forecast,
}

impl RouteSelection {
    pub fn render(&self) -> String {
        format!(
            "{}/{} · {} · {} estimated input tokens · {}",
            self.provider,
            self.model,
            self.capability.label(),
            self.estimated_input_tokens,
            self.forecast.render()
        )
    }
}

impl DelegationRouter {
    pub fn from_config(
        config: &crate::config::CaConfig,
        catalog: &OfficialPriceCatalog,
        now: DateTime<Utc>,
    ) -> Self {
        let mut routes = Vec::new();
        for card in &catalog.cards {
            if !card.fresh_at(now) {
                continue;
            }
            let Some(provider) = config.providers.get(&card.provider) else {
                continue;
            };
            if !provider.models.iter().any(|model| model == &card.model)
                || !credential_ready(&provider.api_key)
            {
                continue;
            }
            let billing = config.provider_billing.get(&card.provider).cloned();
            let billing_ready = match &card.pricing {
                CardPricing::MeteredUsd { .. } => {
                    supported_card_billing_route(card, &provider.base_url, billing.as_ref())
                }
                CardPricing::MimoTokenPlan { .. } => {
                    card.provider == "mimo"
                        && crate::pricing::valid_token_plan_key(provider.api_key.as_bytes())
                        && supported_card_billing_route(card, &provider.base_url, billing.as_ref())
                }
            };
            if billing_ready {
                routes.push(DelegationRoute {
                    provider: card.provider.clone(),
                    model: card.model.clone(),
                    capability: card.capability,
                    card: card.clone(),
                    billing,
                });
            }
        }
        routes.sort_by(|left, right| {
            left.provider
                .cmp(&right.provider)
                .then_with(|| left.model.cmp(&right.model))
        });
        Self { routes }
    }

    pub fn select(
        &self,
        complexity: Complexity,
        task: &str,
        context_summary: &str,
    ) -> Result<RouteSelection, String> {
        self.select_matching(complexity, task, context_summary, |_, _| true)
    }

    pub(crate) fn select_matching(
        &self,
        complexity: Complexity,
        task: &str,
        context_summary: &str,
        matches_route: impl Fn(&str, &str) -> bool,
    ) -> Result<RouteSelection, String> {
        let estimated_input_tokens =
            crate::context::estimate_text_tokens(&format!("{task}\n{context_summary}"));
        let mut candidates = self
            .routes
            .iter()
            .filter(|route| route.capability >= complexity)
            .filter(|route| matches_route(&route.provider, &route.model))
            .filter_map(|route| {
                let forecast = forecast_for(
                    &route.card,
                    route.billing.as_ref(),
                    estimated_input_tokens,
                    4_096,
                )?;
                Some(RouteSelection {
                    provider: route.provider.clone(),
                    model: route.model.clone(),
                    capability: route.capability,
                    estimated_input_tokens,
                    forecast,
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            left.forecast
                .score()
                .cmp(&right.forecast.score())
                .then_with(|| left.capability.cmp(&right.capability))
                .then_with(|| left.provider.cmp(&right.provider))
                .then_with(|| left.model.cmp(&right.model))
        });
        candidates.into_iter().next().ok_or_else(|| {
            format!(
                "no automatic delegation route qualifies for {} complexity; verify credentials, supported billing metadata, capability, and pricing freshness",
                complexity.label()
            )
        })
    }

    pub fn eligible_routes(&self) -> Vec<String> {
        self.routes
            .iter()
            .map(|route| format!("{}/{}", route.provider, route.model))
            .collect()
    }
}

fn credential_ready(value: &str) -> bool {
    let value = value.trim();
    !(value.is_empty()
        || (value.starts_with("${") && value.ends_with('}'))
        || crate::provider_secrets::resembles_saved_reference(value))
}

#[cfg(test)]
mod tests {
    #[test]
    fn bug257_revision_detects_same_length_rewrites_and_bounds_invalid_catalogs() {
        let temporary = tempfile::tempdir().unwrap();
        let store = super::PricingStore::new(temporary.path().join("catalog.json"));
        assert_eq!(store.admission_revision().unwrap(), "seeded");
        std::fs::write(&store.path, b"one").unwrap();
        let first = store.admission_revision().unwrap();
        std::fs::write(&store.path, b"two").unwrap();
        assert_ne!(store.admission_revision().unwrap(), first);
        std::fs::write(&store.path, vec![0; super::MAX_CATALOG_BYTES as usize + 1]).unwrap();
        assert!(store.admission_revision().is_err());
    }

    use super::*;

    const DEEPSEEK_SOURCE_FIXTURE: &str = "<table><tr><td>MODEL</td><td>deepseek-flash <sup>(1)</sup></td><td>deepseek-v4-pro</td></tr></table>
        1M INPUT TOKENS (CACHE HIT) OFF-PEAK $0.003 $0.022 PEAK $0.006 $0.044
        1M INPUT TOKENS (CACHE MISS) OFF-PEAK $0.15 $0.66 PEAK $0.3 $1.32
        1M OUTPUT TOKENS OFF-PEAK $0.6 $1.98 PEAK $1.2 $3.96
        deepseek-v4-flash billed at the Flash price.
        Off-peak rates are half of the peak rates.
        01:00 - 04:00 and 06:00 - 10:00 UTC, Monday through Friday, excluding Chinese public holidays";

    #[test]
    #[ignore = "set CA_PRICING_SOURCE_FIXTURE to a downloaded official pricing page"]
    fn deepseek_downloaded_official_source_matches_current_prices() {
        let path =
            std::env::var_os("CA_PRICING_SOURCE_FIXTURE").expect("explicit public source fixture");
        let body = std::fs::read_to_string(path).unwrap();
        validate_deepseek_source(&body).unwrap();
    }

    fn usage(prompt: u32, output: u32, cached: Option<u32>) -> UsageInfo {
        UsageInfo {
            prompt_tokens: prompt,
            completion_tokens: output,
            total_tokens: prompt + output,
            cached_prompt_tokens: cached,
            cache_write_tokens: Some(0),
            reasoning_tokens: Some(0),
            response_metadata: None,
            cache_usage_override: None,
        }
    }

    fn token_plan() -> ProviderBillingConfig {
        ProviderBillingConfig::mimo_token_plan(
            MimoRegion::Singapore,
            MimoPlanTier::Lite,
            MimoPlanTerm::Monthly,
            "6".parse().unwrap(),
            false,
        )
        .unwrap()
    }

    #[test]
    fn pricing_cli_parses_status_refresh_profile_and_json_without_network() {
        let status = parse_args(["ca-pricing", "status", "--json", "--profile", "work"])
            .expect("status should parse");
        assert!(matches!(status.command, PricingCommand::Status));
        assert!(status.json);
        assert_eq!(status.profile.label(), "work");

        let refresh = parse_args(["ca-pricing", "refresh"]).expect("refresh should parse");
        assert!(matches!(
            refresh.command,
            PricingCommand::Refresh { provider: None }
        ));
        assert!(!refresh.json);
        assert!(parse_args(["ca-pricing", "refresh", "--profile", "bad/name"]).is_err());
        let selected = parse_args(["ca-pricing", "refresh", "--provider", "deepseek"]).unwrap();
        assert!(
            matches!(selected.command, PricingCommand::Refresh { provider: Some(name) } if name == "deepseek")
        );
        assert!(parse_args(["ca-pricing", "refresh", "--provider", "unknown"]).is_err());
    }

    #[tokio::test]
    async fn selected_provider_refresh_ignores_unrelated_failures_and_preserves_cards() {
        let original = OfficialPriceCatalog::seeded();
        let now = DateTime::parse_from_rfc3339("2026-09-23T12:00:00Z")
            .unwrap()
            .to_utc();
        let mut urls = Vec::new();
        let refreshed = refresh_with_fetch(original.clone(), Some("deepseek"), now, |url| {
            urls.push(url);
            std::future::ready(if url == DEEPSEEK_PRICING_SOURCE {
                Ok(DEEPSEEK_SOURCE_FIXTURE.to_owned())
            } else {
                Err(PricingError::Request)
            })
        })
        .await
        .unwrap();
        assert_eq!(urls, [DEEPSEEK_PRICING_SOURCE]);
        for card in &refreshed.cards {
            if card.provider == "deepseek" {
                assert_eq!(card.verified_at, now);
                assert!(card.fresh_at(now));
                assert_eq!(card.cost_basis, CostBasis::PeakUpperBound);
            } else {
                assert_eq!(
                    Some(card),
                    original.find_for_billing_mode(
                        &card.provider,
                        &card.model,
                        card.pricing.billing_mode_label()
                    )
                );
                assert!(!card.fresh_at(now));
            }
        }
        assert!(refresh_with_fetch(original, Some("deepseek"), now, |_| {
            std::future::ready(Ok("incomplete source".into()))
        })
        .await
        .is_err());
    }

    #[test]
    fn saved_legacy_cards_keep_exact_rate_semantics_and_serialized_identity() {
        let card = OfficialPriceCatalog::seeded()
            .find("deepseek", "deepseek-v4-pro")
            .unwrap()
            .clone();
        let mut saved = serde_json::to_value(card).unwrap();
        saved.as_object_mut().unwrap().remove("cost_basis");
        saved["pricing"]["input_per_million_usd"] = serde_json::json!("0.435");
        saved["pricing"]["cached_input_per_million_usd"] = serde_json::json!("0.003625");
        saved["pricing"]["output_per_million_usd"] = serde_json::json!("0.87");
        saved["pricing"]["reasoning_output_per_million_usd"] = serde_json::json!("0.87");
        let restored: OfficialPriceCard = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!(serde_json::to_value(&restored).unwrap(), saved);
        assert_eq!(restored.cost_basis, CostBasis::Exact);
        let now = Utc::now();
        let amount = actual_cost_for(
            &restored,
            None,
            &usage(1_000_000, 1_000_000, Some(0)),
            now,
            now,
        )
        .unwrap();
        assert!(matches!(amount, ActualCost::IncrementalUsd(_)));
        assert_eq!(amount.upper_bound_usd().unwrap().to_string(), "1.305");
    }

    #[test]
    fn current_deepseek_peak_forecast_does_not_underreserve() {
        let catalog = refreshed_catalog_at(Utc::now());
        let card = catalog.find("deepseek", "deepseek-v4-pro").unwrap();
        assert_eq!(
            forecast_for(card, None, 1_000_000, 1_000_000)
                .unwrap()
                .score()
                .to_string(),
            "5.28"
        );
    }

    #[test]
    fn seeded_cards_have_exact_capabilities_rates_and_sources() {
        let catalog = OfficialPriceCatalog::seeded();
        catalog.validate().unwrap();
        assert_eq!(
            catalog.find("openai", "gpt-5.6-luna").unwrap().capability,
            Complexity::Simple
        );
        assert_eq!(
            catalog.find("openai", "gpt-5.6-sol").unwrap().capability,
            Complexity::Complex
        );
        assert_eq!(
            catalog.find("openai", "gpt-5.6-terra").unwrap().capability,
            Complexity::Standard
        );
        assert_eq!(
            catalog.find("mimo", "mimo-v2.5").unwrap().capability,
            Complexity::Simple
        );
        assert_eq!(
            catalog.find("mimo", "mimo-v2.5-pro").unwrap().capability,
            Complexity::Standard
        );
        assert_eq!(
            catalog
                .find("deepseek", "deepseek-v4-flash")
                .unwrap()
                .capability,
            Complexity::Simple
        );
        assert_eq!(
            catalog
                .find("deepseek", "deepseek-v4-pro")
                .unwrap()
                .capability,
            Complexity::Standard
        );
        let sol = forecast_for(
            catalog.find("openai", "gpt-5.6-sol").unwrap(),
            None,
            1_000_000,
            1_000_000,
        )
        .unwrap();
        assert_eq!(sol.score().to_string(), "57.5");
        let luna = forecast_for(
            catalog.find("openai", "gpt-5.6-luna").unwrap(),
            None,
            1_000_000,
            1_000_000,
        )
        .unwrap();
        assert_eq!(luna.score().to_string(), "2.3");
        let terra = forecast_for(
            catalog.find("openai", "gpt-5.6-terra").unwrap(),
            None,
            1_000_000,
            1_000_000,
        )
        .unwrap();
        assert_eq!(terra.score().to_string(), "23");
        let flash = forecast_for(
            catalog.find("deepseek", "deepseek-v4-flash").unwrap(),
            None,
            1_000_000,
            1_000_000,
        )
        .unwrap();
        assert_eq!(flash.score().to_string(), "0.42");
        let pro = forecast_for(
            catalog.find("deepseek", "deepseek-v4-pro").unwrap(),
            None,
            1_000_000,
            1_000_000,
        )
        .unwrap();
        assert_eq!(pro.score().to_string(), "1.305");
        let mimo_forecast = forecast_for(
            catalog.find("mimo", "mimo-v2.5-pro").unwrap(),
            Some(&token_plan()),
            10,
            2,
        )
        .unwrap()
        .render();
        assert!(mimo_forecast.contains("peak forecast; 20% off-peak not included"));
        let mimo_payg = forecast_for(
            catalog
                .find_for_billing_mode("mimo", "mimo-v2.5-pro", "metered_usd")
                .unwrap(),
            None,
            1_000_000,
            1_000_000,
        )
        .unwrap();
        assert_eq!(mimo_payg.score().to_string(), "1.305");
    }

    #[test]
    fn long_context_and_required_subsets_fail_closed() {
        let catalog = OfficialPriceCatalog::seeded();
        let sol = catalog.find("openai", "gpt-5.6-sol").unwrap();
        assert_eq!(
            forecast_for(sol, None, 272_001, 4_096)
                .unwrap()
                .score()
                .to_string(),
            "3.5843325"
        );
        let incomplete = UsageInfo {
            cache_write_tokens: None,
            ..usage(100, 10, Some(0))
        };
        let now = Utc::now();
        assert!(actual_cost_for(sol, None, &incomplete, now, now).is_none());
    }

    #[test]
    fn metered_cache_write_modes_use_exact_subset_math() {
        let mut card = OfficialPriceCatalog::seeded()
            .find("openai", "gpt-5.6-sol")
            .unwrap()
            .clone();
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
        let now = Utc::now();
        let mut price = |billing| {
            let CardPricing::MeteredUsd {
                cache_write_billing,
                ..
            } = &mut card.pricing
            else {
                panic!()
            };
            *cache_write_billing = billing;
            let ActualCost::IncrementalUsd(cost) =
                actual_cost_for(&card, None, &usage, now, now).unwrap()
            else {
                panic!()
            };
            cost.to_string()
        };
        assert_eq!(
            price(CacheWriteBilling::Replacement {
                rate: "6.25".parse().unwrap(),
            }),
            "0.000345"
        );
        assert_eq!(price(CacheWriteBilling::Included), "0.00032");
        assert_eq!(
            price(CacheWriteBilling::Surcharge {
                rate: "1.25".parse().unwrap(),
            }),
            "0.000345"
        );
        assert_eq!(
            price(CacheWriteBilling::Replacement {
                rate: DecimalUsd::ZERO,
            }),
            "0.00022"
        );
        let unknown_write = UsageInfo {
            cache_write_tokens: None,
            ..usage
        };
        if let CardPricing::MeteredUsd {
            cache_write_billing,
            ..
        } = &mut card.pricing
        {
            *cache_write_billing = CacheWriteBilling::Included;
        }
        assert!(actual_cost_for(&card, None, &unknown_write, now, now).is_some());
        if let CardPricing::MeteredUsd {
            cache_write_billing,
            ..
        } = &mut card.pricing
        {
            *cache_write_billing = CacheWriteBilling::Replacement {
                rate: "6.25".parse().unwrap(),
            };
        }
        assert!(actual_cost_for(&card, None, &unknown_write, now, now).is_none());
    }

    #[test]
    fn mimo_credits_off_peak_and_subscription_allocation_are_exact() {
        let catalog = OfficialPriceCatalog::seeded();
        let card = catalog.find("mimo", "mimo-v2.5-pro").unwrap();
        let billing = token_plan();
        let peak_start = DateTime::parse_from_rfc3339("2026-08-15T15:00:00Z")
            .unwrap()
            .to_utc();
        let peak_end = DateTime::parse_from_rfc3339("2026-08-15T15:01:00Z")
            .unwrap()
            .to_utc();
        let mut included_write_usage = usage(10, 2, Some(4));
        included_write_usage.cache_write_tokens = Some(4);
        let peak = actual_cost_for(
            card,
            Some(&billing),
            &included_write_usage,
            peak_start,
            peak_end,
        )
        .unwrap();
        let ActualCost::SubscriptionAllocation {
            credits,
            allocated_usd,
            off_peak,
        } = peak
        else {
            panic!()
        };
        assert_eq!(credits.to_string(), "3010");
        assert_eq!(allocated_usd.to_string(), "903/205000000");
        assert!(!off_peak);

        let off_start = DateTime::parse_from_rfc3339("2026-08-15T16:00:00Z")
            .unwrap()
            .to_utc();
        let off_end = DateTime::parse_from_rfc3339("2026-08-15T16:01:00Z")
            .unwrap()
            .to_utc();
        let ActualCost::SubscriptionAllocation {
            credits, off_peak, ..
        } = actual_cost_for(
            card,
            Some(&billing),
            &usage(10, 2, Some(4)),
            off_start,
            off_end,
        )
        .unwrap()
        else {
            panic!()
        };
        assert_eq!(credits.to_string(), "2408");
        assert!(off_peak);
        let rendered = actual_cost_for(
            card,
            Some(&billing),
            &usage(10, 2, Some(4)),
            off_start,
            off_end,
        )
        .unwrap()
        .render();
        assert!(rendered.contains("20% off-peak"));
        assert!(!rendered.contains("(off-peak)"));

        let boundary_start = DateTime::parse_from_rfc3339("2026-08-15T15:59:59Z")
            .unwrap()
            .to_utc();
        assert!(actual_cost_for(
            card,
            Some(&billing),
            &usage(10, 2, Some(4)),
            boundary_start,
            off_end
        )
        .is_none());
    }

    #[test]
    fn token_plan_regions_billing_diagnostics_and_discount_copy_are_exact() {
        for region in MimoRegion::ALL {
            assert_eq!(
                MimoRegion::from_token_plan_url(region.token_plan_url()),
                Some(region)
            );
            assert_eq!(
                MimoRegion::from_token_plan_url(&format!("{}/", region.token_plan_url())),
                Some(region)
            );
        }
        assert_eq!(
            billing_readiness("mimo", MimoRegion::Singapore.token_plan_url(), None),
            "Token Plan endpoint detected; allocation metadata incomplete"
        );
        assert_eq!(
            billing_readiness("mimo", "https://api.xiaomimimo.com/v1", None),
            "metered USD ready"
        );
        assert!(supported_billing_route(
            "mimo",
            "https://api.xiaomimimo.com/v1",
            None
        ));
        let status = status_for(
            &OfficialPriceCatalog::seeded(),
            &crate::runtime::ActiveProfile::Default,
            false,
            DateTime::parse_from_rfc3339("2026-08-15T12:00:00Z")
                .unwrap()
                .to_utc(),
        );
        let rendered = render_status(&status);
        assert!(rendered.contains(MIMO_OFF_PEAK_COPY));
        assert!(rendered.contains("Routing forecasts use peak credits"));
    }

    #[test]
    fn deepseek_peak_accounting_is_explicitly_an_upper_bound() {
        assert_eq!(
            billing_readiness("openai", OPENAI_API_BASE_URL, None),
            "metered USD ready"
        );
        assert_eq!(
            billing_readiness("openai", "https://proxy.example/v1", None),
            "custom endpoint; delegation ineligible"
        );
        assert_eq!(
            billing_readiness("deepseek", "https://api.deepseek.com", None),
            "metered USD ready"
        );
        assert_eq!(
            billing_readiness("deepseek", "https://api.deepseek.com/v1", None),
            "metered USD ready"
        );
        assert_eq!(
            billing_readiness("deepseek", "https://proxy.example/v1", None),
            "custom endpoint; delegation ineligible"
        );
        assert!(supported_billing_route(
            "deepseek",
            "https://api.deepseek.com/v1/",
            None
        ));
        assert!(!supported_billing_route(
            "deepseek",
            "https://proxy.example/v1",
            None
        ));

        let card = refreshed_catalog_at(Utc::now())
            .find("deepseek", "deepseek-v4-flash")
            .unwrap()
            .clone();
        let now = Utc::now();
        let reported_write = UsageInfo {
            prompt_tokens: 100,
            completion_tokens: 10,
            total_tokens: 110,
            cached_prompt_tokens: Some(40),
            cache_write_tokens: Some(20),
            reasoning_tokens: None,
            response_metadata: None,
            cache_usage_override: None,
        };
        let actual = actual_cost_for(&card, None, &reported_write, now, now).unwrap();
        let ActualCost::UpperBoundIncrementalUsd(value) = actual else {
            panic!()
        };
        assert_eq!(value.to_string(), "0.00003024");
        assert!(actual.render().contains("peak-rate upper bound"));
        let unknown_write = UsageInfo {
            cache_write_tokens: None,
            ..reported_write
        };
        assert_eq!(
            actual_cost_for(&card, None, &unknown_write, now, now),
            Some(ActualCost::UpperBoundIncrementalUsd(value))
        );

        let incomplete = UsageInfo {
            cached_prompt_tokens: None,
            ..usage(100, 10, None)
        };
        assert!(actual_cost_for(&card, None, &incomplete, now, now).is_none());
    }

    #[test]
    fn all_mimo_plan_pools_and_mismatch_confirmation_are_exact() {
        let expected = [
            (
                MimoPlanTier::Lite,
                MimoPlanTerm::Monthly,
                "6",
                4_100_000_000,
            ),
            (
                MimoPlanTier::Standard,
                MimoPlanTerm::Monthly,
                "16",
                11_000_000_000,
            ),
            (
                MimoPlanTier::Pro,
                MimoPlanTerm::Monthly,
                "50",
                38_000_000_000,
            ),
            (
                MimoPlanTier::Max,
                MimoPlanTerm::Monthly,
                "100",
                82_000_000_000,
            ),
            (
                MimoPlanTier::Lite,
                MimoPlanTerm::Annual,
                "63.36",
                49_200_000_000,
            ),
            (
                MimoPlanTier::Standard,
                MimoPlanTerm::Annual,
                "168.96",
                132_000_000_000,
            ),
            (
                MimoPlanTier::Pro,
                MimoPlanTerm::Annual,
                "528",
                456_000_000_000,
            ),
            (
                MimoPlanTier::Max,
                MimoPlanTerm::Annual,
                "1056",
                984_000_000_000,
            ),
        ];
        for (tier, term, price, pool) in expected {
            let (actual_price, actual_pool) = mimo_plan_terms(tier, term);
            assert_eq!(actual_price.to_string(), price);
            assert_eq!(actual_pool, pool);
        }
        assert!(ProviderBillingConfig::mimo_token_plan(
            MimoRegion::Cn,
            MimoPlanTier::Lite,
            MimoPlanTerm::Monthly,
            "5".parse().unwrap(),
            false
        )
        .is_err());
        assert!(ProviderBillingConfig::mimo_token_plan(
            MimoRegion::Cn,
            MimoPlanTier::Lite,
            MimoPlanTerm::Monthly,
            "5".parse().unwrap(),
            true
        )
        .is_ok());
    }

    #[test]
    fn stale_catalog_and_invalid_official_sources_fail_closed() {
        let catalog = OfficialPriceCatalog::seeded();
        let card = catalog.find("openai", "gpt-5.6-sol").unwrap();
        assert!(card.fresh_at(
            DateTime::parse_from_rfc3339("2026-09-14T00:00:00Z")
                .unwrap()
                .to_utc()
        ));
        assert!(!card.fresh_at(
            DateTime::parse_from_rfc3339("2026-09-15T00:00:01Z")
                .unwrap()
                .to_utc()
        ));
        assert!(validate_openai_source(
            "gpt-5.6-sol $5 $0.50 $30 272",
            "gpt-5.6-sol",
            &["$5", "$0.50", "$30", "272", "1.25"]
        )
        .is_err());
        assert!(validate_mimo_source("mimo-v2.5").is_err());
        assert!(validate_mimo_payg_source(
            "mimo-v2.5 mimo-v2.5-pro $0.0028 $0.0036 $0.14 $0.435 $0.28 $0.87"
        )
        .is_ok());
        assert!(validate_deepseek_source(DEEPSEEK_SOURCE_FIXTURE).is_ok());
        assert!(
            validate_deepseek_source(&DEEPSEEK_SOURCE_FIXTURE.replace("$1.32", "$1.33")).is_err()
        );
        assert!(validate_deepseek_source(
            &DEEPSEEK_SOURCE_FIXTURE.replace("$0.006 $0.044", "$0.044 $0.006")
        )
        .is_err());
        assert!(validate_deepseek_source("deepseek-v4-flash deepseek-v4-pro $0.0028 $0.003625 $0.14 $0.435 $0.28 $0.87 CACHE HIT CACHE MISS").is_err());
        assert!(validate_deepseek_source("deepseek-v4-flash $0.14").is_err());
    }

    #[test]
    fn current_mimo_subscription_page_binds_quotas_prices_and_existing_routes() {
        let page = "$6/month $16/month $50/month $100/month Fixed Monthly Quota 4.1 billion Credits 11 billion Credits 38 billion Credits 82 billion Credits $63.36/year $168.96/year USD 528.00/year USD 1,056.00/year Annual Fixed Quota 49.2 billion Credits 132 billion Credits 456 billion Credits 984 billion Credits mimo-v2.5-pro 2.5 Credits 300 Credits 600 Credits mimo-v2.5 2 Credits 100 Credits 200 Credits 16:00–24:00 UTC 0.8x";
        assert!(validate_mimo_source(page).is_ok());
        for (old, new) in [
            ("300 Credits", "301 Credits"),
            ("4.1 billion", "4.2 billion"),
            ("$50/month", "$55/month"),
            ("1,056.00", "1,057.00"),
        ] {
            assert!(validate_mimo_source(&page.replace(old, new)).is_err());
        }
        let mut legacy = OfficialPriceCatalog::seeded();
        for card in &mut legacy.cards {
            if matches!(card.pricing, CardPricing::MimoTokenPlan { .. }) {
                card.source_url = LEGACY_MIMO_TOKEN_PLAN_SOURCE.into();
            }
        }
        legacy.validate().unwrap();
    }

    #[test]
    fn legacy_four_card_catalog_shape_remains_valid_without_inventing_new_rates() {
        let mut catalog = OfficialPriceCatalog::seeded();
        catalog.cards.retain(|card| {
            matches!(
                (card.provider.as_str(), card.model.as_str(), &card.pricing),
                (
                    "openai",
                    "gpt-5.6-sol" | "gpt-5.6-luna",
                    CardPricing::MeteredUsd { .. }
                ) | ("mimo", _, CardPricing::MimoTokenPlan { .. })
            )
        });

        catalog.validate().unwrap();
        assert!(catalog.find("deepseek", "deepseek-v4-flash").is_none());
        assert!(catalog.find("deepseek", "deepseek-v4-pro").is_none());
    }

    #[test]
    fn catalog_store_does_not_replace_valid_data_with_invalid_data() {
        let temp = tempfile::tempdir().unwrap();
        let store = PricingStore::new(temp.path().join("pricing/catalog.json"));
        let catalog = OfficialPriceCatalog::seeded();
        store.save(&catalog).unwrap();
        let original = std::fs::read(&store.path).unwrap();
        let mut invalid = catalog;
        invalid.cards.pop();
        assert!(store.save(&invalid).is_err());
        assert_eq!(std::fs::read(&store.path).unwrap(), original);
    }

    #[test]
    fn catalog_store_upgrades_v1_in_memory_without_rewriting_the_file() {
        let temp = tempfile::tempdir().unwrap();
        let store = PricingStore::new(temp.path().join("pricing/catalog.json"));
        let mut legacy = OfficialPriceCatalog::seeded();
        legacy.cards.retain(|card| {
            matches!(
                (card.provider.as_str(), card.model.as_str(), &card.pricing),
                (
                    "openai",
                    "gpt-5.6-sol" | "gpt-5.6-luna",
                    CardPricing::MeteredUsd { .. }
                ) | ("mimo", _, CardPricing::MimoTokenPlan { .. })
            )
        });
        let encoded = serde_json::to_vec_pretty(&legacy).unwrap();
        let encoded = String::from_utf8(encoded)
            .unwrap()
            .replace(CATALOG_SCHEMA, LEGACY_CATALOG_SCHEMA_V1);
        let mut decoded: OfficialPriceCatalog = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.schema, LEGACY_CATALOG_SCHEMA_V1);
        decoded.schema = CATALOG_SCHEMA.into();
        decoded.validate().unwrap();
        std::fs::create_dir_all(store.path.parent().unwrap()).unwrap();
        std::fs::write(&store.path, encoded.as_bytes()).unwrap();

        let upgraded = store.load().unwrap();
        assert_eq!(upgraded.schema, CATALOG_SCHEMA);
        assert!(String::from_utf8(std::fs::read(&store.path).unwrap())
            .unwrap()
            .contains(LEGACY_CATALOG_SCHEMA_V1));
    }

    #[test]
    fn catalog_store_upgrades_v2_legacy_write_rates_to_explicit_semantics() {
        let temp = tempfile::tempdir().unwrap();
        let store = PricingStore::new(temp.path().join("pricing/catalog.json"));
        let mut encoded = serde_json::to_value(OfficialPriceCatalog::seeded()).unwrap();
        encoded["schema"] = serde_json::Value::String(LEGACY_CATALOG_SCHEMA_V2.into());
        for card in encoded["cards"].as_array_mut().unwrap() {
            let provider = card["provider"].as_str().unwrap().to_string();
            let pricing = card["pricing"].as_object_mut().unwrap();
            let billing_mode = pricing["billing_mode"].as_str().unwrap().to_string();
            match billing_mode.as_str() {
                "metered_usd" => {
                    let billing = pricing.remove("cache_write_billing").unwrap();
                    let legacy_rate = if provider == "openai" {
                        billing["rate"].clone()
                    } else {
                        serde_json::Value::String("0".into())
                    };
                    pricing.insert("cache_write_input_per_million_usd".into(), legacy_rate);
                }
                "mimo_token_plan" => {
                    pricing.remove("cache_write_billing");
                }
                other => panic!("unexpected billing mode {other}"),
            }
        }
        let bytes = serde_json::to_vec_pretty(&encoded).unwrap();
        std::fs::create_dir_all(store.path.parent().unwrap()).unwrap();
        std::fs::write(&store.path, &bytes).unwrap();

        let upgraded = store.load().unwrap();
        assert_eq!(upgraded.schema, CATALOG_SCHEMA);
        for card in &upgraded.cards {
            match (&card.pricing, card.provider.as_str()) {
                (
                    CardPricing::MeteredUsd {
                        cache_write_billing,
                        ..
                    },
                    "openai",
                ) => assert!(matches!(
                    cache_write_billing,
                    CacheWriteBilling::Replacement { .. }
                )),
                (
                    CardPricing::MeteredUsd {
                        cache_write_billing,
                        ..
                    },
                    "mimo" | "deepseek",
                ) => assert_eq!(*cache_write_billing, CacheWriteBilling::Included),
                (
                    CardPricing::MimoTokenPlan {
                        cache_write_billing,
                        ..
                    },
                    "mimo",
                ) => assert_eq!(*cache_write_billing, CacheWriteBilling::Included),
                _ => panic!("unexpected card"),
            }
        }
        assert!(String::from_utf8(std::fs::read(&store.path).unwrap())
            .unwrap()
            .contains(LEGACY_CATALOG_SCHEMA_V2));
    }

    fn provider(base_url: &str, key: &str, models: &[&str]) -> crate::config::ProviderConfig {
        crate::config::ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: base_url.into(),
            api_key: key.into(),
            protocol: crate::agent::ProviderProtocol::ChatCompletions,
            prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
            models: models.iter().map(|model| (*model).into()).collect(),
            timeout: crate::config::ProviderTimeoutConfig::default(),
        }
    }

    #[test]
    fn router_enforces_capability_billing_freshness_and_deterministic_scoring() {
        let mut config = crate::config::CaConfig::default();
        config.providers.insert(
            "openai".into(),
            provider(
                "https://api.openai.com/v1",
                "sk-openai",
                &["gpt-5.6-sol", "gpt-5.6-luna"],
            ),
        );
        config.providers.insert(
            "mimo".into(),
            provider(
                MimoRegion::Singapore.token_plan_url(),
                "tp-mimo",
                &["mimo-v2.5", "mimo-v2.5-pro"],
            ),
        );
        config.provider_billing.insert("mimo".into(), token_plan());
        let now = DateTime::parse_from_rfc3339("2026-08-15T12:00:00Z")
            .unwrap()
            .to_utc();
        let router = DelegationRouter::from_config(&config, &OfficialPriceCatalog::seeded(), now);
        assert_eq!(
            router.eligible_routes(),
            [
                "mimo/mimo-v2.5",
                "mimo/mimo-v2.5-pro",
                "openai/gpt-5.6-luna",
                "openai/gpt-5.6-sol"
            ]
        );
        let simple = router
            .select(Complexity::Simple, "small edit", "bounded context")
            .unwrap();
        assert_eq!(
            (simple.provider.as_str(), simple.model.as_str()),
            ("mimo", "mimo-v2.5")
        );
        let standard = router
            .select(Complexity::Standard, "standard edit", "bounded context")
            .unwrap();
        assert_eq!(
            (standard.provider.as_str(), standard.model.as_str()),
            ("mimo", "mimo-v2.5-pro")
        );
        let complex = router
            .select(Complexity::Complex, "hard edit", "bounded context")
            .unwrap();
        assert_eq!(
            (complex.provider.as_str(), complex.model.as_str()),
            ("openai", "gpt-5.6-sol")
        );
    }

    #[test]
    fn router_fails_before_selection_for_missing_credentials_or_stale_catalog() {
        let mut config = crate::config::CaConfig::default();
        config.providers.insert(
            "openai".into(),
            provider(
                "https://api.openai.com/v1",
                "${OPENAI_API_KEY}",
                &["gpt-5.6-luna"],
            ),
        );
        config.providers.insert(
            "mimo".into(),
            provider("https://api.xiaomimimo.com/v1", "sk-payg", &["mimo-v2.5"]),
        );
        let stale = DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
            .unwrap()
            .to_utc();
        let router = DelegationRouter::from_config(&config, &OfficialPriceCatalog::seeded(), stale);
        assert!(router.eligible_routes().is_empty());
        assert!(router
            .select(Complexity::Simple, "task", "context")
            .is_err());

        let mut custom_endpoint = crate::config::CaConfig::default();
        custom_endpoint.providers.insert(
            "openai".into(),
            provider("https://proxy.example/v1", "sk-present", &["gpt-5.6-luna"]),
        );
        let router = DelegationRouter::from_config(
            &custom_endpoint,
            &OfficialPriceCatalog::seeded(),
            DateTime::parse_from_rfc3339("2026-08-15T12:00:00Z")
                .unwrap()
                .to_utc(),
        );
        assert!(router.eligible_routes().is_empty());
    }

    #[test]
    fn router_uses_mimo_payg_cards_only_on_the_official_payg_endpoint() {
        let now = DateTime::parse_from_rfc3339("2026-08-15T12:00:00Z")
            .unwrap()
            .to_utc();
        let mut config = crate::config::CaConfig::default();
        config.providers.clear();
        config.providers.insert(
            "mimo".into(),
            provider(
                MIMO_PAYG_BASE_URL,
                "sk-payg",
                &["mimo-v2.5", "mimo-v2.5-pro"],
            ),
        );
        let router = DelegationRouter::from_config(&config, &OfficialPriceCatalog::seeded(), now);
        assert_eq!(
            router.eligible_routes(),
            ["mimo/mimo-v2.5", "mimo/mimo-v2.5-pro"]
        );

        config.providers.get_mut("mimo").unwrap().base_url = "https://proxy.example/v1".into();
        let router = DelegationRouter::from_config(&config, &OfficialPriceCatalog::seeded(), now);
        assert!(router.eligible_routes().is_empty());
    }

    #[test]
    fn router_selects_deepseek_flash_and_rejects_custom_endpoints() {
        let now = DateTime::parse_from_rfc3339("2026-08-15T12:00:00Z")
            .unwrap()
            .to_utc();
        let mut config = crate::config::CaConfig::default();
        config.providers.clear();
        config.providers.insert(
            "deepseek".into(),
            provider(
                "https://api.deepseek.com/v1",
                "sk-deepseek",
                &["deepseek-v4-flash", "deepseek-v4-pro"],
            ),
        );
        let router = DelegationRouter::from_config(&config, &OfficialPriceCatalog::seeded(), now);
        assert_eq!(
            router.eligible_routes(),
            ["deepseek/deepseek-v4-flash", "deepseek/deepseek-v4-pro"]
        );
        let simple = router
            .select(Complexity::Simple, "small edit", "bounded context")
            .unwrap();
        assert_eq!(
            (simple.provider.as_str(), simple.model.as_str()),
            ("deepseek", "deepseek-v4-flash")
        );
        let standard = router
            .select(Complexity::Standard, "standard edit", "bounded context")
            .unwrap();
        assert_eq!(
            (standard.provider.as_str(), standard.model.as_str()),
            ("deepseek", "deepseek-v4-pro")
        );
        assert!(router
            .select(Complexity::Complex, "complex edit", "bounded context")
            .is_err());

        config.providers.get_mut("deepseek").unwrap().base_url = "https://proxy.example/v1".into();
        let router = DelegationRouter::from_config(&config, &OfficialPriceCatalog::seeded(), now);
        assert!(router.eligible_routes().is_empty());
    }

    #[test]
    fn repaired_pro_only_token_plan_route_is_eligible_for_simple_delegation() {
        let mut config = crate::config::CaConfig::default();
        config.providers.clear();
        config.providers.insert(
            "mimo".into(),
            provider(
                MimoRegion::Singapore.token_plan_url(),
                "tp-saved-token-plan",
                &["mimo-v2.5-pro"],
            ),
        );
        config.provider_billing.insert(
            "mimo".into(),
            ProviderBillingConfig::mimo_token_plan(
                MimoRegion::Singapore,
                MimoPlanTier::Pro,
                MimoPlanTerm::Annual,
                "528".parse().unwrap(),
                false,
            )
            .unwrap(),
        );
        let router = DelegationRouter::from_config(
            &config,
            &OfficialPriceCatalog::seeded(),
            DateTime::parse_from_rfc3339("2026-08-15T12:00:00Z")
                .unwrap()
                .to_utc(),
        );

        assert_eq!(router.eligible_routes(), ["mimo/mimo-v2.5-pro"]);
        let selected = router
            .select(Complexity::Simple, "return CHILD_OK", "bounded context")
            .unwrap();
        assert_eq!(selected.provider, "mimo");
        assert_eq!(selected.model, "mimo-v2.5-pro");
        assert!(selected
            .render()
            .contains("peak forecast; 20% off-peak not included"));
    }
}
