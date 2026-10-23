//! The native knowledge worker owns evaluation; adoption remains exact-review only.
use crate::runtime::{ActiveProfile, RuntimePaths};
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{self as learning, LearningVault};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

pub(crate) const AUTOMATION_ID: &str = "knowledge-learning";
const LEARNING_INTERVAL_SECS: u64 = 900;
fn route_prices(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
) -> Result<(
    BTreeMap<String, String>,
    BTreeMap<String, learning::LearningRoutePrice>,
)> {
    let vault = LearningVault::existing_at(paths.home.join(".codex-team/self-improvement"))?
        .context("learning is not enrolled")?;
    let state = learning::load_state(&vault)?.context("learning is not enrolled")?;
    let execution = state
        .ca_execution_profile
        .context("learning execution profile is not enrolled")?;
    let config = crate::config::ConfigStore::for_profile(
        paths.clone().for_profile(profile).config_file,
        profile.clone(),
    )
    .load()?;
    let catalog = crate::pricing::PricingStore::for_paths(paths, profile)
        .load()
        .map_err(anyhow::Error::msg)?;
    let mut bindings = BTreeMap::new();
    let mut prices = BTreeMap::new();
    for route in execution.routes {
        for model in &route.models {
            let Some(provider) = config.providers.get(&route.provider) else {
                continue;
            };
            if !provider.models.contains(model) {
                continue;
            }
            let billing = config.provider_billing.get(&route.provider);
            let mode = if billing.is_some_and(|b| {
                matches!(
                    b,
                    crate::pricing::ProviderBillingConfig::MimoTokenPlan { .. }
                )
            }) {
                "mimo_token_plan"
            } else {
                "metered_usd"
            };
            let Some(card) = catalog
                .find_for_billing_mode(&route.provider, model, mode)
                .filter(|c| c.fresh_at(chrono::Utc::now()))
            else {
                continue;
            };
            let mut upper_card = card.clone();
            if let crate::pricing::CardPricing::MeteredUsd {
                output_per_million_usd,
                reasoning_output_per_million_usd,
                ..
            } = &mut upper_card.pricing
            {
                *output_per_million_usd =
                    (*output_per_million_usd).max(*reasoning_output_per_million_usd);
            }
            let money = |input, output| -> Option<crate::pricing::ExactRational> {
                match crate::pricing::forecast_for(&upper_card, billing, input, output)? {
                    crate::pricing::Forecast::IncrementalUsd(value) => Some(value),
                    crate::pricing::Forecast::SubscriptionAllocation { allocated_usd, .. } => {
                        Some(allocated_usd)
                    }
                }
            };
            let Some(input) = money(1_000_000, 0) else {
                continue;
            };
            let Some(mut output) = money(0, 1_000_000) else {
                continue;
            };
            if let crate::pricing::CardPricing::MeteredUsd {
                long_output_multiplier_numerator: n,
                long_output_multiplier_denominator: d,
                ..
            } = &upper_card.pricing
            {
                let Some(multiplier) =
                    crate::pricing::ExactRational::new(u128::from((*n).max(*d)), u128::from(*d))
                else {
                    continue;
                };
                let Some(multiplied) = output.checked_mul(multiplier) else {
                    continue;
                };
                output = multiplied;
            }
            let parse_micro = |usd: crate::pricing::ExactRational| -> Result<u64> {
                u64::try_from(
                    usd.numerator()
                        .checked_mul(1_000_000)
                        .context("learning price overflow")?
                        .div_ceil(usd.denominator()),
                )
                .context("learning rate overflow")
            };
            let key = format!("{}/{}", route.provider, model);
            bindings.insert(
                key.clone(),
                format!(
                    "{:x}",
                    Sha256::digest(serde_json::to_vec(&(card, billing))?)
                ),
            );
            prices.insert(
                key,
                learning::LearningRoutePrice {
                    input_microusd_per_million: parse_micro(input)?,
                    output_microusd_per_million: parse_micro(output)?,
                },
            );
        }
    }
    let providers = prices
        .keys()
        .filter_map(|key| key.split_once('/').map(|(p, _)| p))
        .collect::<std::collections::BTreeSet<_>>();
    if providers.len() < 2 {
        bail!("learning needs two enrolled current providers with fresh USD or subscription-allocation pricing; refresh prices before activation");
    }
    Ok((bindings, prices))
}
pub(crate) fn price_bindings(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
) -> Result<BTreeMap<String, String>> {
    Ok(route_prices(paths, profile)?.0)
}
pub(crate) fn validate_prices(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    expected: &BTreeMap<String, String>,
) -> Result<()> {
    if &price_bindings(paths, profile)? != expected {
        bail!(
            "learning eligible route pricing changed; refresh service enrollment before admission"
        );
    }
    Ok(())
}
pub(crate) fn credential_providers(vault_root: &Path, expected: &str) -> Result<Vec<String>> {
    let vault = LearningVault::existing_at(vault_root.into())?.context("learning vault missing")?;
    let state = learning::load_state(&vault)?.context("learning enrollment missing")?;
    if state.enrollment_digest != expected {
        bail!("learning enrollment drifted");
    }
    Ok(state
        .ca_execution_profile
        .context("learning execution profile missing")?
        .routes
        .into_iter()
        .map(|r| r.provider)
        .collect())
}
fn learning_rolling_budget() -> crate::automation::AutomationBudgetV1 {
    let mut budget = crate::automation::AutomationBudgetV1::rolling_default();
    budget.provider_requests = 43_200;
    budget.input_tokens = 43_200 * 131_072;
    budget.output_tokens = 43_200 * 8_192;
    budget.tool_calls = 43_200;
    budget.output_bytes = 43_200 * 1_048_576;
    // Thirty rolling days can intersect thirty-one Eastern calendar allowances.
    budget.max_cost_usd = "62".parse().expect("valid learning rolling allowance");
    budget
}
pub(crate) fn register_enrolled(paths: &RuntimePaths, profile: &ActiveProfile) -> Result<()> {
    if matches!(profile, ActiveProfile::Bare) {
        return Ok(());
    }
    let vault_root = paths.home.join(".codex-team/self-improvement");
    let Some(vault) = LearningVault::existing_at(vault_root.clone())? else {
        return Ok(());
    };
    let Some(state) = learning::load_state(&vault)? else {
        return Ok(());
    };
    if !state.enrolled || !state.continuous || state.pause.is_some() {
        return Ok(());
    }
    if !state
        .ca_evidence_profiles
        .iter()
        .any(|p| p.name == profile.label())
    {
        return Ok(());
    }
    use crate::automation::*;
    use crate::schedule_v2::*;
    let store = AutomationStore::for_paths(paths, profile);
    if store
        .records()?
        .iter()
        .any(|r| r.definition.id == AUTOMATION_ID)
    {
        return Ok(());
    }
    let prices = price_bindings(paths, profile)?;
    let now = chrono::Utc::now();
    let mut budget = AutomationBudgetV1::per_run_default();
    budget.provider_requests = 1;
    budget.input_tokens = 131_072;
    budget.output_tokens = 8_192;
    budget.tool_calls = 1;
    budget.runtime_secs = 950;
    let definition = AutomationDefinitionV1 {
        schema: AUTOMATION_SCHEMA.into(),
        id: AUTOMATION_ID.into(),
        name: "Knowledge learning".into(),
        automation_kind: AutomationKindV1::Standalone,
        schedule: ScheduleDefinitionV2 {
            version: 2,
            id: AUTOMATION_ID.into(),
            provider: None,
            model: None,
            workspace: paths.home.clone(),
            runtime_secs: 950,
            output_bytes: 1_048_576,
            tools: vec![],
            grants: vec![],
            allow_network: true,
            skills: vec![],
            workspace_policy: WorkspacePolicy::ExistingReadOnly,
            pinned_revision: None,
            trigger: ScheduleTriggerV2::Interval {
                seconds: LEARNING_INTERVAL_SECS,
                anchor: Some(now),
            },
            missed_run_policy: MissedRunPolicy::Skip,
            target: ScheduleTargetV2::KnowledgeLearning {
                vault_root,
                enrollment_digest: state.enrollment_digest,
            },
            delivery: ScheduleDeliveryV2::None,
        },
        reasoning_effort: None,
        pricing: PricingBindingV1::MeteredRoutes {
            price_card_sha256: prices,
        },
        context_sha256: vec![],
        per_run_budget: budget,
        rolling_30_day_budget: learning_rolling_budget(),
        notification_policy: AutomationNotificationPolicy::MeaningfulChanges,
    }
    .normalize(now)?;
    let digest = crate::automation::definition_digest(&definition)?;
    let count =
        crate::schedule::configured_schedule_count(paths, profile).map_err(anyhow::Error::msg)?;
    store.add_confirmed(definition, &digest, count, now)?;
    Ok(())
}
pub(crate) async fn run_cycle(
    paths: RuntimePaths,
    profile: ActiveProfile,
    vault_root: std::path::PathBuf,
    expected: String,
) -> Result<serde_json::Value> {
    let vault =
        LearningVault::existing_at(vault_root.clone())?.context("learning vault missing")?;
    vault.write_mutable("supervisor/native-worker-status.json",&serde_json::json!({"last_tick":chrono::Utc::now(),"stage":"evaluating","detail":"running one ca-owned learning step"}))?;
    let result = run_cycle_inner(paths, profile, vault_root, expected).await;
    let status = match &result {
        Ok(v) => {
            serde_json::json!({"last_tick":chrono::Utc::now(),"stage":"idle","detail":v.get("summary")})
        }
        Err(e) => {
            serde_json::json!({"last_tick":chrono::Utc::now(),"stage":"blocked","detail":learning::sanitize_trace_text(&e.to_string()).chars().take(1024).collect::<String>()})
        }
    };
    vault.write_mutable("supervisor/native-worker-status.json", &status)?;
    result
}
async fn run_cycle_inner(
    paths: RuntimePaths,
    profile: ActiveProfile,
    vault_root: std::path::PathBuf,
    expected: String,
) -> Result<serde_json::Value> {
    let status_root = vault_root.clone();
    let (_, prices) = route_prices(&paths, &profile)?;
    let budget = learning::LearningCallBudget {
        root: learning::daily_learning_budget_root(&paths.home),
        routes: prices,
    };
    let result = tokio::task::spawn_blocking(move || {
        let vault = LearningVault::existing_at(vault_root)?.context("learning vault missing")?;
        let state = learning::load_state(&vault)?.context("learning enrollment missing")?;
        if state.enrollment_digest != expected {
            bail!("learning enrollment drifted");
        }
        let mut execution = state
            .ca_execution_profile
            .clone()
            .context("execution profile missing")?;
        for r in &mut execution.routes {
            r.models
                .retain(|m| budget.routes.contains_key(&format!("{}/{}", r.provider, m)));
        }
        execution.routes.retain(|r| !r.models.is_empty());
        // Freeze experiments only across currently enrolled, priced routes.
        if execution.routes.len() < 2 {
            bail!("learning needs two enrolled priced routes for independent judging");
        }

        let binary = learning::active_component_binary(&vault, "ca")?;
        let bridge = learning::ProcessCaLearningBridgeClient {
            binary: binary.clone(),
        };
        let runner = learning::CaRoleRunLearningRunner {
            binary,
            profile: execution.name,
            vault: vault.clone(),
        };
        learning::with_learning_call_budget_accounted(budget, || {
            learning::run_learning_stage(&vault, &bridge, &runner)
        })
    })
    .await??;
    let after = learning::daily_learning_budget_status(
        &learning::daily_learning_budget_root(&paths.home),
        chrono::Utc::now(),
    )?;
    let (result, accounting) = result;
    let notification_status = if result.contains("needs review") {
        "changed"
    } else if result.contains("terminally rejected") {
        "completed"
    } else if result.contains("quarantined") {
        "failed"
    } else {
        "unchanged"
    };
    let status_vault =
        LearningVault::existing_at(status_root)?.context("learning vault disappeared")?;
    let pending_reviews = learning::pending_learning_reviews(&status_vault)?
        .into_iter()
        .map(|r| r.candidate_id)
        .collect::<Vec<_>>();
    let state = learning::load_state(&status_vault)?.context("learning state disappeared")?;
    let cycles = serde_json::json!({"pending_reviews":pending_reviews,"active_evaluation":if notification_status=="failed" {learning::workflow_summary(&status_vault,state.active_objective_id.as_deref())?} else {None}});
    let usage = stage_usage(accounting.as_ref());
    Ok(
        serde_json::json!({"status":notification_status,"summary":result,"cycles":cycles,"usage":usage,"physical_accounting":accounting,"daily_budget":after}),
    )
}
pub(crate) fn stage_usage(
    accounting: Option<&learning::LearningCallAccounting>,
) -> Option<serde_json::Value> {
    match accounting {
        None => Some(serde_json::json!({"provider_requests":0,"input_tokens":0,"output_tokens":0,"tool_calls":1,"runtime_secs":0,"output_bytes":0,"cost_usd":"0"})),
        Some(accounting) => accounting.usage.as_ref().map(|u| serde_json::json!({"provider_requests":accounting.provider_requests,"input_tokens":u.input_tokens,"output_tokens":u.output_tokens,"tool_calls":1,"runtime_secs":0,"output_bytes":0,"cost_usd":format!("{}.{:06}",u.estimated_microusd/1_000_000,u.estimated_microusd%1_000_000)})),
    }
}
pub(crate) fn status(paths: &RuntimePaths) -> Result<String> {
    let budget = learning::daily_learning_budget_status(
        &learning::daily_learning_budget_root(&paths.home),
        chrono::Utc::now(),
    )?;
    let mut lines = Vec::new();
    lines.push("Knowledge evaluation: memory, inert skills, and evidence-based steering; code generation uses the existing harness. Every adoption requires exact review.".into());
    if let Some(vault) =
        LearningVault::existing_at(paths.home.join(".codex-team/self-improvement"))?
    {
        if let Some(state) = learning::load_state(&vault)? {
            lines.push(format!(
                "Enrolled: {} · paused: {} · health: {} · last health check: {}",
                state.enrolled,
                state.pause.is_some(),
                vault.health()?.healthy,
                state.last_health_at.as_deref().unwrap_or("never")
            ));
            if let Some(pause) = state.pause {
                lines.push(format!("Paused: {}", pause.reason));
            }
            let pending = learning::pending_learning_reviews(&vault)?;
            lines.push(format!("Learning review inbox: {}/8", pending.len()));
            for review in pending {
                lines.push(format!(
                    "Needs review: {} · candidate {}",
                    review.objective_id,
                    review.candidate_id.as_deref().unwrap_or("unavailable")
                ));
            }
            if let Some(worker) =
                vault.read_mutable::<serde_json::Value>("supervisor/native-worker-status.json")?
            {
                lines.push(format!(
                    "Last worker tick: {} · {} · {}",
                    worker["last_tick"], worker["stage"], worker["detail"]
                ));
            } else {
                lines.push("Last worker tick: never".into());
            }
            if let Some(workflow) =
                learning::workflow_summary(&vault, state.active_objective_id.as_deref())?
            {
                lines.push(format!(
                    "Active evaluation: {} · {} · replay {}/{}",
                    workflow.objective_id,
                    workflow.stage,
                    workflow.replay_completed,
                    workflow.replay_total
                ));
            }
        }
    } else {
        lines.push("Learning service is not enrolled".into());
    }
    let profile = ActiveProfile::Default;
    if let Some(record) = crate::automation::AutomationStore::for_paths(paths, &profile)
        .records()?
        .into_iter()
        .find(|r| r.definition.id == AUTOMATION_ID)
    {
        lines.push(format!(
            "Next learning occurrence: {} · active run: {}",
            record
                .next_occurrence
                .map(|o| o.at.to_rfc3339())
                .unwrap_or_else(|| "none".into()),
            record
                .active_run_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "none".into())
        ));
        if let Some(reason) = record.blocked_reason {
            lines.push(format!("Blocked: {reason}"));
        }
        if let crate::automation::PricingBindingV1::MeteredRoutes { price_card_sha256 } =
            &record.definition.pricing
        {
            if let Err(e) = validate_prices(paths, &profile, price_card_sha256) {
                lines.push(format!("Admission blocked: {e}"));
            }
        }
    }
    lines.push(format!("Daily allowance: ${:.2} spent, ${:.2} reserved, ${:.2} remaining · resets {} (America/New_York)",budget.spent_microusd as f64/1e6,budget.reserved_microusd as f64/1e6,budget.remaining_microusd as f64/1e6,budget.resets_at));
    if budget.legacy_unknown_microusd > 0 {
        lines.push(format!("Migration blocked: prior background/harness usage is unavailable; ${:.2} remains reserved until Eastern reset", budget.legacy_unknown_microusd as f64 / 1e6));
    }
    lines.push("Review: ca learn candidate list; ca learn candidate show ID; ca learn candidate approve|reject ID --confirm DIGEST".into());
    Ok(lines.join("\n"))
}

/// Derive secret-free profile bytes through the existing planner, then intersect
/// the current configuration with the previously enrolled provider/model set.
pub(crate) fn plan_execution_refresh(
    paths: &RuntimePaths,
) -> Result<crate::config::LearningExecutionProfilePlan> {
    let temp = tempfile::tempdir()?;
    let mut plan = crate::config::plan_learning_execution_profile(
        &paths.config_file,
        &temp.path().join("not-created.toml"),
    )?;
    if let Some(vault) =
        LearningVault::existing_at(paths.home.join(".codex-team/self-improvement"))?
    {
        if let Some(state) = learning::load_state(&vault)? {
            if let Some(previous) = state.ca_execution_profile {
                let allow: BTreeMap<_, _> = previous
                    .routes
                    .into_iter()
                    .map(|r| (r.provider, r.models))
                    .collect();
                plan.providers.retain_mut(|(provider, models)| {
                    models.retain(|m| allow.get(provider).is_some_and(|old| old.contains(m)));
                    !models.is_empty()
                });
                let mut document: toml::Value = toml::from_str(&plan.contents)?;
                let providers = document
                    .get_mut("providers")
                    .and_then(toml::Value::as_table_mut)
                    .context("learning profile providers missing")?;
                providers.retain(|provider, _| plan.providers.iter().any(|(p, _)| p == provider));
                for (provider, models) in &plan.providers {
                    providers
                        .get_mut(provider)
                        .and_then(toml::Value::as_table_mut)
                        .context("learning provider missing")?
                        .insert(
                            "models".into(),
                            toml::Value::Array(
                                models.iter().cloned().map(toml::Value::String).collect(),
                            ),
                        );
                }
                if plan.providers.len() < 2 {
                    bail!("refresh retains fewer than two previously enrolled current providers; explicit enrollment is required to add routes");
                }
                let (provider, models) = &plan.providers[0];
                let defaults = document
                    .get_mut("defaults")
                    .and_then(toml::Value::as_table_mut)
                    .context("learning profile defaults missing")?;
                defaults.insert("provider".into(), toml::Value::String(provider.clone()));
                defaults.insert("model".into(), toml::Value::String(models[0].clone()));
                plan.contents = toml::to_string_pretty(&document)?;
                plan.digest = format!("{:x}", Sha256::digest(plan.contents.as_bytes()));
            }
        }
    }
    Ok(plan)
}
pub(crate) fn existing_profile_digest(path: &Path) -> Result<Option<String>> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > 1_048_576 => {
            bail!("learning execution profile must be a bounded regular file")
        }
        Ok(_) => Ok(Some(format!("{:x}", Sha256::digest(std::fs::read(path)?)))),
    }
}
pub(crate) fn install_execution_refresh(
    path: &Path,
    plan: &crate::config::LearningExecutionProfilePlan,
    expected: Option<&str>,
) -> Result<bool> {
    use std::io::Write;
    let actual = existing_profile_digest(path)?;
    if actual.as_deref() != expected {
        bail!("learning execution profile changed after refresh preview");
    }
    if actual.as_deref() == Some(plan.digest.as_str()) {
        return Ok(false);
    }
    let parent = path.parent().context("learning profile has no parent")?;
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    if let Some(digest) = actual {
        let recovery = parent.join("learning-recovery");
        std::fs::create_dir_all(&recovery)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&recovery, std::fs::Permissions::from_mode(0o700))?;
        }
        let saved = recovery.join(format!("ca-learning-{digest}.toml"));
        if !saved.exists() {
            let mut out = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&saved)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                out.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            }
            out.write_all(&std::fs::read(path)?)?;
            out.sync_all()?;
        }
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(plan.contents.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(true)
}

pub(crate) fn refresh_registration(paths: &RuntimePaths, profile: &ActiveProfile) -> Result<()> {
    let store = crate::automation::AutomationStore::for_paths(paths, profile);
    if let Some(record) = store
        .records()?
        .into_iter()
        .find(|r| r.definition.id == AUTOMATION_ID)
    {
        let vault = LearningVault::existing_at(paths.home.join(".codex-team/self-improvement"))?
            .context("learning vault missing")?;
        let state = learning::load_state(&vault)?.context("learning enrollment missing")?;
        let mut definition = record.definition;
        if let crate::schedule_v2::ScheduleTriggerV2::Interval { seconds, .. } =
            &mut definition.schedule.trigger
        {
            *seconds = LEARNING_INTERVAL_SECS;
        }
        definition.schedule.target = crate::schedule_v2::ScheduleTargetV2::KnowledgeLearning {
            vault_root: vault.root().into(),
            enrollment_digest: state.enrollment_digest,
        };
        definition.pricing = crate::automation::PricingBindingV1::MeteredRoutes {
            price_card_sha256: price_bindings(paths, profile)?,
        };
        let digest = crate::automation::definition_digest(&definition)?;
        store.update_confirmed(AUTOMATION_ID, definition, &digest, chrono::Utc::now())?;
    } else {
        register_enrolled(paths, profile)?;
    }
    Ok(())
}

pub(crate) fn bootstrap_legacy_budget(
    paths: &RuntimePaths,
    enrollment_digest: &str,
) -> Result<bool> {
    let now = chrono::Utc::now();
    let today = learning::eastern_learning_date(now)?;
    let mut known = crate::automation::legacy_learning_settled_microusd(paths, now)?;
    if let Some(vault) =
        LearningVault::existing_at(paths.home.join(".codex-team/self-improvement"))?
    {
        for trace in vault.list_immutable::<learning::LearningTraceV1>("traces")? {
            if !trace.role.starts_with("learning-") {
                continue;
            }
            let started = chrono::DateTime::parse_from_rfc3339(&trace.timing.started_at)?
                .with_timezone(&chrono::Utc);
            if learning::eastern_learning_date(started)? != today {
                continue;
            }
            if let Some(usage) = trace.usage.filter(|usage| usage.estimated_microusd > 0) {
                known = known
                    .checked_add(usage.estimated_microusd)
                    .context("legacy learning charge overflow")?;
            }
        }
    }
    learning::bootstrap_legacy_daily_learning(
        &learning::daily_learning_budget_root(&paths.home),
        enrollment_digest,
        known,
        now,
    )
}

pub(crate) fn stage_ready(paths: &RuntimePaths, profile: &ActiveProfile) -> Result<bool> {
    let Some(vault) = LearningVault::existing_at(paths.home.join(".codex-team/self-improvement"))?
    else {
        return Ok(false);
    };
    let Some(state) = learning::load_state(&vault)? else {
        return Ok(false);
    };
    if !state
        .ca_evidence_profiles
        .iter()
        .any(|p| p.name == profile.label())
    {
        return Ok(false);
    }
    let now = chrono::Utc::now();
    if !learning::learning_stage_ready(&vault, now, true)? {
        return Ok(false);
    }
    if learning::learning_stage_ready(&vault, now, false)? {
        return Ok(true);
    }
    let imported = vault
        .list_immutable::<learning::ImportedCaLearningIntentV1>("ca/intents")?
        .into_iter()
        .map(|intent| (intent.profile, intent.ca_intent_id))
        .collect::<std::collections::BTreeSet<_>>();
    let store = crate::learning::LearningStore::for_profile(paths, profile, false)
        .map_err(anyhow::Error::msg)?;
    let new_intent = store
        .export_supervised_intents(&state.enrollment_digest, None)
        .map_err(anyhow::Error::msg)?
        .intents
        .into_iter()
        .any(|intent| {
            !imported.contains(&(profile.label().to_string(), intent.intent_id.to_string()))
        });
    learning::learning_stage_ready(&vault, now, new_intent)
}

/// Import bounded host outcomes, verification results and child receipt references
/// as evidence. Unknown completion and coarse satisfaction signals are excluded.
pub(crate) fn harvest_outcomes(paths: &RuntimePaths, profile: &ActiveProfile) -> Result<usize> {
    use crate::session_outcome::{
        ExecutionStatus, UserFeedback, VerificationScope, VerificationStatus,
    };
    let Some(vault) = LearningVault::existing_at(paths.home.join(".codex-team/self-improvement"))?
    else {
        return Ok(0);
    };
    let Some(state) = learning::load_state(&vault)? else {
        return Ok(0);
    };
    let Some(execution) = state.ca_execution_profile.as_ref() else {
        return Ok(0);
    };
    let mut imported = 0;
    for outcome in crate::session_outcome::recent_outcomes(128, &paths.profile_data_dir(profile))? {
        if !outcome.evidence_complete || outcome.execution == ExecutionStatus::Running {
            continue;
        }
        let Some(provider) = outcome.provider.as_ref() else {
            continue;
        };
        let Some(model) = outcome.model.as_ref() else {
            continue;
        };
        if !execution
            .routes
            .iter()
            .any(|r| r.provider == *provider && r.models.contains(model))
        {
            continue;
        }
        let failed = outcome.execution == ExecutionStatus::Failed
            || outcome.verification == VerificationStatus::Failed
            || outcome.feedback == UserFeedback::Rejected;
        let verified = outcome.verification == VerificationStatus::Passed
            && outcome.verification_scope == VerificationScope::TaskContract;
        if !failed && !verified {
            continue;
        }
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&outcome)?));
        let id = format!("trace-{}", &digest[..32]);
        if vault
            .read_optional_immutable::<learning::LearningTraceV1>("traces", &id)?
            .is_some()
        {
            continue;
        }
        let response = serde_json::json!({"execution":outcome.execution,"verification":outcome.verification,"verification_scope":outcome.verification_scope,"feedback":outcome.feedback,"parent_task_id":outcome.parent_task_id,"root_task_id":outcome.root_task_id,"events":outcome.events.iter().rev().take(32).collect::<Vec<_>>(),"harness":outcome.harness});
        let raw = serde_json::to_string(&response)?;
        let prompt = outcome
            .events
            .iter()
            .find(|e| e.kind == "request" || e.kind == "task_request")
            .map(|e| e.excerpt.as_str())
            .unwrap_or("Replay the bounded host task evidence and its verification contract.");
        let now = chrono::Utc::now().to_rfc3339();
        let mut trace = learning::LearningTraceV1 {
            schema: learning::TRACE_SCHEMA.into(),
            trace_id: id,
            provider: provider.clone(),
            model: model.clone(),
            role: "native-task".into(),
            prompt: learning::sanitize_trace_text(prompt),
            response: learning::sanitize_trace_text(&raw)
                .chars()
                .take(16_384)
                .collect(),
            tool_events: vec![],
            task_identity: format!("{:x}", Sha256::digest(outcome.task_id.as_bytes())),
            repository_fingerprint: outcome.workspace_hash.clone(),
            acceptance_evidence_ids: outcome
                .events
                .iter()
                .filter(|e| {
                    e.kind.contains("verification")
                        || e.kind.contains("review")
                        || e.kind.contains("subagent")
                })
                .map(|e| e.id.clone())
                .take(32)
                .collect(),
            timing: learning::TimingV1 {
                started_at: chrono::DateTime::from_timestamp(
                    outcome.started_at.try_into().unwrap_or(0),
                    0,
                )
                .unwrap_or(chrono::Utc::now())
                .to_rfc3339(),
                finished_at: now.clone(),
                elapsed_millis: outcome.duration_secs.saturating_mul(1000),
            },
            usage: None,
            outcome: if failed { "failed" } else { "passed" }.into(),
            core_revision: state
                .shared_core_revision
                .clone()
                .unwrap_or_else(|| "builtin-core".into()),
            overlay_revision: state
                .provider_overlay_revisions
                .get(&format!("{provider}/{model}"))
                .cloned()
                .unwrap_or_else(|| "none".into()),
            created_at: now,
            digest: String::new(),
        };
        vault.write_immutable(&mut trace)?;
        imported += 1;
    }
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_refresh_uses_exact_old_bytes_keeps_recovery_and_retries_idempotently() -> Result<()>
    {
        let temp = tempfile::tempdir()?;
        let target = temp.path().join("profiles/ca-learning.toml");
        std::fs::create_dir_all(target.parent().unwrap())?;
        let old = b"old bounded profile bytes\n";
        std::fs::write(&target, old)?;
        let original = existing_profile_digest(&target)?.unwrap();
        let contents = "new bounded profile bytes\n".to_string();
        let digest = format!("{:x}", Sha256::digest(contents.as_bytes()));
        let plan = crate::config::LearningExecutionProfilePlan {
            contents: contents.clone(),
            digest: digest.clone(),
            providers: vec![],
        };
        assert!(install_execution_refresh(&target, &plan, Some(&"0".repeat(64))).is_err());
        assert_eq!(std::fs::read(&target)?, old);
        assert!(install_execution_refresh(&target, &plan, Some(&original))?);
        assert_eq!(std::fs::read_to_string(&target)?, contents);
        assert_eq!(
            std::fs::read(
                target
                    .parent()
                    .unwrap()
                    .join(format!("learning-recovery/ca-learning-{original}.toml"))
            )?,
            old
        );
        assert!(!install_execution_refresh(&target, &plan, Some(&digest))?);
        std::fs::write(&target, b"operator changed profile")?;
        assert!(install_execution_refresh(&target, &plan, Some(&digest)).is_err());
        assert_eq!(std::fs::read(&target)?, b"operator changed profile");
        Ok(())
    }
}
