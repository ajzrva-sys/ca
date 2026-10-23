//! Machine-wide Eastern-time allowance. Unknown requests retain their charge.
use super::LearningVault;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const DAILY_LEARNING_LIMIT_MICROUSD: u64 = 2_000_000;
#[derive(Debug)]
pub struct DailyLearningBudgetExhausted {
    pub resets_at: DateTime<Utc>,
}
impl std::fmt::Display for DailyLearningBudgetExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "daily $2 learning allowance exhausted; resumes {} (America/New_York)",
            self.resets_at
        )
    }
}
impl std::error::Error for DailyLearningBudgetExhausted {}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DailyLearningBudgetStatus {
    pub date: String,
    pub timezone: String,
    pub limit_microusd: u64,
    pub spent_microusd: u64,
    pub reserved_microusd: u64,
    pub remaining_microusd: u64,
    #[serde(default)]
    pub legacy_unknown_microusd: u64,
    pub resets_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyBudgetMigrationV1 {
    schema: String,
    migration_id: String,
    date: String,
    enrollment_digest: String,
    known_microusd: u64,
    unknown_reserved_microusd: u64,
    retained_existing_ledger: bool,
    created_at: String,
    digest: String,
}
impl super::VaultRecord for LegacyBudgetMigrationV1 {
    fn record_directory(&self) -> &'static str {
        "migration"
    }
    fn record_id(&self) -> &str {
        &self.migration_id
    }
    fn digest(&self) -> &str {
        &self.digest
    }
    fn set_digest(&mut self, digest: String) {
        self.digest = digest;
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DayLedger {
    date: String,
    spent: u64,
    pending: BTreeMap<String, u64>,
    settled: BTreeMap<String, u64>,
}
#[derive(Debug, Clone)]
pub struct DailyLearningTicket {
    root: PathBuf,
    date: String,
    id: String,
    maximum: u64,
}
fn first_sunday(year: i32, month: u32) -> Result<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(year, month, 1).context("invalid Eastern budget date")?;
    Ok(first + Duration::days(i64::from((7 - first.weekday().num_days_from_sunday()) % 7)))
}
fn eastern_offset(now: DateTime<Utc>) -> Result<i64> {
    if now.year() < 2007 {
        bail!("Eastern budget calendar requires year 2007 or later");
    }
    let start = (first_sunday(now.year(), 3)? + Duration::days(7))
        .and_hms_opt(7, 0, 0)
        .context("invalid DST start")?
        .and_utc();
    let end = first_sunday(now.year(), 11)?
        .and_hms_opt(6, 0, 0)
        .context("invalid DST end")?
        .and_utc();
    Ok(if now >= start && now < end { -4 } else { -5 })
}
fn day(now: DateTime<Utc>) -> Result<(String, DateTime<Utc>)> {
    let local = now + Duration::hours(eastern_offset(now)?);
    let next = local
        .date_naive()
        .succ_opt()
        .context("budget calendar overflow")?
        .and_hms_opt(0, 0, 0)
        .context("invalid midnight")?
        .and_utc();
    // At midnight the offset is the one before a same-day 02:00 transition.
    let reset_probe = next + Duration::hours(4);
    let reset = next - Duration::hours(eastern_offset(reset_probe)?);
    Ok((local.date_naive().to_string(), reset))
}
pub fn eastern_learning_date(now: DateTime<Utc>) -> Result<String> {
    day(now).map(|(date, _)| date)
}
pub fn daily_learning_budget_root(home: &Path) -> PathBuf {
    home.join(".local/share/ca/learning-budget")
}
fn existing(root: &Path) -> Result<Option<LearningVault>> {
    LearningVault::existing_at(root.to_path_buf())
}
fn ledger(vault: &LearningVault, date: &str) -> Result<DayLedger> {
    let value = vault
        .read_mutable::<DayLedger>(&format!("budget/{date}.json"))?
        .unwrap_or_else(|| DayLedger {
            date: date.into(),
            spent: 0,
            pending: BTreeMap::new(),
            settled: BTreeMap::new(),
        });
    if value.date != date || value.pending.len() + value.settled.len() > 100_000 {
        bail!("daily learning ledger is corrupt or oversized");
    }
    Ok(value)
}
fn snapshot(value: &DayLedger, reset: DateTime<Utc>) -> Result<DailyLearningBudgetStatus> {
    let reserved = value.pending.values().try_fold(0u64, |a, b| {
        a.checked_add(*b).context("daily budget overflow")
    })?;
    let charged = value
        .spent
        .checked_add(reserved)
        .context("daily budget overflow")?;
    Ok(DailyLearningBudgetStatus {
        date: value.date.clone(),
        timezone: "America/New_York".into(),
        limit_microusd: DAILY_LEARNING_LIMIT_MICROUSD,
        spent_microusd: value.spent,
        reserved_microusd: reserved,
        remaining_microusd: DAILY_LEARNING_LIMIT_MICROUSD.saturating_sub(charged),
        legacy_unknown_microusd: value
            .pending
            .iter()
            .filter(|(id, _)| id.starts_with("legacy:unknown:"))
            .try_fold(0u64, |sum, (_, amount)| {
                sum.checked_add(*amount).context("legacy exposure overflow")
            })?,
        resets_at: reset,
    })
}
/// A legacy supervisor has no complete per-day physical charge journal. Import
/// proven charges and retain unknown exposure for the rest of this day once.
/// Existing new-ledger charges and subsequent days are never reset or reseeded.
pub fn bootstrap_legacy_daily_learning(
    root: &Path,
    enrollment_digest: &str,
    known_microusd: u64,
    now: DateTime<Utc>,
) -> Result<bool> {
    if enrollment_digest.len() != 64
        || !enrollment_digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        bail!("legacy budget migration enrollment digest is invalid");
    }
    let vault = LearningVault::open_at(root.to_path_buf())?;
    bootstrap_legacy_daily_learning_in_vault(&vault, enrollment_digest, known_microusd, now)
}

fn bootstrap_legacy_daily_learning_in_vault(
    vault: &LearningVault,
    enrollment_digest: &str,
    known_microusd: u64,
    now: DateTime<Utc>,
) -> Result<bool> {
    let _lock = vault.acquire_lock()?;
    if vault
        .read_optional_immutable::<LegacyBudgetMigrationV1>("migration", "legacy-learning-v1")?
        .is_some()
    {
        return Ok(false);
    }
    let (date, _) = day(now)?;
    let path = format!("budget/{date}.json");
    let retained_existing_ledger = vault.read_mutable::<DayLedger>(&path)?.is_some();
    let mut value = ledger(vault, &date)?;
    let unknown = if retained_existing_ledger {
        0
    } else {
        DAILY_LEARNING_LIMIT_MICROUSD.saturating_sub(known_microusd)
    };
    if !retained_existing_ledger {
        value.spent = known_microusd;
        if unknown > 0 {
            value.pending.insert(
                format!("legacy:unknown:{}", &enrollment_digest[..16]),
                unknown,
            );
        }
        vault.write_mutable(&path, &value)?;
    }
    let mut migration = LegacyBudgetMigrationV1 {
        schema: "ca.learning-legacy-budget-migration.v1".into(),
        migration_id: "legacy-learning-v1".into(),
        date,
        enrollment_digest: enrollment_digest.into(),
        known_microusd,
        unknown_reserved_microusd: unknown,
        retained_existing_ledger,
        created_at: now.to_rfc3339(),
        digest: String::new(),
    };
    vault.write_immutable(&mut migration)?;
    Ok(!retained_existing_ledger)
}
pub fn daily_learning_budget_status(
    root: &Path,
    now: DateTime<Utc>,
) -> Result<DailyLearningBudgetStatus> {
    let (date, reset) = day(now)?;
    match existing(root)? {
        Some(vault) => {
            let _lock = vault.acquire_lock()?;
            snapshot(&ledger(&vault, &date)?, reset)
        }
        None => snapshot(
            &DayLedger {
                date,
                spent: 0,
                pending: BTreeMap::new(),
                settled: BTreeMap::new(),
            },
            reset,
        ),
    }
}
pub fn reserve_daily_learning(
    root: &Path,
    id: &str,
    maximum: u64,
    now: DateTime<Utc>,
) -> Result<DailyLearningTicket> {
    if id.is_empty() || id.len() > 256 || maximum == 0 || maximum > DAILY_LEARNING_LIMIT_MICROUSD {
        bail!("daily learning reservation is invalid or exceeds $2");
    }
    let vault = LearningVault::open_at(root.to_path_buf())?;
    let _lock = vault.acquire_lock()?;
    let (date, reset) = day(now)?;
    let mut value = ledger(&vault, &date)?;
    if value.pending.contains_key(id) || value.settled.contains_key(id) {
        bail!("learning request already reserved or settled; reconcile before replay");
    }
    if snapshot(&value, reset)?.remaining_microusd < maximum {
        return Err(DailyLearningBudgetExhausted { resets_at: reset }.into());
    }
    value.pending.insert(id.into(), maximum);
    vault.write_mutable(&format!("budget/{date}.json"), &value)?;
    Ok(DailyLearningTicket {
        root: root.into(),
        date,
        id: id.into(),
        maximum,
    })
}
pub fn settle_daily_learning(
    ticket: &DailyLearningTicket,
    actual: Option<u64>,
    proven_absent: bool,
) -> Result<()> {
    let vault = existing(&ticket.root)?.context("daily learning reservation missing")?;
    let _lock = vault.acquire_lock()?;
    let mut value = ledger(&vault, &ticket.date)?;
    if let Some(previous) = value.settled.get(&ticket.id) {
        if actual != Some(*previous) {
            bail!("daily settlement conflicts with its receipt");
        }
        return Ok(());
    }
    if value.pending.get(&ticket.id) != Some(&ticket.maximum) {
        bail!("daily reservation changed");
    }
    if let Some(actual) = actual {
        if actual > ticket.maximum {
            bail!("provider exceeded reserved daily bound; reservation retained");
        }
        value.spent = value
            .spent
            .checked_add(actual)
            .context("daily settlement overflow")?;
        value.pending.remove(&ticket.id);
        value.settled.insert(ticket.id.clone(), actual);
    } else if proven_absent {
        value.pending.remove(&ticket.id);
    }
    vault.write_mutable(&format!("budget/{}.json", ticket.date), &value)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    #[test]
    fn legacy_bootstrap_retains_unknown_exposure_once_and_resets_on_next_eastern_day() -> Result<()>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("budget");
        let vault = super::super::vault::fixture_vault(root.clone())?;
        let now = Utc.with_ymd_and_hms(2026, 10, 10, 3, 50, 0).unwrap();
        assert!(bootstrap_legacy_daily_learning_in_vault(
            &vault,
            &"a".repeat(64),
            350_000,
            now
        )?);
        let status = daily_learning_budget_status(&root, now)?;
        assert_eq!(status.date, "2026-10-09");
        assert_eq!(status.spent_microusd, 350_000);
        assert_eq!(status.reserved_microusd, 1_650_000);
        assert_eq!(status.legacy_unknown_microusd, 1_650_000);
        assert_eq!(status.remaining_microusd, 0);
        assert!(reserve_daily_learning(&root, "new-call", 1, now).is_err());
        assert!(!bootstrap_legacy_daily_learning_in_vault(
            &vault,
            &"b".repeat(64),
            1_900_000,
            now
        )?);
        assert_eq!(daily_learning_budget_status(&root, now)?, status);
        let next = now + Duration::minutes(11);
        assert!(!bootstrap_legacy_daily_learning_in_vault(
            &vault,
            &"b".repeat(64),
            1_900_000,
            next
        )?);
        let next_status = daily_learning_budget_status(&root, next)?;
        assert_eq!(next_status.date, "2026-10-10");
        assert_eq!(
            next_status.remaining_microusd,
            DAILY_LEARNING_LIMIT_MICROUSD
        );
        assert_eq!(next_status.legacy_unknown_microusd, 0);
        assert_eq!(daily_learning_budget_status(&root, now)?, status);
        Ok(())
    }
    #[test]
    fn legacy_bootstrap_preserves_already_initialized_new_ledger() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("budget");
        let vault = super::super::vault::fixture_vault(root.clone())?;
        let now = Utc.with_ymd_and_hms(2026, 10, 10, 5, 0, 0).unwrap();
        let ticket = reserve_daily_learning(&root, "current-call", 500_000, now)?;
        settle_daily_learning(&ticket, Some(250_000), false)?;
        let before = daily_learning_budget_status(&root, now)?;
        assert!(!bootstrap_legacy_daily_learning_in_vault(
            &vault,
            &"a".repeat(64),
            1_000_000,
            now
        )?);
        assert_eq!(daily_learning_budget_status(&root, now)?, before);
        Ok(())
    }
    #[test]
    fn daily_cap_unknown_usage_and_two_profiles_share_allowance() -> Result<()> {
        let root = tempfile::tempdir()?;
        let root = root.path().join("budget");
        let now = Utc.with_ymd_and_hms(2026, 10, 10, 2, 0, 0).unwrap();
        let a = reserve_daily_learning(&root, "harness:a", 1_500_000, now)?;
        settle_daily_learning(&a, None, false)?;
        assert!(reserve_daily_learning(&root, "knowledge:b", 500_001, now).is_err());
        assert_eq!(daily_learning_budget_status(&root, now)?.date, "2026-10-09");
        settle_daily_learning(&a, Some(100_000), false)?;
        let b = reserve_daily_learning(&root, "knowledge:b", 1_900_000, now)?;
        assert_eq!(
            daily_learning_budget_status(&root, now)?.remaining_microusd,
            0
        );
        assert!(settle_daily_learning(&b, Some(1_900_001), false).is_err());
        let next = now + Duration::hours(3);
        assert_eq!(
            daily_learning_budget_status(&root, next)?.remaining_microusd,
            2_000_000
        );
        Ok(())
    }
    #[test]
    fn eastern_day_handles_dst_midnights_and_status_does_not_create_state() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("absent");
        let spring = Utc.with_ymd_and_hms(2026, 3, 8, 6, 59, 0).unwrap();
        let status = daily_learning_budget_status(&root, spring)?;
        assert_eq!(
            status.resets_at,
            Utc.with_ymd_and_hms(2026, 3, 9, 4, 0, 0).unwrap()
        );
        let fall = Utc.with_ymd_and_hms(2026, 11, 1, 5, 59, 0).unwrap();
        assert_eq!(
            daily_learning_budget_status(&root, fall)?.resets_at,
            Utc.with_ymd_and_hms(2026, 11, 2, 5, 0, 0).unwrap()
        );
        assert!(!root.exists());
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningRoutePrice {
    pub input_microusd_per_million: u64,
    pub output_microusd_per_million: u64,
}
impl LearningRoutePrice {
    pub fn charge(&self, input: u64, output: u64) -> Result<u64> {
        let amount = u128::from(input)
            .checked_mul(u128::from(self.input_microusd_per_million))
            .and_then(|v| {
                u128::from(output)
                    .checked_mul(u128::from(self.output_microusd_per_million))
                    .and_then(|o| v.checked_add(o))
            })
            .context("learning price overflow")?;
        u64::try_from(amount.div_ceil(1_000_000)).context("learning charge overflow")
    }
}
#[derive(Debug, Clone)]
pub struct LearningCallBudget {
    pub root: PathBuf,
    pub routes: BTreeMap<String, LearningRoutePrice>,
}
thread_local! {
    static CALL_BUDGET: std::cell::RefCell<Option<LearningCallBudget>> = const { std::cell::RefCell::new(None) };
    static CALL_USAGE: std::cell::RefCell<Option<super::UsageV1>> = const { std::cell::RefCell::new(None) };
}
pub fn with_learning_call_budget<T>(
    budget: LearningCallBudget,
    run: impl FnOnce() -> Result<T>,
) -> Result<T> {
    struct Reset(Option<LearningCallBudget>);
    impl Drop for Reset {
        fn drop(&mut self) {
            CALL_BUDGET.with(|v| *v.borrow_mut() = self.0.take());
        }
    }
    let previous = CALL_BUDGET.with(|value| value.replace(Some(budget)));
    let _reset = Reset(previous);
    run()
}
pub(super) fn learning_call_reservation(
    route: &str,
) -> Result<(DailyLearningTicket, LearningRoutePrice)> {
    CALL_USAGE.with(|v| *v.borrow_mut() = None);
    CALL_ACCOUNTING.with(|v| *v.borrow_mut() = None);
    CALL_BUDGET.with(|value| {
        let value = value.borrow();
        let context = value
            .as_ref()
            .context("learning provider call requires native daily budget admission")?;
        let price = context
            .routes
            .get(route)
            .context("learning route has no eligible frozen USD price")?
            .clone();
        let maximum = price.charge(131_072, 8_192)?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let id = format!("knowledge:{}:{nonce}", std::process::id());
        let ticket = reserve_daily_learning(&context.root, &id, maximum, Utc::now())?;
        CALL_ACCOUNTING.with(|v| {
            *v.borrow_mut() = Some(LearningCallAccounting {
                provider_requests: None,
                reserved_provider_requests: 1,
                usage: None,
                reserved_microusd: maximum,
            })
        });
        Ok((ticket, price))
    })
}
pub(super) fn learning_call_settled(
    ticket: &DailyLearningTicket,
    price: &LearningRoutePrice,
    input: u64,
    output: u64,
    requests: u64,
) -> Result<()> {
    if input > 131_072 || output > 8_192 || requests != 1 {
        bail!("learning physical request exceeded its single-attempt bounds; reservation retained");
    }
    let cost = price.charge(input, output)?;
    settle_daily_learning(ticket, Some(cost), false)?;
    let usage = super::UsageV1 {
        input_tokens: input,
        output_tokens: output,
        estimated_microusd: cost,
    };
    CALL_USAGE.with(|v| *v.borrow_mut() = Some(usage.clone()));
    CALL_ACCOUNTING.with(|v| {
        if let Some(a) = v.borrow_mut().as_mut() {
            a.usage = Some(usage);
            a.provider_requests = Some(requests);
            a.reserved_provider_requests = 0;
            a.reserved_microusd = 0;
        }
    });
    Ok(())
}
pub(super) fn take_learning_call_usage() -> Option<super::UsageV1> {
    CALL_USAGE.with(|v| v.take())
}
pub(super) fn learning_call_usage() -> Option<super::UsageV1> {
    CALL_USAGE.with(|v| v.borrow().clone())
}
pub(super) fn physical_learning_call_count() -> Option<u64> {
    CALL_BUDGET.with(|b| {
        if b.borrow().is_none() {
            Some(1)
        } else {
            CALL_ACCOUNTING.with(|a| a.borrow().as_ref().map_or(Some(0), |a| a.provider_requests))
        }
    })
}

pub(super) fn eligible_learning_route(route: &str) -> bool {
    CALL_BUDGET.with(|v| {
        v.borrow()
            .as_ref()
            .is_none_or(|b| b.routes.contains_key(route))
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct LearningCallAccounting {
    pub provider_requests: Option<u64>,
    pub reserved_provider_requests: u64,
    pub usage: Option<super::UsageV1>,
    pub reserved_microusd: u64,
}
thread_local! {static CALL_ACCOUNTING:std::cell::RefCell<Option<LearningCallAccounting>>=const {std::cell::RefCell::new(None)};}
pub fn with_learning_call_budget_accounted<T>(
    budget: LearningCallBudget,
    run: impl FnOnce() -> Result<T>,
) -> Result<(T, Option<LearningCallAccounting>)> {
    CALL_ACCOUNTING.with(|v| *v.borrow_mut() = None);
    let value = with_learning_call_budget(budget, run)?;
    Ok((value, CALL_ACCOUNTING.with(|v| v.take())))
}
