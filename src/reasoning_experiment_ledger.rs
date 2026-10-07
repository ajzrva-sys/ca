//! Crash-safe local spend authority for the RND-097 paid pilot.
//!
//! The ledger is deliberately synchronous. Callers must use it from blocking
//! work before a provider request is allowed to start. A reservation is
//! committed and fsynced before [`ExperimentLedger::reserve`] returns.

use crate::reasoning_experiment_protocol::{
    ExperimentModelRouteV1, ExperimentPhase, ExperimentRequestLimitsV1, ExperimentUsageV1,
    ProviderCallSpecV1, MAX_INPUT_TOKENS_PER_REQUEST, MAX_OUTPUT_TOKENS_PER_REQUEST,
    MAX_PHYSICAL_ATTEMPTS_PER_SEMANTIC_REQUEST,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

pub const LEDGER_HEADER_SCHEMA_V1: &str = "ca.reasoning_experiment_ledger.v1";
pub const LEDGER_EVENT_SCHEMA_V1: &str = "ca.reasoning_experiment_ledger_event.v1";
pub const FROZEN_PRICE_CARD_SCHEMA_V1: &str = "ca.reasoning_experiment_price_card.v1";
pub const DEFAULT_CAP_ATTO_USD: u128 = 50_000_000_000_000_000_000;
const MAX_LEDGER_EVENTS: usize = 100_000;
const MAX_EVENT_BYTES: u64 = 256 * 1024;
const MAX_REASON_CODE_BYTES: usize = 128;
const EVENT_FILE_DIGITS: usize = 20;
const EVENT_HASH_DOMAIN: &[u8] = b"ca-rnd-ledger-event-v1\0";
const RESERVATION_ID_DOMAIN: &[u8] = b"ca-rnd-reservation-v1\0";
const BINDING_HASH_DOMAIN: &[u8] = b"ca-rnd-ledger-binding-v1\0";
const PRICE_CARD_HASH_DOMAIN: &[u8] = b"ca-rnd-price-card-v1\0";

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct AttoUsd(u128);

impl AttoUsd {
    pub const ZERO: Self = Self(0);
    pub const DEFAULT_CAP: Self = Self(DEFAULT_CAP_ATTO_USD);

    pub const fn from_atto(value: u128) -> Self {
        Self(value)
    }

    pub const fn as_atto(self) -> u128 {
        self.0
    }

    pub fn parse_usd(value: &str) -> Result<Self, LedgerError> {
        if value.is_empty() || value.starts_with('-') || value.starts_with('+') {
            return Err(LedgerError::InvalidMoney);
        }
        let mut pieces = value.split('.');
        let whole = pieces.next().ok_or(LedgerError::InvalidMoney)?;
        let fraction = pieces.next();
        if pieces.next().is_some()
            || whole.is_empty()
            || !whole.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(LedgerError::InvalidMoney);
        }
        let whole = whole
            .parse::<u128>()
            .map_err(|_| LedgerError::InvalidMoney)?;
        let whole = whole
            .checked_mul(1_000_000_000_000_000_000)
            .ok_or(LedgerError::MoneyOverflow)?;
        let fraction = match fraction {
            None => 0,
            Some(value)
                if !value.is_empty()
                    && value.len() <= 18
                    && value.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                let parsed = value
                    .parse::<u128>()
                    .map_err(|_| LedgerError::InvalidMoney)?;
                parsed
                    .checked_mul(10u128.pow((18 - value.len()) as u32))
                    .ok_or(LedgerError::MoneyOverflow)?
            }
            Some(_) => return Err(LedgerError::InvalidMoney),
        };
        whole
            .checked_add(fraction)
            .map(Self)
            .ok_or(LedgerError::MoneyOverflow)
    }

    pub fn checked_add(self, other: Self) -> Result<Self, LedgerError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(LedgerError::MoneyOverflow)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, LedgerError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(LedgerError::State(
                "ledger monetary subtraction underflow".into(),
            ))
    }
}

impl fmt::Debug for AttoUsd {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for AttoUsd {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / 1_000_000_000_000_000_000;
        let fraction = self.0 % 1_000_000_000_000_000_000;
        write!(formatter, "{whole}.{fraction:018}")
    }
}

impl Serialize for AttoUsd {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for AttoUsd {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.is_empty()
            || (value.len() > 1 && value.starts_with('0'))
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(serde::de::Error::custom(
                "atto-USD must be a canonical unsigned decimal string",
            ));
        }
        value
            .parse::<u128>()
            .map(Self)
            .map_err(|_| serde::de::Error::custom("atto-USD is out of range"))
    }
}

impl AttoUsd {
    fn checked_per_million_tokens(self, tokens: u64) -> Result<Self, LedgerError> {
        const TOKENS_PER_MILLION: u128 = 1_000_000;
        if !self.0.is_multiple_of(TOKENS_PER_MILLION) {
            return Err(LedgerError::InvalidMoney);
        }
        self.0
            .checked_mul(u128::from(tokens))
            .map(|value| Self(value / TOKENS_PER_MILLION))
            .ok_or(LedgerError::MoneyOverflow)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FrozenModelPriceV1 {
    pub regular_input_per_million_atto_usd: AttoUsd,
    pub cached_input_per_million_atto_usd: AttoUsd,
    pub cache_write_per_million_atto_usd: AttoUsd,
    pub regular_output_per_million_atto_usd: AttoUsd,
    pub reasoning_output_per_million_atto_usd: AttoUsd,
}

impl FrozenModelPriceV1 {
    fn validate(&self) -> Result<(), LedgerError> {
        for rate in [
            self.regular_input_per_million_atto_usd,
            self.cached_input_per_million_atto_usd,
            self.cache_write_per_million_atto_usd,
            self.regular_output_per_million_atto_usd,
            self.reasoning_output_per_million_atto_usd,
        ] {
            if rate == AttoUsd::ZERO || rate.as_atto() % 1_000_000 != 0 {
                return Err(LedgerError::InvalidMoney);
            }
        }
        Ok(())
    }

    fn maximum_input_rate(&self) -> AttoUsd {
        self.regular_input_per_million_atto_usd
            .max(self.cached_input_per_million_atto_usd)
            .max(self.cache_write_per_million_atto_usd)
    }

    fn maximum_output_rate(&self) -> AttoUsd {
        self.regular_output_per_million_atto_usd
            .max(self.reasoning_output_per_million_atto_usd)
    }

    fn exact_cost(&self, usage: ExperimentUsageV1) -> Result<AttoUsd, LedgerError> {
        usage
            .validate()
            .map_err(|error| LedgerError::InvalidBinding(error.to_string()))?;
        self.regular_input_per_million_atto_usd
            .checked_per_million_tokens(usage.regular_input_tokens)?
            .checked_add(
                self.cached_input_per_million_atto_usd
                    .checked_per_million_tokens(usage.cached_input_tokens)?,
            )?
            .checked_add(
                self.cache_write_per_million_atto_usd
                    .checked_per_million_tokens(usage.cache_write_tokens)?,
            )?
            .checked_add(
                self.regular_output_per_million_atto_usd
                    .checked_per_million_tokens(usage.regular_output_tokens)?,
            )?
            .checked_add(
                self.reasoning_output_per_million_atto_usd
                    .checked_per_million_tokens(usage.reasoning_output_tokens)?,
            )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FrozenPriceCardV1 {
    pub schema_version: String,
    pub price_card_id: String,
    pub effective_at_rfc3339: String,
    pub service_tier: String,
    pub sol: FrozenModelPriceV1,
    pub luna: FrozenModelPriceV1,
}

impl FrozenPriceCardV1 {
    pub fn validate(&self) -> Result<(), LedgerError> {
        if self.schema_version != FROZEN_PRICE_CARD_SCHEMA_V1 {
            return Err(LedgerError::InvalidBinding(
                "price_card.schema_version".into(),
            ));
        }
        validate_id("price_card_id", &self.price_card_id)?;
        chrono::DateTime::parse_from_rfc3339(&self.effective_at_rfc3339)
            .map_err(|_| LedgerError::InvalidBinding("price_card.effective_at_rfc3339".into()))?;
        if self.service_tier != "default" {
            return Err(LedgerError::InvalidBinding(
                "price_card.service_tier".into(),
            ));
        }
        self.sol.validate()?;
        self.luna.validate()
    }

    fn for_route(
        &self,
        route: &ExperimentModelRouteV1,
    ) -> Result<&FrozenModelPriceV1, LedgerError> {
        self.validate()?;
        route
            .validate()
            .map_err(|error| LedgerError::InvalidBinding(error.to_string()))?;
        match route.model.as_str() {
            crate::reasoning_experiment_protocol::STRONG_MODEL => Ok(&self.sol),
            crate::reasoning_experiment_protocol::CHEAP_MODEL => Ok(&self.luna),
            _ => Err(LedgerError::InvalidBinding("price_card.route".into())),
        }
    }

    pub fn worst_case_for_route(
        &self,
        route: &ExperimentModelRouteV1,
    ) -> Result<AttoUsd, LedgerError> {
        let price = self.for_route(route)?;
        price
            .maximum_input_rate()
            .checked_per_million_tokens(MAX_INPUT_TOKENS_PER_REQUEST)?
            .checked_add(
                price
                    .maximum_output_rate()
                    .checked_per_million_tokens(MAX_OUTPUT_TOKENS_PER_REQUEST)?,
            )
    }

    pub fn exact_cost_for_usage(
        &self,
        route: &ExperimentModelRouteV1,
        usage: ExperimentUsageV1,
    ) -> Result<AttoUsd, LedgerError> {
        self.for_route(route)?.exact_cost(usage)
    }

    pub fn sha256(&self) -> Result<String, LedgerError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(json_error)?;
        Ok(domain_sha256(PRICE_CARD_HASH_DOMAIN, &bytes))
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LedgerHeaderV1 {
    pub schema_version: String,
    pub experiment_id: String,
    pub preregistration_sha256: String,
    pub price_card: FrozenPriceCardV1,
    pub price_card_sha256: String,
    pub cap_atto_usd: AttoUsd,
    pub created_at: DateTime<Utc>,
}

impl fmt::Debug for LedgerHeaderV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LedgerHeaderV1")
            .field("schema_version", &self.schema_version)
            .field("experiment_id", &self.experiment_id)
            .field("preregistration_sha256", &self.preregistration_sha256)
            .field("price_card_sha256", &self.price_card_sha256)
            .field("cap_atto_usd", &self.cap_atto_usd)
            .field("created_at", &self.created_at)
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LedgerBindingV1 {
    pub experiment_id: String,
    pub preregistration_sha256: String,
    pub trial_id: String,
    pub slot_index: u16,
    pub phase: ExperimentPhase,
    pub semantic_request_index: u16,
    pub physical_attempt: u8,
    pub route: ExperimentModelRouteV1,
    pub price_card_sha256: String,
    pub request_sha256: String,
    pub client_request_id_sha256: String,
    pub controller_sha256: String,
    pub worker_sha256: String,
}

impl fmt::Debug for LedgerBindingV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LedgerBindingV1")
            .field("experiment_id", &self.experiment_id)
            .field("trial_id", &self.trial_id)
            .field("slot_index", &self.slot_index)
            .field("phase", &self.phase)
            .field("semantic_request_index", &self.semantic_request_index)
            .field("physical_attempt", &self.physical_attempt)
            .field("route", &self.route)
            .field("price_card_sha256", &self.price_card_sha256)
            .field("request_sha256", &self.request_sha256)
            .field("client_request_id_sha256", &self.client_request_id_sha256)
            .field("controller_sha256", &self.controller_sha256)
            .field("worker_sha256", &self.worker_sha256)
            .finish_non_exhaustive()
    }
}

impl LedgerBindingV1 {
    fn from_provider_call(
        call: &ProviderCallSpecV1,
        price_card: &FrozenPriceCardV1,
    ) -> Result<Self, LedgerError> {
        call.validate()
            .map_err(|error| LedgerError::InvalidBinding(error.to_string()))?;
        let price_card_sha256 = price_card.sha256()?;
        Ok(Self {
            experiment_id: call.experiment_id.clone(),
            preregistration_sha256: call.preregistration_sha256.clone(),
            trial_id: call.trial_id.clone(),
            slot_index: call.slot_index,
            phase: call.phase,
            semantic_request_index: call.semantic_request_index,
            physical_attempt: call.physical_attempt,
            route: call.route.clone(),
            price_card_sha256,
            request_sha256: call.request_sha256.clone(),
            client_request_id_sha256: raw_sha256(call.x_client_request_id.as_bytes()),
            controller_sha256: call.controller_sha256.clone(),
            worker_sha256: call.worker_sha256.clone(),
        })
    }

    pub fn validate(&self) -> Result<(), LedgerError> {
        validate_id("experiment_id", &self.experiment_id)?;
        validate_sha256("preregistration_sha256", &self.preregistration_sha256)?;
        validate_id("trial_id", &self.trial_id)?;
        if self.slot_index >= 72 {
            return Err(LedgerError::InvalidBinding("slot_index".into()));
        }
        self.route
            .validate()
            .map_err(|_| LedgerError::InvalidBinding("route".into()))?;
        let limits = ExperimentRequestLimitsV1::for_phase(self.phase);
        if self.semantic_request_index >= limits.max_semantic_requests {
            return Err(LedgerError::InvalidBinding("semantic_request_index".into()));
        }
        if !(1..=MAX_PHYSICAL_ATTEMPTS_PER_SEMANTIC_REQUEST).contains(&self.physical_attempt) {
            return Err(LedgerError::InvalidBinding("physical_attempt".into()));
        }
        for (field, value) in [
            ("request_sha256", self.request_sha256.as_str()),
            ("price_card_sha256", self.price_card_sha256.as_str()),
            (
                "client_request_id_sha256",
                self.client_request_id_sha256.as_str(),
            ),
            ("controller_sha256", self.controller_sha256.as_str()),
            ("worker_sha256", self.worker_sha256.as_str()),
        ] {
            validate_sha256(field, value)?;
        }
        Ok(())
    }

    pub fn sha256(&self) -> Result<String, LedgerError> {
        self.validate()?;
        let bytes =
            serde_json::to_vec(self).map_err(|error| LedgerError::Json(error.to_string()))?;
        Ok(domain_sha256(BINDING_HASH_DOMAIN, &bytes))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LedgerEventKindV1 {
    RequestReserved {
        reservation_id: String,
        binding_sha256: String,
        reserved_atto_usd: AttoUsd,
    },
    RequestSettled {
        reservation_id: String,
        binding_sha256: String,
        actual_atto_usd: AttoUsd,
        provider_receipt_sha256: String,
    },
    RequestUnknown {
        reservation_id: String,
        binding_sha256: String,
        reason_code: String,
    },
    RequestReleasedPreSend {
        reservation_id: String,
        binding_sha256: String,
        reason_code: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LedgerEventPayloadV1 {
    pub schema_version: String,
    pub sequence: u64,
    pub previous_event_sha256: Option<String>,
    pub created_at: DateTime<Utc>,
    pub binding: LedgerBindingV1,
    pub event: LedgerEventKindV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LedgerEventFileV1 {
    pub payload: LedgerEventPayloadV1,
    pub event_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservationGrantV1 {
    pub reservation_id: String,
    pub binding_sha256: String,
    pub reserved_atto_usd: AttoUsd,
    pub ledger_event_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutstandingReservationV1 {
    pub reservation_id: String,
    pub binding_sha256: String,
    pub reserved_atto_usd: AttoUsd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerSnapshotV1 {
    pub cap_atto_usd: AttoUsd,
    pub settled_actual_atto_usd: AttoUsd,
    pub unknown_reserved_atto_usd: AttoUsd,
    pub outstanding_reserved_atto_usd: AttoUsd,
    pub available_atto_usd: AttoUsd,
    pub event_count: u64,
    pub last_event_sha256: Option<String>,
    pub outstanding: Vec<OutstandingReservationV1>,
}

/// Audited spend attributed to the preregistered trial slot carried by every
/// immutable ledger binding. Unknown and outstanding calls are charged at
/// their full reservation so incomplete trials cannot disappear from arm
/// economics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerSlotSpendV1 {
    pub slot_index: u16,
    pub settled_actual_atto_usd: AttoUsd,
    pub unknown_reserved_atto_usd: AttoUsd,
    pub outstanding_reserved_atto_usd: AttoUsd,
    pub settled_requests: u64,
    pub unknown_requests: u64,
    pub outstanding_requests: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerAttributedSnapshotV1 {
    pub totals: LedgerSnapshotV1,
    pub slots: Vec<LedgerSlotSpendV1>,
}

#[derive(Debug, Clone)]
struct ActiveReservation {
    binding_sha256: String,
    reserved_atto_usd: AttoUsd,
    slot_index: u16,
}

#[derive(Debug, Clone, Default)]
struct TerminalSlotSpend {
    settled_actual_atto_usd: AttoUsd,
    unknown_reserved_atto_usd: AttoUsd,
    settled_requests: u64,
    unknown_requests: u64,
}

#[derive(Debug)]
struct LoadedState {
    settled: AttoUsd,
    unknown: AttoUsd,
    outstanding: BTreeMap<String, ActiveReservation>,
    terminal_by_slot: BTreeMap<u16, TerminalSlotSpend>,
    seen_bindings: BTreeSet<String>,
    next_sequence: u64,
    last_event_sha256: Option<String>,
}

impl Default for LoadedState {
    fn default() -> Self {
        Self {
            settled: AttoUsd::ZERO,
            unknown: AttoUsd::ZERO,
            outstanding: BTreeMap::new(),
            terminal_by_slot: BTreeMap::new(),
            seen_bindings: BTreeSet::new(),
            next_sequence: 0,
            last_event_sha256: None,
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum LedgerError {
    #[error("the RND-097 ledger is supported only on Unix")]
    UnsupportedPlatform,
    #[error("ledger I/O failed: {0}")]
    Io(String),
    #[error("ledger JSON is invalid: {0}")]
    Json(String),
    #[error("ledger state is invalid: {0}")]
    State(String),
    #[error("ledger identifier or digest is invalid: {0}")]
    InvalidBinding(String),
    #[error("invalid USD decimal")]
    InvalidMoney,
    #[error("USD arithmetic overflow")]
    MoneyOverflow,
    #[error("the requested reservation would exceed the USD 50 ledger cap")]
    CapExceeded,
    #[error("the physical request already has a ledger event")]
    DuplicateRequest,
    #[error("the reservation does not exist or is no longer outstanding")]
    MissingReservation,
    #[error("actual provider cost exceeds the reserved maximum")]
    SettlementExceedsReservation,
    #[error("another process currently owns the experiment ledger")]
    Busy,
}

pub struct ExperimentLedger {
    root: PathBuf,
    events: PathBuf,
    header: LedgerHeaderV1,
    #[cfg(unix)]
    root_identity: FileIdentity,
    #[cfg(unix)]
    events_identity: FileIdentity,
}

impl fmt::Debug for ExperimentLedger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExperimentLedger")
            .field("root", &"[PRIVATE]")
            .field("experiment_id", &self.header.experiment_id)
            .field(
                "preregistration_sha256",
                &self.header.preregistration_sha256,
            )
            .field("cap_atto_usd", &self.header.cap_atto_usd)
            .finish()
    }
}

impl ExperimentLedger {
    pub fn create(
        root: &Path,
        experiment_id: &str,
        preregistration_sha256: &str,
        cap_atto_usd: AttoUsd,
        price_card: FrozenPriceCardV1,
    ) -> Result<Self, LedgerError> {
        #[cfg(not(unix))]
        {
            let _ = (
                root,
                experiment_id,
                preregistration_sha256,
                cap_atto_usd,
                price_card,
            );
            return Err(LedgerError::UnsupportedPlatform);
        }
        #[cfg(unix)]
        {
            validate_id("experiment_id", experiment_id)?;
            validate_sha256("preregistration_sha256", preregistration_sha256)?;
            if cap_atto_usd == AttoUsd::ZERO || cap_atto_usd > AttoUsd::DEFAULT_CAP {
                return Err(LedgerError::CapExceeded);
            }
            let price_card_sha256 = price_card.sha256()?;
            fs::create_dir(root).map_err(io_error)?;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
            let events = root.join("events");
            fs::create_dir(&events).map_err(io_error)?;
            fs::set_permissions(&events, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
            let header = LedgerHeaderV1 {
                schema_version: LEDGER_HEADER_SCHEMA_V1.into(),
                experiment_id: experiment_id.into(),
                preregistration_sha256: preregistration_sha256.into(),
                price_card,
                price_card_sha256,
                cap_atto_usd,
                created_at: Utc::now(),
            };
            let root_file = open_directory_nofollow(root)?;
            write_create_new_at(
                &root_file,
                "ledger.json",
                &serde_json::to_vec(&header).map_err(json_error)?,
            )?;
            write_create_new_at(&root_file, "ledger.lock", b"")?;
            root_file.sync_all().map_err(io_error)?;
            Self::open(root)
        }
    }

    pub fn open(root: &Path) -> Result<Self, LedgerError> {
        #[cfg(not(unix))]
        {
            let _ = root;
            return Err(LedgerError::UnsupportedPlatform);
        }
        #[cfg(unix)]
        {
            let root_file = open_directory_nofollow(root)?;
            let root_identity = validate_private_directory(&root_file)?;
            let events = root.join("events");
            let events_file = open_directory_nofollow(&events)?;
            let events_identity = validate_private_directory(&events_file)?;
            let header_bytes = read_regular_at(&root_file, "ledger.json", MAX_EVENT_BYTES)?;
            let header: LedgerHeaderV1 =
                serde_json::from_slice(&header_bytes).map_err(json_error)?;
            validate_header(&header)?;
            let ledger = Self {
                root: root.to_path_buf(),
                events,
                header,
                root_identity,
                events_identity,
            };
            ledger.verify_directory_identities()?;
            let _ = ledger.snapshot()?;
            Ok(ledger)
        }
    }

    pub fn header(&self) -> &LedgerHeaderV1 {
        &self.header
    }

    pub fn price_card(&self) -> &FrozenPriceCardV1 {
        &self.header.price_card
    }

    pub fn snapshot(&self) -> Result<LedgerSnapshotV1, LedgerError> {
        let _guard = self.lock()?;
        self.cleanup_uncommitted_temps()?;
        let state = self.load_state()?;
        self.snapshot_from_state(&state)
    }

    /// Returns the aggregate cap view and its exact per-slot decomposition
    /// under one ledger lock. This is the reporting boundary for attributing
    /// spend from completed, failed, interrupted, and ambiguous trials.
    pub fn attributed_snapshot(&self) -> Result<LedgerAttributedSnapshotV1, LedgerError> {
        let _guard = self.lock()?;
        self.cleanup_uncommitted_temps()?;
        let state = self.load_state()?;
        let totals = self.snapshot_from_state(&state)?;
        let slots = self.slot_spend_from_state(&state)?;
        Ok(LedgerAttributedSnapshotV1 { totals, slots })
    }

    pub fn reserve(
        &self,
        binding: LedgerBindingV1,
        maximum_cost: AttoUsd,
    ) -> Result<ReservationGrantV1, LedgerError> {
        binding.validate()?;
        self.validate_binding_header(&binding)?;
        if maximum_cost == AttoUsd::ZERO {
            return Err(LedgerError::InvalidMoney);
        }
        let binding_sha256 = binding.sha256()?;
        let _guard = self.lock()?;
        self.cleanup_uncommitted_temps()?;
        let state = self.load_state()?;
        if state.seen_bindings.contains(&binding_sha256) {
            return Err(LedgerError::DuplicateRequest);
        }
        let committed = total_committed(&state)?;
        if committed.checked_add(maximum_cost)? > self.header.cap_atto_usd {
            return Err(LedgerError::CapExceeded);
        }
        let reservation_id = reservation_id(
            &self.header.preregistration_sha256,
            state.next_sequence,
            &binding_sha256,
        );
        let event = LedgerEventKindV1::RequestReserved {
            reservation_id: reservation_id.clone(),
            binding_sha256: binding_sha256.clone(),
            reserved_atto_usd: maximum_cost,
        };
        let event_sha256 = self.append_event(&state, binding, event)?;
        Ok(ReservationGrantV1 {
            reservation_id,
            binding_sha256,
            reserved_atto_usd: maximum_cost,
            ledger_event_sha256: event_sha256,
        })
    }

    /// Reserve the worst permitted cost before one physical broker POST. The
    /// caller must settle the returned grant or mark it unknown; this helper
    /// intentionally does not issue network traffic.
    pub fn reserve_provider_call(
        &self,
        call: &ProviderCallSpecV1,
    ) -> Result<ReservationGrantV1, LedgerError> {
        let binding = self.provider_call_binding(call)?;
        let maximum = self.header.price_card.worst_case_for_route(&call.route)?;
        self.reserve(binding, maximum)
    }

    pub fn settle_provider_call(
        &self,
        grant: &ReservationGrantV1,
        call: &ProviderCallSpecV1,
        usage: ExperimentUsageV1,
        provider_receipt_sha256: &str,
    ) -> Result<String, LedgerError> {
        let binding = self.provider_call_binding(call)?;
        let actual = self
            .header
            .price_card
            .exact_cost_for_usage(&call.route, usage)?;
        self.settle(grant, binding, actual, provider_receipt_sha256)
    }

    pub fn mark_provider_call_unknown(
        &self,
        grant: &ReservationGrantV1,
        call: &ProviderCallSpecV1,
        reason_code: &str,
    ) -> Result<String, LedgerError> {
        self.mark_unknown(grant, self.provider_call_binding(call)?, reason_code)
    }

    pub fn release_provider_call_pre_send(
        &self,
        grant: &ReservationGrantV1,
        call: &ProviderCallSpecV1,
        reason_code: &str,
    ) -> Result<String, LedgerError> {
        self.release_verified_pre_send(grant, self.provider_call_binding(call)?, reason_code)
    }

    pub fn settle(
        &self,
        grant: &ReservationGrantV1,
        binding: LedgerBindingV1,
        actual_cost: AttoUsd,
        provider_receipt_sha256: &str,
    ) -> Result<String, LedgerError> {
        validate_sha256("provider_receipt_sha256", provider_receipt_sha256)?;
        let binding_sha256 = self.validate_terminal_binding(grant, &binding)?;
        let _guard = self.lock()?;
        self.cleanup_uncommitted_temps()?;
        let state = self.load_state()?;
        let active = state
            .outstanding
            .get(&grant.reservation_id)
            .ok_or(LedgerError::MissingReservation)?;
        if active.binding_sha256 != binding_sha256
            || active.reserved_atto_usd != grant.reserved_atto_usd
        {
            return Err(LedgerError::MissingReservation);
        }
        if actual_cost > active.reserved_atto_usd {
            return Err(LedgerError::SettlementExceedsReservation);
        }
        self.append_event(
            &state,
            binding,
            LedgerEventKindV1::RequestSettled {
                reservation_id: grant.reservation_id.clone(),
                binding_sha256,
                actual_atto_usd: actual_cost,
                provider_receipt_sha256: provider_receipt_sha256.into(),
            },
        )
    }

    pub fn mark_unknown(
        &self,
        grant: &ReservationGrantV1,
        binding: LedgerBindingV1,
        reason_code: &str,
    ) -> Result<String, LedgerError> {
        validate_reason_code(reason_code)?;
        self.append_terminal(grant, binding, |reservation_id, binding_sha256| {
            LedgerEventKindV1::RequestUnknown {
                reservation_id,
                binding_sha256,
                reason_code: reason_code.into(),
            }
        })
    }

    pub fn release_verified_pre_send(
        &self,
        grant: &ReservationGrantV1,
        binding: LedgerBindingV1,
        reason_code: &str,
    ) -> Result<String, LedgerError> {
        validate_reason_code(reason_code)?;
        self.append_terminal(grant, binding, |reservation_id, binding_sha256| {
            LedgerEventKindV1::RequestReleasedPreSend {
                reservation_id,
                binding_sha256,
                reason_code: reason_code.into(),
            }
        })
    }

    fn append_terminal<F>(
        &self,
        grant: &ReservationGrantV1,
        binding: LedgerBindingV1,
        event: F,
    ) -> Result<String, LedgerError>
    where
        F: FnOnce(String, String) -> LedgerEventKindV1,
    {
        let binding_sha256 = self.validate_terminal_binding(grant, &binding)?;
        let _guard = self.lock()?;
        self.cleanup_uncommitted_temps()?;
        let state = self.load_state()?;
        let active = state
            .outstanding
            .get(&grant.reservation_id)
            .ok_or(LedgerError::MissingReservation)?;
        if active.binding_sha256 != binding_sha256
            || active.reserved_atto_usd != grant.reserved_atto_usd
        {
            return Err(LedgerError::MissingReservation);
        }
        self.append_event(
            &state,
            binding,
            event(grant.reservation_id.clone(), binding_sha256),
        )
    }

    fn validate_terminal_binding(
        &self,
        grant: &ReservationGrantV1,
        binding: &LedgerBindingV1,
    ) -> Result<String, LedgerError> {
        binding.validate()?;
        self.validate_binding_header(binding)?;
        validate_id("reservation_id", &grant.reservation_id)?;
        validate_sha256("binding_sha256", &grant.binding_sha256)?;
        validate_sha256("ledger_event_sha256", &grant.ledger_event_sha256)?;
        let binding_sha256 = binding.sha256()?;
        if binding_sha256 != grant.binding_sha256 {
            return Err(LedgerError::MissingReservation);
        }
        Ok(binding_sha256)
    }

    fn validate_binding_header(&self, binding: &LedgerBindingV1) -> Result<(), LedgerError> {
        if binding.experiment_id != self.header.experiment_id
            || binding.preregistration_sha256 != self.header.preregistration_sha256
            || binding.price_card_sha256 != self.header.price_card_sha256
        {
            return Err(LedgerError::InvalidBinding("ledger_header".into()));
        }
        Ok(())
    }

    fn provider_call_binding(
        &self,
        call: &ProviderCallSpecV1,
    ) -> Result<LedgerBindingV1, LedgerError> {
        LedgerBindingV1::from_provider_call(call, &self.header.price_card)
    }

    fn append_event(
        &self,
        state: &LoadedState,
        binding: LedgerBindingV1,
        event: LedgerEventKindV1,
    ) -> Result<String, LedgerError> {
        let payload = LedgerEventPayloadV1 {
            schema_version: LEDGER_EVENT_SCHEMA_V1.into(),
            sequence: state.next_sequence,
            previous_event_sha256: state.last_event_sha256.clone(),
            created_at: Utc::now(),
            binding,
            event,
        };
        let canonical = serde_json::to_vec(&payload).map_err(json_error)?;
        if canonical.len() as u64 > MAX_EVENT_BYTES {
            return Err(LedgerError::State("ledger event exceeds byte cap".into()));
        }
        let event_sha256 = domain_sha256(EVENT_HASH_DOMAIN, &canonical);
        let wrapper = LedgerEventFileV1 {
            payload,
            event_sha256: event_sha256.clone(),
        };
        let bytes = serde_json::to_vec(&wrapper).map_err(json_error)?;
        self.atomic_append_event(state.next_sequence, &bytes)?;
        Ok(event_sha256)
    }

    fn load_state(&self) -> Result<LoadedState, LedgerError> {
        self.verify_directory_identities()?;
        let mut names = Vec::new();
        for entry in fs::read_dir(&self.events).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let file_name = entry.file_name();
            let name = file_name
                .to_str()
                .ok_or_else(|| LedgerError::State("non-UTF-8 ledger event name".into()))?;
            if is_event_file_name(name) {
                names.push(name.to_string());
            } else if !is_temp_file_name(name) {
                return Err(LedgerError::State(format!(
                    "unexpected ledger event entry: {name}"
                )));
            }
        }
        if names.len() > MAX_LEDGER_EVENTS {
            return Err(LedgerError::State("ledger event count exceeds cap".into()));
        }
        names.sort();
        self.verify_directory_identities()?;

        let events_file = open_directory_nofollow(&self.events)?;
        let mut state = LoadedState::default();
        for name in names {
            let expected_name = event_file_name(state.next_sequence);
            if name != expected_name {
                return Err(LedgerError::State(
                    "ledger event sequence is missing or reordered".into(),
                ));
            }
            let bytes = read_regular_at(&events_file, &name, MAX_EVENT_BYTES)?;
            let wrapper: LedgerEventFileV1 = serde_json::from_slice(&bytes).map_err(json_error)?;
            validate_event_file(&wrapper, &state)?;
            apply_event(&mut state, &wrapper.payload)?;
            state.last_event_sha256 = Some(wrapper.event_sha256);
            state.next_sequence = state
                .next_sequence
                .checked_add(1)
                .ok_or_else(|| LedgerError::State("ledger sequence overflow".into()))?;
            if total_committed(&state)? > self.header.cap_atto_usd {
                return Err(LedgerError::State(
                    "historical ledger events exceed the configured cap".into(),
                ));
            }
        }
        Ok(state)
    }

    fn snapshot_from_state(&self, state: &LoadedState) -> Result<LedgerSnapshotV1, LedgerError> {
        let outstanding_reserved = state
            .outstanding
            .values()
            .try_fold(AttoUsd::ZERO, |total, item| {
                total.checked_add(item.reserved_atto_usd)
            })?;
        let committed = state
            .settled
            .checked_add(state.unknown)?
            .checked_add(outstanding_reserved)?;
        let available = self.header.cap_atto_usd.checked_sub(committed)?;
        Ok(LedgerSnapshotV1 {
            cap_atto_usd: self.header.cap_atto_usd,
            settled_actual_atto_usd: state.settled,
            unknown_reserved_atto_usd: state.unknown,
            outstanding_reserved_atto_usd: outstanding_reserved,
            available_atto_usd: available,
            event_count: state.next_sequence,
            last_event_sha256: state.last_event_sha256.clone(),
            outstanding: state
                .outstanding
                .iter()
                .map(|(reservation_id, active)| OutstandingReservationV1 {
                    reservation_id: reservation_id.clone(),
                    binding_sha256: active.binding_sha256.clone(),
                    reserved_atto_usd: active.reserved_atto_usd,
                })
                .collect(),
        })
    }

    fn slot_spend_from_state(
        &self,
        state: &LoadedState,
    ) -> Result<Vec<LedgerSlotSpendV1>, LedgerError> {
        let mut slots = state
            .terminal_by_slot
            .iter()
            .map(|(&slot_index, spend)| {
                (
                    slot_index,
                    LedgerSlotSpendV1 {
                        slot_index,
                        settled_actual_atto_usd: spend.settled_actual_atto_usd,
                        unknown_reserved_atto_usd: spend.unknown_reserved_atto_usd,
                        outstanding_reserved_atto_usd: AttoUsd::ZERO,
                        settled_requests: spend.settled_requests,
                        unknown_requests: spend.unknown_requests,
                        outstanding_requests: 0,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        for active in state.outstanding.values() {
            let slot = slots.entry(active.slot_index).or_insert(LedgerSlotSpendV1 {
                slot_index: active.slot_index,
                settled_actual_atto_usd: AttoUsd::ZERO,
                unknown_reserved_atto_usd: AttoUsd::ZERO,
                outstanding_reserved_atto_usd: AttoUsd::ZERO,
                settled_requests: 0,
                unknown_requests: 0,
                outstanding_requests: 0,
            });
            slot.outstanding_reserved_atto_usd = slot
                .outstanding_reserved_atto_usd
                .checked_add(active.reserved_atto_usd)?;
            slot.outstanding_requests = slot
                .outstanding_requests
                .checked_add(1)
                .ok_or(LedgerError::MoneyOverflow)?;
        }
        Ok(slots.into_values().collect())
    }

    #[cfg(unix)]
    fn lock(&self) -> Result<LedgerLock, LedgerError> {
        self.verify_directory_identities()?;
        let root_file = open_directory_nofollow(&self.root)?;
        let file = open_regular_at(&root_file, "ledger.lock", true)?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if result != 0 {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        self.verify_directory_identities()?;
        Ok(LedgerLock { file })
    }

    #[cfg(not(unix))]
    fn lock(&self) -> Result<LedgerLock, LedgerError> {
        Err(LedgerError::UnsupportedPlatform)
    }

    #[cfg(unix)]
    fn atomic_append_event(&self, sequence: u64, bytes: &[u8]) -> Result<(), LedgerError> {
        self.verify_directory_identities()?;
        let events_file = open_directory_nofollow(&self.events)?;
        let token = format!(
            "{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        let temp_name = format!(".tmp-{sequence:020}-{token}");
        let final_name = event_file_name(sequence);
        write_create_new_at(&events_file, &temp_name, bytes)?;
        let temp = c_string(&temp_name)?;
        let final_path = c_string(&final_name)?;
        let result = unsafe {
            libc::renameat(
                events_file.as_raw_fd(),
                temp.as_ptr(),
                events_file.as_raw_fd(),
                final_path.as_ptr(),
            )
        };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            let _ = unlink_at(&events_file, &temp_name);
            return Err(io_error(error));
        }
        events_file.sync_all().map_err(io_error)
    }

    #[cfg(not(unix))]
    fn atomic_append_event(&self, _sequence: u64, _bytes: &[u8]) -> Result<(), LedgerError> {
        Err(LedgerError::UnsupportedPlatform)
    }

    fn cleanup_uncommitted_temps(&self) -> Result<(), LedgerError> {
        self.verify_directory_identities()?;
        let events_file = open_directory_nofollow(&self.events)?;
        let mut temps = Vec::new();
        for entry in fs::read_dir(&self.events).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| LedgerError::State("non-UTF-8 ledger temp name".into()))?;
            if is_temp_file_name(&name) {
                let file = open_regular_at(&events_file, &name, false)?;
                validate_private_regular(&file)?;
                temps.push(name);
            }
        }
        for name in temps {
            unlink_at(&events_file, &name)?;
        }
        if !fs::read_dir(&self.events).map_err(io_error)?.any(|entry| {
            entry.is_ok_and(|entry| is_temp_file_name(&entry.file_name().to_string_lossy()))
        }) {
            events_file.sync_all().map_err(io_error)?;
        }
        self.verify_directory_identities()
    }

    #[cfg(unix)]
    fn verify_directory_identities(&self) -> Result<(), LedgerError> {
        let root = open_directory_nofollow(&self.root)?;
        let events = open_directory_nofollow(&self.events)?;
        if file_identity(&root)? != self.root_identity
            || file_identity(&events)? != self.events_identity
        {
            return Err(LedgerError::State(
                "ledger directory identity changed".into(),
            ));
        }
        validate_private_directory(&root)?;
        validate_private_directory(&events)?;
        Ok(())
    }

    #[cfg(not(unix))]
    fn verify_directory_identities(&self) -> Result<(), LedgerError> {
        Err(LedgerError::UnsupportedPlatform)
    }
}

#[cfg(unix)]
struct LedgerLock {
    file: File,
}

#[cfg(unix)]
impl Drop for LedgerLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(not(unix))]
struct LedgerLock;

fn validate_header(header: &LedgerHeaderV1) -> Result<(), LedgerError> {
    if header.schema_version != LEDGER_HEADER_SCHEMA_V1 {
        return Err(LedgerError::State("unsupported ledger header".into()));
    }
    validate_id("experiment_id", &header.experiment_id)?;
    validate_sha256("preregistration_sha256", &header.preregistration_sha256)?;
    if header.price_card.sha256()? != header.price_card_sha256 {
        return Err(LedgerError::State(
            "ledger price card digest is mismatched".into(),
        ));
    }
    if header.cap_atto_usd == AttoUsd::ZERO || header.cap_atto_usd > AttoUsd::DEFAULT_CAP {
        return Err(LedgerError::CapExceeded);
    }
    Ok(())
}

fn validate_event_file(
    wrapper: &LedgerEventFileV1,
    state: &LoadedState,
) -> Result<(), LedgerError> {
    if wrapper.payload.schema_version != LEDGER_EVENT_SCHEMA_V1
        || wrapper.payload.sequence != state.next_sequence
        || wrapper.payload.previous_event_sha256 != state.last_event_sha256
    {
        return Err(LedgerError::State(
            "ledger event is not bound to the expected chain position".into(),
        ));
    }
    wrapper.payload.binding.validate()?;
    let canonical = serde_json::to_vec(&wrapper.payload).map_err(json_error)?;
    let expected = domain_sha256(EVENT_HASH_DOMAIN, &canonical);
    if wrapper.event_sha256 != expected {
        return Err(LedgerError::State("ledger event hash mismatch".into()));
    }
    validate_sha256("event_sha256", &wrapper.event_sha256)
}

fn apply_event(state: &mut LoadedState, payload: &LedgerEventPayloadV1) -> Result<(), LedgerError> {
    let actual_binding_sha256 = payload.binding.sha256()?;
    match &payload.event {
        LedgerEventKindV1::RequestReserved {
            reservation_id,
            binding_sha256,
            reserved_atto_usd,
        } => {
            validate_id("reservation_id", reservation_id)?;
            validate_sha256("binding_sha256", binding_sha256)?;
            if *binding_sha256 != actual_binding_sha256
                || *reserved_atto_usd == AttoUsd::ZERO
                || state.outstanding.contains_key(reservation_id)
                || !state.seen_bindings.insert(binding_sha256.clone())
            {
                return Err(LedgerError::State("invalid duplicate reservation".into()));
            }
            state.outstanding.insert(
                reservation_id.clone(),
                ActiveReservation {
                    binding_sha256: binding_sha256.clone(),
                    reserved_atto_usd: *reserved_atto_usd,
                    slot_index: payload.binding.slot_index,
                },
            );
        }
        LedgerEventKindV1::RequestSettled {
            reservation_id,
            binding_sha256,
            actual_atto_usd,
            provider_receipt_sha256,
        } => {
            validate_sha256("provider_receipt_sha256", provider_receipt_sha256)?;
            let active = take_matching_reservation(
                state,
                reservation_id,
                binding_sha256,
                &actual_binding_sha256,
            )?;
            if *actual_atto_usd > active.reserved_atto_usd {
                return Err(LedgerError::SettlementExceedsReservation);
            }
            state.settled = state.settled.checked_add(*actual_atto_usd)?;
            if active.slot_index != payload.binding.slot_index {
                return Err(LedgerError::State(
                    "reservation slot binding mismatch".into(),
                ));
            }
            let slot = state.terminal_by_slot.entry(active.slot_index).or_default();
            slot.settled_actual_atto_usd =
                slot.settled_actual_atto_usd.checked_add(*actual_atto_usd)?;
            slot.settled_requests = slot
                .settled_requests
                .checked_add(1)
                .ok_or(LedgerError::MoneyOverflow)?;
        }
        LedgerEventKindV1::RequestUnknown {
            reservation_id,
            binding_sha256,
            reason_code,
        } => {
            validate_reason_code(reason_code)?;
            let active = take_matching_reservation(
                state,
                reservation_id,
                binding_sha256,
                &actual_binding_sha256,
            )?;
            state.unknown = state.unknown.checked_add(active.reserved_atto_usd)?;
            if active.slot_index != payload.binding.slot_index {
                return Err(LedgerError::State(
                    "reservation slot binding mismatch".into(),
                ));
            }
            let slot = state.terminal_by_slot.entry(active.slot_index).or_default();
            slot.unknown_reserved_atto_usd = slot
                .unknown_reserved_atto_usd
                .checked_add(active.reserved_atto_usd)?;
            slot.unknown_requests = slot
                .unknown_requests
                .checked_add(1)
                .ok_or(LedgerError::MoneyOverflow)?;
        }
        LedgerEventKindV1::RequestReleasedPreSend {
            reservation_id,
            binding_sha256,
            reason_code,
        } => {
            validate_reason_code(reason_code)?;
            let _ = take_matching_reservation(
                state,
                reservation_id,
                binding_sha256,
                &actual_binding_sha256,
            )?;
        }
    }
    Ok(())
}

fn take_matching_reservation(
    state: &mut LoadedState,
    reservation_id: &str,
    binding_sha256: &str,
    actual_binding_sha256: &str,
) -> Result<ActiveReservation, LedgerError> {
    validate_id("reservation_id", reservation_id)?;
    validate_sha256("binding_sha256", binding_sha256)?;
    if binding_sha256 != actual_binding_sha256 {
        return Err(LedgerError::State("terminal binding mismatch".into()));
    }
    let active = state
        .outstanding
        .remove(reservation_id)
        .ok_or(LedgerError::MissingReservation)?;
    if active.binding_sha256 != binding_sha256 {
        return Err(LedgerError::State("reservation binding mismatch".into()));
    }
    Ok(active)
}

fn total_committed(state: &LoadedState) -> Result<AttoUsd, LedgerError> {
    state.outstanding.values().try_fold(
        state.settled.checked_add(state.unknown)?,
        |total, active| total.checked_add(active.reserved_atto_usd),
    )
}

fn reservation_id(preregistration_sha256: &str, sequence: u64, binding_sha256: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(RESERVATION_ID_DOMAIN);
    hasher.update(preregistration_sha256.as_bytes());
    hasher.update(sequence.to_be_bytes());
    hasher.update(binding_sha256.as_bytes());
    format!("rsv-{}", &format!("{:x}", hasher.finalize())[..32])
}

fn domain_sha256(domain: &[u8], bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Hashes caller-provided correlation material without persisting the raw
/// value. This must match the broker/protocol's ordinary SHA-256 binding; it
/// intentionally has no domain prefix because the other endpoint also needs
/// to compute it before it can access the ledger.
fn raw_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn validate_id(field: &str, value: &str) -> Result<(), LedgerError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(LedgerError::InvalidBinding(field.into()));
    }
    Ok(())
}

fn validate_sha256(field: &str, value: &str) -> Result<(), LedgerError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(LedgerError::InvalidBinding(field.into()));
    }
    Ok(())
}

fn validate_reason_code(value: &str) -> Result<(), LedgerError> {
    if value.is_empty()
        || value.len() > MAX_REASON_CODE_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(LedgerError::InvalidBinding("reason_code".into()));
    }
    Ok(())
}

fn event_file_name(sequence: u64) -> String {
    format!("{sequence:0EVENT_FILE_DIGITS$}.json")
}

fn is_event_file_name(name: &str) -> bool {
    name.len() == EVENT_FILE_DIGITS + 5
        && name.ends_with(".json")
        && name[..EVENT_FILE_DIGITS]
            .bytes()
            .all(|byte| byte.is_ascii_digit())
}

fn is_temp_file_name(name: &str) -> bool {
    name.starts_with(".tmp-")
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn io_error(error: std::io::Error) -> LedgerError {
    LedgerError::Io(error.to_string())
}

fn json_error(error: serde_json::Error) -> LedgerError {
    LedgerError::Json(error.to_string())
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

#[cfg(unix)]
fn file_identity(file: &File) -> Result<FileIdentity, LedgerError> {
    let metadata = file.metadata().map_err(io_error)?;
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(unix)]
fn validate_private_directory(file: &File) -> Result<FileIdentity, LedgerError> {
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(LedgerError::State(
            "ledger directory is not private and owner-controlled".into(),
        ));
    }
    file_identity(file)
}

#[cfg(unix)]
fn validate_private_regular(file: &File) -> Result<(), LedgerError> {
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(LedgerError::State(
            "ledger file is not private, regular, and owner-controlled".into(),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn open_directory_nofollow(path: &Path) -> Result<File, LedgerError> {
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| LedgerError::State("ledger path contains NUL".into()))?;
    let descriptor = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(not(unix))]
fn open_directory_nofollow(_path: &Path) -> Result<File, LedgerError> {
    Err(LedgerError::UnsupportedPlatform)
}

#[cfg(unix)]
fn c_string(value: &str) -> Result<std::ffi::CString, LedgerError> {
    std::ffi::CString::new(value)
        .map_err(|_| LedgerError::State("ledger entry name contains NUL".into()))
}

#[cfg(unix)]
fn open_regular_at(directory: &File, name: &str, write: bool) -> Result<File, LedgerError> {
    let name = c_string(name)?;
    let flags = if write { libc::O_RDWR } else { libc::O_RDONLY };
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    let file = unsafe { File::from_raw_fd(descriptor) };
    validate_private_regular(&file)?;
    Ok(file)
}

#[cfg(unix)]
fn read_regular_at(directory: &File, name: &str, max_bytes: u64) -> Result<Vec<u8>, LedgerError> {
    let mut file = open_regular_at(directory, name, false)?;
    let metadata = file.metadata().map_err(io_error)?;
    if metadata.len() > max_bytes {
        return Err(LedgerError::State("ledger file exceeds byte cap".into()));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > max_bytes {
        return Err(LedgerError::State(
            "ledger file grew beyond byte cap".into(),
        ));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn write_create_new_at(directory: &File, name: &str, bytes: &[u8]) -> Result<(), LedgerError> {
    let name = c_string(name)?;
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o600,
        )
    };
    if descriptor < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    let mut file = unsafe { File::from_raw_fd(descriptor) };
    validate_private_regular(&file)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)
}

#[cfg(unix)]
fn unlink_at(directory: &File, name: &str) -> Result<(), LedgerError> {
    let name = c_string(name)?;
    let result = unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) };
    if result != 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::Arc;

    fn digest(byte: char) -> String {
        std::iter::repeat_n(byte, 64).collect()
    }

    fn binding(index: u16, attempt: u8) -> LedgerBindingV1 {
        LedgerBindingV1 {
            experiment_id: "pilot-1".into(),
            preregistration_sha256: digest('a'),
            trial_id: format!("trial-{index}"),
            slot_index: index,
            phase: ExperimentPhase::Executor,
            semantic_request_index: index,
            physical_attempt: attempt,
            route: ExperimentModelRouteV1::openai(
                crate::reasoning_experiment_protocol::CHEAP_MODEL,
            ),
            price_card_sha256: price_card().sha256().unwrap(),
            request_sha256: digest('b'),
            client_request_id_sha256: digest('c'),
            controller_sha256: digest('d'),
            worker_sha256: digest('e'),
        }
    }

    fn ledger(root: &Path, cap: AttoUsd) -> ExperimentLedger {
        ExperimentLedger::create(root, "pilot-1", &digest('a'), cap, price_card()).unwrap()
    }

    fn price(value: &str) -> AttoUsd {
        AttoUsd::parse_usd(value).unwrap()
    }

    fn price_card() -> FrozenPriceCardV1 {
        FrozenPriceCardV1 {
            schema_version: FROZEN_PRICE_CARD_SCHEMA_V1.into(),
            price_card_id: "openai-2026-08-12-default".into(),
            effective_at_rfc3339: "2026-08-12T00:00:00Z".into(),
            service_tier: "default".into(),
            sol: FrozenModelPriceV1 {
                regular_input_per_million_atto_usd: price("5"),
                cached_input_per_million_atto_usd: price("0.5"),
                cache_write_per_million_atto_usd: price("6.25"),
                regular_output_per_million_atto_usd: price("30"),
                reasoning_output_per_million_atto_usd: price("30"),
            },
            luna: FrozenModelPriceV1 {
                regular_input_per_million_atto_usd: price("0.2"),
                cached_input_per_million_atto_usd: price("0.02"),
                cache_write_per_million_atto_usd: price("0.25"),
                regular_output_per_million_atto_usd: price("1.2"),
                reasoning_output_per_million_atto_usd: price("1.2"),
            },
        }
    }

    fn provider_call(index: u16, attempt: u8, model: &str) -> ProviderCallSpecV1 {
        let request_json = format!(
            r#"{{"model":"{model}","input":[{{"role":"user","content":"fixture"}}],"stream":true,"reasoning":{{"effort":"high"}},"include":["reasoning.encrypted_content"],"max_output_tokens":8192,"store":false,"service_tier":"default"}}"#
        );
        ProviderCallSpecV1 {
            schema_version: crate::reasoning_experiment_protocol::PROVIDER_CALL_SPEC_SCHEMA_V1
                .into(),
            call_id: format!("provider-call-{index}-{attempt}"),
            channel_id: "channel-1".into(),
            nonce: format!("nonce-{index}-{attempt}"),
            worker_spec_sha256: digest('b'),
            controller_sha256: digest('c'),
            worker_sha256: digest('d'),
            experiment_id: "pilot-1".into(),
            preregistration_sha256: digest('a'),
            trial_id: format!("trial-{index}"),
            slot_index: index,
            phase: ExperimentPhase::Executor,
            semantic_request_index: index,
            physical_attempt: attempt,
            route: ExperimentModelRouteV1::openai(model),
            max_input_tokens: MAX_INPUT_TOKENS_PER_REQUEST,
            max_output_tokens: MAX_OUTPUT_TOKENS_PER_REQUEST,
            store: false,
            request_sha256: raw_sha256(request_json.as_bytes()),
            request_json,
            x_client_request_id: format!("rnd-097-{index}-{attempt}"),
        }
    }

    #[test]
    fn atto_usd_is_exact_and_canonical() {
        assert_eq!(AttoUsd::parse_usd("50.00").unwrap(), AttoUsd::DEFAULT_CAP);
        assert_eq!(
            AttoUsd::parse_usd("0.000000000000000001")
                .unwrap()
                .as_atto(),
            1
        );
        assert_eq!(AttoUsd::from_atto(1).to_string(), "0.000000000000000001");
        assert!(AttoUsd::parse_usd("1.0000000000000000001").is_err());
        assert!(AttoUsd::parse_usd("-1").is_err());
        assert!(serde_json::from_str::<AttoUsd>("\"01\"").is_err());
    }

    #[test]
    fn frozen_price_card_reserves_the_highest_legal_rate_and_costs_disjoint_usage() {
        let card = price_card();
        let sol =
            ExperimentModelRouteV1::openai(crate::reasoning_experiment_protocol::STRONG_MODEL);
        let luna =
            ExperimentModelRouteV1::openai(crate::reasoning_experiment_protocol::CHEAP_MODEL);
        assert_eq!(card.worst_case_for_route(&sol).unwrap(), price("0.64576"));
        assert_eq!(
            card.worst_case_for_route(&luna).unwrap(),
            price("0.0258304")
        );

        let usage = ExperimentUsageV1 {
            regular_input_tokens: 100,
            cached_input_tokens: 20,
            cache_write_tokens: 30,
            regular_output_tokens: 40,
            reasoning_output_tokens: 50,
        };
        assert_eq!(
            card.exact_cost_for_usage(&sol, usage).unwrap(),
            price("0.0033975")
        );
        assert_eq!(
            card.exact_cost_for_usage(&luna, usage).unwrap(),
            price("0.0001359")
        );
    }

    #[test]
    fn provider_call_reservations_bind_one_post_and_keep_unknown_spend_reserved() {
        let root = tempfile::tempdir().unwrap();
        let ledger = ledger(&root.path().join("ledger"), AttoUsd::DEFAULT_CAP);
        let first = provider_call(0, 1, crate::reasoning_experiment_protocol::STRONG_MODEL);
        let grant = ledger.reserve_provider_call(&first).unwrap();
        assert_eq!(grant.reserved_atto_usd, price("0.64576"));
        assert!(matches!(
            ledger.reserve_provider_call(&first),
            Err(LedgerError::DuplicateRequest)
        ));
        ledger
            .mark_provider_call_unknown(&grant, &first, "missing_usage")
            .unwrap();

        let retry = provider_call(0, 2, crate::reasoning_experiment_protocol::STRONG_MODEL);
        let retry_grant = ledger.reserve_provider_call(&retry).unwrap();
        assert_ne!(grant.reservation_id, retry_grant.reservation_id);
        ledger
            .release_provider_call_pre_send(&retry_grant, &retry, "connect_refused")
            .unwrap();

        let snapshot = ledger.snapshot().unwrap();
        assert_eq!(snapshot.unknown_reserved_atto_usd, price("0.64576"));
        assert_eq!(snapshot.outstanding_reserved_atto_usd, AttoUsd::ZERO);
    }

    #[test]
    fn provider_call_settlement_uses_actual_usage_and_rejects_invalid_price_cards() {
        let root = tempfile::tempdir().unwrap();
        let ledger = ledger(&root.path().join("ledger"), AttoUsd::DEFAULT_CAP);
        let call = provider_call(1, 1, crate::reasoning_experiment_protocol::CHEAP_MODEL);
        let grant = ledger.reserve_provider_call(&call).unwrap();
        let usage = ExperimentUsageV1 {
            regular_input_tokens: 100,
            cached_input_tokens: 20,
            cache_write_tokens: 30,
            regular_output_tokens: 40,
            reasoning_output_tokens: 50,
        };
        ledger
            .settle_provider_call(&grant, &call, usage, &digest('f'))
            .unwrap();
        assert_eq!(
            ledger.snapshot().unwrap().settled_actual_atto_usd,
            price("0.0001359")
        );

        let mut invalid = price_card();
        invalid.service_tier = "priority".into();
        assert!(invalid.validate().is_err());
        let mut non_integral = price_card();
        non_integral.sol.regular_input_per_million_atto_usd = AttoUsd::from_atto(1);
        assert!(non_integral.validate().is_err());
    }

    #[test]
    fn persisted_price_card_is_bound_to_every_ledger_event() {
        let root = tempfile::tempdir().unwrap();
        let ledger_root = root.path().join("ledger");
        let ledger = ledger(&ledger_root, AttoUsd::DEFAULT_CAP);
        let call = provider_call(0, 1, crate::reasoning_experiment_protocol::CHEAP_MODEL);
        ledger.reserve_provider_call(&call).unwrap();

        let header_path = ledger_root.join("ledger.json");
        let mut header: LedgerHeaderV1 =
            serde_json::from_slice(&fs::read(&header_path).unwrap()).unwrap();
        header.price_card.luna.regular_input_per_million_atto_usd = price("0.1");
        fs::write(&header_path, serde_json::to_vec(&header).unwrap()).unwrap();
        assert!(ExperimentLedger::open(&ledger_root).is_err());
    }

    #[test]
    fn reserve_settle_unknown_and_release_are_exact() {
        let root = tempfile::tempdir().unwrap();
        let ledger = ledger(&root.path().join("ledger"), AttoUsd::from_atto(100));

        let first = binding(0, 1);
        let first_grant = ledger
            .reserve(first.clone(), AttoUsd::from_atto(40))
            .unwrap();
        ledger
            .settle(&first_grant, first, AttoUsd::from_atto(25), &digest('f'))
            .unwrap();

        let second = binding(1, 1);
        let second_grant = ledger
            .reserve(second.clone(), AttoUsd::from_atto(30))
            .unwrap();
        ledger
            .mark_unknown(&second_grant, second, "missing_usage")
            .unwrap();

        let third = binding(2, 1);
        let third_grant = ledger
            .reserve(third.clone(), AttoUsd::from_atto(20))
            .unwrap();
        ledger
            .release_verified_pre_send(&third_grant, third, "connect_refused")
            .unwrap();

        let snapshot = ledger.snapshot().unwrap();
        assert_eq!(snapshot.settled_actual_atto_usd.as_atto(), 25);
        assert_eq!(snapshot.unknown_reserved_atto_usd.as_atto(), 30);
        assert_eq!(snapshot.outstanding_reserved_atto_usd, AttoUsd::ZERO);
        assert_eq!(snapshot.available_atto_usd.as_atto(), 45);
        assert_eq!(snapshot.event_count, 6);
    }

    #[test]
    fn attributed_snapshot_charges_every_committed_call_to_its_bound_slot() {
        let root = tempfile::tempdir().unwrap();
        let ledger = ledger(&root.path().join("ledger"), AttoUsd::from_atto(200));

        let settled = binding(4, 1);
        let settled_grant = ledger
            .reserve(settled.clone(), AttoUsd::from_atto(50))
            .unwrap();
        ledger
            .settle(
                &settled_grant,
                settled,
                AttoUsd::from_atto(17),
                &digest('f'),
            )
            .unwrap();

        let unknown = binding(4, 2);
        let unknown_grant = ledger
            .reserve(unknown.clone(), AttoUsd::from_atto(40))
            .unwrap();
        ledger
            .mark_unknown(&unknown_grant, unknown, "missing_usage")
            .unwrap();

        let outstanding = binding(9, 1);
        ledger.reserve(outstanding, AttoUsd::from_atto(31)).unwrap();

        let audit = ledger.attributed_snapshot().unwrap();
        assert_eq!(audit.totals.settled_actual_atto_usd.as_atto(), 17);
        assert_eq!(audit.totals.unknown_reserved_atto_usd.as_atto(), 40);
        assert_eq!(audit.totals.outstanding_reserved_atto_usd.as_atto(), 31);
        assert_eq!(
            audit.slots,
            vec![
                LedgerSlotSpendV1 {
                    slot_index: 4,
                    settled_actual_atto_usd: AttoUsd::from_atto(17),
                    unknown_reserved_atto_usd: AttoUsd::from_atto(40),
                    outstanding_reserved_atto_usd: AttoUsd::ZERO,
                    settled_requests: 1,
                    unknown_requests: 1,
                    outstanding_requests: 0,
                },
                LedgerSlotSpendV1 {
                    slot_index: 9,
                    settled_actual_atto_usd: AttoUsd::ZERO,
                    unknown_reserved_atto_usd: AttoUsd::ZERO,
                    outstanding_reserved_atto_usd: AttoUsd::from_atto(31),
                    settled_requests: 0,
                    unknown_requests: 0,
                    outstanding_requests: 1,
                },
            ]
        );
    }

    #[test]
    fn cap_counts_every_outstanding_and_unknown_reservation() {
        let root = tempfile::tempdir().unwrap();
        let ledger = ledger(&root.path().join("ledger"), AttoUsd::from_atto(100));
        let first = binding(0, 1);
        let first_grant = ledger
            .reserve(first.clone(), AttoUsd::from_atto(60))
            .unwrap();
        assert!(matches!(
            ledger.reserve(binding(1, 1), AttoUsd::from_atto(41)),
            Err(LedgerError::CapExceeded)
        ));
        ledger
            .mark_unknown(&first_grant, first, "worker_died")
            .unwrap();
        assert!(matches!(
            ledger.reserve(binding(1, 1), AttoUsd::from_atto(41)),
            Err(LedgerError::CapExceeded)
        ));
        ledger
            .reserve(binding(1, 1), AttoUsd::from_atto(40))
            .unwrap();
    }

    #[test]
    fn duplicate_physical_request_never_reuses_a_reservation() {
        let root = tempfile::tempdir().unwrap();
        let ledger = ledger(&root.path().join("ledger"), AttoUsd::from_atto(100));
        let request = binding(0, 1);
        ledger
            .reserve(request.clone(), AttoUsd::from_atto(20))
            .unwrap();
        assert!(matches!(
            ledger.reserve(request, AttoUsd::from_atto(20)),
            Err(LedgerError::DuplicateRequest)
        ));
    }

    #[test]
    fn tamper_truncation_and_reordering_fail_closed() {
        for mutation in ["tamper", "truncate", "rename"] {
            let root = tempfile::tempdir().unwrap();
            let ledger_root = root.path().join("ledger");
            let ledger = ledger(&ledger_root, AttoUsd::from_atto(100));
            ledger
                .reserve(binding(0, 1), AttoUsd::from_atto(20))
                .unwrap();
            let event = ledger_root.join("events/00000000000000000000.json");
            match mutation {
                "tamper" => {
                    let mut bytes = fs::read(&event).unwrap();
                    let index = bytes.iter().position(|byte| *byte == b'2').unwrap();
                    bytes[index] = b'3';
                    fs::write(&event, bytes).unwrap();
                }
                "truncate" => fs::write(&event, b"{").unwrap(),
                "rename" => {
                    fs::rename(&event, ledger_root.join("events/00000000000000000001.json"))
                        .unwrap()
                }
                _ => unreachable!(),
            }
            assert!(ExperimentLedger::open(&ledger_root).is_err(), "{mutation}");
        }
    }

    #[test]
    fn uncommitted_temp_is_removed_under_the_lock() {
        let root = tempfile::tempdir().unwrap();
        let ledger_root = root.path().join("ledger");
        let ledger = ledger(&ledger_root, AttoUsd::from_atto(100));
        let temp = ledger_root.join("events/.tmp-00000000000000000000-dead");
        let mut options = OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        options.open(&temp).unwrap().write_all(b"partial").unwrap();
        assert_eq!(ledger.snapshot().unwrap().event_count, 0);
        assert!(!temp.exists());
    }

    #[test]
    fn concurrent_reservations_share_one_hard_cap() {
        let root = tempfile::tempdir().unwrap();
        let ledger = Arc::new(ledger(&root.path().join("ledger"), AttoUsd::from_atto(100)));
        let mut joins = Vec::new();
        for index in 0..4u16 {
            let ledger = ledger.clone();
            joins.push(std::thread::spawn(move || {
                ledger.reserve(binding(index, 1), AttoUsd::from_atto(40))
            }));
        }
        let successes = joins
            .into_iter()
            .map(|join| join.join().unwrap().is_ok())
            .filter(|succeeded| *succeeded)
            .count();
        assert_eq!(successes, 2);
        assert_eq!(
            ledger
                .snapshot()
                .unwrap()
                .outstanding_reserved_atto_usd
                .as_atto(),
            80
        );
    }

    #[cfg(unix)]
    #[test]
    fn widened_or_symlinked_ledger_root_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let ledger_root = root.path().join("ledger");
        let _ledger = ledger(&ledger_root, AttoUsd::from_atto(100));
        fs::set_permissions(&ledger_root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(ExperimentLedger::open(&ledger_root).is_err());

        let link = root.path().join("ledger-link");
        std::os::unix::fs::symlink(&ledger_root, &link).unwrap();
        assert!(ExperimentLedger::open(&link).is_err());
    }
}
