//! Host-owned cycle creation. Evidence and task bytes are frozen before model
//! generation, and the generation path cannot substitute another held-out suite.
use crate::harness_budget::FrozenHarnessProvider;
use crate::harness_cases::{freeze_reasoning_cases, validate_frozen_suite, FrozenCodingCase};
use crate::harness_generation::GuidanceInputs;
use crate::session_outcome::{load_outcome, SessionOutcome};
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{
    canonical_digest, list_harness_cycles, load_harness_cycle, observe_clean_repository,
    start_harness_cycle, HarnessPlan, HarnessSettings, LearningVault, HARNESS_SCHEMA,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[cfg(test)]
use crate::harness_snapshot::identity as build_identity;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrozenInputs {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    source_files: BTreeMap<String, String>,
    digest: String,
    inputs: GuidanceInputs,
}

/// Validate one attributable failure before discovery groups it with peers.
pub(crate) fn validate_failure(
    outcome: &SessionOutcome,
    event: &crate::session_outcome::EvidenceEvent,
) -> Result<()> {
    if outcome.version < 2
        || outcome.execution == crate::session_outcome::ExecutionStatus::Running
        || !outcome.evidence_complete
        || outcome.harness_epochs.len() != 1
        || outcome.harness_epochs.first() != Some(&outcome.harness)
        || outcome.harness.build.is_none()
        || outcome.harness.model_route.is_none()
        || outcome.model.is_none()
        || outcome.provider.is_none()
        || event.id != format!("{}:{}", outcome.task_id, event.sequence)
        || event.kind != "tool_evidence"
        || event.succeeded != Some(false)
        || event.tool.is_none()
        || event.excerpt == "Tool denied by user"
        || event.excerpt == "[private evidence omitted]"
    {
        bail!("selected event is not complete, attributable tool-failure evidence");
    }
    let call = event
        .call_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .context("failure evidence has no tool-call identity")?;
    if outcome
        .events
        .iter()
        .filter(|other| {
            other.kind == "tool_evidence"
                && other.call_id.as_deref() == Some(call)
                && other.tool == event.tool
        })
        .count()
        != 1
    {
        bail!("failure output has ambiguous execution evidence");
    }
    let mut results = outcome.events.iter().filter(|result| {
        result.kind == "tool_result"
            && result.call_id.as_deref() == Some(call)
            && result.tool == event.tool
    });
    let result = results
        .next()
        .context("failure output has no execution receipt")?;
    if results.next().is_some()
        || result.succeeded != Some(false)
        || result.excerpt != "Tool failed"
        || result.sequence >= event.sequence
        || result.id != format!("{}:{}", outcome.task_id, result.sequence)
    {
        bail!(
            "failure output is denied, ambiguous, duplicated, or lacks a matching failure receipt"
        );
    }
    if event.excerpt.len() > 4096 {
        bail!("failure output exceeds the event bound");
    }
    if crate::learning::sanitized_learning_text(&event.excerpt).is_none_or(|s| s.is_empty()) {
        bail!("evidence excerpt is unavailable");
    }
    Ok(())
}

pub(crate) fn select_evidence(
    outcomes: &[SessionOutcome],
    ids: &[String],
) -> Result<BTreeMap<String, String>> {
    if !(3..=64).contains(&ids.len()) || ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
        bail!("select 3 to 64 distinct failure events across at least two tasks");
    }
    let mut selected = BTreeMap::new();
    let mut tasks = BTreeSet::new();
    let mut attempts = BTreeSet::new();
    let mut cohort: Option<&SessionOutcome> = None;
    let mut failed_tool = None;
    for id in ids {
        let mut matches = outcomes
            .iter()
            .flat_map(|outcome| outcome.events.iter().map(move |event| (outcome, event)))
            .filter(|(_, event)| &event.id == id);
        let (outcome, event) = matches
            .next()
            .context("selected evidence event is missing")?;
        if matches.next().is_some() {
            bail!("ambiguous evidence event identity");
        }
        validate_failure(outcome, event)?;
        if !attempts.insert((&outcome.task_id, event.call_id.as_deref().unwrap())) {
            bail!("duplicate execution evidence");
        }
        if let Some(first) = cohort {
            if first.workspace_hash != outcome.workspace_hash
                || first.harness != outcome.harness
                || first.provider != outcome.provider
                || first.model != outcome.model
            {
                bail!("selected evidence crosses runtime cohorts");
            }
        } else {
            cohort = Some(outcome);
        }
        if let Some(tool) = failed_tool {
            if tool != event.tool.as_ref() {
                bail!("selected failure events concern different tools");
            }
        } else {
            failed_tool = Some(event.tool.as_ref());
        }
        if event.excerpt.len() > 4096 {
            bail!("failure output exceeds the event bound");
        }
        let mut excerpt = crate::learning::sanitized_learning_text(&event.excerpt)
            .filter(|s| !s.is_empty())
            .context("evidence excerpt is unavailable")?;
        let mut end = excerpt.len().min(1024);
        while !excerpt.is_char_boundary(end) {
            end -= 1;
        }
        excerpt.truncate(end);
        tasks.insert(outcome.independent_task_id());
        selected.insert(id.clone(), excerpt);
    }
    if tasks.len() < 2 {
        bail!("failure evidence must span at least two tasks");
    }
    Ok(selected)
}

/// Freeze explicit host-selected target/held-out fixture directories. Automatic
/// discovery can call the same boundary after selecting its evidenced family.
#[allow(clippy::too_many_arguments)]
pub fn create_cycle(
    vault: &LearningVault,
    repository: &Path,
    baseline_binary: &Path,
    outcome_data: &Path,
    evidence_ids: &[String],
    provider: FrozenHarnessProvider,
    instructions: String,
    family: &str,
    target: &Path,
    held_out: [&Path; 2],
    source_paths: &[String],
    vm_profile: Option<&codex_team::self_improvement::HarnessVmProfile>,
) -> Result<HarnessPlan> {
    create_cycle_with_preparation(
        vault,
        repository,
        baseline_binary,
        outcome_data,
        evidence_ids,
        provider,
        instructions,
        family,
        target,
        held_out,
        source_paths,
        std::time::Instant::now(),
        false,
        vm_profile,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn create_cycle_with_preparation(
    vault: &LearningVault,
    repository: &Path,
    baseline_binary: &Path,
    outcome_data: &Path,
    evidence_ids: &[String],
    provider: FrozenHarnessProvider,
    instructions: String,
    family: &str,
    target: &Path,
    held_out: [&Path; 2],
    source_paths: &[String],
    preparation_started: std::time::Instant,
    enforce_cooldown: bool,
    vm_profile: Option<&codex_team::self_improvement::HarnessVmProfile>,
) -> Result<HarnessPlan> {
    let running_digest = crate::harness_engineering::artifact_digest(&std::env::current_exe()?)?;
    create_with_source_digest(
        vault,
        repository,
        baseline_binary,
        outcome_data,
        evidence_ids,
        provider,
        instructions,
        family,
        target,
        held_out,
        source_paths,
        (env!("CA_SOURCE_DIGEST"), &running_digest),
        preparation_started,
        enforce_cooldown,
        vm_profile,
    )
}

#[allow(clippy::too_many_arguments)]
fn create_with_source_digest(
    vault: &LearningVault,
    repository: &Path,
    baseline_binary: &Path,
    outcome_data: &Path,
    evidence_ids: &[String],
    provider: FrozenHarnessProvider,
    instructions: String,
    family: &str,
    target: &Path,
    held_out: [&Path; 2],
    source_paths: &[String],
    expected_identity: (&str, &str),
    freezing_started: std::time::Instant,
    enforce_cooldown: bool,
    vm_profile: Option<&codex_team::self_improvement::HarnessVmProfile>,
) -> Result<HarnessPlan> {
    if !(3..=64).contains(&evidence_ids.len())
        || evidence_ids.iter().collect::<BTreeSet<_>>().len() != evidence_ids.len()
    {
        bail!("select 3 to 64 distinct failure events across at least two tasks");
    }
    let source = crate::harness_snapshot::source_digest(repository)?;
    if source != expected_identity.0 {
        bail!("repository source differs from the running baseline build");
    }
    let baseline_binary_digest = crate::harness_engineering::artifact_digest(baseline_binary)?;
    if baseline_binary_digest != expected_identity.1 {
        bail!("baseline executable differs from the running host");
    }
    let _worker = vault.acquire_worker_lock()?;
    let previous_cycles = list_harness_cycles(vault)?;
    if previous_cycles
        .iter()
        .any(crate::harness_control::blocks_new_cycle)
    {
        bail!("an existing harness cycle must finish before creating another");
    }
    if enforce_cooldown {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        if previous_cycles.iter().any(|cycle| {
            now.saturating_sub(cycle.created_at) < HarnessSettings::default().cooldown_seconds
        }) {
            bail!("automatic experiment cooldown has not elapsed");
        }
    }
    let mut outcomes = BTreeMap::new();
    for id in evidence_ids {
        let (task, _) = id
            .rsplit_once(':')
            .context("evidence IDs must identify a task and event")?;
        if !outcomes.contains_key(task) {
            let outcome = load_outcome(task, outcome_data)?.context("evidence task not found")?;
            if outcome.task_id != task {
                bail!("evidence task identity changed");
            }
            outcomes.insert(task.to_owned(), outcome);
        }
    }
    let outcomes = outcomes.into_values().collect::<Vec<_>>();
    let evidence = select_evidence(&outcomes, evidence_ids)?;
    let observed = outcomes.first().context("no evidence tasks")?;
    let request = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::Headless,
        &provider.source.provider,
        &provider.model,
        Vec::new(),
        Vec::new(),
        provider.options.clone(),
    );
    let actual = crate::harness_registry::capture_prepared_on_route(&request, &provider.source)
        .outcome_snapshot();
    if observed.harness.build != actual.build
        || observed.harness.model_route != actual.model_route
        || observed.provider.as_deref() != Some(provider.source.provider.as_str())
        || observed.model.as_deref() != Some(provider.model.as_str())
        || provider.price.provider != provider.source.provider
        || provider.price.model != provider.model
    {
        bail!("experiment build, route, options or price card differs from the evidence cohort");
    }
    if !provider.price.fresh_at(chrono::Utc::now()) {
        bail!("refresh verified pricing before freezing a cycle");
    }
    let maximum = provider.maximum_microusd()?;
    if maximum > HarnessSettings::default().maximum_microusd {
        bail!("one maximum-size provider request exceeds the cycle budget");
    }
    let snapshot = if source_paths.is_empty() {
        Some(crate::harness_snapshot::capture(
            vault,
            repository,
            expected_identity.0,
        )?)
    } else {
        None
    };
    let mut guidance_source = snapshot.as_ref().map(|(source, _)| source.clone());
    let evaluation_repository = guidance_source
        .as_ref()
        .map_or(repository, |source| source.repository.as_path());
    let baseline = observe_clean_repository(evaluation_repository)?;
    let mut cases = freeze_reasoning_cases(evaluation_repository)?;
    cases.push(FrozenCodingCase::freeze(
        target, family, "target", true, false,
    )?);
    for (index, directory) in held_out.into_iter().enumerate() {
        cases.push(FrozenCodingCase::freeze(
            directory,
            family,
            &format!("held-out-{index}"),
            false,
            true,
        )?);
    }
    let source_files =
        crate::harness_source_generation::read_source_files(repository, source_paths)?;
    let vm = vm_profile
        .map(|profile| {
            crate::harness_vm::freeze(
                profile,
                HarnessSettings::default()
                    .maximum_active_seconds
                    .saturating_sub(freezing_started.elapsed().as_secs()),
            )
        })
        .transpose()?;
    if observe_clean_repository(evaluation_repository)? != baseline {
        bail!("repository changed while freezing inputs");
    }
    if let Some(source) = &mut guidance_source {
        source.freezing_seconds = freezing_started.elapsed().as_secs().saturating_add(1);
    }
    let mut plan = HarnessPlan {
        vm,
        preparation_seconds: Some(freezing_started.elapsed().as_secs().saturating_add(1)),
        guidance_source,
        activation_executable: Some(baseline_binary.canonicalize()?),
        source_files: source_files
            .iter()
            .map(|(path, text)| Ok((path.clone(), canonical_digest(text)?)))
            .collect::<Result<BTreeMap<_, _>>>()?,
        schema: HARNESS_SCHEMA.into(),
        id: uuid::Uuid::new_v4().to_string(),
        baseline,
        baseline_binary_digest,
        route_digest: provider.route_digest()?,
        price_digest: provider.price_digest()?,
        evidence_ids: evidence.keys().cloned().collect(),
        cases: cases.iter().map(|case| case.case.clone()).collect(),
        repetitions: 3,
        settings: HarnessSettings {
            enabled: true,
            ..HarnessSettings::default()
        },
        digest: String::new(),
    };
    plan.seal()?;
    validate_frozen_suite(&plan, &cases)?;
    let inputs = GuidanceInputs {
        baseline_hints: crate::harness_guidance::baseline_hints(
            vault,
            &provider.source,
            &provider.model,
            &provider.options,
        )?,
        plan_digest: plan.digest.clone(),
        provider,
        cases,
        baseline_instructions: instructions,
        evidence,
    };
    inputs.validate(&plan)?;
    if !source_files.is_empty() {
        crate::harness_source_generation::SourceInputs {
            shared: inputs.clone(),
            files: source_files.clone(),
        }
        .validate(&plan)?;
    }
    let frozen = FrozenInputs {
        source_files,
        digest: canonical_digest(&inputs)?,
        inputs,
    };
    crate::harness_idle::preparation_boundary()?;
    // Retain the private copy only after all frozen inputs validate. Once the
    // journal write begins, recovery must not lose its referenced source.
    if let Some((_, owner)) = snapshot {
        let _ = owner.keep();
    }
    // Write inputs before the cycle becomes visible. No provider can be called
    // until the plan and its complete input bundle both validate.
    {
        let _lock = vault.acquire_lock()?;
        vault.write_mutable(&format!("harness/inputs/{}.json", plan.id), &frozen)?;
    }
    start_harness_cycle(vault, &plan, chrono::Utc::now().timestamp().try_into()?)?;
    Ok(plan)
}

pub fn load_inputs(vault: &LearningVault, id: &str) -> Result<GuidanceInputs> {
    let (plan, _) = load_harness_cycle(vault, id)?;
    let record: FrozenInputs = vault
        .read_mutable(&format!("harness/inputs/{id}.json"))?
        .context("cycle has no frozen creation inputs")?;
    record.inputs.validate(&plan)?;
    if record.digest != canonical_digest(&record.inputs)? {
        bail!("frozen creation inputs changed");
    }
    if !plan.source_files.is_empty() || !record.source_files.is_empty() {
        crate::harness_source_generation::SourceInputs {
            shared: record.inputs.clone(),
            files: record.source_files,
        }
        .validate(&plan)?;
    }
    Ok(record.inputs)
}

pub fn load_source_inputs(
    vault: &LearningVault,
    id: &str,
) -> Result<Option<crate::harness_source_generation::SourceInputs>> {
    let (plan, _) = load_harness_cycle(vault, id)?;
    let shared = load_inputs(vault, id)?;
    if plan.source_files.is_empty() {
        return Ok(None);
    }
    let record: FrozenInputs = vault
        .read_mutable(&format!("harness/inputs/{id}.json"))?
        .context("frozen inputs missing")?;
    let inputs = crate::harness_source_generation::SourceInputs {
        shared,
        files: record.source_files,
    };
    inputs.validate(&plan)?;
    Ok(Some(inputs))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::session_outcome::{EvidenceEvent, OutcomeTracker};
    use codex_team::self_improvement::HarnessStage;

    pub(crate) fn outcomes() -> Vec<SessionOutcome> {
        (0..2)
            .map(|index| {
                let mut tracker = OutcomeTracker::new(format!("session-{index}"), "/workspace");
                let provider = crate::harness_budget::tests::frozen();
                tracker.set_model(
                    Some(provider.model.clone()),
                    Some(provider.source.provider.clone()),
                );
                let request = crate::request::PreparedModelRequest::new(
                    crate::request::RequestKind::Headless,
                    &provider.source.provider,
                    &provider.model,
                    Vec::new(),
                    Vec::new(),
                    provider.options.clone(),
                );
                tracker.set_runtime_harness(crate::harness_registry::capture_prepared_on_route(
                    &request,
                    &provider.source,
                ));
                let mut outcome = tracker.finish();
                outcome.execution = crate::session_outcome::ExecutionStatus::Completed;
                outcome.events = (0..2)
                    .flat_map(|attempt| {
                        [
                            ("tool_result", "Tool failed"),
                            ("tool_evidence", "boundary check failed"),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(move |(offset, (kind, excerpt))| (attempt, offset, kind, excerpt))
                    })
                    .map(|(attempt, offset, kind, excerpt)| {
                        let sequence = attempt * 2 + offset as u64;
                        EvidenceEvent {
                            id: format!("{}:{sequence}", outcome.task_id),
                            sequence,
                            kind: kind.into(),
                            tool: Some("run_command".into()),
                            call_id: Some(format!("call-{attempt}")),
                            succeeded: Some(false),
                            excerpt: excerpt.into(),
                        }
                    })
                    .collect();
                outcome
            })
            .collect()
    }

    #[test]
    fn hrs253_freeze_requires_distinct_attributable_failures_in_one_cohort() {
        let records = outcomes();
        let ids = vec![
            records[0].events[1].id.clone(),
            records[0].events[3].id.clone(),
            records[1].events[1].id.clone(),
        ];
        assert_eq!(select_evidence(&records, &ids).unwrap().len(), 3);
        assert!(
            select_evidence(&records, &[ids[0].clone(), ids[0].clone(), ids[2].clone()]).is_err()
        );
        let mut changed = records.clone();
        changed[1].root_task_id = Some(changed[0].task_id.clone());
        changed[1].parent_task_id = Some(changed[0].task_id.clone());
        assert!(select_evidence(&changed, &ids).is_err());
        let mut changed = records.clone();
        changed[1].workspace_hash = "other".into();
        assert!(select_evidence(&changed, &ids).is_err());
        changed = records.clone();
        changed[0].evidence_complete = false;
        assert!(select_evidence(&changed, &ids).is_err());
        changed = records.clone();
        changed[0].events[0].excerpt = "Tool denied by user".into();
        assert!(select_evidence(&changed, &ids).is_err());
        changed = records.clone();
        changed[0].version = 1;
        assert!(select_evidence(&changed, &ids).is_err());
        changed = records.clone();
        changed[0].events[0].id = "fabricated:0".into();
        assert!(select_evidence(&changed, &ids).is_err());
    }
    #[test]
    fn hrs253_freeze_persists_nine_cases_and_refuses_replacement_or_parallel_cycle() {
        let temp = tempfile::tempdir().unwrap();
        let repository = temp.path().join("repository");
        let fixtures = repository.join("tests/fixtures/reasoning_pilot");
        std::fs::create_dir_all(&fixtures).unwrap();
        let cases = freeze_reasoning_cases(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        for case in &cases {
            case.materialize(&fixtures.join(&case.fixture_id)).unwrap();
        }
        for (path, content) in [
            ("build.rs", "fn main() {}"),
            ("build_support/helper.rs", "helper"),
            ("Cargo.toml", "fixture manifest"),
            ("Cargo.lock", "fixture lock"),
            ("src/lib.rs", "fixture source"),
            ("prompts/baseline.md", "baseline"),
            ("tests/fixtures/codex-model-catalog.json", "{}"),
            (
                "tests/reasoning_pilot_hidden/src/lib.rs",
                "host verifier fixture",
            ),
        ] {
            let path = repository.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        std::fs::create_dir(repository.join("crates")).unwrap();
        let expected_source = build_identity::digest(
            &repository,
            &build_identity::input_roots(&repository).unwrap(),
            &Default::default(),
            &[],
        )
        .unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(&repository)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "Fixture"]);
        git(&["config", "user.email", "fixture@example.invalid"]);
        git(&["config", "commit.gpgsign", "false"]);
        git(&["add", "."]);
        git(&["commit", "-m", "fixture"]);
        let mut held_paths = Vec::new();
        for (index, implementation) in [
            "pub fn truncate_utf8(_: &str, _: usize) -> &str { \"\" }",
            "pub fn truncate_utf8(s: &str, _: usize) -> &str { s }",
        ]
        .into_iter()
        .enumerate()
        {
            let path = temp.path().join(format!("held-{index}"));
            cases[0].materialize(&path).unwrap();
            std::fs::write(path.join("src/lib.rs"), implementation).unwrap();
            held_paths.push(path);
        }
        let records = outcomes();
        let data = temp.path().join("data");
        for record in &records {
            crate::session_outcome::save_outcome(record, &data).unwrap();
        }
        let ids = vec![
            records[0].events[1].id.clone(),
            records[0].events[3].id.clone(),
            records[1].events[1].id.clone(),
        ];
        let binary = temp.path().join("binary");
        std::fs::write(&binary, "fixture binary").unwrap();
        let vault = LearningVault::open_at(temp.path().join("vault")).unwrap();
        let expected_binary = crate::harness_engineering::artifact_digest(&binary).unwrap();
        let create_with_policy =
            |vault: &LearningVault, source_paths: &[String], enforce_cooldown: bool| {
                create_with_source_digest(
                    vault,
                    &repository,
                    &binary,
                    &data,
                    &ids,
                    crate::harness_budget::tests::frozen(),
                    "Repair the requested behavior and verify it".into(),
                    &cases[0].fixture_id,
                    &fixtures.join(&cases[0].fixture_id),
                    [&held_paths[0], &held_paths[1]],
                    source_paths,
                    (&expected_source, &expected_binary),
                    std::time::Instant::now(),
                    enforce_cooldown,
                    None,
                )
            };
        let create = |vault: &LearningVault, source_paths: &[String]| {
            create_with_policy(vault, source_paths, false)
        };
        let cooldown_vault = LearningVault::open_at(temp.path().join("cooldown-vault")).unwrap();
        let prior = create(&cooldown_vault, &[]).unwrap();
        codex_team::self_improvement::update_harness_cycle(&cooldown_vault, &prior.id, |state| {
            state.stage = HarnessStage::Rejected;
            Ok(())
        })
        .unwrap();
        assert!(create_with_policy(&cooldown_vault, &[], true).is_err());
        assert!(create(&cooldown_vault, &[]).is_ok());
        let plan = create(&vault, &[]).unwrap();
        assert!(plan.source_files.is_empty());
        assert!(load_source_inputs(&vault, &plan.id).unwrap().is_none());
        let source_vault = LearningVault::open_at(temp.path().join("source-vault")).unwrap();
        assert!(create(&source_vault, &["src/harness_budget.rs".into()]).is_err());
        assert!(list_harness_cycles(&source_vault).unwrap().is_empty());
        let source_plan = create(&source_vault, &["src/lib.rs".into()]).unwrap();
        assert_eq!(
            source_plan.source_files["src/lib.rs"],
            canonical_digest(&"fixture source").unwrap()
        );
        let source_inputs = load_source_inputs(&source_vault, &source_plan.id)
            .unwrap()
            .unwrap();
        assert_eq!(source_inputs.files["src/lib.rs"], "fixture source");
        let source_path = format!("harness/inputs/{}.json", source_plan.id);
        let mut altered: FrozenInputs = source_vault.read_mutable(&source_path).unwrap().unwrap();
        altered
            .source_files
            .insert("src/lib.rs".into(), "substituted".into());
        source_vault.write_mutable(&source_path, &altered).unwrap();
        assert!(load_source_inputs(&source_vault, &source_plan.id).is_err());
        let frozen = load_inputs(&vault, &plan.id).unwrap();
        assert_eq!(frozen.cases.len(), 9);
        assert_eq!(frozen.evidence.len(), 3);
        assert_eq!(
            load_harness_cycle(&vault, &plan.id).unwrap().1.stage,
            HarnessStage::EvaluationFrozen
        );
        let pending_vault = LearningVault::open_at(temp.path().join("pending-vault")).unwrap();
        let pending_plan = create(&pending_vault, &[]).unwrap();
        for stage in [
            HarnessStage::Promoted,
            HarnessStage::RolledBack,
            HarnessStage::Rejected,
            HarnessStage::BudgetExhausted,
        ] {
            for provider_charge in [false, true] {
                codex_team::self_improvement::update_harness_cycle(
                    &pending_vault,
                    &pending_plan.id,
                    |cycle| {
                        cycle.stage = stage.clone();
                        cycle.reservations.clear();
                        cycle.active_reservations.clear();
                        if provider_charge {
                            cycle.reservations.insert("unknown-provider".into(), 100);
                        } else {
                            cycle
                                .active_reservations
                                .insert("unfinished-publication".into(), 60);
                        }
                        Ok(())
                    },
                )
                .unwrap();
                assert!(
                    create(&pending_vault, &[]).is_err(),
                    "unsettled terminal cycle must still block admission"
                );
                assert_eq!(list_harness_cycles(&pending_vault).unwrap().len(), 1);
            }
        }
        std::fs::write(held_paths[0].join("src/lib.rs"), "changed after freezing").unwrap();
        assert_eq!(
            canonical_digest(&load_inputs(&vault, &plan.id).unwrap()).unwrap(),
            canonical_digest(&frozen).unwrap()
        );
        assert!(create(&vault, &[]).is_err());
        let path = format!("harness/inputs/{}.json", plan.id);
        let mut record: FrozenInputs = vault.read_mutable(&path).unwrap().unwrap();
        record.inputs.baseline_instructions.push_str(" changed");
        vault.write_mutable(&path, &record).unwrap();
        assert!(load_inputs(&vault, &plan.id).is_err());
    }
}

#[cfg(test)]
#[path = "harness_integration.rs"]
mod integration_tests;
