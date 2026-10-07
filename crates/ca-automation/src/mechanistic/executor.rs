use super::dependency_graph::{select_dependencies, DependencySelection, Ecosystem};
use super::{
    BuiltinRepositoryCheckV1, DelegationReadinessV1, EvidenceRecipeV1, MechanisticActionResultV1,
    MechanisticActionV1, MechanisticCacheStatusV1, MechanisticConfidenceV1, MechanisticDecisionV1,
    MechanisticDependencySelectionV1, MechanisticReceiptStatusV1, MechanisticReceiptV1,
    MechanisticRequestV1, VerificationScopeV1, MAX_MECHANISTIC_OUTPUT_BYTES,
    MECHANISTIC_RECEIPT_SCHEMA,
};
use crate::cancellation::CancellationToken;
use crate::paths::Paths;
use crate::revision::repository_revision;
use crate::verification::isolation::{require_isolation_backend, IsolationSession};
use crate::verification::recipe::{
    CachePolicy, CommandKind, FailurePolicy, Recipe, RecipeSpec, VerificationCommand,
};
use crate::verification::registry::{
    is_deterministic_command_structured_allowed, recipe_spec_to_command, validate_recipe_spec,
};
use crate::verification::runner::{
    run_deterministic_command_with_cancellation,
    run_isolated_structured_command_in_session_with_cancellation,
};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

pub(crate) struct MechanisticExecutor {
    paths: Paths,
}

#[derive(Debug, Clone)]
struct CachedEvidence {
    request_digest: String,
    source_receipt_digest: String,
    selected_paths: Vec<String>,
    selected_recipes: Vec<String>,
    confidence: MechanisticConfidenceV1,
    reason_codes: Vec<String>,
    dependency_selection: Option<MechanisticDependencySelectionV1>,
    avoided_duration_millis: u64,
}

static EVIDENCE_CACHE: OnceLock<Mutex<BTreeMap<String, CachedEvidence>>> = OnceLock::new();

fn evidence_cache() -> &'static Mutex<BTreeMap<String, CachedEvidence>> {
    EVIDENCE_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub(crate) fn invalidate_evidence_cache_for_request(request_digest: &str) {
    evidence_cache()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .retain(|_, entry| entry.request_digest != request_digest);
}

impl MechanisticExecutor {
    pub(crate) fn new(repo: &Path) -> Result<Self> {
        Ok(Self {
            paths: Paths::new(repo)?,
        })
    }

    pub(crate) fn readiness(&self) -> DelegationReadinessV1 {
        match require_isolation_backend() {
            Ok(readiness) => DelegationReadinessV1 {
                queries_available: true,
                evidence_available: readiness.ready,
                observation_available: true,
                evidence_backend: Some(readiness.backend),
                reasons: Vec::new(),
            },
            Err(error) => DelegationReadinessV1 {
                queries_available: true,
                evidence_available: false,
                observation_available: true,
                evidence_backend: None,
                reasons: vec![bounded_reason(&format!("{error:#}"))],
            },
        }
    }

    pub(crate) fn execute_queries(
        &self,
        request: &MechanisticRequestV1,
    ) -> Result<MechanisticReceiptV1> {
        validate_request(request)?;
        if request.actions.iter().any(|action| !action.is_query()) {
            bail!("repo_query accepts repository query actions only");
        }
        self.require_expected_revision(&request.expected_revision)?;
        let started = Instant::now();
        let started_at = Utc::now().to_rfc3339();
        let mut output_bytes = 0_usize;
        let mut results = Vec::new();
        for (index, action) in request.actions.iter().enumerate() {
            let remaining = MAX_MECHANISTIC_OUTPUT_BYTES.saturating_sub(output_bytes);
            let result =
                super::query::execute_query_action(&self.paths.root, index, action, remaining)?;
            output_bytes = output_bytes.saturating_add(
                result
                    .entries
                    .iter()
                    .map(|entry| entry.len() + 1)
                    .sum::<usize>(),
            );
            results.push(result);
            if output_bytes >= MAX_MECHANISTIC_OUTPUT_BYTES {
                break;
            }
        }
        self.require_expected_revision(&request.expected_revision)?;
        Ok(MechanisticReceiptV1 {
            schema: MECHANISTIC_RECEIPT_SCHEMA.to_string(),
            request_digest: request_digest(request)?,
            repository_revision: request.expected_revision.clone(),
            status: MechanisticReceiptStatusV1::Completed,
            readiness: self.readiness(),
            action_results: results,
            output_bytes: output_bytes as u64,
            started_at,
            finished_at: Utc::now().to_rfc3339(),
            elapsed_millis: started.elapsed().as_millis() as u64,
            fallback_reason: None,
            broker: None,
            capsule: None,
        })
    }

    pub(crate) fn execute_evidence(
        &self,
        request: &MechanisticRequestV1,
    ) -> Result<MechanisticReceiptV1> {
        self.execute_evidence_with_cancellation(request, &CancellationToken::default())
    }

    pub(crate) fn execute_evidence_with_cancellation(
        &self,
        request: &MechanisticRequestV1,
        cancellation: &CancellationToken,
    ) -> Result<MechanisticReceiptV1> {
        validate_request(request)?;
        if request.actions.iter().any(|action| !action.is_evidence()) {
            bail!("evidence execution accepts evidence actions only");
        }
        self.require_expected_revision(&request.expected_revision)?;
        let readiness = self.readiness();
        let isolated = readiness.evidence_available;
        if !isolated && !request.allow_unisolated_evidence {
            bail!(
                "verification isolation is unavailable; evidence delegation is disabled: {}",
                bounded_reason(&readiness.reasons.join("; "))
            );
        }
        let started = Instant::now();
        let started_at = Utc::now().to_rfc3339();
        let isolation = if isolated {
            Some(IsolationSession::new(&self.paths)?)
        } else {
            None
        };
        let request_digest = request_digest(request)?;
        let isolation_backend = if isolated {
            readiness.evidence_backend.clone().unwrap_or_default()
        } else {
            "host_unisolated".to_string()
        };
        let mut results = Vec::new();
        let mut overall = MechanisticReceiptStatusV1::Completed;
        for (index, action) in request.actions.iter().enumerate() {
            let action_started = Instant::now();
            let cache_key = evidence_cache_key(
                &self.paths.root,
                action,
                &request.expected_revision,
                &isolation_backend,
            )?;
            if let Some(cached) = isolated
                .then(|| {
                    evidence_cache()
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .get(&cache_key)
                        .cloned()
                })
                .flatten()
            {
                results.push(MechanisticActionResultV1 {
                    action_index: u8::try_from(index).unwrap_or(u8::MAX),
                    category: "evidence".to_string(),
                    status: MechanisticReceiptStatusV1::Completed,
                    summary: format!(
                        "reused prior isolated passing evidence; avoided {} ms",
                        cached.avoided_duration_millis
                    ),
                    entries: cached.selected_recipes.clone(),
                    truncated: false,
                    duration_millis: 0,
                    decision: Some(MechanisticDecisionV1 {
                        confidence: cached.confidence,
                        selected_paths: cached.selected_paths,
                        selected_recipes: cached.selected_recipes,
                        reason_codes: cached.reason_codes,
                        cache_status: MechanisticCacheStatusV1::Hit,
                        source_receipt_digest: Some(cached.source_receipt_digest),
                        isolation_backend: Some(isolation_backend.clone()),
                        dependency_selection: cached.dependency_selection,
                        failure_diagnosis: None,
                    }),
                });
                continue;
            }
            let (commands, selected_paths, confidence, mut reason_codes, dependency_selection) =
                match action {
                    MechanisticActionV1::Evidence {
                        recipe,
                        cwd,
                        timeout_seconds,
                    } => (
                        vec![evidence_command(recipe, cwd.clone(), *timeout_seconds)?],
                        Vec::new(),
                        MechanisticConfidenceV1::High,
                        vec!["explicit_typed_recipe".to_string()],
                        None,
                    ),
                    MechanisticActionV1::VerifyChangedFiles {
                        paths,
                        scope,
                        allow_full_escalation,
                        timeout_seconds,
                        rust_api_baseline_ids,
                    } => verification_commands_for_changed_files(
                        &self.paths,
                        paths,
                        *scope,
                        *allow_full_escalation,
                        VerificationPlanningContext {
                            timeout_seconds: *timeout_seconds,
                            rust_api_baseline_ids,
                            isolation: isolation.as_ref(),
                            cancellation,
                        },
                    )?,
                    _ => unreachable!("evidence-only request checked above"),
                };
            let selected_recipes = commands
                .iter()
                .map(VerificationCommand::display)
                .collect::<Vec<_>>();
            let mut evidence = Vec::new();
            let mut passed = true;
            for command in commands {
                let result = if let Some(isolation) = isolation.as_ref() {
                    run_isolated_structured_command_in_session_with_cancellation(
                        &self.paths,
                        &command,
                        isolation,
                        cancellation,
                    )?
                } else {
                    run_deterministic_command_with_cancellation(
                        &self.paths,
                        &command,
                        cancellation,
                    )?
                };
                passed &= result.category.is_success_like();
                let mut summary = result.evidence.clone();
                if !result.category.is_success_like() {
                    let diagnostic =
                        concise_failure_diagnostic(&result.stdout_tail, &result.stderr_tail);
                    if !diagnostic.is_empty() {
                        summary.push_str("; diagnostic: ");
                        summary.push_str(&diagnostic);
                    }
                }
                evidence.push(bounded_reason(&summary));
                if !passed {
                    break;
                }
            }
            if !passed {
                overall = MechanisticReceiptStatusV1::Failed;
            } else if !isolated {
                overall = MechanisticReceiptStatusV1::Unavailable;
                reason_codes.push("host_unisolated_advisory".to_string());
            }
            let action_result = MechanisticActionResultV1 {
                action_index: u8::try_from(index).unwrap_or(u8::MAX),
                category: "evidence".to_string(),
                status: if passed && isolated {
                    MechanisticReceiptStatusV1::Completed
                } else if passed {
                    MechanisticReceiptStatusV1::Unavailable
                } else {
                    MechanisticReceiptStatusV1::Failed
                },
                summary: bounded_reason(&if passed && !isolated {
                    format!(
                        "advisory host-unisolated pass; network denial is not claimed; {}",
                        evidence.join("; ")
                    )
                } else {
                    evidence.join("; ")
                }),
                entries: selected_recipes.clone(),
                truncated: false,
                duration_millis: action_started.elapsed().as_millis() as u64,
                decision: Some(MechanisticDecisionV1 {
                    confidence,
                    selected_paths,
                    selected_recipes: selected_recipes.clone(),
                    reason_codes,
                    cache_status: MechanisticCacheStatusV1::Miss,
                    source_receipt_digest: None,
                    isolation_backend: Some(isolation_backend.clone()),
                    dependency_selection: dependency_selection.clone(),
                    failure_diagnosis: None,
                }),
            };
            if passed && isolated {
                let source_receipt_digest =
                    evidence_result_digest(&request.expected_revision, action, &action_result)?;
                evidence_cache()
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .insert(
                        cache_key,
                        CachedEvidence {
                            request_digest: request_digest.clone(),
                            source_receipt_digest,
                            selected_paths: action_result
                                .decision
                                .as_ref()
                                .map(|decision| decision.selected_paths.clone())
                                .unwrap_or_default(),
                            selected_recipes: selected_recipes.clone(),
                            confidence,
                            reason_codes: action_result
                                .decision
                                .as_ref()
                                .map(|decision| decision.reason_codes.clone())
                                .unwrap_or_default(),
                            dependency_selection,
                            avoided_duration_millis: action_result.duration_millis,
                        },
                    );
            }
            results.push(action_result);
            if !passed {
                break;
            }
        }
        self.require_expected_revision(&request.expected_revision)?;
        Ok(MechanisticReceiptV1 {
            schema: MECHANISTIC_RECEIPT_SCHEMA.to_string(),
            request_digest,
            repository_revision: request.expected_revision.clone(),
            status: overall,
            readiness,
            action_results: results,
            output_bytes: 0,
            started_at,
            finished_at: Utc::now().to_rfc3339(),
            elapsed_millis: started.elapsed().as_millis() as u64,
            fallback_reason: None,
            broker: None,
            capsule: None,
        })
    }

    fn require_expected_revision(&self, expected: &str) -> Result<()> {
        validate_digest(expected)?;
        let current = repository_revision(&self.paths)?;
        if current != expected {
            bail!("repository revision is stale; refresh status before delegation");
        }
        Ok(())
    }
}

pub(crate) fn validate_request(request: &MechanisticRequestV1) -> Result<()> {
    ca_automation_protocol::validate_request_structure(request)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    for action in &request.actions {
        validate_action_contract(action)?;
    }
    Ok(())
}

pub(crate) fn validate_action_contract(action: &MechanisticActionV1) -> Result<()> {
    ca_automation_protocol::validate_action_structure(action)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    match action {
        MechanisticActionV1::ReadTrackedFile {
            path,
            start_line,
            max_lines,
        } => {
            validate_contract_path(path)?;
            if *start_line == 0 || *max_lines == 0 || *max_lines > 500 {
                bail!("tracked-file read windows must contain 1-500 lines");
            }
        }
        MechanisticActionV1::SearchText {
            pattern,
            match_mode,
            paths,
            max_matches,
        } => {
            if pattern.is_empty() || pattern.len() > 512 || pattern.chars().any(char::is_control) {
                bail!("search patterns must contain 1-512 non-control bytes");
            }
            if paths.len() > 512 || *max_matches == 0 || *max_matches > 512 {
                bail!("search action exceeds its path or match bound");
            }
            if *match_mode == super::SearchMatchModeV1::Regex {
                regex::Regex::new(pattern).context("search pattern is not a valid Rust regex")?;
            }
            for path in paths {
                validate_contract_path(path)?;
            }
        }
        MechanisticActionV1::ListTrackedFiles {
            prefix,
            max_entries,
        } => {
            if let Some(prefix) = prefix {
                validate_contract_path(prefix)?;
            }
            validate_contract_entry_limit(*max_entries)?;
        }
        MechanisticActionV1::ListTrackedDirectory { path, max_entries } => {
            if let Some(path) = path {
                validate_contract_path(path)?;
            }
            validate_contract_entry_limit(*max_entries)?;
        }
        MechanisticActionV1::GlobTrackedFiles {
            prefix,
            max_entries,
            ..
        } => {
            if let Some(prefix) = prefix {
                validate_contract_path(prefix)?;
            }
            validate_contract_entry_limit(*max_entries)?;
        }
        MechanisticActionV1::GitStatusSummary { max_entries }
        | MechanisticActionV1::GitDiffSummary { max_entries } => {
            validate_contract_entry_limit(*max_entries)?;
        }
        MechanisticActionV1::GitHistory { max_entries } => {
            if *max_entries == 0 || *max_entries > 50 {
                bail!("history entry limit must be between 1 and 50");
            }
        }
        MechanisticActionV1::InspectSymbol { paths, .. }
        | MechanisticActionV1::InspectFeature { paths, .. } => {
            for path in paths {
                validate_contract_path(path)?;
            }
        }
        MechanisticActionV1::SummarizeChangedArea { max_entries } => {
            validate_contract_entry_limit(*max_entries)?;
        }
        MechanisticActionV1::FindRelevantTests { paths, max_entries } => {
            validate_contract_entry_limit(*max_entries)?;
            for path in paths {
                validate_contract_path(path)?;
            }
        }
        MechanisticActionV1::DiagnoseFailedRecipe { .. } => {}
        MechanisticActionV1::VerifyChangedFiles { paths, .. } => {
            for path in paths {
                validate_contract_path(path)?;
            }
        }
        MechanisticActionV1::Evidence {
            recipe,
            cwd,
            timeout_seconds,
        } => {
            let _ = evidence_command(recipe, cwd.clone(), *timeout_seconds)?;
        }
        MechanisticActionV1::ObserveProcess {
            label,
            pid,
            interval_seconds,
        } => validate_observation_contract(label, *pid, *interval_seconds)?,
        MechanisticActionV1::ObserveCopy {
            label,
            pid,
            log_path,
            completion_marker,
            interval_seconds,
        } => {
            validate_observation_contract(label, *pid, *interval_seconds)?;
            if let Some(log_path) = log_path {
                validate_contract_path(log_path)?;
            }
            validate_contract_path(completion_marker)?;
        }
    }
    Ok(())
}

fn validate_contract_entry_limit(limit: u16) -> Result<()> {
    if limit == 0 || limit > 512 {
        bail!("query entry limit must be between 1 and 512");
    }
    Ok(())
}

fn validate_observation_contract(label: &str, pid: u32, interval_seconds: u64) -> Result<()> {
    if label.is_empty()
        || label.len() > 128
        || label.chars().any(char::is_control)
        || pid == 0
        || !(2..=300).contains(&interval_seconds)
    {
        bail!("local observation metadata is outside its bounded contract");
    }
    Ok(())
}

fn validate_contract_path(value: &str) -> Result<()> {
    super::query::validate_relative_path(value)?;
    super::query::require_safe_query_path(value)
}

pub(crate) fn request_digest(request: &MechanisticRequestV1) -> Result<String> {
    let bytes = serde_json::to_vec(request).context("serialize mechanistic request")?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn evidence_command(
    recipe: &EvidenceRecipeV1,
    cwd: Option<String>,
    timeout_seconds: Option<u64>,
) -> Result<VerificationCommand> {
    if timeout_seconds.is_some_and(|timeout| timeout == 0 || timeout > 3_600) {
        bail!("evidence timeout must be between 1 and 3600 seconds");
    }
    let recipe = match recipe {
        EvidenceRecipeV1::CargoCheck => Recipe::CargoCheck,
        EvidenceRecipeV1::CargoBuild => Recipe::CargoBuild,
        EvidenceRecipeV1::CargoTest { filter: None } => Recipe::CargoTestAll,
        EvidenceRecipeV1::CargoTest {
            filter: Some(filter),
        } => Recipe::CargoTestNamed {
            filter: filter.clone(),
        },
        EvidenceRecipeV1::CargoClippy => Recipe::CargoClippy,
        EvidenceRecipeV1::CargoFmtCheck => Recipe::CargoFmtCheck,
        EvidenceRecipeV1::NpmTest => Recipe::NpmTest,
        EvidenceRecipeV1::NpmRunTest => Recipe::NpmRunTest,
        EvidenceRecipeV1::NpmRunBuild => Recipe::NpmRunBuild,
        EvidenceRecipeV1::PnpmTest => Recipe::PnpmTest,
        EvidenceRecipeV1::PnpmBuild => Recipe::PnpmBuild,
        EvidenceRecipeV1::YarnTest => Recipe::YarnTest,
        EvidenceRecipeV1::YarnBuild => Recipe::YarnBuild,
        EvidenceRecipeV1::Pytest { args } => Recipe::Pytest { args: args.clone() },
        EvidenceRecipeV1::PythonPytest { args } => Recipe::PythonPytest { args: args.clone() },
        EvidenceRecipeV1::SwiftBuild { product } => Recipe::SwiftBuild {
            product: product.clone(),
        },
        EvidenceRecipeV1::SwiftReleaseBuild { product } => Recipe::SwiftReleaseBuild {
            product: product.clone(),
        },
        EvidenceRecipeV1::SwiftTest { filter } => Recipe::SwiftTest {
            filter: filter.clone(),
        },
        EvidenceRecipeV1::SwiftVersion => Recipe::SwiftVersion,
        EvidenceRecipeV1::LocalSmoke { binary, args } => Recipe::LocalSmoke {
            binary: binary.clone(),
            args: args.clone(),
        },
        EvidenceRecipeV1::Builtin { check } => {
            if cwd.is_some() {
                bail!("built-in repository checks must run from the pinned repository root");
            }
            return builtin_command(*check, timeout_seconds);
        }
    };
    let spec = RecipeSpec {
        id: "mechanistic-evidence".to_string(),
        recipe,
        cwd,
        timeout_seconds,
        kind: CommandKind::Unknown,
        failure_policy: FailurePolicy::Stop,
        cache_policy: CachePolicy::Never,
        expected_exit: Some(0),
        source: "mechanistic.v1".to_string(),
    };
    if !validate_recipe_spec(&spec) {
        bail!("evidence recipe is outside the typed registry");
    }
    let command = recipe_spec_to_command(spec);
    if !is_deterministic_command_structured_allowed(&command) {
        bail!("evidence recipe is outside the final typed allowlist");
    }
    Ok(command)
}

fn builtin_command(
    check: BuiltinRepositoryCheckV1,
    timeout_seconds: Option<u64>,
) -> Result<VerificationCommand> {
    let mut command = VerificationCommand::new(
        "codex-team".to_string(),
        vec!["verify".to_string(), check.as_str().to_string()],
        "mechanistic.v1",
    );
    command.id = "mechanistic-builtin".to_string();
    command.kind = CommandKind::Lint;
    command.timeout_seconds = timeout_seconds;
    command.expected_exit = Some(0);
    if !is_deterministic_command_structured_allowed(&command) {
        bail!("built-in evidence check is outside the final typed allowlist");
    }
    Ok(command)
}

type VerificationSelection = (
    Vec<VerificationCommand>,
    Vec<String>,
    MechanisticConfidenceV1,
    Vec<String>,
    Option<MechanisticDependencySelectionV1>,
);

#[derive(Clone, Copy)]
struct VerificationPlanningContext<'a> {
    timeout_seconds: Option<u64>,
    rust_api_baseline_ids: &'a [String],
    isolation: Option<&'a IsolationSession>,
    cancellation: &'a CancellationToken,
}

fn verification_commands_for_changed_files(
    repository_paths: &Paths,
    changed_paths: &[String],
    requested_scope: VerificationScopeV1,
    allow_full_escalation: bool,
    context: VerificationPlanningContext<'_>,
) -> Result<VerificationSelection> {
    let VerificationPlanningContext {
        timeout_seconds,
        rust_api_baseline_ids,
        isolation,
        cancellation,
    } = context;
    let repo = &repository_paths.root;
    let mut selected_paths = changed_paths.to_vec();
    selected_paths.sort();
    selected_paths.dedup();
    let full = requested_scope == VerificationScopeV1::Full
        || selected_paths
            .iter()
            .any(|path| is_cross_cutting_path(path));
    let tracked = super::query::tracked_files(repo)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    for path in &selected_paths {
        if !tracked.contains(path) {
            bail!("changed-file verification path is not tracked: {path}");
        }
    }
    let mut commands = Vec::new();
    let mut dependency_selection = None;
    let mut reason_codes = Vec::new();
    if full {
        if requested_scope == VerificationScopeV1::Focused && !allow_full_escalation {
            bail!(
                "cross-cutting changed paths require a full suite, but policy forbids escalation"
            );
        }
        append_full_suite_commands(repo, &tracked, timeout_seconds, &mut commands)?;
        reason_codes.push(if requested_scope == VerificationScopeV1::Full {
            "full_scope_requested".to_string()
        } else {
            "cross_cutting_input".to_string()
        });
    } else if selected_paths
        .iter()
        .all(|path| is_documentation_path(path))
    {
        commands.push(command_from_recipe(
            Recipe::GitDiffCheck,
            None,
            timeout_seconds,
            "mechanistic-focused-diff",
        )?);
        reason_codes.push("documentation_only_diff_check".to_string());
    } else {
        let graph_result = isolation
            .context("production isolation is required for dependency graph planning")
            .and_then(|isolation| {
                select_dependencies(
                    repository_paths,
                    &tracked,
                    &selected_paths,
                    isolation,
                    cancellation,
                )
            });
        match graph_result {
            Ok(mut selection) => {
                let rust_api_impact = if selection.ecosystems.as_slice() == ["rust"]
                    && !rust_api_baseline_ids.is_empty()
                {
                    isolation.map(|isolation| {
                        super::compare_rust_api(
                            repo,
                            &selected_paths,
                            rust_api_baseline_ids,
                            isolation,
                            cancellation,
                        )
                    })
                } else {
                    None
                };
                if rust_api_impact.as_ref().is_some_and(|impact| {
                    impact.status == ca_automation_protocol::RustApiImpactStatusV1::Unchanged
                }) {
                    let owners = selection
                        .owning_packages
                        .iter()
                        .cloned()
                        .collect::<BTreeSet<_>>();
                    selection
                        .packages
                        .retain(|package| owners.contains(&package.name));
                    selection.reverse_dependencies.clear();
                    selection
                        .reason_codes
                        .push("rust_api_unchanged_reverse_dependencies_skipped".to_string());
                }
                append_dependency_commands(
                    repo,
                    &selection,
                    &selected_paths,
                    timeout_seconds,
                    &mut commands,
                )?;
                if commands.len() > 8 {
                    if !allow_full_escalation {
                        bail!("dependency verification fan-out requires a full suite, but policy forbids escalation");
                    }
                    commands.clear();
                    append_full_suite_commands(repo, &tracked, timeout_seconds, &mut commands)?;
                    reason_codes.push("dependency_command_limit_escalated_full".to_string());
                } else {
                    reason_codes.extend(selection.reason_codes.clone());
                    let mut metadata =
                        dependency_metadata(&selection, VerificationScopeV1::Focused);
                    metadata.rust_api_impact = rust_api_impact;
                    dependency_selection = Some(metadata);
                }
            }
            Err(_) => {
                if !allow_full_escalation {
                    bail!("dependency graph generation failed and policy forbids the required full suite");
                }
                append_full_suite_commands(repo, &tracked, timeout_seconds, &mut commands)?;
                reason_codes.push("dependency_graph_unavailable_escalated_full".to_string());
            }
        }
    }
    commands.truncate(8);
    if commands.is_empty() {
        bail!("no typed verification recipe matches the changed-file ecosystem");
    }
    Ok((
        commands,
        selected_paths,
        if full || dependency_selection.is_none() {
            MechanisticConfidenceV1::High
        } else {
            dependency_selection
                .as_ref()
                .map(|_| MechanisticConfidenceV1::High)
                .unwrap_or(MechanisticConfidenceV1::Medium)
        },
        reason_codes,
        dependency_selection,
    ))
}

fn dependency_metadata(
    selection: &DependencySelection,
    scope: VerificationScopeV1,
) -> MechanisticDependencySelectionV1 {
    MechanisticDependencySelectionV1 {
        scope,
        ecosystems: selection.ecosystems.clone(),
        owning_packages: selection.owning_packages.clone(),
        reverse_dependencies: selection.reverse_dependencies.clone(),
        selected_targets: selection.selected_targets.clone(),
        graph_cache_status: selection.cache_status,
        reason_codes: selection.reason_codes.clone(),
        rust_api_impact: None,
    }
}

fn append_dependency_commands(
    repo: &Path,
    selection: &DependencySelection,
    changed_paths: &[String],
    timeout_seconds: Option<u64>,
    commands: &mut Vec<VerificationCommand>,
) -> Result<()> {
    let owner_names = selection
        .owning_packages
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let test_filter = changed_paths
        .iter()
        .filter_map(|path| safe_test_filter_from_path(path))
        .next();
    for package in &selection.packages {
        match package.ecosystem {
            Ecosystem::Rust => {
                commands.push(command_from_recipe(
                    Recipe::CargoCheckPackage {
                        package: package.name.clone(),
                    },
                    None,
                    timeout_seconds,
                    "mechanistic-graph-cargo-check",
                )?);
                if owner_names.contains(&package.name) {
                    commands.push(command_from_recipe(
                        Recipe::CargoTestPackage {
                            package: package.name.clone(),
                            filter: test_filter.clone(),
                        },
                        None,
                        timeout_seconds,
                        "mechanistic-graph-cargo-test",
                    )?);
                }
            }
            Ecosystem::Node => {
                let recipe = if package.root.is_empty() {
                    node_test_recipe(repo)
                } else if repo.join("pnpm-lock.yaml").is_file() {
                    Recipe::PnpmFilterTest {
                        package: package.name.clone(),
                    }
                } else if repo.join("yarn.lock").is_file() {
                    Recipe::YarnWorkspaceTest {
                        package: package.name.clone(),
                    }
                } else {
                    Recipe::NpmWorkspaceTest {
                        package: package.name.clone(),
                    }
                };
                commands.push(command_from_recipe(
                    recipe,
                    None,
                    timeout_seconds,
                    "mechanistic-graph-node-test",
                )?);
            }
            Ecosystem::Python => {
                let args = if selection.relevant_test_paths.is_empty() {
                    package.test_paths.clone()
                } else {
                    selection.relevant_test_paths.clone()
                };
                commands.push(command_from_recipe(
                    Recipe::Pytest { args },
                    None,
                    timeout_seconds,
                    "mechanistic-graph-pytest",
                )?);
            }
            Ecosystem::Swift => {
                commands.push(command_from_recipe(
                    Recipe::SwiftTest {
                        filter: package.targets.first().cloned(),
                    },
                    (!package.root.is_empty()).then(|| package.root.clone()),
                    timeout_seconds,
                    "mechanistic-graph-swift-test",
                )?);
            }
        }
    }
    Ok(())
}

fn append_full_suite_commands(
    repo: &Path,
    tracked: &BTreeSet<String>,
    timeout_seconds: Option<u64>,
    commands: &mut Vec<VerificationCommand>,
) -> Result<()> {
    if tracked.contains("Cargo.toml") {
        commands.push(command_from_recipe(
            Recipe::CargoTestAll,
            None,
            timeout_seconds,
            "mechanistic-full-cargo-test",
        )?);
    }
    if tracked.contains("Package.swift") {
        commands.push(command_from_recipe(
            Recipe::SwiftTest { filter: None },
            None,
            timeout_seconds,
            "mechanistic-full-swift-test",
        )?);
    }
    if tracked.contains("package.json") {
        commands.push(command_from_recipe(
            node_test_recipe(repo),
            None,
            timeout_seconds,
            "mechanistic-full-node-test",
        )?);
    }
    if tracked.contains("pyproject.toml")
        || tracked.contains("pytest.ini")
        || tracked.iter().any(|path| path.ends_with("_test.py"))
    {
        commands.push(command_from_recipe(
            Recipe::Pytest { args: Vec::new() },
            None,
            timeout_seconds,
            "mechanistic-full-pytest",
        )?);
    }
    Ok(())
}

fn command_from_recipe(
    recipe: Recipe,
    cwd: Option<String>,
    timeout_seconds: Option<u64>,
    id: &str,
) -> Result<VerificationCommand> {
    let spec = RecipeSpec {
        id: id.to_string(),
        recipe,
        cwd,
        timeout_seconds,
        kind: CommandKind::Unknown,
        failure_policy: FailurePolicy::Stop,
        cache_policy: CachePolicy::Never,
        expected_exit: Some(0),
        source: "mechanistic.changed_files.v1".to_string(),
    };
    if !validate_recipe_spec(&spec) {
        bail!("changed-file recipe is outside the typed registry");
    }
    let command = recipe_spec_to_command(spec);
    if !is_deterministic_command_structured_allowed(&command) {
        bail!("changed-file recipe is outside the final typed allowlist");
    }
    Ok(command)
}

fn safe_test_filter_from_path(path: &str) -> Option<String> {
    let stem = Path::new(path).file_stem()?.to_str()?;
    let filter = stem
        .trim_end_matches("Tests")
        .trim_end_matches("_tests")
        .trim_end_matches("_test");
    (!filter.is_empty()
        && filter.len() <= 128
        && filter
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | ':' | '.')))
    .then(|| filter.to_string())
}

fn node_test_recipe(repo: &Path) -> Recipe {
    if repo.join("pnpm-lock.yaml").is_file() {
        Recipe::PnpmTest
    } else if repo.join("yarn.lock").is_file() {
        Recipe::YarnTest
    } else {
        Recipe::NpmTest
    }
}

fn is_cross_cutting_path(path: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "cargo.toml"
            | "cargo.lock"
            | "build.rs"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "package.swift"
            | "package.resolved"
            | "pyproject.toml"
    )
}

fn is_documentation_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".md") || lower.starts_with("docs/") || lower.starts_with(".agent/")
}

fn evidence_cache_key(
    repo: &Path,
    action: &MechanisticActionV1,
    revision: &str,
    isolation_backend: &str,
) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"codex-team-mechanistic-evidence-cache-v1\0");
    hasher.update(revision.as_bytes());
    hasher.update(b"\0");
    hasher.update(isolation_backend.as_bytes());
    hasher.update(b"\0");
    hasher.update(std::env::consts::OS.as_bytes());
    hasher.update(b"\0");
    hasher.update(std::env::consts::ARCH.as_bytes());
    hasher.update(b"\0");
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(b"\0");
    for name in [
        "PATH",
        "RUSTUP_TOOLCHAIN",
        "RUSTC_WRAPPER",
        "CARGO_HOME",
        "NODE_OPTIONS",
        "VIRTUAL_ENV",
        "SWIFT_EXEC",
    ] {
        hasher.update(name.as_bytes());
        hasher.update(b"=");
        if let Some(value) = std::env::var_os(name) {
            hasher.update(value.to_string_lossy().as_bytes());
        }
        hasher.update(b"\0");
    }
    hasher.update(serde_json::to_vec(action).context("serialize evidence cache action")?);
    if let MechanisticActionV1::VerifyChangedFiles { paths, .. } = action {
        let tracked = super::query::tracked_files(repo)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        for relative in paths {
            if !tracked.contains(relative) {
                bail!("changed-file verification path is not tracked: {relative}");
            }
            let path = repo.join(relative);
            hasher.update(b"\0input\0");
            hasher.update(relative.as_bytes());
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() || !metadata.is_file() {
                        bail!("changed-file verification inputs must be regular files");
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        if metadata.nlink() != 1 {
                            bail!("changed-file verification inputs must not be hardlinked");
                        }
                    }
                    let mut file = std::fs::File::open(&path)?;
                    let mut input_hasher = Sha256::new();
                    let mut buffer = [0_u8; 64 * 1024];
                    loop {
                        let count = std::io::Read::read(&mut file, &mut buffer)?;
                        if count == 0 {
                            break;
                        }
                        input_hasher.update(&buffer[..count]);
                    }
                    hasher.update(input_hasher.finalize());
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    hasher.update(b"deleted");
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn evidence_result_digest(
    revision: &str,
    action: &MechanisticActionV1,
    result: &MechanisticActionResultV1,
) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"codex-team-mechanistic-evidence-result-v1\0");
    hasher.update(revision.as_bytes());
    hasher.update(b"\0");
    hasher.update(serde_json::to_vec(action)?);
    hasher.update(b"\0");
    hasher.update(serde_json::to_vec(result)?);
    Ok(format!("{:x}", hasher.finalize()))
}

fn validate_digest(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("expected_revision must be one lowercase SHA-256 digest");
    }
    Ok(())
}

fn bounded_reason(value: &str) -> String {
    value.chars().take(4_096).collect()
}

fn concise_failure_diagnostic(stdout: &str, stderr: &str) -> String {
    let combined = format!("{stderr}\n{stdout}");
    let lines = combined
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("error")
                || lower.contains("failed")
                || lower.contains("warning")
                || lower.contains("panicked")
                || lower.contains("timed out")
        })
        .take(8)
        .map(super::query::redact_entry)
        .collect::<Vec<_>>();
    bounded_reason(&lines.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ca_automation_protocol::MECHANISTIC_REQUEST_SCHEMA;
    use std::fs;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn planning_fixture() -> Result<PathBuf> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let repo = std::env::temp_dir().join(format!("codex-team-verify-plan-{nonce}"));
        fs::create_dir_all(repo.join("src"))?;
        assert!(Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()?
            .success());
        fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n",
        )?;
        fs::write(repo.join("src/lib.rs"), "pub mod input;\n")?;
        fs::write(repo.join("src/input.rs"), "pub fn left() {}\n")?;
        assert!(Command::new("cargo")
            .arg("generate-lockfile")
            .current_dir(&repo)
            .status()?
            .success());
        assert!(Command::new("git")
            .args([
                "add",
                "Cargo.toml",
                "Cargo.lock",
                "src/lib.rs",
                "src/input.rs",
            ])
            .current_dir(&repo)
            .status()?
            .success());
        Ok(repo)
    }

    #[test]
    fn request_contract_is_closed_versioned_and_bounded() {
        let parsed: MechanisticRequestV1 = serde_json::from_value(serde_json::json!({
            "schema": MECHANISTIC_REQUEST_SCHEMA,
            "expected_revision": "a".repeat(64),
            "actions": [{"kind": "git_history", "max_entries": 2}]
        }))
        .unwrap();
        assert_eq!(
            parsed.delegation_policy,
            super::super::DelegationPolicy::Legacy
        );
        assert!(validate_request(&parsed).is_ok());
        assert!(
            serde_json::from_value::<MechanisticRequestV1>(serde_json::json!({
                "schema": MECHANISTIC_REQUEST_SCHEMA,
                "expected_revision": "a".repeat(64),
                "actions": [{"kind": "git_history", "max_entries": 2}],
                "extra": true
            }))
            .is_err()
        );

        let mut over_bound = parsed.clone();
        over_bound.actions = vec![MechanisticActionV1::GitHistory { max_entries: 1 }; 9];
        assert!(validate_request(&over_bound).is_err());
    }

    #[test]
    fn every_typed_evidence_recipe_reaches_the_shared_registry() {
        let recipes = vec![
            EvidenceRecipeV1::CargoCheck,
            EvidenceRecipeV1::CargoBuild,
            EvidenceRecipeV1::CargoTest { filter: None },
            EvidenceRecipeV1::CargoTest {
                filter: Some("focused_test".to_string()),
            },
            EvidenceRecipeV1::CargoClippy,
            EvidenceRecipeV1::CargoFmtCheck,
            EvidenceRecipeV1::NpmTest,
            EvidenceRecipeV1::NpmRunTest,
            EvidenceRecipeV1::NpmRunBuild,
            EvidenceRecipeV1::PnpmTest,
            EvidenceRecipeV1::PnpmBuild,
            EvidenceRecipeV1::YarnTest,
            EvidenceRecipeV1::YarnBuild,
            EvidenceRecipeV1::Pytest {
                args: vec!["tests/test_unit.py".to_string()],
            },
            EvidenceRecipeV1::PythonPytest {
                args: vec!["tests/test_unit.py".to_string()],
            },
            EvidenceRecipeV1::SwiftBuild { product: None },
            EvidenceRecipeV1::SwiftBuild {
                product: Some("Example".to_string()),
            },
            EvidenceRecipeV1::SwiftReleaseBuild {
                product: "SwiftStackerBench".to_string(),
            },
            EvidenceRecipeV1::SwiftTest { filter: None },
            EvidenceRecipeV1::SwiftTest {
                filter: Some("ExampleTests/testOne".to_string()),
            },
            EvidenceRecipeV1::SwiftVersion,
            EvidenceRecipeV1::LocalSmoke {
                binary: "example".to_string(),
                args: vec!["--local-check".to_string()],
            },
            EvidenceRecipeV1::Builtin {
                check: BuiltinRepositoryCheckV1::SnapshotHygiene,
            },
            EvidenceRecipeV1::Builtin {
                check: BuiltinRepositoryCheckV1::ProtectedTrees,
            },
        ];
        for recipe in recipes {
            let action = MechanisticActionV1::Evidence {
                recipe,
                cwd: None,
                timeout_seconds: Some(60),
            };
            assert!(validate_action_contract(&action).is_ok(), "{action:?}");
        }

        for action in [
            MechanisticActionV1::Evidence {
                recipe: EvidenceRecipeV1::CargoTest {
                    filter: Some("unit;rm".to_string()),
                },
                cwd: None,
                timeout_seconds: Some(60),
            },
            MechanisticActionV1::Evidence {
                recipe: EvidenceRecipeV1::LocalSmoke {
                    binary: "check|other".to_string(),
                    args: vec!["--local-check".to_string()],
                },
                cwd: None,
                timeout_seconds: Some(60),
            },
            MechanisticActionV1::Evidence {
                recipe: EvidenceRecipeV1::CargoCheck,
                cwd: Some("../escape".to_string()),
                timeout_seconds: Some(60),
            },
        ] {
            assert!(validate_action_contract(&action).is_err(), "{action:?}");
        }
    }

    #[test]
    fn changed_file_selection_is_focused_except_for_cross_cutting_inputs() -> Result<()> {
        let repo = planning_fixture()?;
        crate::verification::isolation::enable_test_isolation_backend();
        let repository_paths = Paths::new(&repo)?;
        let isolation = IsolationSession::new(&repository_paths)?;
        let cancellation = CancellationToken::default();
        let planning_context = VerificationPlanningContext {
            timeout_seconds: Some(60),
            rust_api_baseline_ids: &[],
            isolation: Some(&isolation),
            cancellation: &cancellation,
        };
        let (focused, selected, _, reasons, dependency) = verification_commands_for_changed_files(
            &repository_paths,
            &["src/input.rs".into()],
            VerificationScopeV1::Focused,
            true,
            planning_context,
        )?;
        assert_eq!(selected, ["src/input.rs"]);
        assert!(focused
            .iter()
            .any(|command| command.display() == "cargo check -p fixture --locked --offline"));
        assert!(focused
            .iter()
            .any(|command| command.display() == "cargo test -p fixture --locked --offline input"));
        assert_eq!(reasons, ["dependency_graph_transitive_reverse_closure"]);
        assert_eq!(
            dependency.expect("dependency metadata").owning_packages,
            ["fixture"]
        );

        let (full, _, _, reasons, _) = verification_commands_for_changed_files(
            &repository_paths,
            &["Cargo.lock".into()],
            VerificationScopeV1::Focused,
            true,
            planning_context,
        )?;
        assert_eq!(full.len(), 1);
        assert_eq!(full[0].display(), "cargo test");
        assert_eq!(reasons, ["cross_cutting_input"]);
        assert!(verification_commands_for_changed_files(
            &repository_paths,
            &["Cargo.lock".into()],
            VerificationScopeV1::Focused,
            false,
            planning_context,
        )
        .unwrap_err()
        .to_string()
        .contains("policy forbids escalation"));
        assert!(verification_commands_for_changed_files(
            &repository_paths,
            &["untracked.rs".into()],
            VerificationScopeV1::Focused,
            false,
            planning_context,
        )
        .is_err());
        let (unscoped, selected, _, reasons, _) = verification_commands_for_changed_files(
            &repository_paths,
            &[],
            VerificationScopeV1::Full,
            false,
            planning_context,
        )?;
        assert!(selected.is_empty());
        assert_eq!(unscoped[0].display(), "cargo test");
        assert_eq!(reasons, ["full_scope_requested"]);
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn evidence_cache_key_binds_revision_backend_and_action() -> Result<()> {
        let action = MechanisticActionV1::Evidence {
            recipe: EvidenceRecipeV1::CargoCheck,
            cwd: None,
            timeout_seconds: Some(60),
        };
        let repo = planning_fixture()?;
        let first = evidence_cache_key(&repo, &action, &"a".repeat(64), "seatbelt")?;
        assert_ne!(
            first,
            evidence_cache_key(&repo, &action, &"b".repeat(64), "seatbelt")?
        );
        assert_ne!(
            first,
            evidence_cache_key(&repo, &action, &"a".repeat(64), "host_unisolated")?
        );
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn changed_file_cache_key_binds_affected_input_bytes() -> Result<()> {
        let repo = planning_fixture()?;
        let action = MechanisticActionV1::VerifyChangedFiles {
            paths: vec!["src/input.rs".into()],
            scope: VerificationScopeV1::Focused,
            allow_full_escalation: false,
            timeout_seconds: Some(60),
            rust_api_baseline_ids: Vec::new(),
        };
        let revision = "a".repeat(64);
        let before = evidence_cache_key(&repo, &action, &revision, "seatbelt")?;
        fs::write(repo.join("src/input.rs"), "pub fn right() {}\n")?;
        let after = evidence_cache_key(&repo, &action, &revision, "seatbelt")?;
        assert_ne!(before, after);
        fs::remove_dir_all(repo)?;
        Ok(())
    }
}
