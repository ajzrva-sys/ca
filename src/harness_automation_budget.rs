//! Durable reservations inside the ca-owned automation run. Unknown provider
//! outcomes keep their full reservation; observing a worker does not erase it.
use crate::automation::{AutomationBudgetV1, AutomationUsageCountersV1};
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{canonical_digest, LearningVault};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone)]
struct ContextBudget {
    run_id: String,
    budget: AutomationBudgetV1,
    digest: String,
}
tokio::task_local! { static BUDGET: ContextBudget; }

pub async fn with_budget<T>(
    budget: AutomationBudgetV1,
    operation: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    let run = ca_automation::runtime::current_run_store()
        .context("harness automation run ownership missing")?;
    let run_id = run.status()?.run_id;
    if run_id.is_empty()
        || !run_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        bail!("invalid harness automation run identity");
    }
    let digest = canonical_digest(&budget)?;
    BUDGET
        .scope(
            ContextBudget {
                run_id,
                budget,
                digest,
            },
            operation,
        )
        .await
}

#[derive(Clone, Serialize, Deserialize)]
struct Reservation {
    input: u64,
    output: u64,
    cost: u64,
    tools: u32,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    schema: String,
    run_id: String,
    budget_digest: String,
    plan_digest: String,
    requests: u32,
    input: u64,
    output: u64,
    cost: u64,
    tools: u32,
    pending: BTreeMap<String, Reservation>,
}
#[derive(Clone)]
pub(crate) struct Ticket {
    context: ContextBudget,
    path: String,
    request: String,
    daily: Option<codex_team::self_improvement::DailyLearningTicket>,
    local: bool,
}

pub(super) fn cost_usd(microusd: u64) -> Result<crate::usage::DecimalUsd> {
    format!("{}.{:06}", microusd / 1_000_000, microusd % 1_000_000)
        .parse()
        .map_err(anyhow::Error::msg)
}
fn counters(ledger: &Ledger) -> Result<AutomationUsageCountersV1> {
    let mut result = AutomationUsageCountersV1 {
        provider_requests: ledger.requests,
        input_tokens: ledger.input,
        output_tokens: ledger.output,
        tool_calls: ledger.tools,
        cost_usd: cost_usd(ledger.cost)?,
        ..Default::default()
    };
    let mut cost = ledger.cost;
    for pending in ledger.pending.values() {
        result.input_tokens = result
            .input_tokens
            .checked_add(pending.input)
            .context("harness input accounting overflow")?;
        result.output_tokens = result
            .output_tokens
            .checked_add(pending.output)
            .context("harness output accounting overflow")?;
        result.tool_calls = result
            .tool_calls
            .checked_add(pending.tools)
            .context("harness tool accounting overflow")?;
        cost = cost
            .checked_add(pending.cost)
            .context("harness cost accounting overflow")?;
    }
    result.cost_usd = cost_usd(cost)?;
    Ok(result)
}
pub(crate) async fn reserve(
    vault: &LearningVault,
    plan_digest: &str,
    request: &str,
    bounds: (u64, u64, u64, u32),
) -> Result<Option<Ticket>> {
    let (input, output, cost, tools) = bounds;
    let context = match BUDGET.try_with(Clone::clone) {
        Ok(context) => context,
        Err(_) => {
            if cost == 0 {
                return Ok(None);
            }
            let root = shared_daily_root(vault)?;
            let run_id = uuid::Uuid::new_v4().to_string();
            let daily = codex_team::self_improvement::reserve_daily_learning(
                &root,
                &format!("harness:{run_id}:{request}"),
                cost,
                chrono::Utc::now(),
            )?;
            let budget = AutomationBudgetV1::per_run_default();
            return Ok(Some(Ticket {
                context: ContextBudget {
                    run_id,
                    budget: budget.clone(),
                    digest: canonical_digest(&budget)?,
                },
                path: String::new(),
                request: request.into(),
                daily: Some(daily),
                local: false,
            }));
        }
    };
    let mut ticket = Ticket {
        path: format!("harness/automation/{}.json", context.run_id),
        context,
        request: request.into(),
        daily: None,
        local: true,
    };
    let operation = ticket.clone();
    let original_vault = vault.clone();
    let vault = vault.clone();
    let plan_digest = plan_digest.to_owned();
    tokio::task::spawn_blocking(move || {
        let _lock = vault.acquire_lock()?;
        let mut ledger: Ledger = vault
            .read_mutable(&operation.path)?
            .unwrap_or_else(|| Ledger {
                schema: "ca.harness-run-budget.v1".into(),
                run_id: operation.context.run_id.clone(),
                budget_digest: operation.context.digest.clone(),
                plan_digest: plan_digest.clone(),
                requests: 0,
                input: 0,
                output: 0,
                cost: 0,
                tools: 0,
                pending: BTreeMap::new(),
            });
        if ledger.run_id != operation.context.run_id
            || ledger.budget_digest != operation.context.digest
            || ledger.plan_digest != plan_digest
        {
            bail!("harness automation budget identity changed");
        }
        if ledger.pending.contains_key(&operation.request) {
            bail!("harness request reservation already exists");
        }
        ledger.requests = ledger
            .requests
            .checked_add(1)
            .context("harness request accounting overflow")?;
        let tools = tools.min(
            operation
                .context
                .budget
                .tool_calls
                .saturating_sub(ledger.tools),
        );
        ledger.pending.insert(
            operation.request.clone(),
            Reservation {
                input,
                output,
                cost,
                tools,
            },
        );
        let totals = counters(&ledger)?;
        let cap = &operation.context.budget;
        if totals.provider_requests > cap.provider_requests
            || totals.input_tokens > cap.input_tokens
            || totals.output_tokens > cap.output_tokens
            || totals.tool_calls > cap.tool_calls
            || totals.cost_usd > cap.max_cost_usd
        {
            bail!("ca automation inference budget exhausted before provider request");
        }
        vault.write_mutable(&operation.path, &ledger)?;
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    if cost > 0 {
        let root = shared_daily_root(&original_vault)?;
        let id = format!("harness:{}:{}", ticket.context.run_id, request);
        match codex_team::self_improvement::reserve_daily_learning(
            &root,
            &id,
            cost,
            chrono::Utc::now(),
        ) {
            Ok(daily) => ticket.daily = Some(daily),
            Err(error) => {
                finish(&original_vault, Some(ticket), None, true).await?;
                return Err(error);
            }
        }
    }
    Ok(Some(ticket))
}

pub(crate) async fn finish(
    vault: &LearningVault,
    ticket: Option<Ticket>,
    actual: Option<(u64, u64, u64, u32)>,
    proven_absent: bool,
) -> Result<()> {
    let Some(ticket) = ticket else { return Ok(()) };
    let daily = ticket.daily.clone();
    let daily_actual = actual.map(|(_, _, cost, _)| cost);
    if !ticket.local {
        if let Some(daily) = daily {
            codex_team::self_improvement::settle_daily_learning(
                &daily,
                daily_actual,
                proven_absent,
            )?;
        }
        return Ok(());
    }
    let vault = vault.clone();
    tokio::task::spawn_blocking(move || {
        let _lock = vault.acquire_lock()?;
        let mut ledger: Ledger = vault
            .read_mutable(&ticket.path)?
            .context("harness automation reservation missing")?;
        if ledger.run_id != ticket.context.run_id || ledger.budget_digest != ticket.context.digest {
            bail!("harness automation reservation ownership changed");
        }
        let reserved = ledger
            .pending
            .get(&ticket.request)
            .context("harness automation request reservation missing")?;
        if let Some((input, output, cost, tools)) = actual {
            if input > reserved.input
                || output > reserved.output
                || cost > reserved.cost
                || tools > reserved.tools
            {
                bail!(
                    "provider evidence exceeded ca automation reservation; effect remains unknown"
                );
            }
            ledger.input = ledger
                .input
                .checked_add(input)
                .context("harness input accounting overflow")?;
            ledger.output = ledger
                .output
                .checked_add(output)
                .context("harness output accounting overflow")?;
            ledger.cost = ledger
                .cost
                .checked_add(cost)
                .context("harness cost accounting overflow")?;
            ledger.tools = ledger
                .tools
                .checked_add(tools)
                .context("harness tool accounting overflow")?;
            ledger.pending.remove(&ticket.request);
        } else if proven_absent {
            ledger.pending.remove(&ticket.request);
            ledger.requests = ledger.requests.saturating_sub(1);
        }
        // With neither confirmed completion nor proven absence, keep the charge.
        vault.write_mutable(&ticket.path, &ledger).map(|_| ())
    })
    .await??;
    if let Some(daily) = daily {
        codex_team::self_improvement::settle_daily_learning(&daily, daily_actual, proven_absent)?;
    }
    Ok(())
}

pub(crate) fn current_usage(vault: &LearningVault) -> Result<Option<AutomationUsageCountersV1>> {
    let Ok(context) = BUDGET.try_with(Clone::clone) else {
        return Ok(None);
    };
    let ledger: Option<Ledger> =
        vault.read_mutable(&format!("harness/automation/{}.json", context.run_id))?;
    ledger
        .map(|ledger| {
            if ledger.budget_digest != context.digest {
                bail!("harness run budget changed");
            }
            counters(&ledger)
        })
        .transpose()
}

fn shared_daily_root(vault: &LearningVault) -> Result<std::path::PathBuf> {
    #[cfg(test)]
    {
        Ok(vault.root().join("fixture-daily-budget"))
    }
    #[cfg(not(test))]
    {
        let _ = vault;
        Ok(codex_team::self_improvement::daily_learning_budget_root(
            &crate::runtime::RuntimePaths::discover()?.home,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn manual_harness_experiments_share_daily_cap_and_unknown_usage_stays_reserved() {
        let temp = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(temp.path().join("harness")).unwrap();
        let first = reserve(&vault, &"a".repeat(64), "manual-a", (1, 1, 1_500_000, 0))
            .await
            .unwrap();
        finish(&vault, first.clone(), None, false).await.unwrap();
        assert!(
            reserve(&vault, &"a".repeat(64), "manual-b", (1, 1, 500_001, 0))
                .await
                .is_err()
        );
        finish(&vault, first, Some((1, 1, 100_000, 0)), false)
            .await
            .unwrap();
        reserve(&vault, &"a".repeat(64), "manual-c", (1, 1, 1_900_000, 0))
            .await
            .unwrap();
        let status = codex_team::self_improvement::daily_learning_budget_status(
            &shared_daily_root(&vault).unwrap(),
            chrono::Utc::now(),
        )
        .unwrap();
        assert_eq!(status.remaining_microusd, 0);
        assert_eq!(status.spent_microusd, 100_000);
        assert_eq!(status.reserved_microusd, 1_900_000);
    }

    #[tokio::test]
    async fn common_budget_reserves_before_effect_and_keeps_unknown_charges() {
        let temp = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(temp.path().join("vault")).unwrap();
        let mut budget = AutomationBudgetV1::per_run_default();
        budget.provider_requests = 1;
        budget.input_tokens = 16;
        budget.output_tokens = 8;
        let context = ContextBudget {
            run_id: "independent-run".into(),
            digest: canonical_digest(&budget).unwrap(),
            budget,
        };
        BUDGET
            .scope(context.clone(), async {
                let first = reserve(&vault, &"a".repeat(64), "request-a", (16, 8, 100, 0))
                    .await
                    .unwrap();
                finish(&vault, first, None, false).await.unwrap();
                let usage = current_usage(&vault).unwrap().unwrap();
                assert_eq!(usage.provider_requests, 1);
                assert_eq!(usage.input_tokens, 16);
                assert_eq!(usage.output_tokens, 8);
                assert!(reserve(&vault, &"a".repeat(64), "request-b", (1, 1, 1, 0))
                    .await
                    .is_err());
            })
            .await;
        // A new observer retains the same durable reservations rather than
        // resetting caps or interpreting its own cancellation as proof.
        BUDGET
            .scope(context, async {
                assert_eq!(current_usage(&vault).unwrap().unwrap().provider_requests, 1);
            })
            .await;
    }
    #[tokio::test]
    async fn common_budget_settles_confirmed_usage_and_rejects_changed_grants() {
        let temp = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(temp.path().join("vault")).unwrap();
        let budget = AutomationBudgetV1::per_run_default();
        let mut context = ContextBudget {
            run_id: "confirmed-run".into(),
            digest: canonical_digest(&budget).unwrap(),
            budget,
        };
        BUDGET
            .scope(context.clone(), async {
                let ticket = reserve(&vault, &"a".repeat(64), "request-a", (16, 8, 100, 4))
                    .await
                    .unwrap();
                finish(&vault, ticket, Some((3, 2, 10, 1)), false)
                    .await
                    .unwrap();
                let usage = current_usage(&vault).unwrap().unwrap();
                assert_eq!(usage.input_tokens, 3);
                assert_eq!(usage.output_tokens, 2);
                assert_eq!(usage.tool_calls, 1);
                let absent = reserve(&vault, &"a".repeat(64), "request-b", (16, 8, 100, 0))
                    .await
                    .unwrap();
                finish(&vault, absent, None, true).await.unwrap();
                assert_eq!(current_usage(&vault).unwrap().unwrap().provider_requests, 1);
            })
            .await;
        context.budget.provider_requests -= 1;
        context.digest = canonical_digest(&context.budget).unwrap();
        BUDGET
            .scope(context, async {
                assert!(reserve(&vault, &"a".repeat(64), "request-c", (1, 1, 1, 0))
                    .await
                    .is_err());
            })
            .await;
    }
}
