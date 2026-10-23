//! Additive local automation contracts and durable profile-scoped state.
//!
//! Existing Schedule V1/V2 bytes remain owned by `schedule`/`schedule_v2`.
//! This module stores only `ca.automation.v1` envelopes and their own journals.

use crate::config::ReasoningEffort;
use crate::runtime::{ActiveProfile, RuntimePaths};
use crate::schedule_v2::{
    due_occurrence, next_occurrence, normalize_at_confirmation, Occurrence, ScheduleDefinitionV2,
    ScheduleTargetV2,
};
use crate::usage::DecimalUsd;
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use uuid::Uuid;

#[path = "automation_notifications.rs"]
mod notifications;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AutomationOutputFormat {
    Text,
    Json,
}

#[derive(Debug, Parser)]
#[command(
    name = "ca automation",
    about = "Manage confirmed local automations and their run history"
)]
struct RawAutomationArgs {
    #[arg(long, value_name = "NAME")]
    profile: Option<String>,
    #[arg(long, value_enum, conflicts_with = "json")]
    output: Option<AutomationOutputFormat>,
    #[arg(long)]
    json: bool,
    #[command(subcommand)]
    command: AutomationCommand,
}

#[derive(Debug, Subcommand)]
enum AutomationCommand {
    #[command(hide = true, name = "__worker")]
    NativeWorker,
    /// Inspect the durable native execution, without rerunning effects
    Status {
        run: String,
    },
    /// Cancel the run's owned worker; preserve completed effects
    Cancel {
        run: String,
    },
    /// Reconcile persisted outcomes before considering continuation
    Reconcile {
        run: String,
    },
    /// List automations without exposing prompt bodies
    List,
    /// Inspect one automation definition
    Inspect {
        id: String,
    },
    /// Inspect local run receipts
    History {
        id: Option<String>,
    },
    /// Validate stores, routes, prices, and service readiness without mutation
    Doctor,
    /// Normalize a draft and print its exact confirmation digest
    Preview {
        path: PathBuf,
    },
    /// Create and enable an automation after exact preview confirmation
    Create {
        path: PathBuf,
        #[arg(long)]
        confirm: String,
    },
    /// Replace an automation after exact preview confirmation
    Update {
        id: String,
        path: PathBuf,
        #[arg(long)]
        confirm: String,
    },
    Pause {
        id: String,
    },
    Resume {
        id: String,
    },
    /// Run one occurrence immediately without changing its recurrence
    Run {
        id: String,
    },
    /// Cancel-first deletion requiring the exact automation ID
    Delete {
        id: String,
        #[arg(long, value_name = "ID")]
        confirm: String,
    },
    /// Preview or explicitly manage the existing scheduler LaunchAgent
    Service {
        #[command(subcommand)]
        command: AutomationServiceCommand,
    },
}

#[derive(Debug, Subcommand)]
enum AutomationServiceCommand {
    Preview,
    Install {
        #[arg(long)]
        confirm: String,
        #[arg(long, default_value = "/bin/launchctl")]
        launchctl: PathBuf,
    },
    Uninstall {
        #[arg(long)]
        confirm: String,
        #[arg(long, default_value = "/bin/launchctl")]
        launchctl: PathBuf,
    },
    Status {
        #[arg(long, default_value = "/bin/launchctl")]
        launchctl: PathBuf,
    },
}

#[derive(Debug)]
pub struct AutomationCliArgs {
    profile: ActiveProfile,
    output: AutomationOutputFormat,
    command: AutomationCommand,
}

pub fn parse_args<I, T>(args: I) -> Result<AutomationCliArgs, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let raw = RawAutomationArgs::try_parse_from(args)?;
    let profile = ActiveProfile::parse(raw.profile.as_deref()).map_err(|error| {
        <RawAutomationArgs as clap::CommandFactory>::command()
            .error(clap::error::ErrorKind::InvalidValue, error.to_string())
    })?;
    if matches!(profile, ActiveProfile::Bare) {
        return Err(
            <RawAutomationArgs as clap::CommandFactory>::command().error(
                clap::error::ErrorKind::ArgumentConflict,
                "--profile bare is non-persistent; automations are unavailable",
            ),
        );
    }
    Ok(AutomationCliArgs {
        profile,
        output: if raw.json {
            AutomationOutputFormat::Json
        } else {
            raw.output.unwrap_or(AutomationOutputFormat::Text)
        },
        command: raw.command,
    })
}

#[derive(Debug, Serialize)]
struct AutomationListItem<'a> {
    id: &'a str,
    name: &'a str,
    kind: &'static str,
    state: &'static str,
    next_at: Option<DateTime<Utc>>,
    provider: Option<&'a str>,
    model: Option<&'a str>,
    reasoning_effort: Option<ReasoningEffort>,
    notification_policy: AutomationNotificationPolicy,
    definition_sha256: &'a str,
}

pub async fn run_cli(cli: AutomationCliArgs) -> i32 {
    match run_cli_inner(cli).await {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ca automation: {error}");
            match error {
                AutomationError::Invalid(_)
                | AutomationError::Confirmation(_)
                | AutomationError::Duplicate(_)
                | AutomationError::Missing(_) => 2,
                AutomationError::Busy => 4,
                AutomationError::State(_) | AutomationError::Io(_) => 8,
            }
        }
    }
}

async fn run_cli_inner(cli: AutomationCliArgs) -> Result<(), AutomationError> {
    let paths = RuntimePaths::discover().map_err(|error| AutomationError::Io(error.to_string()))?;
    let store = AutomationStore::for_paths(&paths, &cli.profile);
    let cancel = matches!(&cli.command, AutomationCommand::Cancel { .. });
    let reconcile = matches!(&cli.command, AutomationCommand::Reconcile { .. });
    match cli.command {
        AutomationCommand::NativeWorker => {
            crate::automation_runtime::run_stdio(&paths, &cli.profile)
                .await
                .map_err(AutomationError::State)?
        }
        AutomationCommand::Status { run }
        | AutomationCommand::Cancel { run }
        | AutomationCommand::Reconcile { run } => {
            let receipt = crate::automation_runtime::inspect_run(
                &paths,
                &cli.profile,
                &run,
                reconcile,
                cancel,
            )
            .map_err(AutomationError::State)?;
            write_json(&receipt)?;
        }
        AutomationCommand::List => {
            let records = store.records()?;
            let items = records
                .iter()
                .map(list_item)
                .collect::<Vec<AutomationListItem<'_>>>();
            match cli.output {
                AutomationOutputFormat::Json => write_json(&serde_json::json!({
                    "schema": "ca.automation-list.v1",
                    "profile": cli.profile.label(),
                    "automations": items,
                }))?,
                AutomationOutputFormat::Text => {
                    if items.is_empty() {
                        println!("No local automations configured");
                    } else {
                        for item in items {
                            println!(
                                "{}\t{}\t{}\t{}\tnext={}\troute={}/{}\treasoning={}",
                                item.id,
                                item.state,
                                item.kind,
                                item.name,
                                item.next_at
                                    .map(|value| value.to_rfc3339())
                                    .unwrap_or_else(|| "-".into()),
                                item.provider.unwrap_or("-"),
                                item.model.unwrap_or("-"),
                                item.reasoning_effort
                                    .map(|value| value.to_string())
                                    .unwrap_or_else(|| "-".into()),
                            );
                        }
                    }
                }
            }
        }
        AutomationCommand::Inspect { id } => {
            validate_id(&id)?;
            let record = store
                .records()?
                .into_iter()
                .find(|record| record.definition.id == id)
                .ok_or_else(|| AutomationError::Missing(id.clone()))?;
            match cli.output {
                AutomationOutputFormat::Json => write_json(&record)?,
                AutomationOutputFormat::Text => {
                    println!(
                        "Automation {} · {}",
                        record.definition.id, record.definition.name
                    );
                    println!("state: {}", record_state(&record));
                    println!("kind: {}", kind_label(&record.definition.automation_kind));
                    println!("definition_sha256: {}", record.definition_sha256);
                    println!(
                        "route: {}/{}",
                        record
                            .definition
                            .schedule
                            .provider
                            .as_deref()
                            .unwrap_or("-"),
                        record.definition.schedule.model.as_deref().unwrap_or("-")
                    );
                    println!(
                        "reasoning: {}",
                        record
                            .definition
                            .reasoning_effort
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".into())
                    );
                    println!(
                        "next: {}",
                        record
                            .next_occurrence
                            .map(|value| value.at.to_rfc3339())
                            .unwrap_or_else(|| "-".into())
                    );
                    println!("notifications: {:?}", record.definition.notification_policy);
                    if let Some(reason) = record.blocked_reason {
                        println!("blocked: {reason}");
                    }
                }
            }
        }
        AutomationCommand::Preview { path } => {
            let definition = prepare_definition(&paths, &cli.profile, &path, Utc::now())?;
            let digest = definition_digest(&definition)?;
            match cli.output {
                AutomationOutputFormat::Json => write_json(&serde_json::json!({
                    "schema": "ca.automation-preview.v1",
                    "definition": definition,
                    "confirmation_sha256": digest,
                    "starts_active": true,
                    "service_installation_separate": true,
                }))?,
                AutomationOutputFormat::Text => {
                    println!("Automation preview · {}", definition.name);
                    println!("id: {}", definition.id);
                    println!("kind: {}", kind_label(&definition.automation_kind));
                    println!("workspace: {}", definition.schedule.workspace.display());
                    println!(
                        "route: {}/{}",
                        definition.schedule.provider.as_deref().unwrap_or("-"),
                        definition.schedule.model.as_deref().unwrap_or("-")
                    );
                    println!("network: {}", definition.schedule.allow_network);
                    println!("tools: {}", definition.schedule.tools.len());
                    println!("starts active: yes");
                    println!("service setup: separate confirmation required");
                    println!("confirmation_sha256: {digest}");
                }
            }
        }
        AutomationCommand::Create { path, confirm } => {
            let now = Utc::now();
            let definition = prepare_definition(&paths, &cli.profile, &path, now)?;
            let classic = crate::schedule::configured_schedule_count(&paths, &cli.profile)
                .map_err(AutomationError::State)?;
            let record = store.add_confirmed(definition, &confirm, classic, now)?;
            mutation_output(cli.output, "created", &record)?;
        }
        AutomationCommand::Update { id, path, confirm } => {
            let now = Utc::now();
            let definition = prepare_definition(&paths, &cli.profile, &path, now)?;
            let record = store.update_confirmed(&id, definition, &confirm, now)?;
            mutation_output(cli.output, "updated", &record)?;
        }
        AutomationCommand::Pause { id } => {
            let record = store.set_enabled(&id, false, Utc::now())?;
            mutation_output(cli.output, "paused", &record)?;
        }
        AutomationCommand::Resume { id } => {
            let record = store.set_enabled(&id, true, Utc::now())?;
            mutation_output(cli.output, "resumed", &record)?;
        }
        AutomationCommand::Delete { id, confirm } => {
            store.remove(&id, &confirm)?;
            match cli.output {
                AutomationOutputFormat::Json => write_json(&serde_json::json!({
                    "schema": "ca.automation-mutation.v1",
                    "action": "deleted",
                    "id": id,
                }))?,
                AutomationOutputFormat::Text => println!("Deleted automation {id}"),
            }
        }
        AutomationCommand::History { id } => {
            let history =
                AutomationRunJournal::for_paths(&paths, &cli.profile).history(id.as_deref())?;
            match cli.output {
                AutomationOutputFormat::Json => write_json(&serde_json::json!({
                    "schema": "ca.automation-history.v1",
                    "runs": history,
                }))?,
                AutomationOutputFormat::Text => {
                    if history.is_empty() {
                        println!("No automation runs recorded");
                    } else {
                        for event in history {
                            println!(
                                "{}\t{}\t{}\t{}",
                                event.run_id,
                                event.automation_id,
                                event.state.label(),
                                event.at.to_rfc3339()
                            );
                        }
                    }
                }
            }
        }
        AutomationCommand::Doctor => {
            let records = store.records()?;
            let classic = crate::schedule::configured_schedule_count(&paths, &cli.profile)
                .map_err(AutomationError::State)?;
            if classic.saturating_add(records.len()) > MAX_COMBINED_AUTOMATIONS {
                return Err(AutomationError::State(
                    "combined schedule and automation limit exceeded".into(),
                ));
            }
            for record in &records {
                validate_configured_definition(
                    &paths,
                    &cli.profile,
                    &record.definition,
                    Utc::now(),
                )?;
            }
            let journal = AutomationRunJournal::for_paths(&paths, &cli.profile);
            let run_events = journal.history(None)?.len();
            match cli.output {
                AutomationOutputFormat::Json => write_json(&serde_json::json!({
                    "schema": "ca.automation-doctor.v1",
                    "ready": true,
                    "classic_schedules": classic,
                    "automations": records.len(),
                    "run_events": run_events,
                }))?,
                AutomationOutputFormat::Text => println!(
                    "Automation state ready · {} automation(s) · {} classic schedule(s) · {} run event(s)",
                    records.len(),
                    classic,
                    run_events
                ),
            }
        }
        AutomationCommand::Run { id } => {
            recover_pending_deliveries(&paths, &cli.profile)?;
            let claim = store.claim_immediate(cli.profile.label(), &id, Utc::now())?;
            execute_claim(&paths, &cli.profile, &store, claim, Some(cli.output)).await?;
        }
        AutomationCommand::Service { command } => {
            let executable =
                std::env::current_exe().map_err(|error| AutomationError::Io(error.to_string()))?;
            let preview =
                crate::schedule_v2::launch_agent_preview(&paths, &cli.profile, &executable)
                    .map_err(|error| AutomationError::State(error.to_string()))?;
            let (action, confirmation, launchctl) = match command {
                AutomationServiceCommand::Preview => (
                    crate::schedule_v2::ServiceAction::Preview,
                    None,
                    PathBuf::from("/bin/launchctl"),
                ),
                AutomationServiceCommand::Install { confirm, launchctl } => (
                    crate::schedule_v2::ServiceAction::Install,
                    Some(confirm),
                    launchctl,
                ),
                AutomationServiceCommand::Uninstall { confirm, launchctl } => (
                    crate::schedule_v2::ServiceAction::Uninstall,
                    Some(confirm),
                    launchctl,
                ),
                AutomationServiceCommand::Status { launchctl } => {
                    (crate::schedule_v2::ServiceAction::Status, None, launchctl)
                }
            };
            let result = crate::schedule_v2::service_mutation(
                action,
                &preview,
                confirmation.as_deref(),
                &launchctl,
            )
            .map_err(|error| AutomationError::State(error.to_string()))?;
            match cli.output {
                AutomationOutputFormat::Json => write_json(&serde_json::json!({
                    "schema": "ca.automation-service.v1",
                    "action": format!("{action:?}").to_ascii_lowercase(),
                    "status": result,
                    "confirmation_sha256": preview.digest,
                    "path": preview.path,
                }))?,
                AutomationOutputFormat::Text => {
                    if action == crate::schedule_v2::ServiceAction::Preview {
                        print!("{result}");
                        println!("confirmation_sha256: {}", preview.digest);
                    } else {
                        println!("Automation service: {result}");
                    }
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn prepare_definition(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    path: &Path,
    now: DateTime<Utc>,
) -> Result<AutomationDefinitionV1, AutomationError> {
    prepare_proposed_definition(paths, profile, load_definition_file(path)?, now)
}

/// Normalize an in-memory proposal. This does not write a draft or authorize
/// creating an automation; the returned digest still requires confirmation.
pub(crate) fn prepare_proposed_definition(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    mut definition: AutomationDefinitionV1,
    now: DateTime<Utc>,
) -> Result<AutomationDefinitionV1, AutomationError> {
    let profile_paths = paths.clone().for_profile(profile);
    let config =
        crate::config::ConfigStore::for_profile(profile_paths.config_file, profile.clone())
            .load()
            .map_err(|error| AutomationError::Invalid(error.to_string()))?;
    match (
        definition.schedule.provider.as_deref(),
        definition.schedule.model.as_deref(),
    ) {
        (None, None) => {
            definition.reasoning_effort = None;
            if matches!(
                definition.schedule.target,
                ScheduleTargetV2::KnowledgeLearning { .. }
            ) {
                definition.pricing = PricingBindingV1::MeteredRoutes {
                    price_card_sha256: crate::learning_automation::price_bindings(paths, profile)
                        .map_err(|e| AutomationError::Invalid(e.to_string()))?,
                };
            } else {
                definition.pricing = PricingBindingV1::ModelFree;
            }
        }
        (Some(provider_id), Some(model_id)) => {
            let provider = config.providers.get(provider_id).ok_or_else(|| {
                AutomationError::Invalid("automation provider is not configured".into())
            })?;
            if !provider
                .models
                .iter()
                .any(|configured| configured == model_id)
            {
                return Err(AutomationError::Invalid(
                    "automation model is not configured for the selected provider".into(),
                ));
            }
            let model_profile = config.effective_model_profile(provider_id, model_id);
            definition.reasoning_effort = match (definition.reasoning_effort, model_profile) {
                (Some(effort), Some(model_profile))
                    if model_profile.reasoning_efforts.contains(&effort) =>
                {
                    Some(effort)
                }
                (Some(_), _) => {
                    return Err(AutomationError::Invalid(
                        "automation reasoning effort is unsupported by the exact route".into(),
                    ));
                }
                (None, Some(model_profile)) => model_profile
                    .default_reasoning_effort
                    .filter(|effort| model_profile.reasoning_efforts.contains(effort)),
                (None, None) => None,
            };
            definition.pricing = if literal_loopback_url(&provider.base_url) {
                PricingBindingV1::LocalZero
            } else {
                let catalog = crate::pricing::PricingStore::for_paths(paths, profile)
                    .load()
                    .map_err(|error| AutomationError::Invalid(error.to_string()))?;
                let card = catalog.find(provider_id, model_id).ok_or_else(|| {
                    AutomationError::Invalid(
                        "unknown-priced remote automation routes cannot run unattended".into(),
                    )
                })?;
                if !card.fresh_at(now)
                    || !matches!(card.pricing, crate::pricing::CardPricing::MeteredUsd { .. })
                {
                    return Err(AutomationError::Invalid(
                        "automation route lacks a fresh metered price card".into(),
                    ));
                }
                let digest = hex_digest(&Sha256::digest(
                    serde_json::to_vec(card)
                        .map_err(|error| AutomationError::Invalid(error.to_string()))?,
                ));
                PricingBindingV1::Metered {
                    price_card_sha256: digest,
                }
            };
        }
        _ => {
            return Err(AutomationError::Invalid(
                "automation provider and model must both be present or both omitted".into(),
            ));
        }
    }
    if let AutomationKindV1::Heartbeat {
        session_id,
        workspace_identity_sha256,
    } = &definition.automation_kind
    {
        let sessions = crate::session::SessionStore::with_archive(
            profile_paths.sessions_dir,
            profile_paths.archived_sessions_dir,
        );
        // Definition preparation runs in the management worker. Keep cold
        // legacy identity parsing out of later short owned-write guards.
        sessions.prepare_identity_inventory().map_err(|error| {
            AutomationError::Invalid(format!(
                "heartbeat session identity preparation failed: {error}"
            ))
        })?;
        let (session, _) = sessions
            .automation_snapshot(*session_id)
            .map_err(|_| AutomationError::Invalid("heartbeat session is unavailable".into()))?;
        let expected_workspace = session_workspace_identity(&session)?;
        if *workspace_identity_sha256 != expected_workspace {
            return Err(AutomationError::Invalid(
                "heartbeat workspace identity does not match its saved session".into(),
            ));
        }
        let context_digest = session_context_authority_digest(&session)?;
        definition.context_sha256 = vec![context_digest];
    }
    let definition = definition.normalize(now)?;
    crate::harness_automation::validate_target(
        &definition.schedule,
        &definition.per_run_budget,
        &definition.pricing,
    )
    .map_err(|e| AutomationError::Invalid(e.to_string()))?;
    Ok(definition)
}

pub(crate) fn validate_configured_definition(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    definition: &AutomationDefinitionV1,
    now: DateTime<Utc>,
) -> Result<(), AutomationError> {
    crate::harness_automation::validate_target(
        &definition.schedule,
        &definition.per_run_budget,
        &definition.pricing,
    )
    .map_err(|e| AutomationError::State(e.to_string()))?;
    let profile_paths = paths.clone().for_profile(profile);
    let config =
        crate::config::ConfigStore::for_profile(profile_paths.config_file, profile.clone())
            .load()
            .map_err(|error| AutomationError::State(error.to_string()))?;
    match (
        definition.schedule.provider.as_deref(),
        definition.schedule.model.as_deref(),
        &definition.pricing,
    ) {
        (None, None, PricingBindingV1::MeteredRoutes { price_card_sha256 })
            if matches!(
                definition.schedule.target,
                ScheduleTargetV2::KnowledgeLearning { .. }
            ) =>
        {
            crate::learning_automation::validate_prices(paths, profile, price_card_sha256)
                .map_err(|e| AutomationError::State(e.to_string()))?;
        }
        (None, None, PricingBindingV1::ModelFree) => {}
        (Some(provider_id), Some(model_id), pricing) => {
            let provider = config.providers.get(provider_id).ok_or_else(|| {
                AutomationError::State("automation provider is no longer configured".into())
            })?;
            if !provider
                .models
                .iter()
                .any(|configured| configured == model_id)
            {
                return Err(AutomationError::State(
                    "automation model is no longer configured".into(),
                ));
            }
            if let Some(effort) = definition.reasoning_effort {
                if config
                    .effective_model_profile(provider_id, model_id)
                    .is_none_or(|profile| !profile.reasoning_efforts.contains(&effort))
                {
                    return Err(AutomationError::State(
                        "automation reasoning contract drifted".into(),
                    ));
                }
            }
            match pricing {
                PricingBindingV1::LocalZero if literal_loopback_url(&provider.base_url) => {}
                PricingBindingV1::Metered { price_card_sha256 } => {
                    let catalog = crate::pricing::PricingStore::for_paths(paths, profile)
                        .load()
                        .map_err(|error| AutomationError::State(error.to_string()))?;
                    let card = catalog.find(provider_id, model_id).ok_or_else(|| {
                        AutomationError::State("automation price card is unavailable".into())
                    })?;
                    let digest = hex_digest(&Sha256::digest(
                        serde_json::to_vec(card)
                            .map_err(|error| AutomationError::State(error.to_string()))?,
                    ));
                    if &digest != price_card_sha256 || !card.fresh_at(now) {
                        return Err(AutomationError::State(
                            "automation pricing contract drifted".into(),
                        ));
                    }
                }
                _ => {
                    return Err(AutomationError::State(
                        "automation route authority or pricing contract drifted".into(),
                    ));
                }
            }
        }
        _ => {
            return Err(AutomationError::State(
                "automation route contract is invalid".into(),
            ));
        }
    }
    if let AutomationKindV1::Heartbeat {
        session_id,
        workspace_identity_sha256,
    } = &definition.automation_kind
    {
        let sessions = crate::session::SessionStore::with_archive(
            profile_paths.sessions_dir,
            profile_paths.archived_sessions_dir,
        );
        sessions.prepare_identity_inventory().map_err(|error| {
            AutomationError::State(format!(
                "heartbeat session identity preparation failed: {error}"
            ))
        })?;
        let (session, _) = sessions
            .automation_snapshot(*session_id)
            .map_err(|_| AutomationError::State("heartbeat session is unavailable".into()))?;
        if session_workspace_identity(&session)? != *workspace_identity_sha256
            || definition.context_sha256 != vec![session_context_authority_digest(&session)?]
        {
            return Err(AutomationError::State(
                "heartbeat session context authority drifted".into(),
            ));
        }
    }
    Ok(())
}

fn literal_loopback_url(value: &str) -> bool {
    reqwest::Url::parse(value).ok().is_some_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url
                .host_str()
                .and_then(|host| host.parse::<std::net::IpAddr>().ok())
                .is_some_and(|address| address.is_loopback())
    })
}

fn session_workspace_identity(
    session: &crate::session::SessionV2,
) -> Result<String, AutomationError> {
    let mut roots = if session.workspace_roots.is_empty() {
        vec![session.workspace.clone()]
    } else {
        session.workspace_roots.clone()
    };
    roots.retain(|root| !root.is_empty());
    roots.sort();
    roots.dedup();
    let bytes =
        serde_json::to_vec(&roots).map_err(|error| AutomationError::State(error.to_string()))?;
    Ok(hex_digest(&Sha256::digest(bytes)))
}

fn session_context_authority_digest(
    session: &crate::session::SessionV2,
) -> Result<String, AutomationError> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "manual_rules": session.manual_rule_ids,
        "active_skills": session.active_skills,
        "persistent_skills": session.persistent_skill_refs,
    }))
    .map_err(|error| AutomationError::State(error.to_string()))?;
    Ok(hex_digest(&Sha256::digest(bytes)))
}

fn list_item(record: &AutomationRecordV1) -> AutomationListItem<'_> {
    AutomationListItem {
        id: &record.definition.id,
        name: &record.definition.name,
        kind: kind_label(&record.definition.automation_kind),
        state: record_state(record),
        next_at: record.next_occurrence.as_ref().map(|value| value.at),
        provider: record.definition.schedule.provider.as_deref(),
        model: record.definition.schedule.model.as_deref(),
        reasoning_effort: record.definition.reasoning_effort,
        notification_policy: record.definition.notification_policy,
        definition_sha256: &record.definition_sha256,
    }
}

fn kind_label(kind: &AutomationKindV1) -> &'static str {
    match kind {
        AutomationKindV1::Heartbeat { .. } => "heartbeat",
        AutomationKindV1::Standalone => "standalone",
    }
}

fn record_state(record: &AutomationRecordV1) -> &'static str {
    if record.blocked_reason.is_some() {
        "blocked"
    } else if record.active_run_id.is_some() {
        "running"
    } else if record.enabled {
        "enabled"
    } else {
        "paused"
    }
}

fn write_json(value: &impl Serialize) -> Result<(), AutomationError> {
    let output = serde_json::to_string_pretty(value)
        .map_err(|error| AutomationError::State(error.to_string()))?;
    println!("{output}");
    Ok(())
}

fn mutation_output(
    format: AutomationOutputFormat,
    action: &str,
    record: &AutomationRecordV1,
) -> Result<(), AutomationError> {
    match format {
        AutomationOutputFormat::Json => write_json(&serde_json::json!({
            "schema": "ca.automation-mutation.v1",
            "action": action,
            "id": record.definition.id,
            "state": record_state(record),
            "definition_sha256": record.definition_sha256,
        })),
        AutomationOutputFormat::Text => {
            println!("{} automation {}", title_case(action), record.definition.id);
            Ok(())
        }
    }
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

async fn execute_claim(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    store: &AutomationStore,
    mut claim: AutomationClaimV1,
    output: Option<AutomationOutputFormat>,
) -> Result<(), AutomationError> {
    let _run_lease = match claim.runtime_lease.take() {
        Some(lease) => lease,
        None => run_access(&store.root)
            .try_writer(claim.run_id)
            .map_err(access_error)?,
    };
    run_access(&store.root)
        .validate(&_run_lease, claim.run_id)
        .map_err(access_error)?;
    if !store.validates_active_claim(&claim)? {
        return Err(AutomationError::State(
            "automation claim changed before execution".into(),
        ));
    }
    let profile_paths = paths.clone().for_profile(profile);
    let sessions = crate::session::SessionStore::with_archive(
        profile_paths.sessions_dir,
        profile_paths.archived_sessions_dir,
    );
    let heartbeat_lease = match &claim.definition.automation_kind {
        AutomationKindV1::Heartbeat { session_id, .. } => {
            match sessions.try_writer_lease(*session_id) {
                Ok(lease) => Some(lease),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    store.defer_busy_heartbeat(&claim, Utc::now())?;
                    return Err(AutomationError::Busy);
                }
                Err(error) => return Err(access_error(error)),
            }
        }
        AutomationKindV1::Standalone => None,
    };
    prepare_session_identity_inventory(&sessions).await?;
    let (history, heartbeat_binding) = match &claim.definition.automation_kind {
        AutomationKindV1::Heartbeat { session_id, .. } => {
            let (session, binding) = sessions
                .automation_snapshot(*session_id)
                .map_err(|_| AutomationError::State("heartbeat session is unavailable".into()))?;
            (bounded_heartbeat_history(session.messages)?, Some(binding))
        }
        AutomationKindV1::Standalone => (Vec::new(), None),
    };
    let intent = DeliveryIntentV1::new(&claim, heartbeat_binding);
    write_run_intent(paths, profile, &intent)?;
    let journal = AutomationRunJournal::for_paths(paths, profile);
    journal.append(
        &claim,
        AutomationRunStateV1::Claimed,
        RunEventUpdate::default(),
        Utc::now(),
    )?;
    if let Err(error) =
        validate_configured_definition(paths, profile, &claim.definition, Utc::now())
    {
        store.block(&claim.automation_id, &error.to_string())?;
        journal.append(
            &claim,
            AutomationRunStateV1::Blocked,
            RunEventUpdate {
                reason_class: Some("authority_or_pricing_drift".into()),
                ..Default::default()
            },
            Utc::now(),
        )?;
        return Err(error);
    }

    let budget = AutomationBudgetLedger::for_paths(paths, profile);
    let reserved = match budget.reserve(&claim, Utc::now()) {
        Ok(reserved) => reserved,
        Err(error) => {
            store.block(&claim.automation_id, &error.to_string())?;
            journal.append(
                &claim,
                AutomationRunStateV1::Blocked,
                RunEventUpdate {
                    reason_class: Some("budget_reservation_failed".into()),
                    ..Default::default()
                },
                Utc::now(),
            )?;
            return Err(error);
        }
    };
    journal.append(
        &claim,
        AutomationRunStateV1::BudgetReserved,
        RunEventUpdate {
            reserved: Some(reserved.clone()),
            ..Default::default()
        },
        Utc::now(),
    )?;
    journal.append(
        &claim,
        AutomationRunStateV1::Running,
        RunEventUpdate::default(),
        Utc::now(),
    )?;
    let started = Utc::now();
    let execution = crate::schedule::execute_automation_target(
        paths,
        profile,
        &claim.definition,
        claim.run_id,
        history,
    )
    .await;
    let completed = Utc::now();
    let execution = match execution {
        Ok(execution) => execution,
        Err(error) => {
            let _ = budget.settle(&claim, &reserved, &reserved, false, completed)?;
            if crate::automation_runtime::inspect_run(
                paths,
                profile,
                &claim.run_id.to_string(),
                true,
                false,
            )
            .is_ok_and(|receipt| {
                matches!(
                    receipt.state,
                    ca_automation::runtime::ExecutionState::NeedsAttention
                        | ca_automation::runtime::ExecutionState::Running
                )
            }) {
                journal.append(
                    &claim,
                    AutomationRunStateV1::Interrupted,
                    RunEventUpdate {
                        reserved: Some(reserved.clone()),
                        actual: Some(reserved.clone()),
                        refunded: Some(AutomationUsageCountersV1::default()),
                        reason_class: Some("native_reconciliation_required".into()),
                        ..Default::default()
                    },
                    completed,
                )?;
                store.block(&claim.automation_id,"native run needs attention; inspect and reconcile recorded effects before retry")?;
                notify_async(
                    paths,
                    profile,
                    notifications::freeze(&claim, None, true, None),
                )
                .await;
                return Err(AutomationError::State(error));
            }
            if matches!(
                claim.definition.automation_kind,
                AutomationKindV1::Heartbeat { .. }
            ) {
                record_failed_heartbeat(
                    paths,
                    profile,
                    store,
                    &sessions,
                    &claim,
                    intent,
                    heartbeat_lease.as_ref(),
                    reserved,
                    completed,
                    &error,
                )
                .await?;
                return Err(AutomationError::State(error));
            }
            journal.append(
                &claim,
                AutomationRunStateV1::Failed,
                RunEventUpdate {
                    reserved: Some(reserved.clone()),
                    actual: Some(reserved.clone()),
                    refunded: Some(AutomationUsageCountersV1::default()),
                    reason_class: Some("execution_failed".into()),
                    ..Default::default()
                },
                completed,
            )?;
            store.settle(&claim)?;
            let alert = notifications::freeze(&claim, None, true, None);
            notify_async(paths, profile, alert).await;
            return Err(AutomationError::State(error));
        }
    };

    let (actual, authoritative) = execution_usage(
        paths,
        profile,
        &claim.definition,
        &execution,
        started,
        completed,
    )?;
    if !actual.within(&claim.definition.per_run_budget) {
        store.block(&claim.automation_id, "per-run automation budget exceeded")?;
        journal.append(
            &claim,
            AutomationRunStateV1::Blocked,
            RunEventUpdate {
                reserved: Some(reserved),
                actual: Some(actual),
                reason_class: Some("per_run_budget_exceeded".into()),
                artifact_sha256: execution.artifact_digest,
                ..Default::default()
            },
            completed,
        )?;
        return Err(AutomationError::State(
            "automation exceeded its per-run budget".into(),
        ));
    }
    let refunded = budget.settle(&claim, &reserved, &actual, authoritative, completed)?;
    let previous_state = store
        .records()?
        .into_iter()
        .find(|record| record.definition.id == claim.automation_id)
        .and_then(|record| record.last_heartbeat_state_sha256);
    let (delivery_content, heartbeat_notification) = heartbeat_notification(
        &claim.definition,
        &execution.content,
        previous_state.as_deref(),
    );
    let native_alert = notifications::freeze(
        &claim,
        heartbeat_notification.as_ref(),
        false,
        Some(intent.target_session_id),
    );
    store_private_result(paths, profile, &claim, &delivery_content)?;
    let checkpoint = DeliveryCheckpointV1 {
        schema: DELIVERY_CHECKPOINT_SCHEMA.into(),
        intent,
        result_sha256: hex_digest(&Sha256::digest(delivery_content.as_bytes())),
        result_bytes: delivery_content.len(),
        heartbeat_notification,
        execution_failed: false,
        native_alert,
        reserved: reserved.clone(),
        actual: if authoritative {
            actual.clone()
        } else {
            reserved.clone()
        },
        refunded: refunded.clone(),
        artifact_sha256: execution.artifact_digest.clone(),
        completed_at: completed,
    };
    write_delivery_checkpoint(paths, profile, &checkpoint)?;
    journal.append(
        &claim,
        AutomationRunStateV1::ResultReady,
        RunEventUpdate {
            reserved: Some(reserved.clone()),
            actual: Some(if authoritative {
                actual.clone()
            } else {
                reserved.clone()
            }),
            refunded: Some(refunded.clone()),
            artifact_sha256: execution.artifact_digest.clone(),
            ..Default::default()
        },
        completed,
    )?;
    journal.append(
        &claim,
        AutomationRunStateV1::Delivering,
        RunEventUpdate::default(),
        Utc::now(),
    )?;

    let saved_session_id = match deliver_checkpoint(
        &sessions,
        &checkpoint,
        &delivery_content,
        heartbeat_lease.as_ref(),
    ) {
        Ok(id) => id,
        Err(AutomationError::Busy) => return Err(AutomationError::Busy),
        Err(error) => {
            store.block_pending_delivery(
                &claim,
                "recorded result is pending because session delivery changed",
            )?;
            journal.append(
                &claim,
                AutomationRunStateV1::Interrupted,
                RunEventUpdate {
                    reason_class: Some("session_delivery_drift".into()),
                    ..Default::default()
                },
                Utc::now(),
            )?;
            return Err(error);
        }
    };
    journal.append(
        &claim,
        AutomationRunStateV1::Succeeded,
        RunEventUpdate {
            reserved: Some(reserved),
            actual: Some(if authoritative {
                actual
            } else {
                AutomationUsageCountersV1::from_budget(&claim.definition.per_run_budget)
            }),
            refunded: Some(refunded),
            saved_session_id,
            artifact_sha256: execution.artifact_digest,
            ..Default::default()
        },
        Utc::now(),
    )?;
    notify_async(paths, profile, checkpoint.native_alert.clone()).await;
    store.settle_delivery(&claim, checkpoint.heartbeat_notification.as_ref())?;
    match output {
        Some(AutomationOutputFormat::Json) => write_json(&serde_json::json!({
            "schema": AUTOMATION_RUN_SCHEMA,
            "automation_id": claim.automation_id,
            "run_id": claim.run_id,
            "outcome": "succeeded",
            "saved_session_id": saved_session_id,
        }))?,
        Some(AutomationOutputFormat::Text) => println!(
            "Automation {} completed · run {}{}",
            claim.automation_id,
            claim.run_id,
            saved_session_id.map_or_else(String::new, |id| format!(" · task {id}"))
        ),
        None => {}
    }
    Ok(())
}

async fn prepare_session_identity_inventory(
    sessions: &crate::session::SessionStore,
) -> Result<(), AutomationError> {
    let sessions = sessions.clone();
    tokio::task::spawn_blocking(move || sessions.prepare_identity_inventory())
        .await
        .map_err(|_| AutomationError::State("session identity preparation worker stopped".into()))?
        .map_err(|error| AutomationError::Io(error.to_string()))
}

pub(crate) async fn run_due(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    drain: bool,
    now: DateTime<Utc>,
) -> Result<usize, AutomationError> {
    run_due_with_output(
        paths,
        profile,
        drain,
        now,
        Some(AutomationOutputFormat::Text),
    )
    .await
}
pub(crate) async fn run_due_quiet(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    drain: bool,
    now: DateTime<Utc>,
) -> Result<usize, AutomationError> {
    run_due_with_output(paths, profile, drain, now, None).await
}
async fn run_due_with_output(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    drain: bool,
    now: DateTime<Utc>,
    output: Option<AutomationOutputFormat>,
) -> Result<usize, AutomationError> {
    let store = AutomationStore::for_paths(paths, profile);
    if !store.path.exists() {
        return Ok(0);
    }
    let recovered = recover_active_runs(paths, profile, &store)?;
    if recovered > 0 && !drain {
        return Ok(recovered);
    }
    let mut executed = recovered;
    loop {
        let Some(claim) = store.claim_due(profile.label(), now)? else {
            return Ok(executed);
        };
        match execute_claim(paths, profile, &store, claim, output).await {
            Ok(()) => {}
            Err(AutomationError::Busy) => return Ok(executed),
            Err(error) => return Err(error),
        }
        executed = executed.saturating_add(1);
        if !drain {
            return Ok(executed);
        }
    }
}

pub(crate) fn run_history(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    id: Option<&str>,
) -> Result<Vec<AutomationRunEventV1>, AutomationError> {
    AutomationRunJournal::for_paths(paths, profile).history(id)
}

/// Explicit result inspection hydrates only the selected private checkpoint;
/// dashboards and model metadata tools never call this for background previews.
pub(crate) fn read_recorded_result(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    automation_id: &str,
    run_id: Uuid,
) -> Result<String, AutomationError> {
    let root = run_directory(paths, profile, run_id);
    let content = if root.join("delivery.json").is_file() {
        let checkpoint: DeliveryCheckpointV1 = serde_json::from_slice(&read_owner_file(
            &root.join("delivery.json"),
            MAX_STORE_BYTES,
        )?)
        .map_err(|error| AutomationError::State(error.to_string()))?;
        let claim = &checkpoint.intent.claim;
        if claim.run_id != run_id || claim.automation_id != automation_id {
            return Err(AutomationError::State(
                "recorded result navigation identity changed".into(),
            ));
        }
        read_delivery_checkpoint(paths, profile, claim)?.1
    } else {
        // A retained repository candidate can need integration review before
        // there is a deliverable session. Bind private inspection to the exact
        // original claim and envelope, even if its definition was later deleted.
        let intent: DeliveryIntentV1 = serde_json::from_slice(&read_owner_file(
            &root.join("intent.json"),
            MAX_STORE_BYTES,
        )?)
        .map_err(|error| AutomationError::State(error.to_string()))?;
        intent.validate(&intent.claim)?;
        let claim = &intent.claim;
        if claim.run_id != run_id || claim.automation_id != automation_id {
            return Err(AutomationError::State(
                "recorded result navigation identity changed".into(),
            ));
        }
        let context = ca_automation::runtime::RunStore::load_context(
            &crate::automation_runtime::state_root(paths, profile),
            &run_id.to_string(),
        )
        .map_err(|error| AutomationError::State(error.to_string()))?;
        if context.profile != profile.label()
            || context.definition_digest != claim.definition_sha256
        {
            return Err(AutomationError::State(
                "native result claim identity changed".into(),
            ));
        }
        let receipt = ca_automation::runtime::RunStore::open(&context)
            .and_then(|store| store.status())
            .map_err(|error| AutomationError::State(error.to_string()))?;
        match receipt.result {
            Some(value) => match serde_json::from_value::<crate::automation_runtime::NativeResult>(
                value.clone(),
            ) {
                Ok(result) => result.content,
                Err(_) => serde_json::to_string_pretty(&value)
                    .map_err(|error| AutomationError::State(error.to_string()))?,
            },
            None => format!(
                "Run {:?}: {}",
                receipt.state,
                receipt
                    .reason
                    .unwrap_or_else(|| "No confirmed result is available".into())
            ),
        }
    };
    const DISPLAY_BYTES: usize = 256 * 1024;
    if content.len() <= DISPLAY_BYTES {
        return Ok(crate::terminal_text::sanitize_terminal_text(&content));
    }
    let mut end = DISPLAY_BYTES;
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    Ok(format!(
        "{}\n\nPrivate result display truncated at 256 KiB.",
        crate::terminal_text::sanitize_terminal_text(&content[..end])
    ))
}

pub(crate) fn recover_pending_deliveries(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
) -> Result<usize, AutomationError> {
    let store = AutomationStore::for_paths(paths, profile);
    if !store.path.exists() {
        return Ok(0);
    }
    recover_active_runs(paths, profile, &store)
}

pub(crate) async fn run_now(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    id: &str,
) -> Result<Option<Uuid>, AutomationError> {
    recover_pending_deliveries(paths, profile)?;
    let store = AutomationStore::for_paths(paths, profile);
    let claim = store.claim_immediate(profile.label(), id, Utc::now())?;
    let run_id = claim.run_id;
    execute_claim(paths, profile, &store, claim, None).await?;
    Ok(run_history(paths, profile, Some(id))?
        .into_iter()
        .filter(|event| event.run_id == run_id)
        .max_by_key(|event| event.sequence)
        .and_then(|event| event.saved_session_id))
}

fn bounded_heartbeat_history(
    mut messages: Vec<crate::app::Message>,
) -> Result<Vec<crate::app::Message>, AutomationError> {
    const LIMIT: usize = 384 * 1024;
    while serde_json::to_vec(&messages)
        .map_err(|error| AutomationError::State(error.to_string()))?
        .len()
        > LIMIT
        && !messages.is_empty()
    {
        messages.remove(0);
    }
    while messages
        .first()
        .is_some_and(|message| message.role == crate::app::Role::Tool)
    {
        messages.remove(0);
    }
    Ok(messages)
}

pub(crate) fn execution_usage(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    definition: &AutomationDefinitionV1,
    execution: &crate::schedule::AutomationExecution,
    started: DateTime<Utc>,
    completed: DateTime<Utc>,
) -> Result<(AutomationUsageCountersV1, bool), AutomationError> {
    if let Some(frozen) = &execution.dispatch_usage {
        return Ok((frozen.clone(), true));
    }
    let mut counters = AutomationUsageCountersV1 {
        provider_requests: u32::try_from(execution.usages.len()).unwrap_or(u32::MAX),
        tool_calls: execution.tool_calls,
        runtime_secs: u64::try_from((completed - started).num_seconds().max(0)).unwrap_or(u64::MAX),
        output_bytes: u64::try_from(execution.content.len()).unwrap_or(u64::MAX),
        ..Default::default()
    };
    for usage in &execution.usages {
        counters.input_tokens = counters
            .input_tokens
            .checked_add(u64::from(usage.prompt_tokens))
            .ok_or_else(|| {
                AutomationError::State("automation input-token accounting overflow".into())
            })?;
        counters.output_tokens = counters
            .output_tokens
            .checked_add(u64::from(usage.completion_tokens))
            .ok_or_else(|| {
                AutomationError::State("automation output-token accounting overflow".into())
            })?;
    }
    let model_free = matches!(definition.pricing, PricingBindingV1::ModelFree);
    let mut authoritative = model_free || !execution.usages.is_empty();
    match &definition.pricing {
        PricingBindingV1::Metered { price_card_sha256 } => {
            let provider = definition
                .schedule
                .provider
                .as_deref()
                .expect("metered provider");
            let model = definition.schedule.model.as_deref().expect("metered model");
            let catalog = crate::pricing::PricingStore::for_paths(paths, profile)
                .load()
                .map_err(|error| AutomationError::State(error.to_string()))?;
            let card = catalog.find(provider, model).ok_or_else(|| {
                AutomationError::State("automation price card disappeared during execution".into())
            })?;
            let digest = hex_digest(&Sha256::digest(
                serde_json::to_vec(card)
                    .map_err(|error| AutomationError::State(error.to_string()))?,
            ));
            if &digest != price_card_sha256 {
                return Err(AutomationError::State(
                    "automation price card drifted during execution".into(),
                ));
            }
            for usage in &execution.usages {
                // Exact-only journal: keep the full reservation for a peak
                // upper bound rather than call it an authoritative paid cost.
                let Some(crate::pricing::ActualCost::IncrementalUsd(cost)) =
                    crate::pricing::actual_cost_for(card, None, usage, started, completed)
                else {
                    authoritative = false;
                    continue;
                };
                let Some(cost) = cost.rounded_decimal(12) else {
                    authoritative = false;
                    continue;
                };
                counters.cost_usd = counters
                    .cost_usd
                    .checked_add(cost)
                    .ok_or_else(|| AutomationError::State("automation cost overflow".into()))?;
            }
        }
        PricingBindingV1::MeteredRoutes { .. } => {
            authoritative = false;
        }
        PricingBindingV1::LocalZero | PricingBindingV1::ModelFree => {}
    }
    Ok((counters, authoritative))
}

fn store_private_result(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    claim: &AutomationClaimV1,
    content: &str,
) -> Result<(), AutomationError> {
    let root = paths
        .profile_data_dir(profile)
        .join("automations/v1/runs")
        .join(claim.run_id.to_string());
    let path = root.join("result.txt");
    atomic_owner_write(&root, &path, content.as_bytes(), false)
}

const DELIVERY_INTENT_SCHEMA: &str = "ca.automation-delivery-intent.v1";
const DELIVERY_CHECKPOINT_SCHEMA: &str = "ca.automation-delivery-checkpoint.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeliveryIntentV1 {
    schema: String,
    claim: AutomationClaimV1,
    heartbeat_binding: Option<crate::session::SessionAutomationBinding>,
    provenance: crate::session::AutomationSessionProvenanceV1,
    target_session_id: Uuid,
}

impl DeliveryIntentV1 {
    fn new(
        claim: &AutomationClaimV1,
        heartbeat_binding: Option<crate::session::SessionAutomationBinding>,
    ) -> Self {
        Self {
            schema: DELIVERY_INTENT_SCHEMA.into(),
            claim: claim.clone(),
            heartbeat_binding,
            provenance: crate::session::AutomationSessionProvenanceV1 {
                schema: AUTOMATION_DELIVERY_SCHEMA.into(),
                automation_id: claim.automation_id.clone(),
                run_id: claim.run_id,
                occurrence_id: claim.occurrence.id.clone(),
                kind: kind_label(&claim.definition.automation_kind).into(),
            },
            target_session_id: match claim.definition.automation_kind {
                AutomationKindV1::Heartbeat { session_id, .. } => session_id,
                AutomationKindV1::Standalone => standalone_session_id(claim.run_id),
            },
        }
    }

    fn validate(&self, claim: &AutomationClaimV1) -> Result<(), AutomationError> {
        let expected = Self::new(claim, self.heartbeat_binding.clone());
        if self.schema != DELIVERY_INTENT_SCHEMA
            || &self.claim != claim
            || claim.schema != AUTOMATION_RUN_SCHEMA
            || definition_digest(&claim.definition)? != claim.definition_sha256
            || stable_run_id(&claim.definition_sha256, &claim.occurrence.id) != claim.run_id
            || self.provenance != expected.provenance
            || self.target_session_id != expected.target_session_id
        {
            return Err(AutomationError::State(
                "recorded delivery claim or provenance changed".into(),
            ));
        }
        match (&claim.definition.automation_kind, &self.heartbeat_binding) {
            (AutomationKindV1::Standalone, None) => Ok(()),
            (AutomationKindV1::Heartbeat { session_id, .. }, Some(binding))
                if binding.session_id == *session_id
                    && binding.committed.session_id == *session_id
                    && binding.message_count == binding.committed.message_count
                    && binding.storage_generation == binding.committed.storage_generation
                    && binding.history_sha256 == binding.committed.history_sha256 =>
            {
                Ok(())
            }
            _ => Err(AutomationError::State(
                "recorded delivery binding is inconsistent".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeliveryCheckpointV1 {
    schema: String,
    intent: DeliveryIntentV1,
    result_sha256: String,
    result_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    heartbeat_notification: Option<HeartbeatNotificationV1>,
    #[serde(default)]
    execution_failed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_alert: Option<notifications::NativeAlertV1>,
    reserved: AutomationUsageCountersV1,
    actual: AutomationUsageCountersV1,
    refunded: AutomationUsageCountersV1,
    artifact_sha256: Option<String>,
    completed_at: DateTime<Utc>,
}

fn run_directory(paths: &RuntimePaths, profile: &ActiveProfile, id: Uuid) -> PathBuf {
    paths
        .profile_data_dir(profile)
        .join("automations/v1/runs")
        .join(id.to_string())
}
fn write_run_intent(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    intent: &DeliveryIntentV1,
) -> Result<(), AutomationError> {
    intent.validate(&intent.claim)?;
    let root = run_directory(paths, profile, intent.claim.run_id);
    atomic_owner_write(
        &root,
        &root.join("intent.json"),
        &serde_json::to_vec(intent).map_err(|error| AutomationError::State(error.to_string()))?,
        false,
    )
}
fn write_delivery_checkpoint(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    checkpoint: &DeliveryCheckpointV1,
) -> Result<(), AutomationError> {
    let root = run_directory(paths, profile, checkpoint.intent.claim.run_id);
    atomic_owner_write(
        &root,
        &root.join("delivery.json"),
        &serde_json::to_vec(checkpoint)
            .map_err(|error| AutomationError::State(error.to_string()))?,
        false,
    )
}
fn read_delivery_checkpoint(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    claim: &AutomationClaimV1,
) -> Result<(DeliveryCheckpointV1, String), AutomationError> {
    let root = run_directory(paths, profile, claim.run_id);
    let checkpoint: DeliveryCheckpointV1 = serde_json::from_slice(&read_owner_file(
        &root.join("delivery.json"),
        MAX_STORE_BYTES,
    )?)
    .map_err(|error| AutomationError::State(error.to_string()))?;
    checkpoint.intent.validate(claim)?;
    let original: DeliveryIntentV1 = serde_json::from_slice(&read_owner_file(
        &root.join("intent.json"),
        MAX_STORE_BYTES,
    )?)
    .map_err(|error| AutomationError::State(error.to_string()))?;
    original.validate(claim)?;
    let (reserved, refunded) =
        AutomationBudgetLedger::for_paths(paths, profile).settlement_evidence(claim)?;
    if serde_json::to_vec(&original).map_err(|error| AutomationError::State(error.to_string()))?
        != serde_json::to_vec(&checkpoint.intent)
            .map_err(|error| AutomationError::State(error.to_string()))?
        || checkpoint.schema != DELIVERY_CHECKPOINT_SCHEMA
        || !notifications::validate(&checkpoint)
        || checkpoint
            .heartbeat_notification
            .as_ref()
            .is_some_and(|decision| !decision.validate(&claim.definition))
        || (checkpoint.execution_failed
            && !checkpoint
                .heartbeat_notification
                .as_ref()
                .is_some_and(|decision| decision.status == HeartbeatStatus::Failed))
        || !is_sha256(&checkpoint.result_sha256)
        || checkpoint.result_bytes as u64 > claim.definition.per_run_budget.output_bytes
        || checkpoint
            .artifact_sha256
            .as_ref()
            .is_some_and(|digest| !is_sha256(digest))
        || !checkpoint.actual.within(&claim.definition.per_run_budget)
        || reserved != checkpoint.reserved
        || refunded != checkpoint.refunded
        || claim.profile != profile.label()
        || checkpoint.reserved.checked_sub(&checkpoint.actual).as_ref()
            != Some(&checkpoint.refunded)
    {
        return Err(AutomationError::State(
            "recorded automation result metadata changed".into(),
        ));
    }
    let bytes = read_owner_file(
        &root.join("result.txt"),
        claim.definition.per_run_budget.output_bytes,
    )?;
    if bytes.len() != checkpoint.result_bytes
        || hex_digest(&Sha256::digest(&bytes)) != checkpoint.result_sha256
    {
        return Err(AutomationError::State(
            "recorded automation result digest changed".into(),
        ));
    }
    Ok((
        checkpoint,
        String::from_utf8(bytes)
            .map_err(|_| AutomationError::State("recorded result is not UTF-8".into()))?,
    ))
}

fn deliver_checkpoint(
    sessions: &crate::session::SessionStore,
    checkpoint: &DeliveryCheckpointV1,
    content: &str,
    heartbeat_lease: Option<&crate::session_access::SessionWriterLease>,
) -> Result<Option<Uuid>, AutomationError> {
    let claim = &checkpoint.intent.claim;
    if matches!(
        claim.definition.automation_kind,
        AutomationKindV1::Standalone
    ) && checkpoint
        .heartbeat_notification
        .as_ref()
        .is_some_and(|d| !d.publish)
    {
        return Ok(None);
    }
    match (
        &claim.definition.automation_kind,
        &checkpoint.intent.heartbeat_binding,
        heartbeat_lease,
    ) {
        (AutomationKindV1::Heartbeat { session_id, .. }, Some(recorded), Some(lease)) => {
            if checkpoint
                .heartbeat_notification
                .as_ref()
                .is_some_and(|decision| !decision.publish)
            {
                sessions
                    .access_coordinator()
                    .validate(lease, *session_id)
                    .map_err(access_error)?;
                return Ok(None);
            }
            let (_, current) = sessions
                .automation_snapshot(*session_id)
                .map_err(access_error)?;
            let mut binding = recorded.clone();
            // Moving the same metadata/history between active and archive does
            // not change the recorded binding or permit rebasing onto edits.
            binding.archived = current.archived;
            binding.storage_key = current.storage_key;
            let prompt = match &claim.definition.schedule.target {
                ScheduleTargetV2::Prompt { prompt } => prompt.as_str(),
                _ => "Scheduled observation",
            };
            sessions
                .append_automation_delivery_owned(
                    &binding,
                    checkpoint.intent.provenance.clone(),
                    &claim.definition.name,
                    prompt,
                    content,
                    lease,
                )
                .map_err(access_error)?;
            Ok(Some(*session_id))
        }
        (AutomationKindV1::Standalone, None, None) => save_standalone_session(
            sessions,
            claim,
            checkpoint.intent.provenance.clone(),
            content,
        )
        .map(Some),
        _ => Err(AutomationError::State(
            "automation delivery binding is inconsistent".into(),
        )),
    }
}

/// Only publish a recorded result or settle a terminal receipt. This path has
/// no provider execution call; an ambiguous interrupted run is paused.
fn recover_active_runs(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    store: &AutomationStore,
) -> Result<usize, AutomationError> {
    let mut settled = 0;
    let journal = AutomationRunJournal::for_paths(paths, profile);
    let profile_paths = paths.clone().for_profile(profile);
    let sessions = crate::session::SessionStore::with_archive(
        profile_paths.sessions_dir,
        profile_paths.archived_sessions_dir,
    );
    for record in store.records()? {
        let Some(run_id) = record.active_run_id else {
            continue;
        };
        if record.blocked_reason.is_some() {
            continue;
        }
        let _run_lease = match run_access(&store.root).try_writer(run_id) {
            Ok(lease) => lease,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(error) => return Err(access_error(error)),
        };
        let Some(claim) = record.active_claim else {
            store.block(
                &record.definition.id,
                "interrupted legacy claim needs review; provider execution was not replayed",
            )?;
            continue;
        };
        if !store.validates_active_claim(&claim)? {
            continue;
        }
        let last = journal
            .history(Some(&claim.automation_id))?
            .into_iter()
            .filter(|event| event.run_id == run_id)
            .max_by_key(|event| event.sequence);
        if last.as_ref().is_some_and(|event| {
            matches!(
                event.state,
                AutomationRunStateV1::Succeeded
                    | AutomationRunStateV1::Failed
                    | AutomationRunStateV1::TimedOut
                    | AutomationRunStateV1::Blocked
            )
        }) {
            if matches!(
                last.as_ref().map(|event| event.state),
                Some(AutomationRunStateV1::Succeeded | AutomationRunStateV1::Failed)
            ) && run_directory(paths, profile, run_id)
                .join("delivery.json")
                .exists()
            {
                let (checkpoint, _) = read_delivery_checkpoint(paths, profile, &claim)?;
                notifications::attempt(paths, profile, checkpoint.native_alert.as_ref());
                store.settle_delivery(&claim, checkpoint.heartbeat_notification.as_ref())?;
            } else {
                store.settle(&claim)?;
            }
            settled += 1;
            continue;
        }
        if !run_directory(paths, profile, run_id)
            .join("delivery.json")
            .exists()
        {
            let native_root = crate::automation_runtime::state_root(paths, profile);
            if native_root
                .join("runs")
                .join(run_id.to_string())
                .join("context.json")
                .is_file()
            {
                let receipt = crate::automation_runtime::inspect_run(
                    paths,
                    profile,
                    &run_id.to_string(),
                    true,
                    false,
                )
                .map_err(AutomationError::State)?;
                if receipt.state == ca_automation::runtime::ExecutionState::Running {
                    continue;
                }
                if receipt.state == ca_automation::runtime::ExecutionState::Succeeded {
                    recover_native_checkpoint(paths, profile, store, &claim, receipt)?;
                }
            }
        }
        if !run_directory(paths, profile, run_id)
            .join("delivery.json")
            .exists()
        {
            journal.append(
                &claim,
                AutomationRunStateV1::Interrupted,
                RunEventUpdate {
                    reason_class: Some("ambiguous_interrupted_execution".into()),
                    ..Default::default()
                },
                Utc::now(),
            )?;
            store.block(
                &claim.automation_id,
                "interrupted run needs review; provider execution was not replayed",
            )?;
            continue;
        }
        let (checkpoint, content) = match read_delivery_checkpoint(paths, profile, &claim) {
            Ok(result) => result,
            Err(error) => {
                store.block_pending_delivery(
                    &claim,
                    "recorded result needs review before delivery",
                )?;
                return Err(error);
            }
        };
        // Recorded-result recovery is a management worker operation. Complete
        // bounded inventory preparation before short delivery/CAS checks;
        // provider execution is never repeated on this path.
        sessions
            .prepare_identity_inventory()
            .map_err(|error| AutomationError::Io(error.to_string()))?;
        let heartbeat_lease = match &claim.definition.automation_kind {
            AutomationKindV1::Heartbeat { session_id, .. } => {
                match sessions.try_writer_lease(*session_id) {
                    Ok(lease) => Some(lease),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                    Err(error) => return Err(access_error(error)),
                }
            }
            AutomationKindV1::Standalone => None,
        };
        let saved_session_id =
            match deliver_checkpoint(&sessions, &checkpoint, &content, heartbeat_lease.as_ref()) {
                Ok(id) => id,
                Err(AutomationError::Busy) => continue,
                Err(error) => {
                    store.block_pending_delivery(
                        &claim,
                        "recorded result is pending because session delivery changed",
                    )?;
                    return Err(error);
                }
            };
        journal.append(
            &claim,
            if checkpoint.execution_failed {
                AutomationRunStateV1::Failed
            } else {
                AutomationRunStateV1::Succeeded
            },
            RunEventUpdate {
                reserved: Some(checkpoint.reserved),
                actual: Some(checkpoint.actual),
                refunded: Some(checkpoint.refunded),
                saved_session_id,
                artifact_sha256: checkpoint.artifact_sha256,
                reason_class: Some("recorded_delivery_recovered".into()),
            },
            Utc::now(),
        )?;
        notifications::attempt(paths, profile, checkpoint.native_alert.as_ref());
        store.settle_delivery(&claim, checkpoint.heartbeat_notification.as_ref())?;
        settled += 1;
    }
    Ok(settled)
}

/// A child confirms its complete result before sending it to the parent. A
/// crash after that boundary can rebuild delivery without repeating execution.
fn recover_native_checkpoint(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    store: &AutomationStore,
    claim: &AutomationClaimV1,
    receipt: ca_automation::runtime::ExecutionReceipt,
) -> Result<(), AutomationError> {
    let result =
        crate::automation_runtime::result_from_receipt(receipt).map_err(AutomationError::State)?;
    let intent: DeliveryIntentV1 = serde_json::from_slice(&read_owner_file(
        &run_directory(paths, profile, claim.run_id).join("intent.json"),
        MAX_STORE_BYTES,
    )?)
    .map_err(|e| AutomationError::State(e.to_string()))?;
    intent.validate(claim)?;
    let ledger = AutomationBudgetLedger::for_paths(paths, profile);
    let (reserved, refunded) = match ledger.settlement_evidence(claim) {
        Ok(evidence) => evidence,
        Err(_) => {
            let events = AutomationRunJournal::for_paths(paths, profile)
                .history(Some(&claim.automation_id))?;
            let reserved = events
                .iter()
                .rev()
                .filter(|e| e.run_id == claim.run_id)
                .find_map(|e| e.reserved.clone())
                .ok_or_else(|| {
                    AutomationError::State("native completion has no budget reservation".into())
                })?;
            let mut actual = result.usage.clone();
            actual.output_bytes = result.content.len() as u64;
            // Child runtime can be measured across parent failure only as an
            // admitted bound. Retain that part of the reservation.
            actual.runtime_secs = reserved.runtime_secs;
            let refunded = ledger.settle(claim, &reserved, &actual, true, Utc::now())?;
            (reserved, refunded)
        }
    };
    let previous = store
        .records()?
        .into_iter()
        .find(|r| r.definition.id == claim.automation_id)
        .and_then(|r| r.last_heartbeat_state_sha256);
    let (content, notification) =
        heartbeat_notification(&claim.definition, &result.content, previous.as_deref());
    let alert = notifications::freeze(
        claim,
        notification.as_ref(),
        false,
        Some(intent.target_session_id),
    );
    store_private_result(paths, profile, claim, &content)?;
    let actual = reserved
        .checked_sub(&refunded)
        .ok_or_else(|| AutomationError::State("recovered budget settlement changed".into()))?;
    write_delivery_checkpoint(
        paths,
        profile,
        &DeliveryCheckpointV1 {
            schema: DELIVERY_CHECKPOINT_SCHEMA.into(),
            intent,
            result_sha256: hex_digest(&Sha256::digest(content.as_bytes())),
            result_bytes: content.len(),
            heartbeat_notification: notification,
            execution_failed: false,
            native_alert: alert,
            reserved,
            actual,
            refunded,
            artifact_sha256: None,
            completed_at: Utc::now(),
        },
    )
}

#[allow(clippy::too_many_arguments)]
async fn record_failed_heartbeat(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    store: &AutomationStore,
    sessions: &crate::session::SessionStore,
    claim: &AutomationClaimV1,
    intent: DeliveryIntentV1,
    lease: Option<&crate::session_access::SessionWriterLease>,
    reserved: AutomationUsageCountersV1,
    completed_at: DateTime<Utc>,
    error: &str,
) -> Result<(), AutomationError> {
    let content = bounded(
        &format!("Heartbeat execution failed.\n\n{error}"),
        usize::try_from(claim.definition.per_run_budget.output_bytes).unwrap_or(usize::MAX),
    );
    let previous = store
        .records()?
        .into_iter()
        .find(|record| record.definition.id == claim.automation_id)
        .and_then(|record| record.last_heartbeat_state_sha256);
    let state_sha256 = hex_digest(&Sha256::digest(
        format!("Failed\0{}", content.trim()).as_bytes(),
    ));
    let publish = change_report_publish(
        &claim.definition,
        HeartbeatStatus::Failed,
        &state_sha256,
        previous.as_deref(),
    );
    let notification = HeartbeatNotificationV1 {
        status: HeartbeatStatus::Failed,
        state_sha256,
        publish,
        previous_state_sha256: previous,
    };
    let native_alert = notifications::freeze(
        claim,
        Some(&notification),
        true,
        Some(intent.target_session_id),
    );
    store_private_result(paths, profile, claim, &content)?;
    let checkpoint = DeliveryCheckpointV1 {
        schema: DELIVERY_CHECKPOINT_SCHEMA.into(),
        intent,
        result_sha256: hex_digest(&Sha256::digest(content.as_bytes())),
        result_bytes: content.len(),
        heartbeat_notification: Some(notification),
        execution_failed: true,
        native_alert,
        reserved: reserved.clone(),
        actual: reserved.clone(),
        refunded: AutomationUsageCountersV1::default(),
        artifact_sha256: None,
        completed_at,
    };
    write_delivery_checkpoint(paths, profile, &checkpoint)?;
    let saved_session_id = match deliver_checkpoint(sessions, &checkpoint, &content, lease) {
        Ok(id) => id,
        Err(AutomationError::Busy) => return Err(AutomationError::Busy),
        Err(error) => {
            store.block_pending_delivery(
                claim,
                "recorded execution failure is pending because session delivery changed",
            )?;
            return Err(error);
        }
    };
    AutomationRunJournal::for_paths(paths, profile).append(
        claim,
        AutomationRunStateV1::Failed,
        RunEventUpdate {
            reserved: Some(reserved.clone()),
            actual: Some(reserved),
            refunded: Some(AutomationUsageCountersV1::default()),
            saved_session_id,
            reason_class: Some("execution_failed".into()),
            ..Default::default()
        },
        completed_at,
    )?;
    notify_async(paths, profile, checkpoint.native_alert.clone()).await;
    store.settle_delivery(claim, checkpoint.heartbeat_notification.as_ref())
}

async fn notify_async(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    alert: Option<notifications::NativeAlertV1>,
) {
    if alert.is_none() {
        return;
    }
    let paths = paths.clone();
    let profile = profile.clone();
    let _ = tokio::task::spawn_blocking(move || {
        notifications::attempt(&paths, &profile, alert.as_ref())
    })
    .await;
}

fn standalone_session_id(run_id: Uuid) -> Uuid {
    Uuid::new_v5(&run_id, b"ca-standalone-result-v1")
}

fn save_standalone_session(
    sessions: &crate::session::SessionStore,
    claim: &AutomationClaimV1,
    provenance: crate::session::AutomationSessionProvenanceV1,
    result: &str,
) -> Result<Uuid, AutomationError> {
    let prompt = match &claim.definition.schedule.target {
        ScheduleTargetV2::Prompt { prompt } => prompt.clone(),
        other => format!("Run confirmed automation target {}", target_label(other)),
    };
    let id = standalone_session_id(claim.run_id);
    let lease = sessions.try_writer_lease(id).map_err(access_error)?;
    if let Ok((existing, _)) = sessions.automation_snapshot(id) {
        // Provenance and the initial result were committed atomically. Once
        // present, acknowledge that delivery even if the user later edits the
        // conversation; restart must not overwrite their current transcript.
        if existing.automation_provenance.as_ref() == Some(&provenance) {
            return Ok(id);
        }
        return Err(AutomationError::State(
            "standalone result session identity changed".into(),
        ));
    }
    let now = claim.occurrence.at;
    let messages = vec![
        crate::app::Message {
            role: crate::app::Role::User,
            content: format!(
                "[Automation: {} · occurrence {}]\n\n{}",
                claim.definition.name, claim.occurrence.id, prompt
            ),
            tool_calls: None,
            thinking: None,
            response_items: Vec::new(),
            timestamp: now,
        },
        crate::app::Message {
            role: crate::app::Role::Assistant,
            content: result.into(),
            tool_calls: None,
            thinking: None,
            response_items: Vec::new(),
            timestamp: now,
        },
    ];
    let mut session = crate::session::SessionV2::new(
        format!(
            "Automation · {} · {}",
            claim.definition.name,
            now.format("%Y-%m-%d %H:%M")
        ),
        claim.definition.schedule.workspace.to_string_lossy(),
        claim
            .definition
            .schedule
            .provider
            .clone()
            .unwrap_or_default(),
        claim.definition.schedule.model.clone().unwrap_or_default(),
        messages,
    );
    session.workspace_roots = vec![claim
        .definition
        .schedule
        .workspace
        .to_string_lossy()
        .into_owned()];
    session.reasoning_effort = claim.definition.reasoning_effort;
    session.automation_provenance = Some(provenance);
    session.id = id;
    sessions
        .save_owned(&mut session, &lease)
        .map_err(|error| AutomationError::Io(error.to_string()))?;
    Ok(id)
}

fn target_label(target: &ScheduleTargetV2) -> &'static str {
    match target {
        ScheduleTargetV2::Prompt { .. } => "prompt",
        ScheduleTargetV2::Workflow { .. } => "workflow",
        ScheduleTargetV2::ScriptOnly { .. } => "script_only",
        ScheduleTargetV2::Mechanistic { .. } => "mechanistic",
        ScheduleTargetV2::AutomationWorkflow { .. } => "automation_workflow",
        ScheduleTargetV2::RepositoryTask { .. } => "repository_task",
        ScheduleTargetV2::HarnessDiscovery { .. } => "harness_discovery",
        ScheduleTargetV2::HarnessEvaluation { .. } => "harness_evaluation",
        ScheduleTargetV2::HarnessCycle { .. } => "harness_cycle",
        ScheduleTargetV2::KnowledgeLearning { .. } => "knowledge_learning",
        ScheduleTargetV2::Monitor { .. } => "monitor",
    }
}

pub const AUTOMATION_SCHEMA: &str = "ca.automation.v1";
pub const AUTOMATION_RUN_SCHEMA: &str = "ca.automation-run.v1";
pub const AUTOMATION_DELIVERY_SCHEMA: &str = "ca.automation-session-delivery.v1";
const STORE_SCHEMA: &str = "ca.automation-store.v1";
const MAX_STORE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DEFINITION_BYTES: u64 = 256 * 1024;
pub const MAX_COMBINED_AUTOMATIONS: usize = 256;
const MAX_NAME_BYTES: usize = 128;
const MAX_CONTEXT_DIGESTS: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum AutomationError {
    #[error("invalid automation definition: {0}")]
    Invalid(String),
    #[error("automation confirmation digest required: {0}")]
    Confirmation(String),
    #[error("automation already exists: {0}")]
    Duplicate(String),
    #[error("automation was not found: {0}")]
    Missing(String),
    #[error("automation state is busy")]
    Busy,
    #[error("unsafe or corrupt automation state: {0}")]
    State(String),
    #[error("automation I/O failed: {0}")]
    Io(String),
}

impl From<std::io::Error> for AutomationError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationKindV1 {
    Heartbeat {
        session_id: Uuid,
        workspace_identity_sha256: String,
    },
    Standalone,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PricingBindingV1 {
    Metered {
        price_card_sha256: String,
    },
    MeteredRoutes {
        price_card_sha256: std::collections::BTreeMap<String, String>,
    },
    LocalZero,
    ModelFree,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutomationNotificationPolicy {
    #[default]
    MeaningfulChanges,
    AllResults,
    FailuresOnly,
    Muted,
}

pub(crate) fn heartbeat_report_instruction(kind: &AutomationKindV1) -> &'static str {
    match kind {
        AutomationKindV1::Standalone => "",
        AutomationKindV1::Heartbeat { .. } => {
            "\n\nHeartbeat reporting contract: return one JSON object with schema ca.heartbeat-report.v1, status unchanged|changed|completed|failed|needs_attention, summary (plain text), and optional state (a stable, concise description of the observed state, without check timestamps). Use unchanged only when no meaningful change or action is needed. Report failures and requests for user action explicitly. Do not include credentials in any field. The host keeps unchanged checks in private history and publishes meaningful changes according to the saved notification policy."
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum HeartbeatStatus {
    Unchanged,
    Changed,
    Completed,
    Failed,
    NeedsAttention,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatReportV1 {
    schema: String,
    status: HeartbeatStatus,
    summary: String,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct HeartbeatNotificationV1 {
    status: HeartbeatStatus,
    state_sha256: String,
    previous_state_sha256: Option<String>,
    publish: bool,
}

impl HeartbeatNotificationV1 {
    fn validate(&self, definition: &AutomationDefinitionV1) -> bool {
        let publish = change_report_publish(
            definition,
            self.status,
            &self.state_sha256,
            self.previous_state_sha256.as_deref(),
        );
        uses_change_report(definition)
            && is_sha256(&self.state_sha256)
            && self
                .previous_state_sha256
                .as_ref()
                .is_none_or(|digest| is_sha256(digest))
            && self.publish == publish
    }
}

fn change_report_publish(
    definition: &AutomationDefinitionV1,
    status: HeartbeatStatus,
    digest: &str,
    previous: Option<&str>,
) -> bool {
    let attention = matches!(
        status,
        HeartbeatStatus::Failed | HeartbeatStatus::NeedsAttention
    );
    match definition.notification_policy {
        AutomationNotificationPolicy::AllResults => true,
        AutomationNotificationPolicy::FailuresOnly => attention,
        AutomationNotificationPolicy::Muted => false,
        AutomationNotificationPolicy::MeaningfulChanges => {
            if matches!(
                definition.schedule.target,
                ScheduleTargetV2::KnowledgeLearning { .. }
            ) {
                status != HeartbeatStatus::Unchanged && previous != Some(digest)
            } else {
                attention || (status != HeartbeatStatus::Unchanged && previous != Some(digest))
            }
        }
    }
}

fn uses_change_report(definition: &AutomationDefinitionV1) -> bool {
    matches!(
        definition.automation_kind,
        AutomationKindV1::Heartbeat { .. }
    ) || matches!(
        definition.schedule.target,
        ScheduleTargetV2::Monitor { .. }
            | ScheduleTargetV2::HarnessEvaluation { .. }
            | ScheduleTargetV2::HarnessCycle { .. }
            | ScheduleTargetV2::KnowledgeLearning { .. }
    )
}

fn heartbeat_notification(
    definition: &AutomationDefinitionV1,
    raw: &str,
    previous: Option<&str>,
) -> (String, Option<HeartbeatNotificationV1>) {
    if !uses_change_report(definition) {
        return (raw.to_owned(), None);
    }
    // An absent or malformed report is attention-worthy, never an implicit
    // unchanged success. Host execution failures never enter this parser.
    let text = raw.trim();
    let report_text = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .and_then(|body| body.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(text);
    let parsed = serde_json::from_str::<HeartbeatReportV1>(report_text)
        .ok()
        .filter(|r| {
            r.schema == "ca.heartbeat-report.v1"
                && (r.status == HeartbeatStatus::Unchanged || !r.summary.trim().is_empty())
                && r.state
                    .as_ref()
                    .is_none_or(|s| !s.trim().is_empty() && s.len() <= 4096)
        });
    let (status, content, state) = match parsed {
        Some(report) => (report.status, report.summary, report.state),
        None => (HeartbeatStatus::NeedsAttention, raw.to_owned(), None),
    };
    let normalized = state.as_deref().unwrap_or(&content).replace("\r\n", "\n");
    let canonical = normalized
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    let state_sha256 = hex_digest(&Sha256::digest(
        format!("{status:?}\0{}", canonical.trim()).as_bytes(),
    ));
    let publish = change_report_publish(definition, status, &state_sha256, previous);
    (
        content,
        Some(HeartbeatNotificationV1 {
            status,
            state_sha256,
            previous_state_sha256: previous.map(str::to_owned),
            publish,
        }),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AutomationBudgetV1 {
    pub provider_requests: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub tool_calls: u32,
    pub runtime_secs: u64,
    pub output_bytes: u64,
    pub max_cost_usd: DecimalUsd,
}

impl AutomationBudgetV1 {
    pub fn per_run_default() -> Self {
        Self {
            provider_requests: 16,
            input_tokens: 500_000,
            output_tokens: 64_000,
            tool_calls: 64,
            runtime_secs: 15 * 60,
            output_bytes: 2 * 1024 * 1024,
            max_cost_usd: DecimalUsd::from_str("2").expect("valid default automation cost"),
        }
    }

    pub fn rolling_default() -> Self {
        Self {
            provider_requests: 256,
            input_tokens: 10_000_000,
            output_tokens: 1_000_000,
            tool_calls: 1_024,
            runtime_secs: 30 * 24 * 60 * 60,
            output_bytes: 64 * 1024 * 1024,
            max_cost_usd: DecimalUsd::from_str("25")
                .expect("valid default rolling automation cost"),
        }
    }

    fn validate_per_run(&self) -> Result<(), AutomationError> {
        if !(1..=64).contains(&self.provider_requests)
            || !(1..=2_000_000).contains(&self.input_tokens)
            || !(1..=262_144).contains(&self.output_tokens)
            || !(1..=512).contains(&self.tool_calls)
            || !(1..=86_400).contains(&self.runtime_secs)
            || !(1..=16 * 1024 * 1024).contains(&self.output_bytes)
            || self.max_cost_usd == DecimalUsd::ZERO
        {
            return Err(AutomationError::Invalid(
                "per-run budget is outside supported bounds".into(),
            ));
        }
        Ok(())
    }

    fn validate_rolling(&self, per_run: &Self) -> Result<(), AutomationError> {
        if self.provider_requests < per_run.provider_requests
            || self.provider_requests > 100_000
            || self.input_tokens < per_run.input_tokens
            || self.output_tokens < per_run.output_tokens
            || self.tool_calls < per_run.tool_calls
            || self.runtime_secs < per_run.runtime_secs
            || self.output_bytes < per_run.output_bytes
            || self.max_cost_usd < per_run.max_cost_usd
        {
            return Err(AutomationError::Invalid(
                "rolling budget must contain the per-run budget and remain bounded".into(),
            ));
        }
        Ok(())
    }
}

fn default_per_run_budget() -> AutomationBudgetV1 {
    AutomationBudgetV1::per_run_default()
}

fn default_rolling_budget() -> AutomationBudgetV1 {
    AutomationBudgetV1::rolling_default()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AutomationDefinitionV1 {
    pub schema: String,
    pub id: String,
    pub name: String,
    pub automation_kind: AutomationKindV1,
    pub schedule: ScheduleDefinitionV2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    pub pricing: PricingBindingV1,
    #[serde(default)]
    pub context_sha256: Vec<String>,
    #[serde(default = "default_per_run_budget")]
    pub per_run_budget: AutomationBudgetV1,
    #[serde(default = "default_rolling_budget")]
    pub rolling_30_day_budget: AutomationBudgetV1,
    #[serde(default)]
    pub notification_policy: AutomationNotificationPolicy,
}

impl AutomationDefinitionV1 {
    pub fn normalize(self, now: DateTime<Utc>) -> Result<Self, AutomationError> {
        if self.schema != AUTOMATION_SCHEMA {
            return Err(AutomationError::Invalid(
                "schema must be ca.automation.v1".into(),
            ));
        }
        validate_id(&self.id)?;
        validate_name(&self.name)?;
        if self.schedule.id != self.id {
            return Err(AutomationError::Invalid(
                "embedded schedule ID must match the automation ID".into(),
            ));
        }
        if self.context_sha256.len() > MAX_CONTEXT_DIGESTS
            || self.context_sha256.iter().any(|value| !is_sha256(value))
        {
            return Err(AutomationError::Invalid(
                "context digest list is invalid or oversized".into(),
            ));
        }
        match &self.automation_kind {
            AutomationKindV1::Heartbeat {
                workspace_identity_sha256,
                ..
            } => {
                if !is_sha256(workspace_identity_sha256)
                    || !matches!(
                        self.schedule.target,
                        ScheduleTargetV2::Prompt { .. }
                            | ScheduleTargetV2::Monitor { .. }
                            | ScheduleTargetV2::Mechanistic { .. }
                    )
                {
                    return Err(AutomationError::Invalid(
                        "heartbeats require a workspace identity and prompt target".into(),
                    ));
                }
            }
            AutomationKindV1::Standalone => {}
        }
        validate_pricing(&self)?;
        self.per_run_budget.validate_per_run()?;
        self.rolling_30_day_budget
            .validate_rolling(&self.per_run_budget)?;
        let mut normalized = self;
        normalized.schedule = normalize_at_confirmation(normalized.schedule, now)
            .map_err(|error| AutomationError::Invalid(error.to_string()))?;
        normalized.schedule.runtime_secs = normalized.per_run_budget.runtime_secs;
        normalized.schedule.output_bytes = normalized.per_run_budget.output_bytes;
        Ok(normalized)
    }
}

fn validate_pricing(definition: &AutomationDefinitionV1) -> Result<(), AutomationError> {
    let model_free = definition.schedule.provider.is_none() && definition.schedule.model.is_none();
    match (&definition.pricing, model_free) {
        (PricingBindingV1::MeteredRoutes { price_card_sha256 }, true)
            if matches!(
                definition.schedule.target,
                ScheduleTargetV2::KnowledgeLearning { .. }
            ) =>
        {
            if price_card_sha256.is_empty()
                || price_card_sha256.len() > 16
                || price_card_sha256.values().any(|digest| !is_sha256(digest))
            {
                return Err(AutomationError::Invalid(
                    "knowledge learning requires exact prices for eligible routes".into(),
                ));
            }
        }
        (PricingBindingV1::ModelFree, true) => {
            if definition.reasoning_effort.is_some() {
                return Err(AutomationError::Invalid(
                    "model-free automation cannot select reasoning effort".into(),
                ));
            }
        }
        (PricingBindingV1::Metered { price_card_sha256 }, false) => {
            if !is_sha256(price_card_sha256) {
                return Err(AutomationError::Invalid(
                    "metered automation requires an exact price-card digest".into(),
                ));
            }
        }
        (PricingBindingV1::LocalZero, false) => {}
        _ => {
            return Err(AutomationError::Invalid(
                "pricing binding does not match the automation route".into(),
            ));
        }
    }
    Ok(())
}

pub fn definition_digest(definition: &AutomationDefinitionV1) -> Result<String, AutomationError> {
    let bytes = serde_json::to_vec(definition)
        .map_err(|error| AutomationError::Invalid(error.to_string()))?;
    Ok(hex_digest(&Sha256::digest(bytes)))
}

pub fn load_definition_file(path: &Path) -> Result<AutomationDefinitionV1, AutomationError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| AutomationError::Io(error.to_string()))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_DEFINITION_BYTES
    {
        return Err(AutomationError::Invalid(
            "definition file is unsafe or oversized".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)?
        .take(MAX_DEFINITION_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_DEFINITION_BYTES {
        return Err(AutomationError::Invalid(
            "definition file is oversized".into(),
        ));
    }
    match path.extension().and_then(|value| value.to_str()) {
        Some("json") => serde_json::from_slice(&bytes)
            .map_err(|error| AutomationError::Invalid(error.to_string())),
        Some("toml") => std::str::from_utf8(&bytes)
            .map_err(|_| AutomationError::Invalid("definition is not UTF-8".into()))
            .and_then(|text| {
                toml::from_str(text).map_err(|error| AutomationError::Invalid(error.to_string()))
            }),
        _ => Err(AutomationError::Invalid(
            "definition must be a .json or .toml file".into(),
        )),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AutomationRecordV1 {
    pub definition: AutomationDefinitionV1,
    pub definition_sha256: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_occurrence: Option<Occurrence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_claim: Option<AutomationClaimV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_heartbeat: Option<Occurrence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_heartbeat_state_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_revision: Option<u64>,
    #[serde(default)]
    pub coalesced_occurrences: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

/// Binds a reviewed definition and its exact current claim/state. Runtime lease
/// handles are intentionally skipped by the persisted claim representation.
pub(crate) fn automation_record_digest(
    record: &AutomationRecordV1,
) -> Result<String, AutomationError> {
    let bytes =
        serde_json::to_vec(record).map_err(|error| AutomationError::State(error.to_string()))?;
    Ok(hex_digest(&Sha256::digest(bytes)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AutomationStoreDataV1 {
    schema: String,
    revision: u64,
    records: Vec<AutomationRecordV1>,
}

impl Default for AutomationStoreDataV1 {
    fn default() -> Self {
        Self {
            schema: STORE_SCHEMA.into(),
            revision: 0,
            records: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AutomationStore {
    root: PathBuf,
    path: PathBuf,
}

impl AutomationStore {
    pub fn for_paths(paths: &RuntimePaths, profile: &ActiveProfile) -> Self {
        Self::new(paths.profile_data_dir(profile).join("automations/v1"))
    }

    pub fn new(root: PathBuf) -> Self {
        let root = normalize_missing_path(&root).unwrap_or(root);
        Self {
            path: root.join("automations.json"),
            root,
        }
    }

    fn load(&self) -> Result<AutomationStoreDataV1, AutomationError> {
        if !self.path.exists() {
            return Ok(AutomationStoreDataV1::default());
        }
        let bytes = read_owner_file(&self.path, MAX_STORE_BYTES)?;
        let data: AutomationStoreDataV1 = serde_json::from_slice(&bytes)
            .map_err(|error| AutomationError::State(error.to_string()))?;
        if data.schema != STORE_SCHEMA || data.records.len() > MAX_COMBINED_AUTOMATIONS {
            return Err(AutomationError::State(
                "unsupported or oversized automation store".into(),
            ));
        }
        for record in &data.records {
            if record
                .last_heartbeat_state_sha256
                .as_ref()
                .is_some_and(|digest| !is_sha256(digest))
            {
                return Err(AutomationError::State(
                    "heartbeat observation digest is invalid".into(),
                ));
            }
            if definition_digest(&record.definition)? != record.definition_sha256 {
                return Err(AutomationError::State(format!(
                    "definition digest mismatch for {}",
                    record.definition.id
                )));
            }
            if let Some(claim) = &record.active_claim {
                if record.active_run_id != Some(claim.run_id)
                    || record.claim_revision != Some(claim.claim_revision)
                    || claim.definition_sha256 != record.definition_sha256
                    || claim.automation_id != record.definition.id
                    || claim.definition != record.definition
                {
                    return Err(AutomationError::State(
                        "automation active claim identity is inconsistent".into(),
                    ));
                }
            }
        }
        Ok(data)
    }

    fn save(&self, data: &mut AutomationStoreDataV1) -> Result<(), AutomationError> {
        data.revision = data
            .revision
            .checked_add(1)
            .ok_or_else(|| AutomationError::State("automation revision overflow".into()))?;
        let bytes = serde_json::to_vec_pretty(data)
            .map_err(|error| AutomationError::State(error.to_string()))?;
        if bytes.len() as u64 > MAX_STORE_BYTES {
            return Err(AutomationError::State(
                "automation store is oversized".into(),
            ));
        }
        atomic_owner_write(&self.root, &self.path, &bytes, true)
    }

    pub fn records(&self) -> Result<Vec<AutomationRecordV1>, AutomationError> {
        Ok(self.load()?.records)
    }

    pub fn add_confirmed(
        &self,
        definition: AutomationDefinitionV1,
        confirmation: &str,
        classic_count: usize,
        now: DateTime<Utc>,
    ) -> Result<AutomationRecordV1, AutomationError> {
        let definition = definition.normalize(now)?;
        let digest = definition_digest(&definition)?;
        require_confirmation(confirmation, &digest)?;
        let _lock = OwnerLock::acquire(self.root.join("automations.lock"))?;
        let mut data = self.load()?;
        if data
            .records
            .iter()
            .any(|record| record.definition.id == definition.id)
        {
            return Err(AutomationError::Duplicate(definition.id));
        }
        if classic_count.saturating_add(data.records.len()) >= MAX_COMBINED_AUTOMATIONS {
            return Err(AutomationError::State(
                "combined schedule and automation limit reached".into(),
            ));
        }
        let next = next_occurrence(&definition.schedule.trigger, now)
            .map_err(|error| AutomationError::Invalid(error.to_string()))?;
        let record = AutomationRecordV1 {
            definition,
            definition_sha256: digest,
            enabled: true,
            next_occurrence: next,
            active_run_id: None,
            active_claim: None,
            pending_heartbeat: None,
            last_heartbeat_state_sha256: None,
            claim_revision: None,
            coalesced_occurrences: 0,
            blocked_reason: None,
        };
        data.records.push(record.clone());
        data.records
            .sort_by(|left, right| left.definition.id.cmp(&right.definition.id));
        self.save(&mut data)?;
        Ok(record)
    }

    pub fn update_confirmed(
        &self,
        id: &str,
        definition: AutomationDefinitionV1,
        confirmation: &str,
        now: DateTime<Utc>,
    ) -> Result<AutomationRecordV1, AutomationError> {
        validate_id(id)?;
        if definition.id != id {
            return Err(AutomationError::Invalid(
                "update cannot change an automation ID".into(),
            ));
        }
        let definition = definition.normalize(now)?;
        let digest = definition_digest(&definition)?;
        require_confirmation(confirmation, &digest)?;
        let _lock = OwnerLock::acquire(self.root.join("automations.lock"))?;
        let mut data = self.load()?;
        let record = data
            .records
            .iter_mut()
            .find(|record| record.definition.id == id)
            .ok_or_else(|| AutomationError::Missing(id.into()))?;
        if record.active_run_id.is_some() {
            return Err(AutomationError::State(
                "cannot update an automation with an active run".into(),
            ));
        }
        record.definition = definition;
        record.last_heartbeat_state_sha256 = None;
        record.definition_sha256 = digest;
        record.blocked_reason = None;
        record.next_occurrence = if record.enabled {
            next_occurrence(&record.definition.schedule.trigger, now)
                .map_err(|error| AutomationError::Invalid(error.to_string()))?
        } else {
            None
        };
        let result = record.clone();
        self.save(&mut data)?;
        Ok(result)
    }

    pub fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> Result<AutomationRecordV1, AutomationError> {
        validate_id(id)?;
        let _lock = OwnerLock::acquire(self.root.join("automations.lock"))?;
        let mut data = self.load()?;
        let record = data
            .records
            .iter_mut()
            .find(|record| record.definition.id == id)
            .ok_or_else(|| AutomationError::Missing(id.into()))?;
        if record.active_run_id.is_some()
            && !(record.blocked_reason.is_some()
                && record.active_run_id.is_some_and(|id| {
                    self.root
                        .join("runs")
                        .join(id.to_string())
                        .join("delivery.json")
                        .is_file()
                }))
        {
            return Err(AutomationError::State(
                "automation has an active run".into(),
            ));
        }
        record.enabled = enabled;
        record.blocked_reason = (!enabled && record.active_run_id.is_some())
            .then(|| "paused pending result delivery".into());
        record.next_occurrence = if enabled {
            next_occurrence(&record.definition.schedule.trigger, now)
                .map_err(|error| AutomationError::Invalid(error.to_string()))?
        } else {
            None
        };
        let result = record.clone();
        self.save(&mut data)?;
        Ok(result)
    }

    pub fn remove(&self, id: &str, confirmation: &str) -> Result<(), AutomationError> {
        validate_id(id)?;
        require_confirmation(confirmation, id)?;
        self.remove_reviewed(id, None)
    }

    pub(crate) fn remove_confirmed_snapshot(
        &self,
        id: &str,
        expected_record_digest: &str,
    ) -> Result<(), AutomationError> {
        validate_id(id)?;
        if !is_sha256(expected_record_digest) {
            return Err(AutomationError::Confirmation(
                "the reviewed automation record digest is required".into(),
            ));
        }
        self.remove_reviewed(id, Some(expected_record_digest))
    }

    fn remove_reviewed(
        &self,
        id: &str,
        expected_record_digest: Option<&str>,
    ) -> Result<(), AutomationError> {
        let _lock = OwnerLock::acquire(self.root.join("automations.lock"))?;
        let mut data = self.load()?;
        let index = data
            .records
            .iter()
            .position(|record| record.definition.id == id)
            .ok_or_else(|| AutomationError::Missing(id.into()))?;
        if let Some(expected) = expected_record_digest {
            if automation_record_digest(&data.records[index])? != expected {
                return Err(AutomationError::Confirmation(
                    "automation definition or run state changed since review".into(),
                ));
            }
        }
        // A persisted claim can outlive its process. Its stable run lease is the
        // authority for liveness; retaining it through the index commit prevents
        // a restarted delivery worker from racing deletion. Run receipts remain.
        let _run_lease = data.records[index]
            .active_run_id
            .map(|run_id| {
                run_access(&self.root)
                    .try_writer(run_id)
                    .map_err(access_error)
            })
            .transpose()?;
        data.records.remove(index);
        self.save(&mut data)
    }

    pub fn claim_due(
        &self,
        profile: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<AutomationClaimV1>, AutomationError> {
        self.claim(profile, None, now)
    }

    pub fn claim_immediate(
        &self,
        profile: &str,
        id: &str,
        now: DateTime<Utc>,
    ) -> Result<AutomationClaimV1, AutomationError> {
        self.claim(profile, Some(id), now)?.ok_or_else(|| {
            AutomationError::State("automation could not be claimed immediately".into())
        })
    }

    fn claim(
        &self,
        profile: &str,
        immediate: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Option<AutomationClaimV1>, AutomationError> {
        let _lock = OwnerLock::acquire(self.root.join("automations.lock"))?;
        let mut data = self.load()?;
        let index = if let Some(id) = immediate {
            validate_id(id)?;
            Some(
                data.records
                    .iter()
                    .position(|record| record.definition.id == id)
                    .ok_or_else(|| AutomationError::Missing(id.into()))?,
            )
        } else {
            data.records
                .iter()
                .enumerate()
                .filter(|(_, record)| {
                    record.enabled
                        && record.blocked_reason.is_none()
                        && record.active_run_id.is_none()
                        && (record
                            .pending_heartbeat
                            .as_ref()
                            .is_some_and(|occurrence| occurrence.at <= now)
                            || record
                                .next_occurrence
                                .as_ref()
                                .is_some_and(|occurrence| occurrence.at <= now))
                })
                .min_by_key(|(_, record)| {
                    (
                        record
                            .pending_heartbeat
                            .as_ref()
                            .or(record.next_occurrence.as_ref())
                            .map(|item| item.at),
                        &record.definition.id,
                    )
                })
                .map(|(index, _)| index)
        };
        let Some(index) = index else {
            return Ok(None);
        };
        let record = data.records[index].clone();
        if record.active_run_id.is_some() {
            return Err(AutomationError::Busy);
        }
        let occurrence = if immediate.is_some() {
            Occurrence {
                id: format!("manual-{}", Uuid::new_v4()),
                at: now,
            }
        } else if let Some(pending) = &record.pending_heartbeat {
            pending.clone()
        } else {
            let durable = record
                .next_occurrence
                .as_ref()
                .ok_or_else(|| AutomationError::State("due automation has no occurrence".into()))?;
            let occurrence = due_occurrence(
                &record.definition.schedule.trigger,
                durable,
                now,
                record.definition.schedule.missed_run_policy,
            )
            .map_err(|error| AutomationError::Invalid(error.to_string()))?;
            let Some(occurrence) = occurrence else {
                data.records[index].next_occurrence =
                    next_occurrence(&record.definition.schedule.trigger, now)
                        .map_err(|error| AutomationError::Invalid(error.to_string()))?;
                self.save(&mut data)?;
                return Ok(None);
            };
            occurrence
        };
        let run_id = stable_run_id(&record.definition_sha256, &occurrence.id);
        let claim_revision = data
            .revision
            .checked_add(1)
            .ok_or_else(|| AutomationError::State("claim revision overflow".into()))?;
        let run_lease = run_access(&self.root)
            .try_writer(run_id)
            .map_err(access_error)?;
        let claim = AutomationClaimV1 {
            schema: AUTOMATION_RUN_SCHEMA.into(),
            run_id,
            profile: profile.into(),
            automation_id: record.definition.id.clone(),
            definition_sha256: record.definition_sha256.clone(),
            occurrence: occurrence.clone(),
            claim_revision,
            coalesced_occurrences: record.coalesced_occurrences,
            definition: record.definition,
            runtime_lease: Some(run_lease),
        };
        data.records[index].active_run_id = Some(run_id);
        data.records[index].active_claim = Some(claim.clone());
        data.records[index].pending_heartbeat = None;
        data.records[index].claim_revision = Some(claim_revision);
        data.records[index].coalesced_occurrences = 0;
        if immediate.is_none() && record.pending_heartbeat.is_none() {
            data.records[index].next_occurrence = next_occurrence(
                &data.records[index].definition.schedule.trigger,
                occurrence.at,
            )
            .map_err(|error| AutomationError::Invalid(error.to_string()))?;
            if matches!(
                data.records[index].definition.schedule.trigger,
                crate::schedule_v2::ScheduleTriggerV2::Absolute { .. }
            ) {
                data.records[index].enabled = false;
            }
        }
        self.save(&mut data)?;
        Ok(Some(claim))
    }

    pub fn settle(&self, claim: &AutomationClaimV1) -> Result<(), AutomationError> {
        self.settle_delivery(claim, None)
    }

    fn settle_delivery(
        &self,
        claim: &AutomationClaimV1,
        notification: Option<&HeartbeatNotificationV1>,
    ) -> Result<(), AutomationError> {
        let _lock = OwnerLock::acquire(self.root.join("automations.lock"))?;
        let mut data = self.load()?;
        let record = data
            .records
            .iter_mut()
            .find(|record| record.definition.id == claim.automation_id)
            .ok_or_else(|| AutomationError::Missing(claim.automation_id.clone()))?;
        if record.definition_sha256 != claim.definition_sha256
            || record.active_run_id != Some(claim.run_id)
            || record.claim_revision != Some(claim.claim_revision)
        {
            return Err(AutomationError::State(
                "automation settlement compare-and-swap failed".into(),
            ));
        }
        record.active_run_id = None;
        record.active_claim = None;
        record.claim_revision = None;
        if let Some(notification) = notification.filter(|notification| notification.publish) {
            record.last_heartbeat_state_sha256 = Some(notification.state_sha256.clone());
        }
        if record.next_occurrence.is_none()
            && record.pending_heartbeat.is_none()
            && matches!(
                record.definition.schedule.trigger,
                crate::schedule_v2::ScheduleTriggerV2::Absolute { .. }
            )
        {
            record.enabled = false;
        }
        self.save(&mut data)
    }

    fn defer_busy_heartbeat(
        &self,
        claim: &AutomationClaimV1,
        now: DateTime<Utc>,
    ) -> Result<(), AutomationError> {
        let _lock = OwnerLock::acquire(self.root.join("automations.lock"))?;
        let mut data = self.load()?;
        let record = data
            .records
            .iter_mut()
            .find(|record| record.definition.id == claim.automation_id)
            .ok_or_else(|| AutomationError::Missing(claim.automation_id.clone()))?;
        if record.active_run_id != Some(claim.run_id)
            || record.claim_revision != Some(claim.claim_revision)
            || record.definition_sha256 != claim.definition_sha256
        {
            return Err(AutomationError::State(
                "busy heartbeat compare-and-swap failed".into(),
            ));
        }
        record.pending_heartbeat = Some(claim.occurrence.clone());
        record.enabled = true;
        record.coalesced_occurrences = claim.coalesced_occurrences.max(1);
        // Keep one pending run and advance recurrence beyond missed ticks.
        if record
            .next_occurrence
            .as_ref()
            .is_some_and(|item| item.at <= now)
        {
            record.coalesced_occurrences = record.coalesced_occurrences.saturating_add(1);
            record.next_occurrence = next_occurrence(&record.definition.schedule.trigger, now)
                .map_err(|error| AutomationError::Invalid(error.to_string()))?;
        }
        record.active_run_id = None;
        record.active_claim = None;
        record.claim_revision = None;
        self.save(&mut data)
    }

    fn validates_active_claim(&self, claim: &AutomationClaimV1) -> Result<bool, AutomationError> {
        Ok(self.load()?.records.iter().any(|record| {
            record.definition.id == claim.automation_id
                && record.definition_sha256 == claim.definition_sha256
                && record.active_run_id == Some(claim.run_id)
                && record.claim_revision == Some(claim.claim_revision)
        }))
    }

    fn block_pending_delivery(
        &self,
        claim: &AutomationClaimV1,
        reason: &str,
    ) -> Result<(), AutomationError> {
        let _lock = OwnerLock::acquire(self.root.join("automations.lock"))?;
        let mut data = self.load()?;
        let record = data
            .records
            .iter_mut()
            .find(|record| record.definition.id == claim.automation_id)
            .ok_or_else(|| AutomationError::Missing(claim.automation_id.clone()))?;
        if record.active_run_id != Some(claim.run_id)
            || record.claim_revision != Some(claim.claim_revision)
            || record.definition_sha256 != claim.definition_sha256
        {
            return Err(AutomationError::State(
                "pending delivery compare-and-swap failed".into(),
            ));
        }
        record.blocked_reason = Some(bounded(reason, 512));
        record.enabled = false;
        record.next_occurrence = None;
        self.save(&mut data)
    }

    pub fn block(&self, id: &str, reason: &str) -> Result<(), AutomationError> {
        validate_id(id)?;
        let reason = bounded(reason.trim(), 512);
        if reason.is_empty() {
            return Err(AutomationError::Invalid("blocked reason is empty".into()));
        }
        let _lock = OwnerLock::acquire(self.root.join("automations.lock"))?;
        let mut data = self.load()?;
        let record = data
            .records
            .iter_mut()
            .find(|record| record.definition.id == id)
            .ok_or_else(|| AutomationError::Missing(id.into()))?;
        record.blocked_reason = Some(reason);
        record.enabled = false;
        record.next_occurrence = None;
        record.active_run_id = None;
        record.active_claim = None;
        record.claim_revision = None;
        self.save(&mut data)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationClaimV1 {
    pub schema: String,
    pub run_id: Uuid,
    pub profile: String,
    pub automation_id: String,
    pub definition_sha256: String,
    pub occurrence: Occurrence,
    pub claim_revision: u64,
    pub coalesced_occurrences: u32,
    pub definition: AutomationDefinitionV1,
    #[serde(skip)]
    runtime_lease: Option<crate::session_access::SessionWriterLease>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStateV1 {
    Claimed,
    BudgetReserved,
    Running,
    ResultReady,
    Delivering,
    Succeeded,
    Failed,
    Interrupted,
    TimedOut,
    Blocked,
}

impl AutomationRunStateV1 {
    fn label(self) -> &'static str {
        match self {
            Self::Claimed => "claimed",
            Self::BudgetReserved => "budget_reserved",
            Self::Running => "running",
            Self::ResultReady => "result_ready",
            Self::Delivering => "delivering",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::TimedOut => "timed_out",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AutomationUsageCountersV1 {
    pub provider_requests: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub tool_calls: u32,
    pub runtime_secs: u64,
    pub output_bytes: u64,
    pub cost_usd: DecimalUsd,
}

impl Default for AutomationUsageCountersV1 {
    fn default() -> Self {
        Self {
            provider_requests: 0,
            input_tokens: 0,
            output_tokens: 0,
            tool_calls: 0,
            runtime_secs: 0,
            output_bytes: 0,
            cost_usd: DecimalUsd::ZERO,
        }
    }
}

impl AutomationUsageCountersV1 {
    pub(crate) fn from_budget(value: &AutomationBudgetV1) -> Self {
        Self {
            provider_requests: value.provider_requests,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            tool_calls: value.tool_calls,
            runtime_secs: value.runtime_secs,
            output_bytes: value.output_bytes,
            cost_usd: value.max_cost_usd,
        }
    }

    fn checked_add(&self, other: &Self) -> Option<Self> {
        Some(Self {
            provider_requests: self
                .provider_requests
                .checked_add(other.provider_requests)?,
            input_tokens: self.input_tokens.checked_add(other.input_tokens)?,
            output_tokens: self.output_tokens.checked_add(other.output_tokens)?,
            tool_calls: self.tool_calls.checked_add(other.tool_calls)?,
            runtime_secs: self.runtime_secs.checked_add(other.runtime_secs)?,
            output_bytes: self.output_bytes.checked_add(other.output_bytes)?,
            cost_usd: self.cost_usd.checked_add(other.cost_usd)?,
        })
    }

    fn checked_sub(&self, other: &Self) -> Option<Self> {
        Some(Self {
            provider_requests: self
                .provider_requests
                .checked_sub(other.provider_requests)?,
            input_tokens: self.input_tokens.checked_sub(other.input_tokens)?,
            output_tokens: self.output_tokens.checked_sub(other.output_tokens)?,
            tool_calls: self.tool_calls.checked_sub(other.tool_calls)?,
            runtime_secs: self.runtime_secs.checked_sub(other.runtime_secs)?,
            output_bytes: self.output_bytes.checked_sub(other.output_bytes)?,
            cost_usd: decimal_sub(self.cost_usd, other.cost_usd)?,
        })
    }

    fn within(&self, limit: &AutomationBudgetV1) -> bool {
        self.provider_requests <= limit.provider_requests
            && self.input_tokens <= limit.input_tokens
            && self.output_tokens <= limit.output_tokens
            && self.tool_calls <= limit.tool_calls
            && self.runtime_secs <= limit.runtime_secs
            && self.output_bytes <= limit.output_bytes
            && self.cost_usd <= limit.max_cost_usd
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationRunEventV1 {
    pub schema: String,
    pub sequence: u64,
    pub run_id: Uuid,
    pub automation_id: String,
    pub occurrence_id: String,
    pub at: DateTime<Utc>,
    pub state: AutomationRunStateV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserved: Option<AutomationUsageCountersV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<AutomationUsageCountersV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refunded: Option<AutomationUsageCountersV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_session_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_class: Option<String>,
    pub previous_sha256: String,
    pub event_sha256: String,
}

#[derive(Debug, Clone)]
struct AutomationRunJournal {
    root: PathBuf,
}

impl AutomationRunJournal {
    fn for_paths(paths: &RuntimePaths, profile: &ActiveProfile) -> Self {
        Self {
            root: paths.profile_data_dir(profile).join("automations/v1/runs"),
        }
    }

    fn history(
        &self,
        automation_id: Option<&str>,
    ) -> Result<Vec<AutomationRunEventV1>, AutomationError> {
        if let Some(id) = automation_id {
            validate_id(id)?;
        }
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        reject_symlink_components(&self.root)?;
        let mut events = Vec::new();
        for run in std::fs::read_dir(&self.root)? {
            let run = run?;
            let run_path = run.path();
            if !run.file_type()?.is_dir() {
                continue;
            }
            let mut run_events = Vec::new();
            for entry in std::fs::read_dir(&run_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                if !path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| {
                        name.len() == 8 && name.bytes().all(|byte| byte.is_ascii_digit())
                    })
                {
                    continue;
                }
                let event: AutomationRunEventV1 =
                    serde_json::from_slice(&read_owner_file(&path, 64 * 1024)?)
                        .map_err(|error| AutomationError::State(error.to_string()))?;
                verify_run_event(&event)?;
                run_events.push(event);
            }
            run_events.sort_by_key(|event| event.sequence);
            let mut previous = "0".repeat(64);
            for event in &run_events {
                if event.previous_sha256 != previous {
                    return Err(AutomationError::State(
                        "automation run hash chain is invalid".into(),
                    ));
                }
                previous.clone_from(&event.event_sha256);
            }
            events.extend(
                run_events
                    .into_iter()
                    .filter(|event| automation_id.is_none_or(|id| event.automation_id == id)),
            );
        }
        events.sort_by(|left, right| {
            left.at
                .cmp(&right.at)
                .then_with(|| left.run_id.cmp(&right.run_id))
                .then_with(|| left.sequence.cmp(&right.sequence))
        });
        Ok(events)
    }

    fn append(
        &self,
        claim: &AutomationClaimV1,
        state: AutomationRunStateV1,
        update: RunEventUpdate,
        now: DateTime<Utc>,
    ) -> Result<AutomationRunEventV1, AutomationError> {
        let _lock = OwnerLock::acquire(self.root.join("journal.lock"))?;
        let run_root = self.root.join(claim.run_id.to_string());
        ensure_private_dir(&run_root)?;
        let mut current = self
            .history(Some(&claim.automation_id))?
            .into_iter()
            .filter(|event| event.run_id == claim.run_id)
            .collect::<Vec<_>>();
        current.sort_by_key(|event| event.sequence);
        let sequence = u64::try_from(current.len())
            .map_err(|_| AutomationError::State("automation run sequence overflow".into()))?;
        let previous_sha256 = current
            .last()
            .map_or_else(|| "0".repeat(64), |event| event.event_sha256.clone());
        let mut event = AutomationRunEventV1 {
            schema: AUTOMATION_RUN_SCHEMA.into(),
            sequence,
            run_id: claim.run_id,
            automation_id: claim.automation_id.clone(),
            occurrence_id: bounded(&claim.occurrence.id, 256),
            at: now,
            state,
            reserved: update.reserved,
            actual: update.actual,
            refunded: update.refunded,
            saved_session_id: update.saved_session_id,
            artifact_sha256: update.artifact_sha256,
            reason_class: update.reason_class.map(|value| bounded(&value, 128)),
            previous_sha256,
            event_sha256: String::new(),
        };
        event.event_sha256 = run_event_digest(&event)?;
        let path = run_root.join(format!("{sequence:08}.json"));
        let bytes = serde_json::to_vec_pretty(&event)
            .map_err(|error| AutomationError::State(error.to_string()))?;
        atomic_owner_write(&run_root, &path, &bytes, false)?;
        Ok(event)
    }
}

#[derive(Default)]
struct RunEventUpdate {
    reserved: Option<AutomationUsageCountersV1>,
    actual: Option<AutomationUsageCountersV1>,
    refunded: Option<AutomationUsageCountersV1>,
    saved_session_id: Option<Uuid>,
    artifact_sha256: Option<String>,
    reason_class: Option<String>,
}

fn run_event_digest(event: &AutomationRunEventV1) -> Result<String, AutomationError> {
    let mut value = event.clone();
    value.event_sha256.clear();
    let bytes =
        serde_json::to_vec(&value).map_err(|error| AutomationError::State(error.to_string()))?;
    Ok(hex_digest(&Sha256::digest(bytes)))
}

fn verify_run_event(event: &AutomationRunEventV1) -> Result<(), AutomationError> {
    if event.schema != AUTOMATION_RUN_SCHEMA
        || !is_sha256(&event.previous_sha256)
        || !is_sha256(&event.event_sha256)
        || run_event_digest(event)? != event.event_sha256
    {
        return Err(AutomationError::State(
            "automation run receipt is invalid".into(),
        ));
    }
    Ok(())
}

fn decimal_sub(left: DecimalUsd, right: DecimalUsd) -> Option<DecimalUsd> {
    let scale = left.scale().max(right.scale());
    let left = left
        .coefficient()
        .checked_mul(10_u128.checked_pow(u32::from(scale - left.scale()))?)?;
    let right = right
        .coefficient()
        .checked_mul(10_u128.checked_pow(u32::from(scale - right.scale()))?)?;
    DecimalUsd::from_parts(left.checked_sub(right)?, scale)
}

const BUDGET_LEDGER_SCHEMA: &str = "ca.automation-budget-ledger.v1";
const BUDGET_RUN_RECEIPT_SCHEMA: &str = "ca.automation-budget-run-receipt.v1";
const MAX_BUDGET_EVENTS: usize = 16_384;
const MAX_BUDGET_RECEIPT_BYTES: u64 = 16 * 1024;
// A typed event's bounded ID, hashes, counters and decimal cannot exceed this.
// Reserve room for every future refund before admitting another dispatch.
const MAX_BUDGET_EVENT_STORAGE_BYTES: u64 = 4096;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BudgetEventKindV1 {
    Reserve,
    Refund,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetEventV1 {
    sequence: u64,
    at: DateTime<Utc>,
    automation_id: String,
    run_id: Uuid,
    kind: BudgetEventKindV1,
    amount: AutomationUsageCountersV1,
    previous_sha256: String,
    event_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetLedgerDataV1 {
    schema: String,
    events: Vec<BudgetEventV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetRunReceiptV1 {
    schema: String,
    reserve: BudgetEventV1,
    refund: BudgetEventV1,
}

impl BudgetRunReceiptV1 {
    fn validate(&self, run_id: Uuid, automation_id: &str) -> Result<(), AutomationError> {
        if self.schema != BUDGET_RUN_RECEIPT_SCHEMA
            || self.reserve.kind != BudgetEventKindV1::Reserve
            || self.refund.kind != BudgetEventKindV1::Refund
            || self.reserve.run_id != run_id
            || self.refund.run_id != run_id
            || self.reserve.automation_id != automation_id
            || self.refund.automation_id != automation_id
            || self.refund.sequence <= self.reserve.sequence
            || !is_sha256(&self.reserve.previous_sha256)
            || !is_sha256(&self.refund.previous_sha256)
            || budget_event_digest(&self.reserve)? != self.reserve.event_sha256
            || budget_event_digest(&self.refund)? != self.refund.event_sha256
            || self
                .reserve
                .amount
                .checked_sub(&self.refund.amount)
                .is_none()
        {
            return Err(AutomationError::State(
                "immutable budget receipt is invalid".into(),
            ));
        }
        validate_id(automation_id)
    }
}

impl Default for BudgetLedgerDataV1 {
    fn default() -> Self {
        Self {
            schema: BUDGET_LEDGER_SCHEMA.into(),
            events: Vec::new(),
        }
    }
}

struct AutomationBudgetLedger {
    root: PathBuf,
    path: PathBuf,
}

impl AutomationBudgetLedger {
    fn for_paths(paths: &RuntimePaths, profile: &ActiveProfile) -> Self {
        let root = paths.profile_data_dir(profile).join("automations/v1");
        Self {
            path: root.join("budget-ledger.json"),
            root,
        }
    }

    fn load(&self) -> Result<BudgetLedgerDataV1, AutomationError> {
        if !self.path.exists() {
            return Ok(BudgetLedgerDataV1::default());
        }
        let data: BudgetLedgerDataV1 =
            serde_json::from_slice(&read_owner_file(&self.path, MAX_STORE_BYTES)?)
                .map_err(|error| AutomationError::State(error.to_string()))?;
        if data.schema != BUDGET_LEDGER_SCHEMA || data.events.len() > MAX_BUDGET_EVENTS {
            return Err(AutomationError::State(
                "automation budget ledger is invalid or oversized".into(),
            ));
        }
        let mut previous = "0".repeat(64);
        for (index, event) in data.events.iter().enumerate() {
            if event.sequence != index as u64
                || event.previous_sha256 != previous
                || event.event_sha256 != budget_event_digest(event)?
            {
                return Err(AutomationError::State(
                    "automation budget hash chain is invalid".into(),
                ));
            }
            previous.clone_from(&event.event_sha256);
        }
        Ok(data)
    }

    fn receipt_path(&self, run_id: Uuid) -> PathBuf {
        self.root
            .join("runs")
            .join(run_id.to_string())
            .join("budget.json")
    }

    fn read_run_receipt(
        &self,
        run_id: Uuid,
        automation_id: &str,
    ) -> Result<Option<BudgetRunReceiptV1>, AutomationError> {
        let path = self.receipt_path(run_id);
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let receipt: BudgetRunReceiptV1 =
            serde_json::from_slice(&read_owner_file(&path, MAX_BUDGET_RECEIPT_BYTES)?)
                .map_err(|error| AutomationError::State(error.to_string()))?;
        receipt.validate(run_id, automation_id)?;
        Ok(Some(receipt))
    }

    fn write_run_receipt(
        &self,
        reserve: &BudgetEventV1,
        refund: &BudgetEventV1,
    ) -> Result<(), AutomationError> {
        let receipt = BudgetRunReceiptV1 {
            schema: BUDGET_RUN_RECEIPT_SCHEMA.into(),
            reserve: reserve.clone(),
            refund: refund.clone(),
        };
        receipt.validate(reserve.run_id, &reserve.automation_id)?;
        if let Some(existing) = self.read_run_receipt(reserve.run_id, &reserve.automation_id)? {
            // Rebased global hashes differ; immutable semantic evidence does not.
            if !same_budget_event(&existing.reserve, reserve)
                || !same_budget_event(&existing.refund, refund)
            {
                return Err(AutomationError::State(
                    "immutable budget receipt differs from settlement".into(),
                ));
            }
            // Reconfirm durability after any previously failed directory sync.
            let path = self.receipt_path(reserve.run_id);
            File::open(&path)?.sync_all()?;
            File::open(path.parent().ok_or_else(|| {
                AutomationError::State("budget receipt has no directory".into())
            })?)?
            .sync_all()?;
            return Ok(());
        }
        let bytes = serde_json::to_vec(&receipt)
            .map_err(|error| AutomationError::State(error.to_string()))?;
        if bytes.len() as u64 > MAX_BUDGET_RECEIPT_BYTES {
            return Err(AutomationError::State(
                "immutable budget receipt is oversized".into(),
            ));
        }
        let path = self.receipt_path(reserve.run_id);
        atomic_owner_write(
            path.parent()
                .ok_or_else(|| AutomationError::State("budget receipt has no directory".into()))?,
            &path,
            &bytes,
            false,
        )
    }

    fn settlement_evidence(
        &self,
        claim: &AutomationClaimV1,
    ) -> Result<(AutomationUsageCountersV1, AutomationUsageCountersV1), AutomationError> {
        if let Some(receipt) = self.read_run_receipt(claim.run_id, &claim.automation_id)? {
            return Ok((receipt.reserve.amount, receipt.refund.amount));
        }
        let data = self.load()?;
        let evidence = budget_run_pairs(&data)?.into_iter().find(|(reserve, _)| {
            reserve.run_id == claim.run_id && reserve.automation_id == claim.automation_id
        });
        if let Some((reserve, Some(refund))) = evidence {
            return Ok((reserve.amount, refund.amount));
        }
        // A compactor may have published the receipt between the first probe
        // and the atomic ledger read. It always syncs evidence before rebasing.
        if let Some(receipt) = self.read_run_receipt(claim.run_id, &claim.automation_id)? {
            return Ok((receipt.reserve.amount, receipt.refund.amount));
        }
        Err(AutomationError::State(
            "recorded budget settlement evidence is missing".into(),
        ))
    }

    fn compact_settled(
        &self,
        data: &mut BudgetLedgerDataV1,
        now: DateTime<Utc>,
    ) -> Result<(), AutomationError> {
        let cutoff = now - chrono::Duration::days(30);
        let mut removed = HashSet::new();
        for (reserve, refund) in budget_run_pairs(data)? {
            if reserve.at < cutoff {
                if let Some(refund) = refund {
                    // Never drop evidence before its exact private run receipt
                    // reaches durable storage. Unsettled reservations stay.
                    self.write_run_receipt(&reserve, &refund)?;
                    removed.insert(reserve.run_id);
                }
            }
        }
        if removed.is_empty() {
            return Ok(());
        }
        data.events.retain(|event| !removed.contains(&event.run_id));
        let mut previous = "0".repeat(64);
        for (index, event) in data.events.iter_mut().enumerate() {
            event.sequence = index as u64;
            event.previous_sha256 = previous;
            event.event_sha256 = budget_event_digest(event)?;
            previous = event.event_sha256.clone();
        }
        Ok(())
    }

    fn reserve(
        &self,
        claim: &AutomationClaimV1,
        now: DateTime<Utc>,
    ) -> Result<AutomationUsageCountersV1, AutomationError> {
        let _lock = OwnerLock::acquire(self.root.join("budget.lock"))?;
        let mut data = self.load()?;
        self.compact_settled(&mut data, now)?;
        if data
            .events
            .iter()
            .any(|event| event.run_id == claim.run_id && event.kind == BudgetEventKindV1::Reserve)
        {
            return Err(AutomationError::State(
                "automation run budget was already reserved".into(),
            ));
        }
        if self
            .read_run_receipt(claim.run_id, &claim.automation_id)?
            .is_some()
        {
            return Err(AutomationError::State(
                "automation run budget was already settled".into(),
            ));
        }
        let mut reservation =
            AutomationUsageCountersV1::from_budget(&claim.definition.per_run_budget);
        if !matches!(
            claim.definition.pricing,
            PricingBindingV1::Metered { .. } | PricingBindingV1::MeteredRoutes { .. }
        ) {
            reservation.cost_usd = DecimalUsd::ZERO;
        }
        let cutoff = now - chrono::Duration::days(30);
        let mut rolling = AutomationUsageCountersV1::default();
        let mut outstanding = 0_u64;
        for (reserve, refund) in budget_run_pairs(&data)? {
            outstanding += u64::from(refund.is_none());
            if reserve.automation_id != claim.automation_id
                || (reserve.at < cutoff && refund.is_some())
            {
                continue;
            }
            let net = match refund {
                Some(refund) => reserve.amount.checked_sub(&refund.amount),
                None => Some(reserve.amount),
            }
            .ok_or_else(|| {
                AutomationError::State("automation rolling refund exceeded reservation".into())
            })?;
            rolling = rolling.checked_add(&net).ok_or_else(|| {
                AutomationError::State("automation rolling budget arithmetic failed".into())
            })?;
        }
        let projected = rolling
            .checked_add(&reservation)
            .ok_or_else(|| AutomationError::State("automation rolling budget overflow".into()))?;
        if !projected.within(&claim.definition.rolling_30_day_budget) {
            return Err(AutomationError::State(
                "automation rolling 30-day budget is exhausted".into(),
            ));
        }
        append_budget_event(
            &mut data,
            &claim.automation_id,
            claim.run_id,
            BudgetEventKindV1::Reserve,
            reservation.clone(),
            now,
        )?;
        let future_refunds = outstanding + 1;
        let bytes = serde_json::to_vec_pretty(&data)
            .map_err(|error| AutomationError::State(error.to_string()))?
            .len() as u64;
        if data.events.len() as u64 + future_refunds > MAX_BUDGET_EVENTS as u64
            || bytes + future_refunds * MAX_BUDGET_EVENT_STORAGE_BYTES > MAX_STORE_BYTES
        {
            return Err(AutomationError::State("automation budget evidence capacity is exhausted; active reservations were preserved".into()));
        }
        self.save(&data)?;
        Ok(reservation)
    }

    fn settle(
        &self,
        claim: &AutomationClaimV1,
        reserved: &AutomationUsageCountersV1,
        actual: &AutomationUsageCountersV1,
        usage_authoritative: bool,
        now: DateTime<Utc>,
    ) -> Result<AutomationUsageCountersV1, AutomationError> {
        if !actual.within(&claim.definition.per_run_budget) {
            return Err(AutomationError::State(
                "automation exceeded its per-run budget".into(),
            ));
        }
        let charged = if usage_authoritative {
            actual.clone()
        } else {
            reserved.clone()
        };
        let refund = reserved.checked_sub(&charged).ok_or_else(|| {
            AutomationError::State("automation budget settlement exceeded its reservation".into())
        })?;
        let _lock = OwnerLock::acquire(self.root.join("budget.lock"))?;
        let mut data = self.load()?;
        self.compact_settled(&mut data, now)?;
        let reserve_count = data
            .events
            .iter()
            .filter(|event| {
                event.run_id == claim.run_id && event.kind == BudgetEventKindV1::Reserve
            })
            .count();
        let refund_exists = data
            .events
            .iter()
            .any(|event| event.run_id == claim.run_id && event.kind == BudgetEventKindV1::Refund);
        if reserve_count != 1 || refund_exists {
            return Err(AutomationError::State(
                "automation budget settlement state is invalid".into(),
            ));
        }
        if data
            .events
            .iter()
            .find(|event| event.run_id == claim.run_id && event.kind == BudgetEventKindV1::Reserve)
            .is_none_or(|event| {
                event.automation_id != claim.automation_id || &event.amount != reserved
            })
        {
            return Err(AutomationError::State(
                "budget settlement does not match its exact reservation".into(),
            ));
        }
        append_budget_event(
            &mut data,
            &claim.automation_id,
            claim.run_id,
            BudgetEventKindV1::Refund,
            refund.clone(),
            now,
        )?;
        let reserve = data
            .events
            .iter()
            .find(|event| event.run_id == claim.run_id && event.kind == BudgetEventKindV1::Reserve)
            .ok_or_else(|| AutomationError::State("budget reservation is missing".into()))?
            .clone();
        let refund_event = data
            .events
            .last()
            .ok_or_else(|| AutomationError::State("budget settlement is missing".into()))?
            .clone();
        // Archive exact evidence before either refund commit or any later
        // global rebase. A crash here retains the full outstanding reservation.
        self.write_run_receipt(&reserve, &refund_event)?;
        self.save(&data)?;
        Ok(refund)
    }

    fn save(&self, data: &BudgetLedgerDataV1) -> Result<(), AutomationError> {
        let bytes = serde_json::to_vec_pretty(data)
            .map_err(|error| AutomationError::State(error.to_string()))?;
        if bytes.len() as u64 > MAX_STORE_BYTES || data.events.len() > MAX_BUDGET_EVENTS {
            return Err(AutomationError::State(
                "automation budget ledger is oversized".into(),
            ));
        }
        atomic_owner_write(&self.root, &self.path, &bytes, true)
    }
}

/// Import only proven paid settlements from the pre-daily-cap native journals.
/// Unknown usage retains the full grant and has no positive USD refund; the
/// migration's separate conservative hold covers that historical ambiguity.
pub(crate) fn legacy_learning_settled_microusd(
    paths: &RuntimePaths,
    now: DateTime<Utc>,
) -> Result<u64, AutomationError> {
    let eastern_date = codex_team::self_improvement::eastern_learning_date(now)
        .map_err(|error| AutomationError::State(error.to_string()))?;
    let mut profiles = vec![ActiveProfile::Default];
    let profile_root = paths.data_dir.join("profiles");
    if profile_root.exists() {
        reject_symlink_components(&profile_root)?;
        for entry in std::fs::read_dir(&profile_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let metadata = entry.metadata()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt as _;
                if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
                    continue;
                }
            }
            #[cfg(not(unix))]
            let _ = metadata;
            let name = entry.file_name();
            if let Some(Ok(profile @ ActiveProfile::Named(_))) =
                name.to_str().map(|name| ActiveProfile::parse(Some(name)))
            {
                profiles.push(profile);
            }
        }
    }

    let mut total = 0_u64;
    for profile in profiles {
        let ledger = AutomationBudgetLedger::for_paths(paths, &profile);
        let mut pairs: BTreeMap<_, _> = budget_run_pairs(&ledger.load()?)?
            .into_iter()
            .map(|(reserve, refund)| (reserve.run_id, (reserve, refund)))
            .collect();
        let runs_root = ledger.root.join("runs");
        if runs_root.exists() {
            reject_symlink_components(&runs_root)?;
            for entry in std::fs::read_dir(&runs_root)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let Some(run_id) = entry
                    .file_name()
                    .to_str()
                    .and_then(|name| Uuid::parse_str(name).ok())
                else {
                    continue;
                };
                let receipt_path = ledger.receipt_path(run_id);
                if !receipt_path.exists() {
                    continue;
                }
                let receipt: BudgetRunReceiptV1 = serde_json::from_slice(&read_owner_file(
                    &receipt_path,
                    MAX_BUDGET_RECEIPT_BYTES,
                )?)
                .map_err(|error| AutomationError::State(error.to_string()))?;
                receipt.validate(run_id, &receipt.reserve.automation_id)?;
                if let Some((reserve, refund)) = pairs.get(&run_id) {
                    if !same_budget_event(reserve, &receipt.reserve)
                        || refund
                            .as_ref()
                            .is_some_and(|refund| !same_budget_event(refund, &receipt.refund))
                    {
                        return Err(AutomationError::State(
                            "legacy learning settlement evidence changed".into(),
                        ));
                    }
                }
                pairs.insert(run_id, (receipt.reserve, Some(receipt.refund)));
            }
        }
        for (reserve, refund) in pairs.into_values() {
            let Some(refund) = refund else { continue };
            if reserve.at > now
                || refund.at > now
                || refund.amount.cost_usd == DecimalUsd::ZERO
                || codex_team::self_improvement::eastern_learning_date(reserve.at)
                    .map_err(|error| AutomationError::State(error.to_string()))?
                    != eastern_date
            {
                continue;
            }
            let intent: DeliveryIntentV1 = serde_json::from_slice(&read_owner_file(
                &run_directory(paths, &profile, reserve.run_id).join("intent.json"),
                MAX_STORE_BYTES,
            )?)
            .map_err(|error| AutomationError::State(error.to_string()))?;
            intent.validate(&intent.claim)?;
            if intent.claim.run_id != reserve.run_id
                || intent.claim.automation_id != reserve.automation_id
                || intent.claim.profile != profile.label()
            {
                return Err(AutomationError::State(
                    "legacy learning settlement claim changed".into(),
                ));
            }
            if !matches!(
                intent.claim.definition.schedule.target,
                ScheduleTargetV2::RepositoryTask { .. }
                    | ScheduleTargetV2::HarnessDiscovery { .. }
                    | ScheduleTargetV2::HarnessEvaluation { .. }
                    | ScheduleTargetV2::HarnessCycle { .. }
                    | ScheduleTargetV2::KnowledgeLearning { .. }
            ) {
                continue;
            }
            let actual = reserve.amount.checked_sub(&refund.amount).ok_or_else(|| {
                AutomationError::State("legacy learning refund exceeded reservation".into())
            })?;
            let usd = actual.cost_usd;
            let micros = if usd.scale() <= 6 {
                usd.coefficient()
                    .checked_mul(10_u128.pow(u32::from(6 - usd.scale())))
            } else {
                let denominator = 10_u128.pow(u32::from(usd.scale() - 6));
                Some(usd.coefficient().div_ceil(denominator))
            }
            .and_then(|micros| u64::try_from(micros).ok())
            .ok_or_else(|| AutomationError::State("legacy learning cost overflow".into()))?;
            total = total
                .checked_add(micros)
                .ok_or_else(|| AutomationError::State("legacy learning total overflow".into()))?;
        }
    }
    Ok(total)
}

fn same_budget_event(left: &BudgetEventV1, right: &BudgetEventV1) -> bool {
    left.at == right.at
        && left.automation_id == right.automation_id
        && left.run_id == right.run_id
        && left.kind == right.kind
        && left.amount == right.amount
}

fn budget_run_pairs(
    data: &BudgetLedgerDataV1,
) -> Result<Vec<(BudgetEventV1, Option<BudgetEventV1>)>, AutomationError> {
    let mut runs = BTreeMap::<Uuid, (Option<BudgetEventV1>, Option<BudgetEventV1>)>::new();
    for event in &data.events {
        validate_id(&event.automation_id)?;
        let pair = runs.entry(event.run_id).or_default();
        let slot = match event.kind {
            BudgetEventKindV1::Reserve => &mut pair.0,
            BudgetEventKindV1::Refund => &mut pair.1,
        };
        if slot.replace(event.clone()).is_some() {
            return Err(AutomationError::State(
                "duplicate budget run evidence".into(),
            ));
        }
    }
    runs.into_values()
        .map(|(reserve, refund)| {
            let reserve = reserve
                .ok_or_else(|| AutomationError::State("budget refund has no reservation".into()))?;
            if refund.as_ref().is_some_and(|refund| {
                refund.automation_id != reserve.automation_id
                    || refund.sequence <= reserve.sequence
                    || reserve.amount.checked_sub(&refund.amount).is_none()
            }) {
                return Err(AutomationError::State(
                    "budget refund identity or amount is invalid".into(),
                ));
            }
            Ok((reserve, refund))
        })
        .collect()
}

fn append_budget_event(
    data: &mut BudgetLedgerDataV1,
    automation_id: &str,
    run_id: Uuid,
    kind: BudgetEventKindV1,
    amount: AutomationUsageCountersV1,
    at: DateTime<Utc>,
) -> Result<(), AutomationError> {
    let sequence = u64::try_from(data.events.len())
        .map_err(|_| AutomationError::State("automation budget sequence overflow".into()))?;
    let previous_sha256 = data
        .events
        .last()
        .map_or_else(|| "0".repeat(64), |event| event.event_sha256.clone());
    let mut event = BudgetEventV1 {
        sequence,
        at,
        automation_id: automation_id.into(),
        run_id,
        kind,
        amount,
        previous_sha256,
        event_sha256: String::new(),
    };
    event.event_sha256 = budget_event_digest(&event)?;
    if serde_json::to_vec_pretty(&event)
        .map_err(|error| AutomationError::State(error.to_string()))?
        .len() as u64
        > MAX_BUDGET_EVENT_STORAGE_BYTES
    {
        return Err(AutomationError::State(
            "automation budget event is oversized".into(),
        ));
    }
    data.events.push(event);
    Ok(())
}

fn budget_event_digest(event: &BudgetEventV1) -> Result<String, AutomationError> {
    let mut value = event.clone();
    value.event_sha256.clear();
    let bytes =
        serde_json::to_vec(&value).map_err(|error| AutomationError::State(error.to_string()))?;
    Ok(hex_digest(&Sha256::digest(bytes)))
}

fn validate_id(value: &str) -> Result<(), AutomationError> {
    if !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        Ok(())
    } else {
        Err(AutomationError::Invalid("invalid automation ID".into()))
    }
}

fn validate_name(value: &str) -> Result<(), AutomationError> {
    if value.trim() != value
        || value.is_empty()
        || value.len() > MAX_NAME_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(AutomationError::Invalid("invalid automation name".into()));
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn stable_run_id(definition_digest: &str, occurrence_id: &str) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(definition_digest);
    hasher.update([0]);
    hasher.update(occurrence_id);
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn require_confirmation(value: &str, expected: &str) -> Result<(), AutomationError> {
    let equal = value.len() == expected.len()
        && value
            .bytes()
            .zip(expected.bytes())
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0;
    if equal {
        Ok(())
    } else {
        Err(AutomationError::Confirmation(expected.into()))
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn bounded(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn normalize_missing_path(path: &Path) -> Option<PathBuf> {
    let existing = path.ancestors().find(|ancestor| ancestor.exists())?;
    let canonical = existing.canonicalize().ok()?;
    Some(canonical.join(path.strip_prefix(existing).ok()?))
}

fn ensure_private_dir(path: &Path) -> Result<(), AutomationError> {
    reject_symlink_components(path)?;
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(AutomationError::State(
            "private path is not a directory".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(AutomationError::State(
                "private directory ownership or mode is unsafe".into(),
            ));
        }
    }
    Ok(())
}

fn read_owner_file(path: &Path, limit: u64) -> Result<Vec<u8>, AutomationError> {
    reject_symlink_components(path)?;
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit {
        return Err(AutomationError::State(
            "state file is unsafe or oversized".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(AutomationError::State(
                "state file ownership or mode is unsafe".into(),
            ));
        }
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(AutomationError::State("state file is oversized".into()));
    }
    Ok(bytes)
}

fn atomic_owner_write(
    dir: &Path,
    path: &Path,
    bytes: &[u8],
    replace: bool,
) -> Result<(), AutomationError> {
    ensure_private_dir(dir)?;
    let temporary = dir.join(format!(".automation.{}.tmp", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let result = (|| -> Result<(), AutomationError> {
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if !replace && path.exists() {
            return Err(AutomationError::State(
                "immutable automation state already exists".into(),
            ));
        }
        std::fs::rename(&temporary, path)?;
        File::open(dir)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn reject_symlink_components(path: &Path) -> Result<(), AutomationError> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if std::fs::symlink_metadata(ancestor)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(AutomationError::State(
                "automation path contains a symbolic link".into(),
            ));
        }
    }
    Ok(())
}

struct OwnerLock {
    _lease: crate::session_access::SessionWriterLease,
}

impl OwnerLock {
    fn acquire(path: PathBuf) -> Result<Self, AutomationError> {
        let parent = path
            .parent()
            .ok_or_else(|| AutomationError::State("lock has no parent".into()))?;
        ensure_private_dir(parent)?;
        let id = Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            path.file_name().unwrap_or_default().as_encoded_bytes(),
        );
        let _lease =
            crate::session_access::SessionAccessCoordinator::new(parent.join(".automation-access"))
                .try_writer(id)
                .map_err(access_error)?;
        Ok(Self { _lease })
    }
}

fn access_error(error: std::io::Error) -> AutomationError {
    if error.kind() == std::io::ErrorKind::WouldBlock {
        AutomationError::Busy
    } else {
        AutomationError::Io(error.to_string())
    }
}

fn run_access(root: &Path) -> crate::session_access::SessionAccessCoordinator {
    crate::session_access::SessionAccessCoordinator::new(root.join(".run-access"))
}

impl PartialEq for AutomationClaimV1 {
    fn eq(&self, other: &Self) -> bool {
        self.schema == other.schema
            && self.run_id == other.run_id
            && self.profile == other.profile
            && self.automation_id == other.automation_id
            && self.definition_sha256 == other.definition_sha256
            && self.occurrence == other.occurrence
            && self.claim_revision == other.claim_revision
            && self.coalesced_occurrences == other.coalesced_occurrences
            && self.definition == other.definition
    }
}
impl Eq for AutomationClaimV1 {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule_v2::{
        MissedRunPolicy, ScheduleDeliveryV2, ScheduleTriggerV2, WorkspacePolicy,
    };
    use tempfile::TempDir;

    fn definition(workspace: &Path) -> AutomationDefinitionV1 {
        AutomationDefinitionV1 {
            schema: AUTOMATION_SCHEMA.into(),
            id: "daily-review".into(),
            name: "Daily review".into(),
            automation_kind: AutomationKindV1::Standalone,
            schedule: ScheduleDefinitionV2 {
                version: 2,
                id: "daily-review".into(),
                provider: Some("local".into()),
                model: Some("fixture".into()),
                workspace: workspace.to_path_buf(),
                runtime_secs: 300,
                output_bytes: 1024,
                tools: vec![],
                grants: vec![],
                allow_network: false,
                skills: vec![],
                workspace_policy: WorkspacePolicy::ExistingReadOnly,
                pinned_revision: None,
                trigger: ScheduleTriggerV2::Interval {
                    seconds: 3600,
                    anchor: None,
                },
                missed_run_policy: MissedRunPolicy::Skip,
                target: ScheduleTargetV2::Prompt {
                    prompt: "Review the repository".into(),
                },
                delivery: ScheduleDeliveryV2::None,
            },
            reasoning_effort: Some(ReasoningEffort::High),
            pricing: PricingBindingV1::LocalZero,
            context_sha256: vec![],
            per_run_budget: AutomationBudgetV1::per_run_default(),
            rolling_30_day_budget: AutomationBudgetV1::rolling_default(),
            notification_policy: AutomationNotificationPolicy::AllResults,
        }
    }

    #[test]
    fn knowledge_notifications_only_publish_new_review_completion_failure_or_action() {
        let mut definition = definition(Path::new("/fixture"));
        definition.schedule.target = ScheduleTargetV2::KnowledgeLearning {
            vault_root: PathBuf::from("/fixture/vault"),
            enrollment_digest: "a".repeat(64),
        };
        definition.notification_policy = AutomationNotificationPolicy::MeaningfulChanges;
        let unchanged = r#"{"schema":"ca.heartbeat-report.v1","status":"unchanged","summary":"one replay finished","state":"candidate:1"}"#;
        assert!(
            !heartbeat_notification(&definition, unchanged, None)
                .1
                .unwrap()
                .publish
        );
        for status in ["changed", "completed", "failed", "needs_attention"] {
            let raw=serde_json::json!({"schema":"ca.heartbeat-report.v1","status":status,"summary":"evaluation event","state":"candidate:1"}).to_string();
            let notification = heartbeat_notification(&definition, &raw, None).1.unwrap();
            assert!(notification.publish);
            assert!(notification.validate(&definition));
            let duplicate =
                heartbeat_notification(&definition, &raw, Some(&notification.state_sha256))
                    .1
                    .unwrap();
            assert!(!duplicate.publish);
            assert!(duplicate.validate(&definition));
        }
    }

    #[test]
    fn schema_is_strict_and_defaults_are_balanced() {
        let temp = TempDir::new().unwrap();
        let raw = serde_json::to_value(definition(temp.path())).unwrap();
        let mut object = raw.as_object().unwrap().clone();
        object.insert("unexpected".into(), serde_json::json!(true));
        assert!(serde_json::from_value::<AutomationDefinitionV1>(object.into()).is_err());
        assert_eq!(default_per_run_budget().provider_requests, 16);
        assert_eq!(default_rolling_budget().input_tokens, 10_000_000);
    }

    #[test]
    fn native_mechanistic_batch_rejects_exhausted_budget_before_storage() {
        let temp = TempDir::new().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(temp.path());
        let profile = crate::runtime::ActiveProfile::Default;
        let mut proposal = definition(temp.path());
        proposal.schedule.target = ScheduleTargetV2::Mechanistic {
            actions: vec![
                codex_team_protocol::MechanisticActionV1::GitHistory { max_entries: 1 },
                codex_team_protocol::MechanisticActionV1::GitHistory { max_entries: 2 },
            ],
        };
        proposal.per_run_budget.tool_calls = 1;
        let error = crate::automation_runtime::execution_context(
            &paths,
            &profile,
            &proposal,
            Uuid::new_v4(),
        )
        .unwrap_err();
        assert!(error.contains("tool-call budget exhausted"));
        assert!(!crate::automation_runtime::state_root(&paths, &profile).exists());
        assert!(!crate::automation_runtime::lease_root(&paths).exists());
    }

    #[test]
    fn exact_confirmation_creates_enabled_private_record_without_touching_v2() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let store = AutomationStore::new(temp.path().join("store"));
        let now = "2026-08-29T12:00:00Z".parse().unwrap();
        let normalized = definition(&workspace).normalize(now).unwrap();
        let digest = definition_digest(&normalized).unwrap();
        assert!(matches!(
            store.add_confirmed(definition(&workspace), "bad", 0, now),
            Err(AutomationError::Confirmation(_))
        ));
        let record = store
            .add_confirmed(definition(&workspace), &digest, 0, now)
            .unwrap();
        assert!(record.enabled);
        assert!(record.next_occurrence.is_some());
        assert_eq!(store.records().unwrap(), vec![record]);
        assert!(!temp.path().join("store/schedules-v2.json").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            let metadata = std::fs::metadata(temp.path().join("store/automations.json")).unwrap();
            assert_eq!(metadata.mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn claim_and_settle_use_compare_and_swap() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let store = AutomationStore::new(temp.path().join("store"));
        let now = "2026-08-29T12:00:00Z".parse().unwrap();
        let mut draft = definition(&workspace);
        draft.schedule.trigger = ScheduleTriggerV2::Absolute { at: now };
        let normalized = draft
            .clone()
            .normalize(now - chrono::Duration::seconds(1))
            .unwrap();
        let digest = definition_digest(&normalized).unwrap();
        store
            .add_confirmed(draft, &digest, 0, now - chrono::Duration::seconds(1))
            .unwrap();
        let claim = store.claim_due("default", now).unwrap().unwrap();
        assert!(store.claim_due("default", now).unwrap().is_none());
        let mut wrong = claim.clone();
        wrong.claim_revision += 1;
        assert!(matches!(
            store.settle(&wrong),
            Err(AutomationError::State(_))
        ));
        store.settle(&claim).unwrap();
        assert!(store.records().unwrap()[0].active_run_id.is_none());
    }

    #[tokio::test]
    async fn due_runner_without_automation_state_is_read_only() {
        let temp = TempDir::new().unwrap();
        let paths = RuntimePaths::from_home(temp.path());
        let profile = ActiveProfile::Default;
        let now = "2026-08-29T12:00:00Z".parse().unwrap();

        assert_eq!(run_due(&paths, &profile, true, now).await.unwrap(), 0);
        assert!(!paths
            .profile_data_dir(&profile)
            .join("automations/v1")
            .exists());
    }

    #[test]
    fn combined_capacity_fails_closed() {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let store = AutomationStore::new(temp.path().join("store"));
        let now = "2026-08-29T12:00:00Z".parse().unwrap();
        let normalized = definition(&workspace).normalize(now).unwrap();
        let digest = definition_digest(&normalized).unwrap();
        assert!(matches!(
            store.add_confirmed(
                definition(&workspace),
                &digest,
                MAX_COMBINED_AUTOMATIONS,
                now
            ),
            Err(AutomationError::State(_))
        ));
        assert!(store.records().unwrap().is_empty());
    }

    fn recorded_result(
        temp: &TempDir,
        heartbeat: bool,
    ) -> (
        RuntimePaths,
        ActiveProfile,
        AutomationStore,
        crate::session::SessionStore,
        AutomationClaimV1,
        DeliveryCheckpointV1,
    ) {
        recorded_report(temp, heartbeat, None)
    }

    fn recorded_report(
        temp: &TempDir,
        heartbeat: bool,
        report: Option<(&str, AutomationNotificationPolicy, Option<&str>)>,
    ) -> (
        RuntimePaths,
        ActiveProfile,
        AutomationStore,
        crate::session::SessionStore,
        AutomationClaimV1,
        DeliveryCheckpointV1,
    ) {
        let home = temp.path().canonicalize().unwrap();
        let paths = RuntimePaths::from_home(&home);
        let profile = ActiveProfile::Default;
        let workspace = home.join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let profile_paths = paths.clone().for_profile(&profile);
        let sessions = crate::session::SessionStore::with_archive(
            profile_paths.sessions_dir,
            profile_paths.archived_sessions_dir,
        );
        let mut definition = definition(&workspace);
        if let Some((_, policy, _)) = report {
            definition.notification_policy = policy;
        }
        let now: DateTime<Utc> = "2026-08-29T12:00:00Z".parse().unwrap();
        let binding = if heartbeat {
            let mut source = crate::session::SessionV2::new(
                "source",
                workspace.to_string_lossy(),
                "local",
                "fixture",
                vec![],
            );
            sessions.save(&mut source).unwrap();
            definition.automation_kind = AutomationKindV1::Heartbeat {
                session_id: source.id,
                workspace_identity_sha256: session_workspace_identity(&source).unwrap(),
            };
            definition.context_sha256 = vec![session_context_authority_digest(&source).unwrap()];
            Some(sessions.automation_snapshot(source.id).unwrap().1)
        } else {
            None
        };
        let definition = definition.normalize(now).unwrap();
        let digest = definition_digest(&definition).unwrap();
        let store = AutomationStore::for_paths(&paths, &profile);
        store.add_confirmed(definition, &digest, 0, now).unwrap();
        if let Some((_, _, previous)) = report {
            let mut data = store.load().unwrap();
            data.records[0].last_heartbeat_state_sha256 = previous.map(str::to_owned);
            store.save(&mut data).unwrap();
        }
        let mut claim = store
            .claim_immediate(profile.label(), "daily-review", now)
            .unwrap();
        let lease = claim.runtime_lease.take();
        let intent = DeliveryIntentV1::new(&claim, binding);
        write_run_intent(&paths, &profile, &intent).unwrap();
        let budget = AutomationBudgetLedger::for_paths(&paths, &profile);
        let reserved = budget.reserve(&claim, now).unwrap();
        let actual = AutomationUsageCountersV1::default();
        let refunded = budget
            .settle(&claim, &reserved, &actual, true, now)
            .unwrap();
        let (content, heartbeat_notification) = match report {
            Some((raw, _, previous)) => heartbeat_notification(&claim.definition, raw, previous),
            None => ("recorded result".to_owned(), None),
        };
        store_private_result(&paths, &profile, &claim, &content).unwrap();
        let checkpoint = DeliveryCheckpointV1 {
            schema: DELIVERY_CHECKPOINT_SCHEMA.into(),
            intent,
            result_sha256: hex_digest(&Sha256::digest(content.as_bytes())),
            result_bytes: content.len(),
            heartbeat_notification,
            execution_failed: false,
            native_alert: None,
            reserved,
            actual,
            refunded,
            artifact_sha256: None,
            completed_at: now,
        };
        write_delivery_checkpoint(&paths, &profile, &checkpoint).unwrap();
        drop(lease);
        (paths, profile, store, sessions, claim, checkpoint)
    }

    fn next_budget_claim(claim: &AutomationClaimV1, now: DateTime<Utc>) -> AutomationClaimV1 {
        let mut next = claim.clone();
        next.occurrence = Occurrence {
            id: format!("budget-test-{}", Uuid::new_v4()),
            at: now,
        };
        next.run_id = stable_run_id(&next.definition_sha256, &next.occurrence.id);
        next
    }

    #[test]
    fn legacy_learning_settlements_cover_profiles_eastern_day_and_archive_duplicates() {
        fn settlement(
            paths: &RuntimePaths,
            profile: &ActiveProfile,
            target: ScheduleTargetV2,
            at: DateTime<Utc>,
            cost: &str,
            authoritative: bool,
        ) -> AutomationClaimV1 {
            let mut definition = definition(&paths.data_dir);
            definition.schedule.target = target;
            definition.pricing = PricingBindingV1::Metered {
                price_card_sha256: "a".repeat(64),
            };
            let definition_sha256 = definition_digest(&definition).unwrap();
            let occurrence = Occurrence {
                id: Uuid::new_v4().to_string(),
                at,
            };
            let claim = AutomationClaimV1 {
                schema: AUTOMATION_RUN_SCHEMA.into(),
                run_id: stable_run_id(&definition_sha256, &occurrence.id),
                profile: profile.label().into(),
                automation_id: definition.id.clone(),
                definition_sha256,
                occurrence,
                claim_revision: 1,
                coalesced_occurrences: 0,
                definition,
                runtime_lease: None,
            };
            write_run_intent(paths, profile, &DeliveryIntentV1::new(&claim, None)).unwrap();
            let ledger = AutomationBudgetLedger::for_paths(paths, profile);
            let reserved = ledger.reserve(&claim, at).unwrap();
            let actual = AutomationUsageCountersV1 {
                cost_usd: DecimalUsd::from_str(cost).unwrap(),
                ..Default::default()
            };
            ledger
                .settle(&claim, &reserved, &actual, authoritative, at)
                .unwrap();
            claim
        }

        let temp = TempDir::new().unwrap();
        let paths = RuntimePaths::from_home(temp.path().canonicalize().unwrap());
        let now = "2026-10-10T03:30:00Z".parse::<DateTime<Utc>>().unwrap();
        let default = ActiveProfile::Default;
        let named = ActiveProfile::Named("work".into());
        let harness = ScheduleTargetV2::HarnessCycle {
            vault_root: paths.home.join("fixture-vault"),
        };
        let utc_previous_day = "2026-10-09T23:00:00Z".parse().unwrap();
        let default_claim = settlement(
            &paths,
            &default,
            harness.clone(),
            utc_previous_day,
            "0.1000001",
            true,
        );
        settlement(
            &paths,
            &named,
            ScheduleTargetV2::KnowledgeLearning {
                vault_root: paths.home.join("fixture-vault"),
                enrollment_digest: "b".repeat(64),
            },
            now,
            "0.2",
            true,
        );
        // Simulate the old ledger rotation: immutable receipts are sufficient.
        let named_ledger = AutomationBudgetLedger::for_paths(&paths, &named);
        named_ledger.save(&BudgetLedgerDataV1::default()).unwrap();
        settlement(
            &paths,
            &default,
            harness.clone(),
            now - chrono::Duration::days(1),
            "0.7",
            true,
        );
        settlement(
            &paths,
            &default,
            ScheduleTargetV2::Prompt {
                prompt: "An unrelated scheduled task".into(),
            },
            now,
            "0.9",
            true,
        );
        settlement(&paths, &default, harness, now, "0", false);

        let default_ledger = AutomationBudgetLedger::for_paths(&paths, &default);
        let ledger_bytes = std::fs::read(&default_ledger.path).unwrap();
        let receipt_bytes =
            std::fs::read(default_ledger.receipt_path(default_claim.run_id)).unwrap();
        assert_eq!(
            legacy_learning_settled_microusd(&paths, now).unwrap(),
            300_001
        );
        assert_eq!(std::fs::read(&default_ledger.path).unwrap(), ledger_bytes);
        assert_eq!(
            std::fs::read(default_ledger.receipt_path(default_claim.run_id)).unwrap(),
            receipt_bytes
        );

        let mut tampered = default_ledger
            .read_run_receipt(default_claim.run_id, &default_claim.automation_id)
            .unwrap()
            .unwrap();
        tampered.refund.at -= chrono::Duration::seconds(1);
        tampered.refund.event_sha256 = budget_event_digest(&tampered.refund).unwrap();
        atomic_owner_write(
            default_ledger
                .receipt_path(default_claim.run_id)
                .parent()
                .unwrap(),
            &default_ledger.receipt_path(default_claim.run_id),
            &serde_json::to_vec(&tampered).unwrap(),
            true,
        )
        .unwrap();
        assert!(legacy_learning_settled_microusd(&paths, now).is_err());
    }

    #[test]
    fn legacy_learning_settlement_inspection_creates_no_runtime_state() {
        let temp = TempDir::new().unwrap();
        let paths = RuntimePaths::from_home(temp.path().canonicalize().unwrap());
        assert_eq!(
            legacy_learning_settled_microusd(&paths, "2026-10-10T03:30:00Z".parse().unwrap())
                .unwrap(),
            0
        );
        assert!(!paths.data_dir.exists());
    }

    #[test]
    fn native_completion_before_parent_checkpoint_recovers_and_delivers_once() {
        let temp = TempDir::new().unwrap();
        let (paths, profile, store, sessions, claim, _) = recorded_result(&temp, false);
        let root = run_directory(&paths, &profile, claim.run_id);
        std::fs::remove_file(root.join("delivery.json")).unwrap();
        std::fs::remove_file(root.join("result.txt")).unwrap();
        let context = crate::automation_runtime::execution_context(
            &paths,
            &profile,
            &claim.definition,
            claim.run_id,
        )
        .unwrap();
        let native = ca_automation::runtime::RunStore::open(&context).unwrap();
        native.start().unwrap();
        native.prepare_action("target").unwrap();
        native.begin_action("target").unwrap();
        let result = serde_json::to_value(crate::automation_runtime::NativeResult {
            content: "recorded result".into(),
            usage: AutomationUsageCountersV1::default(),
        })
        .unwrap();
        native.confirm_action("target", result.clone()).unwrap();
        native
            .finish(
                ca_automation::runtime::ExecutionState::Succeeded,
                Some(result),
                None,
            )
            .unwrap();
        drop(native);
        assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 1);
        assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 0);
        let saved = sessions
            .load(&standalone_session_id(claim.run_id).to_string())
            .unwrap();
        assert_eq!(
            saved
                .messages
                .iter()
                .filter(|m| m.content == "recorded result")
                .count(),
            1
        );
        assert!(store.records().unwrap()[0].active_run_id.is_none());
    }

    #[test]
    fn expired_budget_rotation_archives_legacy_evidence_before_rebase_and_keeps_results_readable() {
        let temp = TempDir::new().unwrap();
        let (paths, profile, _, _, claim, _) = recorded_result(&temp, false);
        let ledger = AutomationBudgetLedger::for_paths(&paths, &profile);
        std::fs::remove_file(ledger.receipt_path(claim.run_id)).unwrap();
        let old_bytes = std::fs::read(&ledger.path).unwrap();
        let now = claim.occurrence.at + chrono::Duration::days(31);
        let mut rotated = ledger.load().unwrap();
        ledger.compact_settled(&mut rotated, now).unwrap();
        assert!(rotated.events.is_empty());
        assert_eq!(std::fs::read(&ledger.path).unwrap(), old_bytes);
        let receipt_bytes = std::fs::read(ledger.receipt_path(claim.run_id)).unwrap();
        // Simulate death after receipt sync, before the global atomic rebase.
        let next = next_budget_claim(&claim, now);
        ledger.reserve(&next, now).unwrap();
        assert_eq!(ledger.load().unwrap().events.len(), 1);
        assert_eq!(
            std::fs::read(ledger.receipt_path(claim.run_id)).unwrap(),
            receipt_bytes
        );
        assert_eq!(
            read_recorded_result(&paths, &profile, &claim.automation_id, claim.run_id).unwrap(),
            "recorded result"
        );
        assert!(ledger.reserve(&claim, now).is_err());
    }

    #[test]
    fn old_unsettled_budget_remains_charged_and_late_refund_never_underflows_window() {
        let temp = TempDir::new().unwrap();
        let (paths, profile, _, _, claim, _) = recorded_result(&temp, false);
        let ledger = AutomationBudgetLedger::for_paths(&paths, &profile);
        let mut data = ledger.load().unwrap();
        data.events.truncate(1);
        ledger.save(&data).unwrap();
        std::fs::remove_file(ledger.receipt_path(claim.run_id)).unwrap();
        let now = claim.occurrence.at + chrono::Duration::days(31);
        let mut next = next_budget_claim(&claim, now);
        next.definition.rolling_30_day_budget = next.definition.per_run_budget.clone();
        let before = std::fs::read(&ledger.path).unwrap();
        assert!(ledger.reserve(&next, now).is_err());
        assert_eq!(std::fs::read(&ledger.path).unwrap(), before);
        let reserved = data.events[0].amount.clone();
        ledger
            .settle(
                &claim,
                &reserved,
                &AutomationUsageCountersV1::default(),
                true,
                now,
            )
            .unwrap();
        ledger.reserve(&next, now).unwrap();
        assert_eq!(ledger.load().unwrap().events.len(), 1);
        assert!(ledger
            .read_run_receipt(claim.run_id, &claim.automation_id)
            .unwrap()
            .is_some());
    }

    #[test]
    fn budget_rotation_refuses_receipt_failure_without_discarding_legacy_accounting() {
        let temp = TempDir::new().unwrap();
        let (paths, profile, _, _, claim, _) = recorded_result(&temp, false);
        let ledger = AutomationBudgetLedger::for_paths(&paths, &profile);
        let receipt = ledger.receipt_path(claim.run_id);
        std::fs::remove_file(&receipt).unwrap();
        std::fs::create_dir(&receipt).unwrap();
        let before = std::fs::read(&ledger.path).unwrap();
        let now = claim.occurrence.at + chrono::Duration::days(31);
        assert!(ledger
            .reserve(&next_budget_claim(&claim, now), now)
            .is_err());
        assert_eq!(std::fs::read(&ledger.path).unwrap(), before);
    }

    #[test]
    fn high_frequency_live_budget_evidence_reserves_future_refunds_and_fails_closed_at_cap() {
        let temp = TempDir::new().unwrap();
        let (paths, profile, _, _, claim, _) = recorded_result(&temp, false);
        let ledger = AutomationBudgetLedger::for_paths(&paths, &profile);
        let mut data = BudgetLedgerDataV1::default();
        let reserved = AutomationUsageCountersV1::from_budget(&claim.definition.per_run_budget);
        for index in 0..(MAX_STORE_BYTES / MAX_BUDGET_EVENT_STORAGE_BYTES) {
            append_budget_event(
                &mut data,
                &format!("orphan-{index}"),
                Uuid::new_v4(),
                BudgetEventKindV1::Reserve,
                reserved.clone(),
                claim.occurrence.at,
            )
            .unwrap();
        }
        ledger.save(&data).unwrap();
        let before = std::fs::read(&ledger.path).unwrap();
        for days in [0, 31] {
            let now = claim.occurrence.at + chrono::Duration::days(days);
            let error = ledger
                .reserve(&next_budget_claim(&claim, now), now)
                .unwrap_err();
            assert!(error.to_string().contains("capacity"));
            assert_eq!(std::fs::read(&ledger.path).unwrap(), before);
        }
    }

    #[test]
    fn recorded_delivery_prepares_cold_identity_inventory_without_repeating_execution() {
        for heartbeat in [true, false] {
            let temp = TempDir::new().unwrap();
            let (paths, profile, store, sessions, _, checkpoint) =
                recorded_result(&temp, heartbeat);
            let profile_paths = paths.clone().for_profile(&profile);
            std::fs::create_dir_all(&profile_paths.sessions_dir).unwrap();
            let unrelated_path = profile_paths
                .sessions_dir
                .join("cold-unrelated-legacy.json");
            let unrelated = crate::session::SessionV2::new(
                "Unrelated legacy history",
                temp.path().to_string_lossy(),
                "local",
                "fixture",
                (0..18)
                    .map(|_| crate::app::Message {
                        role: crate::app::Role::User,
                        content: "x".repeat(512 * 1024),
                        tool_calls: None,
                        thinking: None,
                        response_items: Vec::new(),
                        timestamp: Utc::now(),
                    })
                    .collect(),
            );
            let before = serde_json::to_vec(&unrelated).unwrap();
            std::fs::write(&unrelated_path, &before).unwrap();
            assert_eq!(
                sessions
                    .committed_binding(checkpoint.intent.target_session_id)
                    .unwrap_err()
                    .kind(),
                std::io::ErrorKind::WouldBlock
            );
            assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 1);
            assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 0);
            let delivered = sessions
                .load(&checkpoint.intent.target_session_id.to_string())
                .unwrap();
            assert_eq!(
                delivered.messages.last().unwrap().content,
                "recorded result"
            );
            assert_eq!(std::fs::read(&unrelated_path).unwrap(), before);
        }
    }

    #[test]
    fn restart_delivers_standalone_result_once_before_or_after_session_commit() {
        for already_delivered in [false, true] {
            let temp = TempDir::new().unwrap();
            let (paths, profile, store, sessions, claim, checkpoint) =
                recorded_result(&temp, false);
            if already_delivered {
                deliver_checkpoint(&sessions, &checkpoint, "recorded result", None).unwrap();
            }
            assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 1);
            assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 0);
            let saved = sessions
                .load(&standalone_session_id(claim.run_id).to_string())
                .unwrap();
            assert_eq!(saved.messages.len(), 2);
            assert_eq!(saved.messages[1].content, "recorded result");
            assert_eq!(sessions.list().unwrap().len(), 1);
            assert_eq!(
                run_history(&paths, &profile, None)
                    .unwrap()
                    .last()
                    .unwrap()
                    .state,
                AutomationRunStateV1::Succeeded
            );
        }
    }

    #[test]
    fn heartbeat_policy_keeps_unchanged_and_duplicate_states_quiet_but_surfaces_attention() {
        let temp = TempDir::new().unwrap();
        let mut definition = definition(temp.path());
        definition.automation_kind = AutomationKindV1::Heartbeat {
            session_id: Uuid::new_v4(),
            workspace_identity_sha256: "0".repeat(64),
        };
        definition.notification_policy = AutomationNotificationPolicy::MeaningfulChanges;
        let raw = r#"{"schema":"ca.heartbeat-report.v1","status":"changed","summary":"Ready at 10:00","state":"ready"}"#;
        let (_, first) = heartbeat_notification(&definition, raw, None);
        let first = first.unwrap();
        assert!(first.publish && first.validate(&definition));
        assert!(
            heartbeat_notification(&definition, &format!("```json\n{raw}\n```"), None)
                .1
                .unwrap()
                .publish
        );
        assert!(
            !heartbeat_notification(
                &definition,
                r#"{"schema":"ca.heartbeat-report.v1","status":"unchanged","summary":""}"#,
                None
            )
            .1
            .unwrap()
            .publish
        );
        let repeated = raw.replace("10:00", "10:01");
        let (_, duplicate) =
            heartbeat_notification(&definition, &repeated, Some(&first.state_sha256));
        assert!(!duplicate.unwrap().publish);
        let unchanged = r#"{"schema":"ca.heartbeat-report.v1","status":"unchanged","summary":"Still ready","state":"ready"}"#;
        assert!(
            !heartbeat_notification(&definition, unchanged, None)
                .1
                .unwrap()
                .publish
        );
        let failed =
            r#"{"schema":"ca.heartbeat-report.v1","status":"failed","summary":"Check failed"}"#;
        assert!(
            heartbeat_notification(&definition, failed, None)
                .1
                .unwrap()
                .publish
        );
        assert!(
            heartbeat_notification(&definition, "bad report", None)
                .1
                .unwrap()
                .publish
        );
        definition.notification_policy = AutomationNotificationPolicy::FailuresOnly;
        assert!(
            !heartbeat_notification(&definition, raw, None)
                .1
                .unwrap()
                .publish
        );
        assert!(
            heartbeat_notification(&definition, failed, None)
                .1
                .unwrap()
                .publish
        );
        definition.notification_policy = AutomationNotificationPolicy::Muted;
        assert!(
            !heartbeat_notification(&definition, failed, None)
                .1
                .unwrap()
                .publish
        );
    }

    #[test]
    fn restart_reuses_frozen_quiet_decision_without_recomputing_changed_baseline() {
        for status in ["unchanged", "changed"] {
            let temp = TempDir::new().unwrap();
            let raw = format!(
                r#"{{"schema":"ca.heartbeat-report.v1","status":"{status}","summary":"Ready","state":"ready"}}"#
            );
            let state = hex_digest(&Sha256::digest(
                format!(
                    "{}\0ready",
                    if status == "changed" {
                        "Changed"
                    } else {
                        "Unchanged"
                    }
                )
                .as_bytes(),
            ));
            let (paths, profile, store, sessions, claim, checkpoint) = recorded_report(
                &temp,
                true,
                Some((
                    &raw,
                    AutomationNotificationPolicy::MeaningfulChanges,
                    Some(&state),
                )),
            );
            assert!(!checkpoint.heartbeat_notification.as_ref().unwrap().publish);
            let mut data = store.load().unwrap();
            data.records[0].last_heartbeat_state_sha256 = None;
            store.save(&mut data).unwrap();
            assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 1);
            assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 0);
            let target = sessions
                .load(&checkpoint.intent.target_session_id.to_string())
                .unwrap();
            assert!(target.messages.is_empty() && target.automation_deliveries.is_empty());
            let last = run_history(&paths, &profile, None).unwrap().pop().unwrap();
            assert!(last.saved_session_id.is_none());
            assert_eq!(last.run_id, claim.run_id);
            assert!(run_directory(&paths, &profile, claim.run_id)
                .join("result.txt")
                .is_file());
        }
    }

    #[test]
    fn restart_after_notification_commit_settles_baseline_without_duplicate_transcript() {
        let temp = TempDir::new().unwrap();
        let raw = r#"{"schema":"ca.heartbeat-report.v1","status":"completed","summary":"Work completed","state":"done"}"#;
        let (paths, profile, store, sessions, claim, checkpoint) = recorded_report(
            &temp,
            true,
            Some((raw, AutomationNotificationPolicy::MeaningfulChanges, None)),
        );
        let (_, content) = read_delivery_checkpoint(&paths, &profile, &claim).unwrap();
        let id = checkpoint.intent.target_session_id;
        let lease = sessions.try_writer_lease(id).unwrap();
        deliver_checkpoint(&sessions, &checkpoint, &content, Some(&lease)).unwrap();
        drop(lease);
        AutomationRunJournal::for_paths(&paths, &profile)
            .append(
                &claim,
                AutomationRunStateV1::Succeeded,
                RunEventUpdate {
                    saved_session_id: Some(id),
                    ..Default::default()
                },
                Utc::now(),
            )
            .unwrap();
        assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 1);
        assert_eq!(
            sessions
                .load(&id.to_string())
                .unwrap()
                .automation_deliveries
                .len(),
            1
        );
        let record = store.records().unwrap().remove(0);
        assert_eq!(
            record.last_heartbeat_state_sha256.as_deref(),
            Some(
                checkpoint
                    .heartbeat_notification
                    .as_ref()
                    .unwrap()
                    .state_sha256
                    .as_str()
            )
        );
        assert!(
            !heartbeat_notification(
                &record.definition,
                raw,
                record.last_heartbeat_state_sha256.as_deref()
            )
            .1
            .unwrap()
            .publish
        );
    }

    #[test]
    fn restart_heartbeat_rebinds_archive_location_and_deduplicates_committed_delivery() {
        for already_delivered in [false, true] {
            let temp = TempDir::new().unwrap();
            let (paths, profile, store, sessions, _, checkpoint) = recorded_result(&temp, true);
            let id = checkpoint.intent.target_session_id;
            if already_delivered {
                let lease = sessions.try_writer_lease(id).unwrap();
                deliver_checkpoint(&sessions, &checkpoint, "recorded result", Some(&lease))
                    .unwrap();
            }
            sessions.archive(&id.to_string(), id).unwrap();
            assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 1);
            let saved = sessions.load_archived(&id.to_string()).unwrap();
            assert_eq!(saved.messages.len(), 2);
            assert_eq!(saved.automation_deliveries.len(), 1);
            assert_eq!(saved.messages[1].content, "recorded result");
        }
    }

    #[test]
    fn delivery_tamper_or_target_drift_preserves_result_and_exact_pending_claim() {
        for result_tamper in [false, true] {
            let temp = TempDir::new().unwrap();
            let (paths, profile, store, sessions, claim, checkpoint) = recorded_result(&temp, true);
            if result_tamper {
                std::fs::write(
                    run_directory(&paths, &profile, claim.run_id).join("result.txt"),
                    "tampered",
                )
                .unwrap();
            } else {
                let mut current = sessions
                    .load(&checkpoint.intent.target_session_id.to_string())
                    .unwrap();
                current.title = "new title".into();
                sessions.save(&mut current).unwrap();
            }
            assert!(recover_active_runs(&paths, &profile, &store).is_err());
            let record = store.records().unwrap().remove(0);
            assert_eq!(record.active_run_id, Some(claim.run_id));
            assert!(record.blocked_reason.is_some());
            assert!(!record.enabled);
            assert!(run_directory(&paths, &profile, claim.run_id)
                .join("delivery.json")
                .exists());
            assert!(sessions
                .automation_snapshot(checkpoint.intent.target_session_id)
                .unwrap()
                .0
                .automation_deliveries
                .is_empty());
        }
    }

    #[test]
    fn interrupted_reservation_or_uncheckpointed_result_never_replays_or_refunds() {
        for boundary in ["reservation", "uncheckpointed_result", "settled_budget"] {
            let temp = TempDir::new().unwrap();
            let (paths, profile, store, _, claim, _) = recorded_result(&temp, false);
            let run_dir = run_directory(&paths, &profile, claim.run_id);
            std::fs::remove_file(run_dir.join("delivery.json")).unwrap();
            let ledger = AutomationBudgetLedger::for_paths(&paths, &profile);
            if boundary != "settled_budget" {
                let mut data = ledger.load().unwrap();
                data.events.truncate(1);
                ledger.save(&data).unwrap();
            }
            if boundary == "reservation" {
                std::fs::remove_file(run_dir.join("result.txt")).unwrap();
            }
            let budget_bytes = std::fs::read(&ledger.path).unwrap();
            let result_bytes = std::fs::read(run_dir.join("result.txt")).ok();
            assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 0);
            assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 0);
            assert_eq!(std::fs::read(&ledger.path).unwrap(), budget_bytes);
            assert_eq!(std::fs::read(run_dir.join("result.txt")).ok(), result_bytes);
            let record = store.records().unwrap().remove(0);
            assert!(record.active_run_id.is_none());
            assert!(!record.enabled);
            assert!(record.blocked_reason.unwrap().contains("not replayed"));
            assert_eq!(
                run_history(&paths, &profile, None)
                    .unwrap()
                    .last()
                    .unwrap()
                    .state,
                AutomationRunStateV1::Interrupted
            );
        }
    }

    #[test]
    fn delivered_standalone_provenance_acknowledges_later_human_edits() {
        let temp = TempDir::new().unwrap();
        let (paths, profile, store, sessions, _, checkpoint) = recorded_result(&temp, false);
        let id = deliver_checkpoint(&sessions, &checkpoint, "recorded result", None)
            .unwrap()
            .unwrap();
        let mut current = sessions.load(&id.to_string()).unwrap();
        current.messages[1].content = "Human edited the recorded answer".into();
        current.title = "Human title".into();
        sessions.save(&mut current).unwrap();
        assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 1);
        assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 0);
        let saved = sessions.load(&id.to_string()).unwrap();
        assert_eq!(saved.messages[1].content, current.messages[1].content);
        assert_eq!(saved.title, current.title);
    }

    #[test]
    fn native_candidate_is_inspectable_before_delivery_and_bound_to_original_claim() {
        let temp = TempDir::new().unwrap();
        let (paths, profile, _, _, claim, _) = recorded_result(&temp, false);
        let root = run_directory(&paths, &profile, claim.run_id);
        std::fs::remove_file(root.join("delivery.json")).unwrap();
        let context = crate::automation_runtime::execution_context(
            &paths,
            &profile,
            &claim.definition,
            claim.run_id,
        )
        .unwrap();
        let native = ca_automation::runtime::RunStore::open(&context).unwrap();
        native.start().unwrap();
        native.prepare_action("target").unwrap();
        native.begin_action("target").unwrap();
        let value = serde_json::to_value(crate::automation_runtime::NativeResult {
            content: "candidate retained for exact integration review".into(),
            usage: Default::default(),
        })
        .unwrap();
        native.confirm_action("target", value.clone()).unwrap();
        native
            .finish(
                ca_automation::runtime::ExecutionState::NeedsAttention,
                Some(value),
                Some("target advanced".into()),
            )
            .unwrap();
        assert_eq!(
            read_recorded_result(&paths, &profile, &claim.automation_id, claim.run_id).unwrap(),
            "candidate retained for exact integration review"
        );
        assert!(
            read_recorded_result(&paths, &profile, "different-automation", claim.run_id).is_err()
        );
        assert!(!root.join("delivery.json").exists());
    }

    #[test]
    fn confirmed_deletion_retains_recorded_receipts_and_refuses_a_live_run() {
        let temp = TempDir::new().unwrap();
        let (paths, profile, store, _, claim, _) = recorded_result(&temp, false);
        let run_dir = run_directory(&paths, &profile, claim.run_id);
        AutomationRunJournal::for_paths(&paths, &profile)
            .append(
                &claim,
                AutomationRunStateV1::Succeeded,
                RunEventUpdate::default(),
                claim.occurrence.at,
            )
            .unwrap();
        let budget_path = AutomationBudgetLedger::for_paths(&paths, &profile).path;
        let budget_bytes = std::fs::read(&budget_path).unwrap();
        let receipts = ["intent.json", "result.txt", "delivery.json"].map(|name| {
            (
                run_dir.join(name),
                std::fs::read(run_dir.join(name)).unwrap(),
            )
        });
        let reviewed = automation_record_digest(&store.records().unwrap()[0]).unwrap();
        let run_lease = run_access(&store.root).try_writer(claim.run_id).unwrap();
        assert!(matches!(
            store.remove_confirmed_snapshot(&claim.automation_id, &reviewed),
            Err(AutomationError::Busy)
        ));
        assert_eq!(store.records().unwrap().len(), 1);
        drop(run_lease);
        store
            .remove_confirmed_snapshot(&claim.automation_id, &reviewed)
            .unwrap();
        assert!(store.records().unwrap().is_empty());
        for (path, bytes) in receipts {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        assert_eq!(std::fs::read(budget_path).unwrap(), budget_bytes);
        assert_eq!(
            run_history(&paths, &profile, Some(&claim.automation_id))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            read_recorded_result(&paths, &profile, &claim.automation_id, claim.run_id).unwrap(),
            "recorded result"
        );
        assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 0);
    }

    #[test]
    fn confirmed_deletion_refuses_reviewed_definition_or_claim_drift() {
        let temp = TempDir::new().unwrap();
        let (paths, profile, store, _, claim, _) = recorded_result(&temp, true);
        let reviewed = automation_record_digest(&store.records().unwrap()[0]).unwrap();
        store
            .block_pending_delivery(&claim, "pending reviewed delivery")
            .unwrap();
        assert!(matches!(
            store.remove_confirmed_snapshot(&claim.automation_id, &reviewed),
            Err(AutomationError::Confirmation(_))
        ));
        let pending = store.records().unwrap()[0].clone();
        let reviewed = automation_record_digest(&pending).unwrap();
        store
            .remove_confirmed_snapshot(&claim.automation_id, &reviewed)
            .unwrap();
        assert!(store.records().unwrap().is_empty());
        assert!(run_directory(&paths, &profile, claim.run_id)
            .join("delivery.json")
            .is_file());
    }

    #[test]
    fn busy_heartbeat_coalesces_pending_occurrences_and_live_run_owner_blocks_recovery() {
        let temp = TempDir::new().unwrap();
        let (paths, profile, store, _, claim, _) = recorded_result(&temp, true);
        let run_lease = run_access(&store.root).try_writer(claim.run_id).unwrap();
        assert_eq!(recover_active_runs(&paths, &profile, &store).unwrap(), 0);
        drop(run_lease);
        let now = claim.occurrence.at + chrono::Duration::hours(3);
        store.defer_busy_heartbeat(&claim, now).unwrap();
        let record = store.records().unwrap().remove(0);
        assert!(record.pending_heartbeat.is_some());
        assert!(record.active_run_id.is_none());
        assert!(record.next_occurrence.as_ref().is_none_or(|o| o.at > now));
        let retry = store.claim_due(profile.label(), now).unwrap().unwrap();
        assert_eq!(retry.run_id, claim.run_id);
        assert!(retry.coalesced_occurrences >= 1);
        store.defer_busy_heartbeat(&retry, now).unwrap();
        drop(retry);
        let record = store.records().unwrap().remove(0);
        assert_eq!(record.pending_heartbeat.unwrap().id, claim.occurrence.id);
    }
}
