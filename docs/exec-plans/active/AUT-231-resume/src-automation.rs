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
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use uuid::Uuid;

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
    /// List automations without exposing prompt bodies
    List,
    /// Inspect one automation definition
    Inspect { id: String },
    /// Inspect local run receipts
    History { id: Option<String> },
    /// Validate stores, routes, prices, and service readiness without mutation
    Doctor,
    /// Normalize a draft and print its exact confirmation digest
    Preview { path: PathBuf },
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
    Pause { id: String },
    Resume { id: String },
    /// Run one occurrence immediately without changing its recurrence
    Run { id: String },
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
        return Err(<RawAutomationArgs as clap::CommandFactory>::command().error(
            clap::error::ErrorKind::ArgumentConflict,
            "--profile bare is non-persistent; automations are unavailable",
        ));
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
    match cli.command {
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
                    println!("Automation {} · {}", record.definition.id, record.definition.name);
                    println!("state: {}", record_state(&record));
                    println!("kind: {}", kind_label(&record.definition.automation_kind));
                    println!("definition_sha256: {}", record.definition_sha256);
                    println!(
                        "route: {}/{}",
                        record.definition.schedule.provider.as_deref().unwrap_or("-"),
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
                    println!(
                        "notifications: {:?}",
                        record.definition.notification_policy
                    );
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
            let history = AutomationRunJournal::for_paths(&paths, &cli.profile).history(id.as_deref())?;
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
                validate_configured_definition(&paths, &cli.profile, &record.definition, Utc::now())?;
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
                    records.len(), classic, run_events
                ),
            }
        }
        AutomationCommand::Run { id } => {
            let claim = store.claim_immediate(cli.profile.label(), &id, Utc::now())?;
            execute_claim(&paths, &cli.profile, &store, claim, cli.output).await?;
        }
        AutomationCommand::Service { command } => {
            let executable = std::env::current_exe()
                .map_err(|error| AutomationError::Io(error.to_string()))?;
            let preview = crate::schedule_v2::launch_agent_preview(&paths, &cli.profile, &executable)
                .map_err(|error| AutomationError::State(error.to_string()))?;
            let (action, confirmation, launchctl) = match command {
                AutomationServiceCommand::Preview => {
                    (crate::schedule_v2::ServiceAction::Preview, None, PathBuf::from("/bin/launchctl"))
                }
                AutomationServiceCommand::Install { confirm, launchctl } => {
                    (crate::schedule_v2::ServiceAction::Install, Some(confirm), launchctl)
                }
                AutomationServiceCommand::Uninstall { confirm, launchctl } => {
                    (crate::schedule_v2::ServiceAction::Uninstall, Some(confirm), launchctl)
                }
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

fn prepare_definition(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    path: &Path,
    now: DateTime<Utc>,
) -> Result<AutomationDefinitionV1, AutomationError> {
    let mut definition = load_definition_file(path)?;
    let profile_paths = paths.clone().for_profile(profile);
    let config = crate::config::ConfigStore::for_profile(profile_paths.config_file, profile.clone())
        .load()
        .map_err(|error| AutomationError::Invalid(error.to_string()))?;
    match (
        definition.schedule.provider.as_deref(),
        definition.schedule.model.as_deref(),
    ) {
        (None, None) => {
            definition.reasoning_effort = None;
            definition.pricing = PricingBindingV1::ModelFree;
        }
        (Some(provider_id), Some(model_id)) => {
            let provider = config.providers.get(provider_id).ok_or_else(|| {
                AutomationError::Invalid("automation provider is not configured".into())
            })?;
            if !provider.models.iter().any(|configured| configured == model_id) {
                return Err(AutomationError::Invalid(
                    "automation model is not configured for the selected provider".into(),
                ));
            }
            let model_profile = config.effective_model_profile(provider_id, model_id);
            definition.reasoning_effort = match (definition.reasoning_effort, model_profile) {
                (Some(effort), Some(model_profile))
                    if model_profile.reasoning_efforts.contains(&effort) => Some(effort),
                (Some(_), _) => {
                    return Err(AutomationError::Invalid(
                        "automation reasoning effort is unsupported by the exact route".into(),
                    ))
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
            ))
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
    definition.normalize(now)
}

fn validate_configured_definition(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    definition: &AutomationDefinitionV1,
    now: DateTime<Utc>,
) -> Result<(), AutomationError> {
    let profile_paths = paths.clone().for_profile(profile);
    let config = crate::config::ConfigStore::for_profile(profile_paths.config_file, profile.clone())
        .load()
        .map_err(|error| AutomationError::State(error.to_string()))?;
    match (
        definition.schedule.provider.as_deref(),
        definition.schedule.model.as_deref(),
        &definition.pricing,
    ) {
        (None, None, PricingBindingV1::ModelFree) => {}
        (Some(provider_id), Some(model_id), pricing) => {
            let provider = config.providers.get(provider_id).ok_or_else(|| {
                AutomationError::State("automation provider is no longer configured".into())
            })?;
            if !provider.models.iter().any(|configured| configured == model_id) {
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
                    ))
                }
            }
        }
        _ => {
            return Err(AutomationError::State(
                "automation route contract is invalid".into(),
            ))
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
    let bytes = serde_json::to_vec(&roots)
        .map_err(|error| AutomationError::State(error.to_string()))?;
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
    chars
        .next()
        .map_or_else(String::new, |first| first.to_uppercase().collect::<String>() + chars.as_str())
}

async fn execute_claim(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    store: &AutomationStore,
    claim: AutomationClaimV1,
    output: AutomationOutputFormat,
) -> Result<(), AutomationError> {
    let journal = AutomationRunJournal::for_paths(paths, profile);
    journal.append(
        &claim,
        AutomationRunStateV1::Claimed,
        RunEventUpdate::default(),
        Utc::now(),
    )?;
    if let Err(error) = validate_configured_definition(paths, profile, &claim.definition, Utc::now()) {
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

    let profile_paths = paths.clone().for_profile(profile);
    let sessions = crate::session::SessionStore::with_archive(
        profile_paths.sessions_dir,
        profile_paths.archived_sessions_dir,
    );
    let (history, heartbeat_binding) = match &claim.definition.automation_kind {
        AutomationKindV1::Heartbeat { session_id, .. } => {
            let (session, binding) = sessions
                .automation_snapshot(*session_id)
                .map_err(|_| AutomationError::State("heartbeat session is unavailable".into()))?;
            (bounded_heartbeat_history(session.messages)?, Some(binding))
        }
        AutomationKindV1::Standalone => (Vec::new(), None),
    };

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
    store_private_result(paths, profile, &claim, &execution.content)?;
    journal.append(
        &claim,
        AutomationRunStateV1::ResultReady,
        RunEventUpdate {
            reserved: Some(reserved.clone()),
            actual: Some(if authoritative { actual.clone() } else { reserved.clone() }),
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

    let provenance = crate::session::AutomationSessionProvenanceV1 {
        schema: AUTOMATION_DELIVERY_SCHEMA.into(),
        automation_id: claim.automation_id.clone(),
        run_id: claim.run_id,
        occurrence_id: claim.occurrence.id.clone(),
        kind: kind_label(&claim.definition.automation_kind).into(),
    };
    let saved_session_id = match (&claim.definition.automation_kind, heartbeat_binding) {
        (AutomationKindV1::Heartbeat { session_id, .. }, Some(binding)) => {
            let prompt = match &claim.definition.schedule.target {
                ScheduleTargetV2::Prompt { prompt } => prompt,
                _ => unreachable!("heartbeat normalization requires prompt target"),
            };
            if let Err(error) = sessions.append_automation_delivery(
                &binding,
                provenance,
                &claim.definition.name,
                prompt,
                &execution.content,
            ) {
                store.block(
                    &claim.automation_id,
                    "heartbeat result is pending because the saved session drifted",
                )?;
                journal.append(
                    &claim,
                    AutomationRunStateV1::Interrupted,
                    RunEventUpdate {
                        reserved: Some(reserved),
                        actual: Some(if authoritative { actual } else { AutomationUsageCountersV1::from_budget(&claim.definition.per_run_budget) }),
                        refunded: Some(refunded),
                        reason_class: Some("session_delivery_drift".into()),
                        artifact_sha256: execution.artifact_digest,
                        ..Default::default()
                    },
                    Utc::now(),
                )?;
                return Err(AutomationError::State(format!(
                    "heartbeat result retained pending delivery: {error}"
                )));
            }
            Some(*session_id)
        }
        (AutomationKindV1::Standalone, None) => Some(save_standalone_session(
            &sessions,
            &claim,
            provenance,
            &execution.content,
        )?),
        _ => {
            return Err(AutomationError::State(
                "automation delivery binding is inconsistent".into(),
            ))
        }
    };
    journal.append(
        &claim,
        AutomationRunStateV1::Succeeded,
        RunEventUpdate {
            reserved: Some(reserved),
            actual: Some(if authoritative { actual } else { AutomationUsageCountersV1::from_budget(&claim.definition.per_run_budget) }),
            refunded: Some(refunded),
            saved_session_id,
            artifact_sha256: execution.artifact_digest,
            ..Default::default()
        },
        Utc::now(),
    )?;
    store.settle(&claim)?;
    match output {
        AutomationOutputFormat::Json => write_json(&serde_json::json!({
            "schema": AUTOMATION_RUN_SCHEMA,
            "automation_id": claim.automation_id,
            "run_id": claim.run_id,
            "outcome": "succeeded",
            "saved_session_id": saved_session_id,
        }))?,
        AutomationOutputFormat::Text => println!(
            "Automation {} completed · run {}{}",
            claim.automation_id,
            claim.run_id,
            saved_session_id.map_or_else(String::new, |id| format!(" · task {id}"))
        ),
    }
    Ok(())
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

fn execution_usage(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    definition: &AutomationDefinitionV1,
    execution: &crate::schedule::AutomationExecution,
    started: DateTime<Utc>,
    completed: DateTime<Utc>,
) -> Result<(AutomationUsageCountersV1, bool), AutomationError> {
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
            .ok_or_else(|| AutomationError::State("automation input-token accounting overflow".into()))?;
        counters.output_tokens = counters
            .output_tokens
            .checked_add(u64::from(usage.completion_tokens))
            .ok_or_else(|| AutomationError::State("automation output-token accounting overflow".into()))?;
    }
    let model_free = matches!(definition.pricing, PricingBindingV1::ModelFree);
    let mut authoritative = model_free || !execution.usages.is_empty();
    match &definition.pricing {
        PricingBindingV1::Metered { price_card_sha256 } => {
            let provider = definition.schedule.provider.as_deref().expect("metered provider");
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
    let now = Utc::now();
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
        format!("Automation · {} · {}", claim.definition.name, now.format("%Y-%m-%d %H:%M")),
        claim.definition.schedule.workspace.to_string_lossy(),
        claim.definition.schedule.provider.clone().unwrap_or_default(),
        claim.definition.schedule.model.clone().unwrap_or_default(),
        messages,
    );
    session.workspace_roots = vec![claim.definition.schedule.workspace.to_string_lossy().into_owned()];
    session.reasoning_effort = claim.definition.reasoning_effort;
    session.automation_provenance = Some(provenance);
    let id = session.id;
    sessions
        .save(&mut session)
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
    Metered { price_card_sha256: String },
    LocalZero,
    ModelFree,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutomationNotificationPolicy {
    #[default]
    AllResults,
    FailuresOnly,
    Muted,
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
                    || !matches!(self.schedule.target, ScheduleTargetV2::Prompt { .. })
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
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| AutomationError::Io(error.to_string()))?;
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
    pub claim_revision: Option<u64>,
    #[serde(default)]
    pub coalesced_occurrences: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
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
            if definition_digest(&record.definition)? != record.definition_sha256 {
                return Err(AutomationError::State(format!(
                    "definition digest mismatch for {}",
                    record.definition.id
                )));
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
            return Err(AutomationError::State("automation store is oversized".into()));
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
        if record.active_run_id.is_some() {
            return Err(AutomationError::State(
                "automation has an active run".into(),
            ));
        }
        record.enabled = enabled;
        record.blocked_reason = None;
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
        let _lock = OwnerLock::acquire(self.root.join("automations.lock"))?;
        let mut data = self.load()?;
        let index = data
            .records
            .iter()
            .position(|record| record.definition.id == id)
            .ok_or_else(|| AutomationError::Missing(id.into()))?;
        if data.records[index].active_run_id.is_some() {
            return Err(AutomationError::State(
                "cannot delete an automation with an active run".into(),
            ));
        }
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
                        && record
                            .next_occurrence
                            .as_ref()
                            .is_some_and(|occurrence| occurrence.at <= now)
                })
                .min_by_key(|(_, record)| {
                    (
                        record.next_occurrence.as_ref().map(|item| item.at),
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
        };
        data.records[index].active_run_id = Some(run_id);
        data.records[index].claim_revision = Some(claim_revision);
        data.records[index].coalesced_occurrences = 0;
        if immediate.is_none() {
            data.records[index].next_occurrence =
                next_occurrence(&data.records[index].definition.schedule.trigger, occurrence.at)
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
        record.claim_revision = None;
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
    fn from_budget(value: &AutomationBudgetV1) -> Self {
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
            provider_requests: self.provider_requests.checked_add(other.provider_requests)?,
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
            provider_requests: self.provider_requests.checked_sub(other.provider_requests)?,
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
            root: paths
                .profile_data_dir(profile)
                .join("automations/v1/runs"),
        }
    }

    fn history(&self, automation_id: Option<&str>) -> Result<Vec<AutomationRunEventV1>, AutomationError> {
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
                let event: AutomationRunEventV1 = serde_json::from_slice(&read_owner_file(&path, 64 * 1024)?)
                    .map_err(|error| AutomationError::State(error.to_string()))?;
                verify_run_event(&event)?;
                run_events.push(event);
            }
            run_events.sort_by_key(|event| event.sequence);
            let mut previous = "0".repeat(64);
            for event in &run_events {
                if event.previous_sha256 != previous {
                    return Err(AutomationError::State("automation run hash chain is invalid".into()));
                }
                previous.clone_from(&event.event_sha256);
            }
            events.extend(run_events.into_iter().filter(|event| {
                automation_id.is_none_or(|id| event.automation_id == id)
            }));
        }
        events.sort_by(|left, right| left.at.cmp(&right.at).then_with(|| left.run_id.cmp(&right.run_id)).then_with(|| left.sequence.cmp(&right.sequence)));
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
    let bytes = serde_json::to_vec(&value)
        .map_err(|error| AutomationError::State(error.to_string()))?;
    Ok(hex_digest(&Sha256::digest(bytes)))
}

fn verify_run_event(event: &AutomationRunEventV1) -> Result<(), AutomationError> {
    if event.schema != AUTOMATION_RUN_SCHEMA
        || !is_sha256(&event.previous_sha256)
        || !is_sha256(&event.event_sha256)
        || run_event_digest(event)? != event.event_sha256
    {
        return Err(AutomationError::State("automation run receipt is invalid".into()));
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
        let data: BudgetLedgerDataV1 = serde_json::from_slice(&read_owner_file(&self.path, MAX_STORE_BYTES)?)
            .map_err(|error| AutomationError::State(error.to_string()))?;
        if data.schema != BUDGET_LEDGER_SCHEMA || data.events.len() > 16_384 {
            return Err(AutomationError::State("automation budget ledger is invalid or oversized".into()));
        }
        let mut previous = "0".repeat(64);
        for (index, event) in data.events.iter().enumerate() {
            if event.sequence != index as u64
                || event.previous_sha256 != previous
                || event.event_sha256 != budget_event_digest(event)?
            {
                return Err(AutomationError::State("automation budget hash chain is invalid".into()));
            }
            previous.clone_from(&event.event_sha256);
        }
        Ok(data)
    }

    fn reserve(
        &self,
        claim: &AutomationClaimV1,
        now: DateTime<Utc>,
    ) -> Result<AutomationUsageCountersV1, AutomationError> {
        let _lock = OwnerLock::acquire(self.root.join("budget.lock"))?;
        let mut data = self.load()?;
        if data.events.iter().any(|event| {
            event.run_id == claim.run_id && event.kind == BudgetEventKindV1::Reserve
        }) {
            return Err(AutomationError::State("automation run budget was already reserved".into()));
        }
        let mut reservation = AutomationUsageCountersV1::from_budget(&claim.definition.per_run_budget);
        if !matches!(claim.definition.pricing, PricingBindingV1::Metered { .. }) {
            reservation.cost_usd = DecimalUsd::ZERO;
        }
        let cutoff = now - chrono::Duration::days(30);
        let mut rolling = AutomationUsageCountersV1::default();
        for event in data.events.iter().filter(|event| {
            event.automation_id == claim.automation_id && event.at >= cutoff
        }) {
            rolling = match event.kind {
                BudgetEventKindV1::Reserve => rolling.checked_add(&event.amount),
                BudgetEventKindV1::Refund => rolling.checked_sub(&event.amount),
            }
            .ok_or_else(|| AutomationError::State("automation rolling budget arithmetic failed".into()))?;
        }
        let projected = rolling
            .checked_add(&reservation)
            .ok_or_else(|| AutomationError::State("automation rolling budget overflow".into()))?;
        if !projected.within(&claim.definition.rolling_30_day_budget) {
            return Err(AutomationError::State("automation rolling 30-day budget is exhausted".into()));
        }
        append_budget_event(
            &mut data,
            &claim.automation_id,
            claim.run_id,
            BudgetEventKindV1::Reserve,
            reservation.clone(),
            now,
        )?;
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
            return Err(AutomationError::State("automation exceeded its per-run budget".into()));
        }
        let charged = if usage_authoritative {
            actual.clone()
        } else {
            reserved.clone()
        };
        let refund = reserved
            .checked_sub(&charged)
            .ok_or_else(|| AutomationError::State("automation budget settlement exceeded its reservation".into()))?;
        let _lock = OwnerLock::acquire(self.root.join("budget.lock"))?;
        let mut data = self.load()?;
        let reserve_count = data.events.iter().filter(|event| {
            event.run_id == claim.run_id && event.kind == BudgetEventKindV1::Reserve
        }).count();
        let refund_exists = data.events.iter().any(|event| {
            event.run_id == claim.run_id && event.kind == BudgetEventKindV1::Refund
        });
        if reserve_count != 1 || refund_exists {
            return Err(AutomationError::State("automation budget settlement state is invalid".into()));
        }
        append_budget_event(
            &mut data,
            &claim.automation_id,
            claim.run_id,
            BudgetEventKindV1::Refund,
            refund.clone(),
            now,
        )?;
        self.save(&data)?;
        Ok(refund)
    }

    fn save(&self, data: &BudgetLedgerDataV1) -> Result<(), AutomationError> {
        let bytes = serde_json::to_vec_pretty(data)
            .map_err(|error| AutomationError::State(error.to_string()))?;
        if bytes.len() as u64 > MAX_STORE_BYTES {
            return Err(AutomationError::State("automation budget ledger is oversized".into()));
        }
        atomic_owner_write(&self.root, &self.path, &bytes, true)
    }
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
    data.events.push(event);
    Ok(())
}

fn budget_event_digest(event: &BudgetEventV1) -> Result<String, AutomationError> {
    let mut value = event.clone();
    value.event_sha256.clear();
    let bytes = serde_json::to_vec(&value)
        .map_err(|error| AutomationError::State(error.to_string()))?;
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
            .fold(0_u8, |difference, (left, right)| difference | (left ^ right))
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
    path: PathBuf,
    token: Uuid,
}

#[derive(Serialize, Deserialize)]
struct OwnerLockReceipt {
    pid: u32,
    token: Uuid,
}

impl OwnerLock {
    fn acquire(path: PathBuf) -> Result<Self, AutomationError> {
        let parent = path
            .parent()
            .ok_or_else(|| AutomationError::State("lock has no parent".into()))?;
        ensure_private_dir(parent)?;
        let token = Uuid::new_v4();
        let bytes = serde_json::to_vec(&OwnerLockReceipt {
            pid: std::process::id(),
            token,
        })
        .map_err(|error| AutomationError::State(error.to_string()))?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                AutomationError::Busy
            } else {
                AutomationError::Io(error.to_string())
            }
        })?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(Self { path, token })
    }
}

impl Drop for OwnerLock {
    fn drop(&mut self) {
        let owned = read_owner_file(&self.path, 4096)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<OwnerLockReceipt>(&bytes).ok())
            .is_some_and(|receipt| receipt.token == self.token && receipt.pid == std::process::id());
        if owned {
            let _ = std::fs::remove_file(&self.path);
            if let Some(parent) = self.path.parent() {
                let _ = File::open(parent).and_then(|directory| directory.sync_all());
            }
        }
    }
}

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
}
