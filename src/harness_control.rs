//! Readable controls for durable host-owned harness experiment records.
use codex_team::self_improvement::{
    list_harness_cycles, load_harness_cycle, pause_harness_cycle, resume_harness_cycle,
    LearningVault,
};
use std::ffi::OsString;
use std::path::Path;

#[derive(Debug, Clone, thiserror::Error)]
#[error("Automatic experiments blocked: verified pricing is unavailable for {provider}/{model}; update the verified pricing catalog or configured experiment route. Ordinary chat remains available")]
pub(crate) struct PricingAdmissionBlocked {
    pub provider: String,
    pub model: String,
    pub profile: String,
    pub revision: String,
}

/// A settled pre-decision budget stop cannot deploy anything. Keep its evidence
/// and budget receipts, but allow a later independent cycle after the cooldown.
/// A decision-bearing budget stop may have interrupted promotion; it must remain
/// blocking until the existing recovery path finishes.
pub(crate) fn blocks_new_cycle(cycle: &codex_team::self_improvement::HarnessCycle) -> bool {
    use codex_team::self_improvement::HarnessStage;
    if cycle.stage == HarnessStage::Rejected && cycle.abandonment.is_some() {
        return false; // Closed evidence retains unknown reservations; never resumed.
    }
    if !cycle.reservations.is_empty() || !cycle.active_reservations.is_empty() {
        return true;
    }
    match cycle.stage {
        HarnessStage::Rejected | HarnessStage::Promoted | HarnessStage::RolledBack => false,
        HarnessStage::BudgetExhausted => cycle.decision.is_some(),
        _ => true,
    }
}

pub fn run(args: &[OsString], data_dir: &Path) -> Option<Result<(), String>> {
    let verb = args.first()?.to_str()?;
    if !matches!(
        verb,
        "status" | "candidates" | "show" | "pause" | "resume" | "feedback" | "abandon"
    ) {
        return None;
    }
    Some(run_inner(args, data_dir).map_err(|e| e.to_string()))
}

fn run_inner(args: &[OsString], data_dir: &Path) -> anyhow::Result<()> {
    let verb = args[0].to_str().unwrap();
    let json = args.iter().any(|v| v == "--json");
    let positional: Vec<_> = args
        .iter()
        .skip(1)
        .filter(|v| *v != "--json")
        .map(|v| {
            v.to_str()
                .ok_or_else(|| anyhow::anyhow!("arguments must be UTF-8"))
        })
        .collect::<anyhow::Result<_>>()?;
    if verb == "feedback" {
        if positional.len() != 2 {
            anyhow::bail!("usage: ca harness feedback TASK_ID accepted|rejected|unknown [--json]");
        }
        let feedback = match positional[1] {
            "accepted" => crate::session_outcome::UserFeedback::Accepted,
            "rejected" => crate::session_outcome::UserFeedback::Rejected,
            "unknown" => crate::session_outcome::UserFeedback::Unknown,
            _ => anyhow::bail!("feedback must be accepted, rejected or unknown"),
        };
        crate::session_outcome::set_feedback(positional[0], feedback, data_dir)?;
        if json {
            println!(
                "{}",
                serde_json::json!({"task_id":positional[0],"feedback":feedback})
            );
        } else {
            println!("Feedback recorded for {}: {}", positional[0], positional[1]);
        }
        return Ok(());
    }
    let vault = LearningVault::existing_machine()?;
    if matches!(verb, "status" | "candidates") {
        if !positional.is_empty() {
            anyhow::bail!("usage: ca harness {verb} [--json]");
        }
        let cycles = match vault {
            Some(ref v) => list_harness_cycles(v)?,
            None => Vec::new(),
        };
        if json {
            println!(
                "{}",
                serde_json::json!({"schema":"ca.harness_status.v1","cycles":cycles})
            );
        } else if cycles.is_empty() {
            println!("No harness experiments recorded.");
        } else {
            for cycle in cycles {
                println!(
                    "{}  {:?}  ${:.6} accounted  {}s active",
                    cycle.id,
                    cycle.stage,
                    cycle.settled_microusd as f64 / 1_000_000.0,
                    cycle.active_seconds
                );
                if cycle.abandonment.is_some() {
                    println!("  Abandoned; unknown charges and durations remain reserved.");
                }
            }
        }
        return Ok(());
    }
    if positional.len() != 1 {
        anyhow::bail!("usage: ca harness {verb} CYCLE_ID [--json]");
    }
    let vault = vault.ok_or_else(|| anyhow::anyhow!("no harness experiment vault"))?;
    let id = positional[0];
    match verb {
        "pause" => pause_harness_cycle(&vault, id)?,
        "resume" => resume_harness_cycle(&vault, id)?,
        "abandon" => {
            crate::harness_recovery::abandon(&vault, id)?;
        }
        "show" => {}
        _ => unreachable!(),
    }
    let (plan, cycle) = load_harness_cycle(&vault, id)?;
    let generation = crate::harness_generation::report(&vault, id)?;
    let engineering = crate::harness_engineering::report(&vault, id)?;
    let source_generation = crate::harness_source_generation::report(&vault, id)?;
    let source_checkout = crate::harness_source_checkout::report(&vault, id)?;
    let source_build = crate::harness_source_build::report(&vault, id)?;
    let activation = crate::harness_activation::report(&vault, id)?;
    let promotion = crate::harness_promotion::report(&vault, id)?;
    let guidance_activation = crate::harness_guidance::report(&vault, id)?;
    if json {
        println!(
            "{}",
            serde_json::json!({"plan":plan,"cycle":cycle,"generation":generation,"source_generation":source_generation,"source_checkout":source_checkout,"source_build":source_build,"engineering":engineering,"activation":activation,"promotion":promotion,"guidance_activation":guidance_activation})
        );
    } else {
        println!("Experiment {}: {:?}\nPlan: {}\nTrials: {}/{}\nAccounted: ${:.6}\nReserved: ${:.6}\nActive: {} / {} seconds\n{}",
            id,cycle.stage,plan.digest,cycle.trials.len(),plan.cases.len()*usize::from(plan.repetitions)*2,
            cycle.settled_microusd as f64/1_000_000.0,
            cycle.reservations.values().map(|v| u128::from(*v)).sum::<u128>() as f64/1_000_000.0,
            cycle.active_seconds,plan.settings.maximum_active_seconds,cycle.reason);
        println!("Accounting uses the frozen price basis, including conservative upper bounds where specified; it is not an invoice.");
        if cycle.abandonment.is_some() {
            println!("Abandoned without replay or refund. Reserved charges and durations remain unknown; this cycle cannot resume.");
        }
        if let Some(report) = engineering.as_ref().or(source_build.as_ref()) {
            let gates = report.get("gates").and_then(|v| v.as_object());
            let passed = gates.map_or(0, |gates| {
                gates
                    .values()
                    .filter(|gate| gate.get("passed").and_then(|v| v.as_bool()) == Some(true))
                    .count()
            });
            println!(
                "Engineering checks: {passed}/{} passed",
                report["required_gates"]
            );
        }
        if let Some(proposal) = generation
            .as_ref()
            .or(source_generation.as_ref())
            .and_then(|v| v.get("proposal"))
        {
            if let Some(prediction) = proposal.get("prediction").and_then(|v| v.as_str()) {
                println!("Prediction: {prediction}");
            }
        }
    }
    Ok(())
}

struct EvaluationArgs {
    id: String,
    repository: std::path::PathBuf,
    binary: std::path::PathBuf,
    json: bool,
    quiet: bool,
}
impl EvaluationArgs {
    fn parse(args: &[OsString]) -> anyhow::Result<Self> {
        let usage =
            "usage: ca harness run CYCLE_ID --repository PATH --baseline-binary PATH [--json]";
        if args.first().and_then(|s| s.to_str()) != Some("run") {
            anyhow::bail!("{usage}");
        }
        let id = args
            .get(1)
            .and_then(|s| s.to_str())
            .filter(|s| !s.starts_with('-'))
            .ok_or_else(|| anyhow::anyhow!("{usage}"))?
            .to_owned();
        let mut repository = None;
        let mut binary = None;
        let mut json = false;
        let mut index = 2;
        while index < args.len() {
            match args[index].to_str() {
                Some("--json") if !json => {
                    json = true;
                    index += 1;
                }
                Some(flag @ ("--repository" | "--baseline-binary")) => {
                    let value = args
                        .get(index + 1)
                        .filter(|v| !v.to_string_lossy().starts_with('-'))
                        .ok_or_else(|| anyhow::anyhow!("{usage}"))?;
                    let slot = if flag == "--repository" {
                        &mut repository
                    } else {
                        &mut binary
                    };
                    if slot.replace(std::path::PathBuf::from(value)).is_some() {
                        anyhow::bail!("duplicate {flag}");
                    }
                    index += 2;
                }
                _ => anyhow::bail!("{usage}"),
            }
        }
        Ok(Self {
            id,
            repository: repository.ok_or_else(|| anyhow::anyhow!("{usage}"))?,
            binary: binary.ok_or_else(|| anyhow::anyhow!("{usage}"))?,
            json,
            quiet: false,
        })
    }
}

/// Resume evaluation of an already generated, sealed guidance candidate. New
/// cycle creation belongs to the evidence freezer, not arbitrary CLI JSON.
pub async fn run_evaluation(args: &[OsString]) -> anyhow::Result<()> {
    let args = EvaluationArgs::parse(args)?;
    let vault = LearningVault::existing_machine()?
        .ok_or_else(|| anyhow::anyhow!("no harness experiment vault"))?;
    Box::pin(run_evaluation_inner(vault, args, None, None)).await
}

pub(crate) async fn run_background(
    vault: LearningVault,
    id: String,
    repository: std::path::PathBuf,
    binary: std::path::PathBuf,
    config: crate::config::CaConfig,
    controller: Option<codex_team::self_improvement::HarnessControllerLock>,
) -> anyhow::Result<()> {
    Box::pin(run_evaluation_inner(
        vault,
        EvaluationArgs {
            id,
            repository,
            binary,
            json: false,
            quiet: true,
        },
        Some(config),
        controller,
    ))
    .await
}

async fn run_evaluation_inner(
    vault: LearningVault,
    mut args: EvaluationArgs,
    configured: Option<crate::config::CaConfig>,
    controller: Option<codex_team::self_improvement::HarnessControllerLock>,
) -> anyhow::Result<()> {
    crate::harness_idle::boundary(&vault, &args.id).await?;
    let _resources = crate::harness_automation::compatibility_resources(&vault, &args.repository)?;
    let repository_lease = crate::harness_automation::compatibility_repository_lease(
        &_resources,
        &vault,
        &args.repository,
    )?;
    let _controller = match controller {
        Some(lock) if lock.matches(&vault) => lock,
        Some(_) => anyhow::bail!("harness controller belongs to another vault"),
        None => vault.acquire_harness_controller_lock()?,
    };
    let (plan, cycle) = load_harness_cycle(&vault, &args.id)?;
    // Evaluation is provider-free once a decision is durable. Reconcile only
    // activations that already crossed their publication boundary.
    if plan.source_files.is_empty() && crate::harness_guidance::recovery_started(&vault, &args.id)?
    {
        let activation = crate::harness_guidance::promote(
            vault.clone(),
            &args.id,
            &args.repository,
            &args.binary,
        )
        .await?;
        if !args.quiet {
            if args.json {
                println!("{}", serde_json::to_string(&activation)?);
            } else {
                println!("Guidance {}: {:?}", args.id, activation.phase);
            }
        }
        return Ok(());
    }
    if crate::harness_promotion::report(&vault, &args.id)?.is_some_and(|journal| {
        matches!(
            journal.phase,
            crate::harness_promotion::Phase::SourceAdopting
                | crate::harness_promotion::Phase::SourceAdopted
                | crate::harness_promotion::Phase::Replacing
                | crate::harness_promotion::Phase::Active
        )
    }) {
        let promotion = crate::harness_promotion::promote_owned(
            vault.clone(),
            &args.id,
            &args.repository,
            repository_lease,
        )
        .await?;
        if !args.quiet {
            print_promotion(&promotion, args.json)?;
        }
        return Ok(());
    }
    if cycle.stage == codex_team::self_improvement::HarnessStage::ReadyToPromote {
        if !args.quiet {
            if args.json {
                println!(
                    "{}",
                    serde_json::json!({"cycle":cycle,"stop":"needs_review","activated":false})
                );
            } else {
                println!("Experiment {}: Needs review; confirm the exact candidate and evaluation before promotion", args.id);
            }
        }
        return Ok(());
    }
    crate::harness_execution::require_native_baseline(&args.binary).await?;
    crate::harness_vm::validate_backend(&plan).await?;
    if !matches!(
        cycle.stage,
        codex_team::self_improvement::HarnessStage::EvaluationFrozen
            | codex_team::self_improvement::HarnessStage::CandidateReady
            | codex_team::self_improvement::HarnessStage::Evaluating
    ) {
        anyhow::bail!(
            "cycle is {:?}; run does not resume paused or terminal cycles",
            cycle.stage
        );
    }
    args.repository = crate::harness_snapshot::resolve(&vault, &plan, &args.repository)?;
    let source_cycle = !plan.source_files.is_empty();
    let source_paths = plan.source_files.keys().cloned().collect::<Vec<_>>();
    let source_generation = crate::harness_source_generation::load(&vault, &args.id)?;
    let mut generate = !source_cycle
        && cycle.stage == codex_team::self_improvement::HarnessStage::EvaluationFrozen;
    if generate
        && crate::harness_generation::report(&vault, &args.id)?
            .is_some_and(|report| !report["proposal"].is_null())
    {
        crate::harness_generation::recover_candidate(&vault, &args.id)?;
        generate = false;
    }
    let inputs = if let Some(saved) = &source_generation {
        if saved.proposal.is_none() {
            codex_team::self_improvement::update_harness_cycle(&vault, &args.id, |state| {
                state.stage = codex_team::self_improvement::HarnessStage::RecoveryRequired;
                state.reason = "Interrupted source generation; no automatic replay".into();
                Ok(())
            })?;
            anyhow::bail!("source generation is incomplete; recovery required");
        }
        saved.inputs.shared.clone()
    } else if generate || source_cycle {
        crate::harness_freeze::load_inputs(&vault, &args.id)?
    } else {
        crate::harness_generation::recover_candidate(&vault, &args.id)?;
        crate::harness_generation::load_candidate(&vault, &args.id)?.0
    };
    let binary = args.binary.clone();
    let repository = args.repository.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        if crate::harness_engineering::artifact_digest(&binary)? != plan.baseline_binary_digest {
            anyhow::bail!("baseline executable changed before generation/evaluation");
        }
        if (generate || source_cycle)
            && codex_team::self_improvement::observe_clean_repository(&repository)? != plan.baseline
        {
            anyhow::bail!("baseline source changed before generation");
        }
        Ok(())
    })
    .await??;
    let config = if let Some(config) = configured {
        config
    } else {
        let paths = crate::runtime::RuntimePaths::discover()?;
        tokio::task::spawn_blocking(move || {
            crate::config::ConfigStore::for_read_only_load(
                paths.config_file,
                crate::runtime::ActiveProfile::Default,
            )
            .load()
        })
        .await?
        .map_err(|_| anyhow::anyhow!("could not load configured evaluation route"))?
    };
    let provider_id = &inputs.provider.source.provider;
    let provider = config
        .providers
        .get(provider_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("frozen provider is no longer configured"))?;
    let provider = crate::provider_connect::resolve_runtime_provider(provider_id, provider)
        .await
        .map_err(|_| anyhow::anyhow!("evaluation provider credentials are unavailable"))?;
    let client = crate::agent::AgentClient::with_call_policy(
        provider_id.clone(),
        &provider,
        &inputs.provider.model,
        crate::agent::client::ProviderCallPolicy::one_attempt(),
        crate::request::RequestStore::default(),
    );
    if source_cycle && source_generation.is_none() {
        crate::harness_idle::boundary(&vault, &args.id).await?;
        let frozen = crate::harness_freeze::load_source_inputs(&vault, &args.id)?
            .ok_or_else(|| anyhow::anyhow!("source inputs missing"))?;
        let inspected = crate::harness_source_generation::freeze_inputs(
            vault.clone(),
            &args.id,
            &args.repository,
            inputs.clone(),
            source_paths,
        )
        .await?;
        if codex_team::self_improvement::canonical_digest(&frozen)?
            != codex_team::self_improvement::canonical_digest(&inspected)?
        {
            anyhow::bail!("selected source differs from frozen creation inputs");
        }
        crate::harness_idle::boundary(&vault, &args.id).await?;
        crate::harness_source_generation::generate(vault.clone(), &args.id, client.clone(), frozen)
            .await?;
    }
    if source_cycle && cycle.stage == codex_team::self_improvement::HarnessStage::EvaluationFrozen {
        crate::harness_idle::boundary(&vault, &args.id).await?;
        crate::harness_source_checkout::prepare(vault.clone(), &args.id, &args.repository).await?;
        crate::harness_source_build::build_and_admit(vault.clone(), &args.id).await?;
    }
    if generate {
        crate::harness_idle::boundary(&vault, &args.id).await?;
        crate::harness_generation::generate_guidance(
            vault.clone(),
            &args.id,
            client.clone(),
            inputs,
        )
        .await?;
    }
    let result = crate::harness_controller::evaluate_saved_candidate(
        vault,
        &args.id,
        client,
        &args.binary,
        &args.repository,
    )
    .await?;
    if args.quiet {
        return Ok(());
    }
    if args.json {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        println!(
            "Experiment {}: {:?}; {} trials recorded; {:?}",
            args.id,
            result.cycle.stage,
            result.cycle.trials.len(),
            result.stop
        );
    }
    Ok(())
}

fn print_promotion(
    promotion: &crate::harness_promotion::Promotion,
    json: bool,
) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string(promotion)?);
    } else {
        println!(
            "Harness transition: {:?}; executable: {}",
            promotion.phase,
            promotion.artifacts.destination.display()
        );
    }
    Ok(())
}

#[derive(clap::Parser)]
#[command(
    name = "ca harness review",
    about = "Preview the exact candidate, frozen evaluation and current baseline for promotion"
)]
struct ReviewArgs {
    id: String,
    #[arg(long)]
    repository: std::path::PathBuf,
    #[arg(long)]
    baseline_binary: std::path::PathBuf,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    confirm: Option<String>,
    #[arg(long)]
    vault: Option<std::path::PathBuf>,
}

pub async fn review(args: &[OsString]) -> anyhow::Result<()> {
    promotion_review(args, false).await
}
pub async fn promote(args: &[OsString]) -> anyhow::Result<()> {
    promotion_review(args, true).await
}

pub async fn migrate_vault(args: &[OsString]) -> anyhow::Result<()> {
    #[derive(clap::Parser)]
    #[command(
        name = "ca harness migrate",
        about = "Preview or confirm a recoverable vault copy while retaining pending legacy authority"
    )]
    struct Args {
        #[arg(long)]
        source: std::path::PathBuf,
        #[arg(long)]
        destination: std::path::PathBuf,
        #[arg(long)]
        confirm: Option<String>,
        #[arg(long)]
        json: bool,
    }
    use clap::Parser;
    let args = match Args::try_parse_from(args) {
        Ok(args) => args,
        Err(error) if error.kind() == clap::error::ErrorKind::DisplayHelp => {
            error.print()?;
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let json = args.json;
    let confirmed = args.confirm.is_some();
    let preview = tokio::task::spawn_blocking(move || match args.confirm {
        Some(digest) => crate::harness_automation::apply_vault_migration(
            &args.source,
            &args.destination,
            &digest,
        ),
        None => crate::harness_automation::preview_vault_migration(&args.source, &args.destination),
    })
    .await??;
    if json {
        println!("{}", serde_json::to_string(&preview)?);
    } else {
        println!("Harness vault {}: {} files; {} pending cycles; original retained\nSource: {}\nDestination: {}\nConfirmation: {}", if confirmed {"copied and validated"} else {"migration preview"}, preview.files.len(), preview.pending_cycles.len(), preview.source.display(), preview.destination.display(), preview.digest);
    }
    Ok(())
}
async fn promotion_review(args: &[OsString], activate: bool) -> anyhow::Result<()> {
    use clap::Parser;
    let args = match ReviewArgs::try_parse_from(args) {
        Ok(args) => args,
        Err(error) if error.kind() == clap::error::ErrorKind::DisplayHelp => {
            error.print()?;
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let vault = match args.vault {
        Some(root) => LearningVault::existing_at(root)?,
        None => LearningVault::existing_machine()?,
    }
    .ok_or_else(|| anyhow::anyhow!("no harness experiment vault"))?;
    if !activate {
        if args.confirm.is_some() {
            anyhow::bail!("review is read-only; use harness promote --confirm DIGEST");
        }
        let review = crate::harness_automation::preview_review(
            &vault,
            &args.id,
            &args.repository,
            &args.baseline_binary,
        )
        .await?;
        if args.json {
            println!("{}", serde_json::to_string(&review)?);
        } else {
            println!("Experiment {}: Needs review\nCandidate: {}\nEvaluation: {}\nCurrent baseline: {}\nConfirmation: {}", args.id, review.candidate_digest, review.evaluation_digest, review.baseline.head, review.digest);
        }
        return Ok(());
    }
    let confirmation = args.confirm.as_deref().ok_or_else(|| {
        anyhow::anyhow!("promotion requires --confirm REVIEW_DIGEST from harness review")
    })?;
    let _resources = crate::harness_automation::compatibility_resources(&vault, &args.repository)?;
    let repository_lease = crate::harness_automation::compatibility_repository_lease(
        &_resources,
        &vault,
        &args.repository,
    )?;
    let _controller = vault.acquire_harness_controller_lock()?;
    crate::harness_automation::confirm_review(
        &vault,
        &args.id,
        &args.repository,
        &args.baseline_binary,
        confirmation,
    )
    .await?;
    let (plan, _) = load_harness_cycle(&vault, &args.id)?;
    if plan.source_files.is_empty() {
        let activation = crate::harness_guidance::promote(
            vault,
            &args.id,
            &args.repository,
            &args.baseline_binary,
        )
        .await?;
        if args.json {
            println!("{}", serde_json::to_string(&activation)?);
        } else {
            println!("Guidance {}: {:?}", args.id, activation.phase);
        }
    } else {
        crate::harness_source_checkout::import_candidate(vault.clone(), &args.id, &args.repository)
            .await?;
        crate::harness_activation::prepare(vault.clone(), &args.id).await?;
        let promotion = crate::harness_promotion::promote_owned(
            vault,
            &args.id,
            &args.repository,
            repository_lease,
        )
        .await?;
        print_promotion(&promotion, args.json)?;
    }
    Ok(())
}

#[derive(clap::Parser)]
#[command(
    name = "rollback",
    about = "Restore previous guidance, or restore the baseline executable and revert adopted source"
)]
struct RollbackArgs {
    id: String,
    #[arg(long)]
    repository: Option<std::path::PathBuf>,
    #[arg(long)]
    json: bool,
}

pub async fn rollback(args: &[OsString]) -> anyhow::Result<()> {
    use clap::Parser;
    let args = match RollbackArgs::try_parse_from(args) {
        Ok(args) => args,
        Err(error) if error.kind() == clap::error::ErrorKind::DisplayHelp => {
            error.print()?;
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let vault = LearningVault::existing_machine()?
        .ok_or_else(|| anyhow::anyhow!("no harness experiment vault"))?;
    let (plan, _) = load_harness_cycle(&vault, &args.id)?;
    if plan.source_files.is_empty() {
        let _resources = crate::harness_automation::compatibility_harness_resources(&vault)?;
        let activation = crate::harness_guidance::rollback(vault, &args.id).await?;
        if args.json {
            println!("{}", serde_json::to_string(&activation)?);
        } else {
            println!("Guidance {}: {:?}", args.id, activation.phase);
        }
        Ok(())
    } else {
        let repository = args
            .repository
            .ok_or_else(|| anyhow::anyhow!("source rollback requires --repository PATH"))?;
        let _resources = crate::harness_automation::compatibility_resources(&vault, &repository)?;
        let repository_lease = crate::harness_automation::compatibility_repository_lease(
            &_resources,
            &vault,
            &repository,
        )?;
        let promotion = crate::harness_promotion::rollback_owned(
            vault,
            &args.id,
            &repository,
            repository_lease,
        )
        .await?;
        print_promotion(&promotion, args.json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hrs253_exhausted_admission_preserves_charges_and_blocks_recovery() {
        use codex_team::self_improvement::{
            HarnessCycle, HarnessDecision, HarnessStage, HarnessVerdict,
        };
        let (plan, _, _) = crate::harness_generation::tests::inputs();
        let mut cycle = HarnessCycle::new(&plan, 1).unwrap();
        assert!(cycle
            .reserve(&plan, "too-large", plan.settings.maximum_microusd + 1)
            .is_err());
        assert_eq!(cycle.stage, HarnessStage::BudgetExhausted);
        let before = serde_json::to_value(&cycle).unwrap();
        assert!(!blocks_new_cycle(&cycle));
        assert_eq!(serde_json::to_value(&cycle).unwrap(), before);
        cycle.reservations.insert("unknown-provider".into(), 1);
        assert!(blocks_new_cycle(&cycle));
        cycle.reservations.clear();
        cycle
            .active_reservations
            .insert("interrupted-work".into(), 1);
        assert!(blocks_new_cycle(&cycle));
        cycle.active_reservations.clear();
        cycle.decision = Some(HarnessDecision {
            verdict: HarnessVerdict::Promote,
            reasons: vec![],
            fixed_cases: vec![],
            regressed_cases: vec![],
            trials_digest: "a".repeat(64),
        });
        assert!(blocks_new_cycle(&cycle));
        cycle.decision = None;
        for stage in [
            HarnessStage::Paused,
            HarnessStage::RecoveryRequired,
            HarnessStage::ReadyToPromote,
            HarnessStage::SourceAdopting,
            HarnessStage::SourceAdopted,
            HarnessStage::WaitingForCleanSource,
        ] {
            cycle.stage = stage;
            assert!(blocks_new_cycle(&cycle));
        }
    }

    #[test]
    fn hrs253_run_requires_explicit_artifacts_and_rejects_unknown_options() {
        let parse = |args: &[&str]| {
            EvaluationArgs::parse(&args.iter().map(OsString::from).collect::<Vec<_>>())
        };
        assert!(parse(&["run", "cycle"]).is_err());
        assert!(parse(&[
            "run",
            "cycle",
            "--repository",
            "/repo",
            "--baseline-binary",
            "/ca",
            "--force"
        ])
        .is_err());
        assert!(parse(&[
            "run",
            "cycle",
            "--repository",
            "/repo",
            "--baseline-binary",
            "/ca",
            "--json",
            "--json"
        ])
        .is_err());
        let parsed = parse(&[
            "run",
            "cycle",
            "--repository",
            "/repo",
            "--baseline-binary",
            "/ca",
            "--json",
        ])
        .unwrap();
        assert_eq!(parsed.id, "cycle");
        assert!(parsed.json);
    }
}

#[derive(clap::Parser)]
#[command(
    name = "ca harness create",
    about = "Freeze one evidenced guidance or source experiment without making model requests"
)]
struct CreateArgs {
    #[arg(long)]
    repository: std::path::PathBuf,
    /// Select unused matching evidence and host-owned target/held-out cases.
    #[arg(long = "auto", conflicts_with_all = ["target_family", "target", "held_out", "evidence", "source_files"])]
    automatic: bool,
    #[arg(long, required_unless_present = "automatic")]
    target_family: Option<String>,
    #[arg(long, required_unless_present = "automatic")]
    target: Option<std::path::PathBuf>,
    #[arg(long, num_args = 2, required_unless_present = "automatic")]
    held_out: Vec<std::path::PathBuf>,
    #[arg(long, num_args = 3..=64, required_unless_present = "automatic")]
    evidence: Vec<String>,
    #[arg(long)]
    instructions: Option<std::path::PathBuf>,
    /// Select a source experiment; repeat for up to eight repository-relative files.
    #[arg(long = "source-file")]
    source_files: Vec<String>,
    #[arg(long)]
    json: bool,
}

pub async fn create_cycle(args: &[OsString]) -> anyhow::Result<()> {
    use clap::{CommandFactory, Parser};
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        CreateArgs::command().print_help()?;
        println!();
        return Ok(());
    }
    let args = CreateArgs::try_parse_from(
        std::iter::once(OsString::from("ca harness create")).chain(args.iter().skip(1).cloned()),
    )?;
    if !args.automatic && (args.held_out.len() != 2 || !(3..=64).contains(&args.evidence.len())) {
        anyhow::bail!("create requires exactly two held-out directories and 3 to 64 evidence IDs");
    }
    let json = args.json;
    // Configuration and file hashing stay off the async foreground executor.
    let plan = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        use anyhow::Context;
        use std::io::Read;
        use std::os::unix::fs::OpenOptionsExt;
        let paths = crate::runtime::RuntimePaths::discover()?;
        let profile = crate::runtime::ActiveProfile::Default;
        let config = crate::config::ConfigStore::for_read_only_load(
            paths.config_file.clone(),
            profile.clone(),
        )
        .load()
        .map_err(|_| anyhow::anyhow!("could not load configured experiment route"))?;
        let provider = configured_provider(&config, &paths, &profile)?;
        let vault = LearningVault::machine()?;
        let instructions = if let Some(path) = &args.instructions {
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(path)?;
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.len() > 120 * 1024 {
                anyhow::bail!("instructions must be a regular file no larger than 120 KiB");
            }
            let mut instructions = String::new();
            file.by_ref()
                .take(120 * 1024 + 1)
                .read_to_string(&mut instructions)?;
            if instructions.is_empty() || instructions.len() > 120 * 1024 {
                anyhow::bail!("invalid instruction size");
            }
            instructions
        } else {
            crate::harness_guidance::baseline_instructions(
                &vault,
                &provider.source,
                &provider.model,
                &provider.options,
            )?
        };
        if args.source_files.is_empty() && instructions != crate::harness_guidance::baseline_instructions(&vault, &provider.source, &provider.model, &provider.options)? {
            anyhow::bail!("guidance must evaluate the active runtime paragraph; omit --instructions to select it");
        }
        if args.automatic {
            return crate::harness_discovery::create_automatic(&vault, &args.repository, &std::env::current_exe()?, &paths.data_dir, provider, instructions, false, config.harness_improvement.vm.as_ref());
        }
        crate::harness_freeze::create_cycle(
            &vault,
            &args.repository,
            &std::env::current_exe()?,
            &paths.data_dir,
            &args.evidence,
            provider,
            instructions,
            args.target_family.as_deref().context("missing target family")?,
            args.target.as_deref().context("missing target")?,
            [&args.held_out[0], &args.held_out[1]],
            &args.source_files,
            config.harness_improvement.vm.as_ref(),
        )
    })
    .await??;
    if json {
        println!("{}", serde_json::json!({"cycle_id":plan.id,"plan":plan}));
    } else {
        println!("Frozen experiment {}. Use harness run with this executable to generate and evaluate it.", plan.id);
    }
    Ok(())
}

#[cfg(test)]
mod source_create_tests {
    use super::*;
    use clap::Parser;
    #[test]
    fn hrs253_automatic_create_does_not_require_or_accept_manual_case_selection() {
        let args =
            CreateArgs::try_parse_from(["create", "--repository", "/repo", "--auto"]).unwrap();
        assert!(args.automatic);
        assert!(args.target.is_none());
        assert!(CreateArgs::try_parse_from([
            "create",
            "--repository",
            "/repo",
            "--auto",
            "--target",
            "/case"
        ])
        .is_err());
        assert!(CreateArgs::try_parse_from([
            "create",
            "--repository",
            "/repo",
            "--auto",
            "--source-file",
            "src/example.rs"
        ])
        .is_err());
    }
    #[test]
    fn hrs253_create_selects_source_only_when_explicit_paths_are_present() {
        let base = [
            "create",
            "--repository",
            "/repo",
            "--target-family",
            "utf8_truncate",
            "--target",
            "/target",
            "--held-out",
            "/held-a",
            "/held-b",
            "--evidence",
            "a:1",
            "a:2",
            "b:1",
            "--instructions",
            "/instructions",
        ];
        assert!(CreateArgs::try_parse_from(base)
            .unwrap()
            .source_files
            .is_empty());
        let args = base.into_iter().chain([
            "--source-file",
            "src/loop_guard.rs",
            "--source-file",
            "prompts/baseline.md",
        ]);
        assert_eq!(
            CreateArgs::try_parse_from(args).unwrap().source_files,
            ["src/loop_guard.rs", "prompts/baseline.md"]
        );
    }
}

/// Preview discovery without creating a vault, running a candidate or loading credentials.
pub async fn discover(args: &[OsString]) -> anyhow::Result<()> {
    #[derive(clap::Parser)]
    #[command(
        name = "ca harness discover",
        about = "Find unused current-build failure groups without making model requests"
    )]
    struct Args {
        #[arg(long)]
        json: bool,
    }
    use clap::{CommandFactory, Parser};
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        Args::command().print_help()?;
        println!();
        return Ok(());
    }
    let args = Args::try_parse_from(
        std::iter::once(OsString::from("ca harness discover")).chain(args.iter().skip(1).cloned()),
    )?;
    let report = tokio::task::spawn_blocking(|| -> anyhow::Result<_> {
        let paths = crate::runtime::RuntimePaths::discover()?;
        let vault = LearningVault::existing_machine()?;
        crate::harness_discovery::discover(&paths.data_dir, vault.as_ref())
    })
    .await??;
    if args.json {
        println!("{}", serde_json::to_string(&report)?);
    } else {
        println!(
            "{} eligible failure groups; {} outcomes inspected, {} unreadable.",
            report.groups.len(),
            report.scanned_outcomes,
            report.unreadable_outcomes
        );
        for group in report.groups {
            println!(
                "{}: {} failures across {} tasks; evaluation-family hypothesis: {}",
                group.tool,
                group.evidence_ids.len(),
                group.independent_tasks,
                group.family_hypothesis.as_deref().unwrap_or("unclassified")
            );
            println!("  Evidence: {}", group.evidence_ids.join(" "));
        }
    }
    Ok(())
}

pub(crate) fn configured_provider(
    config: &crate::config::CaConfig,
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
) -> anyhow::Result<crate::harness_budget::FrozenHarnessProvider> {
    use anyhow::Context;
    let provider_id = &config.defaults.provider;
    let model = &config.defaults.model;
    let route = config
        .providers
        .get(provider_id)
        .context("default provider is not configured")?;
    let pricing_store = crate::pricing::PricingStore::for_paths(paths, profile);
    let blocked = || PricingAdmissionBlocked {
        provider: provider_id.clone(),
        model: model.clone(),
        profile: profile.label().into(),
        revision: pricing_store
            .admission_revision()
            .unwrap_or_else(|_| "unreadable".into()),
    };
    let catalog = pricing_store.load().map_err(|_| blocked())?;
    let price = catalog
        .find(provider_id, model)
        .ok_or_else(blocked)?
        .clone();
    let configured_context = config
        .context_for_model(provider_id, model)
        .and_then(|context| context.context_window_tokens);
    let (context_tokens, input_byte_limit) =
        provider_context_limits(provider_id, model, route, configured_context)?;
    let provider = crate::harness_budget::FrozenHarnessProvider {
        source: crate::agent::stream::ProviderStateSourceV1::from_route(
            provider_id,
            route.protocol,
            &route.base_url,
        )?,
        model: model.clone(),
        options: crate::chat_request_options(config, provider_id, model, None),
        context_tokens,
        input_byte_limit: Some(input_byte_limit),
        output_tokens: context_tokens.min(8192),
        request_seconds: 120,
        price,
        billing: config.provider_billing.get(provider_id).cloned(),
    };
    Ok(provider)
}

fn provider_context_limits(
    provider: &str,
    model: &str,
    route: &crate::config::ProviderConfig,
    configured: Option<u64>,
) -> anyhow::Result<(u32, u32)> {
    use anyhow::Context;
    // An arbitrary local setting is not evidence of the provider's billable
    // ceiling. Bind the reservation to the exact supported first-party route.
    let maximum = crate::provider_contract::resolve(provider, route, model)
        .and_then(|contract| contract.model_profile().context_window_tokens)
        .context("harness experiments require a supported provider context contract")?;
    let input = configured.unwrap_or(maximum).min(maximum);
    if maximum == 0 || input == 0 {
        anyhow::bail!("invalid harness context limit");
    }
    Ok((maximum.try_into()?, input.try_into()?))
}

#[cfg(test)]
#[test]
fn hrs253_context_reservation_uses_provider_ceiling_and_preserves_smaller_payload_cap() {
    let mut route = crate::config::ProviderConfig {
        base_url: "https://api.deepseek.com/v1".into(),
        api_key: String::new(),
        protocol: crate::agent::provider::ProviderProtocol::ChatCompletions,
        prompt_cache_mode: Default::default(),
        models: vec!["deepseek-v4-pro".into()],
        timeout: Default::default(),
    };
    assert_eq!(
        provider_context_limits("deepseek", "deepseek-v4-pro", &route, Some(32768)).unwrap(),
        (1_000_000, 32768)
    );
    assert_eq!(
        provider_context_limits("deepseek", "deepseek-v4-pro", &route, Some(2_000_000)).unwrap(),
        (1_000_000, 1_000_000)
    );
    assert!(provider_context_limits("deepseek", "deepseek-v4-pro", &route, Some(0)).is_err());
    route.base_url = "https://unverified.example/v1".into();
    assert!(provider_context_limits("deepseek", "deepseek-v4-pro", &route, Some(32768)).is_err());
}
