use super::{
    FailureCauseV1, MechanisticActionResultV1, MechanisticActionV1, MechanisticCacheStatusV1,
    MechanisticConfidenceV1, MechanisticDecisionV1, MechanisticFailureDiagnosisV1,
    MechanisticReceiptStatusV1, RecommendedRerunV1, SearchMatchModeV1,
};
use crate::mcp::jobs::{JobKind, JobState, JobStore};
use crate::revision::repository_revision;
use anyhow::{bail, Context, Result};
use globset::GlobBuilder;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

const MAX_FILE_INPUT_BYTES: u64 = 1024 * 1024;
const MAX_LINE_WINDOW: u16 = 500;
const MAX_QUERY_ENTRIES: u16 = 512;
const MAX_SEARCH_PATTERN_BYTES: usize = 512;
const MAX_GIT_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_COMPOSITE_SCAN_BYTES: u64 = 64 * 1024 * 1024;
const MAX_COMPOSITE_SCAN_FILES: usize = 20_000;

pub(crate) fn execute_query_action(
    repo: &Path,
    index: usize,
    action: &MechanisticActionV1,
    remaining_bytes: usize,
) -> Result<MechanisticActionResultV1> {
    let started = Instant::now();
    let (summary, mut entries, source_truncated, decision) = match action {
        MechanisticActionV1::ReadTrackedFile {
            path,
            start_line,
            max_lines,
        } => with_no_decision(read_tracked_file(repo, path, *start_line, *max_lines)?),
        MechanisticActionV1::SearchText {
            pattern,
            match_mode,
            paths,
            max_matches,
        } => with_no_decision(search_text(
            repo,
            pattern,
            *match_mode,
            paths,
            *max_matches,
        )?),
        MechanisticActionV1::ListTrackedFiles {
            prefix,
            max_entries,
        } => with_no_decision(list_tracked_files(repo, prefix.as_deref(), *max_entries)?),
        MechanisticActionV1::ListTrackedDirectory { path, max_entries } => {
            with_no_decision(list_tracked_directory(repo, path.as_deref(), *max_entries)?)
        }
        MechanisticActionV1::GlobTrackedFiles {
            pattern,
            prefix,
            max_entries,
        } => with_no_decision(glob_tracked_files(
            repo,
            pattern,
            prefix.as_deref(),
            *max_entries,
        )?),
        MechanisticActionV1::GitStatusSummary { max_entries } => {
            with_no_decision(git_status_summary(repo, *max_entries)?)
        }
        MechanisticActionV1::GitDiffSummary { max_entries } => {
            with_no_decision(git_diff_summary(repo, *max_entries)?)
        }
        MechanisticActionV1::GitHistory { max_entries } => {
            with_no_decision(git_history(repo, *max_entries)?)
        }
        MechanisticActionV1::InspectSymbol {
            symbol,
            paths,
            max_matches,
            context_lines,
        } => inspect_symbol(repo, symbol, paths, *max_matches, *context_lines)?,
        MechanisticActionV1::InspectFeature {
            terms,
            paths,
            max_files,
        } => inspect_feature(repo, terms, paths, *max_files)?,
        MechanisticActionV1::SummarizeChangedArea { max_entries } => {
            summarize_changed_area(repo, *max_entries)?
        }
        MechanisticActionV1::FindRelevantTests { paths, max_entries } => {
            find_relevant_tests(repo, paths, *max_entries)?
        }
        MechanisticActionV1::DiagnoseFailedRecipe { job_id } => {
            diagnose_failed_recipe(repo, job_id)?
        }
        _ => bail!("repo_query accepts repository query actions only"),
    };
    let mut used = 0_usize;
    let mut bounded = Vec::new();
    let mut output_truncated = source_truncated;
    for entry in entries.drain(..) {
        let entry = redact_entry(&entry);
        let needed = entry.len().saturating_add(1);
        if needed > remaining_bytes.saturating_sub(used) {
            output_truncated = true;
            break;
        }
        used = used.saturating_add(needed);
        bounded.push(entry);
    }
    Ok(MechanisticActionResultV1 {
        action_index: u8::try_from(index).unwrap_or(u8::MAX),
        category: "query".to_string(),
        status: MechanisticReceiptStatusV1::Completed,
        summary,
        entries: bounded,
        truncated: output_truncated,
        duration_millis: started.elapsed().as_millis() as u64,
        decision,
    })
}

type QueryTuple = (String, Vec<String>, bool);
type CompositeQueryTuple = (String, Vec<String>, bool, Option<MechanisticDecisionV1>);

fn with_no_decision(value: QueryTuple) -> CompositeQueryTuple {
    (value.0, value.1, value.2, None)
}

fn read_tracked_file(
    repo: &Path,
    relative: &str,
    start_line: u32,
    max_lines: u16,
) -> Result<(String, Vec<String>, bool)> {
    if start_line == 0 || max_lines == 0 || max_lines > MAX_LINE_WINDOW {
        bail!("read window must use start_line >= 1 and 1-{MAX_LINE_WINDOW} lines");
    }
    let (relative, path) = require_tracked_regular_file(repo, relative)?;
    let metadata = fs::metadata(&path)?;
    if metadata.len() > MAX_FILE_INPUT_BYTES {
        bail!("tracked file exceeds the 1 MiB query input limit");
    }
    let bytes = fs::read(&path)?;
    if bytes.contains(&0) {
        bail!("binary tracked files cannot be read through repo_query");
    }
    let text = std::str::from_utf8(&bytes).context("tracked file is not valid UTF-8")?;
    let total_lines = text.lines().count();
    let start = usize::try_from(start_line - 1)?;
    let mut entries = Vec::new();
    let mut total_after_start = 0_usize;
    for (line_index, line) in text.lines().enumerate().skip(start) {
        total_after_start += 1;
        if entries.len() >= usize::from(max_lines) {
            continue;
        }
        entries.push(format!("{}:{}:{}", relative, line_index + 1, line));
    }
    let truncated = total_after_start > entries.len();
    Ok((
        format!(
            "read {} line(s) from a tracked file;total_lines={total_lines};total_bytes={}",
            entries.len(),
            metadata.len()
        ),
        entries,
        truncated,
    ))
}

fn search_text(
    repo: &Path,
    pattern: &str,
    match_mode: SearchMatchModeV1,
    requested_paths: &[String],
    max_matches: u16,
) -> Result<(String, Vec<String>, bool)> {
    if pattern.is_empty()
        || pattern.len() > MAX_SEARCH_PATTERN_BYTES
        || pattern.chars().any(char::is_control)
    {
        bail!("search pattern must contain 1-512 non-control bytes");
    }
    if max_matches == 0 || max_matches > MAX_QUERY_ENTRIES {
        bail!("max_matches must be between 1 and {MAX_QUERY_ENTRIES}");
    }
    let regex = match match_mode {
        SearchMatchModeV1::Literal => None,
        SearchMatchModeV1::Regex => {
            Some(Regex::new(pattern).context("search pattern is not a valid Rust regex")?)
        }
    };
    let tracked = tracked_files(repo)?;
    let candidates = if requested_paths.is_empty() {
        tracked
    } else {
        if requested_paths.len() > usize::from(MAX_QUERY_ENTRIES) {
            bail!("search path list exceeds the query entry limit");
        }
        let mut candidates = BTreeSet::new();
        for requested in requested_paths {
            let scope = validate_relative_path(requested)?;
            require_safe_query_path(&scope)?;
            let mut matched_scope = false;
            for relative in &tracked {
                if relative == &scope
                    || relative
                        .strip_prefix(&scope)
                        .is_some_and(|suffix| suffix.starts_with('/'))
                {
                    matched_scope = true;
                    candidates.insert(relative.clone());
                }
            }
            if !matched_scope {
                bail!("search scope does not match a tracked file or directory prefix: {scope}");
            }
        }
        candidates.into_iter().collect()
    };
    let mut entries = Vec::new();
    let mut matched_more = false;
    for relative in candidates {
        let path = require_regular_file(repo, &relative)?;
        let metadata = fs::metadata(&path)?;
        if metadata.len() > MAX_FILE_INPUT_BYTES {
            continue;
        }
        let bytes = fs::read(&path)?;
        if bytes.contains(&0) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        for (line_index, line) in text.lines().enumerate() {
            let is_match = regex
                .as_ref()
                .map_or_else(|| line.contains(pattern), |regex| regex.is_match(line));
            if !is_match {
                continue;
            }
            if entries.len() >= usize::from(max_matches) {
                matched_more = true;
                break;
            }
            entries.push(format!("{}:{}:{}", relative, line_index + 1, line));
        }
        if matched_more {
            break;
        }
    }
    Ok((
        format!("found {} bounded text match(es)", entries.len()),
        entries,
        matched_more,
    ))
}

fn inspect_symbol(
    repo: &Path,
    symbol: &str,
    requested_paths: &[String],
    max_matches: u16,
    context_lines: u8,
) -> Result<CompositeQueryTuple> {
    let candidates = scoped_tracked_files(repo, requested_paths)?;
    let mut entries = Vec::new();
    let mut selected = BTreeSet::new();
    let mut matches = 0_usize;
    let mut definition_found = false;
    let mut scanned_bytes = 0_u64;
    let mut truncated = false;
    for relative in candidates.into_iter().take(MAX_COMPOSITE_SCAN_FILES) {
        let path = require_regular_file(repo, &relative)?;
        let metadata = fs::metadata(&path)?;
        if metadata.len() > MAX_FILE_INPUT_BYTES {
            continue;
        }
        scanned_bytes = scanned_bytes.saturating_add(metadata.len());
        if scanned_bytes > MAX_COMPOSITE_SCAN_BYTES {
            truncated = true;
            break;
        }
        let bytes = fs::read(&path)?;
        if bytes.contains(&0) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let lines = text.lines().collect::<Vec<_>>();
        for (line_index, line) in lines.iter().enumerate() {
            if !line.contains(symbol) {
                continue;
            }
            if matches >= usize::from(max_matches) {
                truncated = true;
                break;
            }
            matches += 1;
            selected.insert(relative.clone());
            definition_found |= looks_like_definition(line, symbol);
            let radius = usize::from(context_lines);
            let start = line_index.saturating_sub(radius);
            let end = (line_index + radius + 1).min(lines.len());
            for (context_index, context) in lines[start..end].iter().enumerate() {
                entries.push(format!(
                    "{}:{}:{}",
                    relative,
                    start + context_index + 1,
                    context
                ));
            }
        }
        if truncated {
            break;
        }
    }
    let selected_paths = selected.into_iter().collect::<Vec<_>>();
    let confidence = if definition_found {
        MechanisticConfidenceV1::High
    } else if !selected_paths.is_empty() {
        MechanisticConfidenceV1::Medium
    } else {
        MechanisticConfidenceV1::Unknown
    };
    Ok((
        format!(
            "inspected exact symbol `{symbol}` across tracked source and found {matches} match(es)"
        ),
        entries,
        truncated,
        Some(MechanisticDecisionV1 {
            confidence,
            selected_paths,
            selected_recipes: Vec::new(),
            reason_codes: if definition_found {
                vec![
                    "exact_identifier_match".into(),
                    "definition_candidate".into(),
                ]
            } else {
                vec!["exact_identifier_match".into()]
            },
            cache_status: MechanisticCacheStatusV1::NotApplicable,
            source_receipt_digest: None,
            isolation_backend: None,
            dependency_selection: None,
            failure_diagnosis: None,
        }),
    ))
}

fn looks_like_definition(line: &str, symbol: &str) -> bool {
    let compact = line.trim_start();
    [
        "fn ", "struct ", "enum ", "trait ", "type ", "const ", "static ", "mod ", "class ",
        "def ", "func ", "let ", "var ",
    ]
    .iter()
    .any(|prefix| compact.contains(&format!("{prefix}{symbol}")))
        || compact.starts_with(&format!("{symbol}:"))
        || compact.starts_with(&format!("\"{symbol}\""))
}

fn inspect_feature(
    repo: &Path,
    terms: &[String],
    requested_paths: &[String],
    max_files: u16,
) -> Result<CompositeQueryTuple> {
    let candidates = scoped_tracked_files(repo, requested_paths)?;
    let folded_terms = terms
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut ranked = Vec::new();
    let mut scanned_bytes = 0_u64;
    let mut scan_truncated = false;
    for relative in candidates.into_iter().take(MAX_COMPOSITE_SCAN_FILES) {
        let path = require_regular_file(repo, &relative)?;
        let metadata = fs::metadata(&path)?;
        if metadata.len() > MAX_FILE_INPUT_BYTES {
            continue;
        }
        scanned_bytes = scanned_bytes.saturating_add(metadata.len());
        if scanned_bytes > MAX_COMPOSITE_SCAN_BYTES {
            scan_truncated = true;
            break;
        }
        let bytes = fs::read(&path)?;
        if bytes.contains(&0) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let folded_path = relative.to_ascii_lowercase();
        let mut score = folded_terms
            .iter()
            .filter(|term| folded_path.contains(term.as_str()))
            .count()
            * 8;
        let mut best_line = None;
        let mut best_line_score = 0_usize;
        for (line_index, line) in text.lines().enumerate() {
            let folded = line.to_ascii_lowercase();
            let line_score = folded_terms
                .iter()
                .filter(|term| folded.contains(term.as_str()))
                .count();
            if line_score > best_line_score {
                best_line_score = line_score;
                best_line = Some((line_index + 1, line.to_string()));
            }
        }
        score = score.saturating_add(best_line_score * 3);
        if score > 0 {
            ranked.push((score, relative, best_line));
        }
    }
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    let truncated = scan_truncated || ranked.len() > usize::from(max_files);
    ranked.truncate(usize::from(max_files));
    let selected_paths = ranked
        .iter()
        .map(|(_, path, _)| path.clone())
        .collect::<Vec<_>>();
    let selected_recipes = test_recipes_for_paths(&selected_paths);
    let entries = ranked
        .iter()
        .map(|(score, path, line)| match line {
            Some((line_number, text)) => format!("{path}:{line_number}:score={score}:{text}"),
            None => format!("{path}:score={score}"),
        })
        .collect::<Vec<_>>();
    let top_score = ranked.first().map(|entry| entry.0).unwrap_or_default();
    let confidence = if top_score >= folded_terms.len().saturating_mul(3).max(6) {
        MechanisticConfidenceV1::High
    } else if top_score > 0 {
        MechanisticConfidenceV1::Medium
    } else {
        MechanisticConfidenceV1::Unknown
    };
    Ok((
        format!(
            "ranked {} tracked feature file(s) using {} lexical term(s)",
            selected_paths.len(),
            terms.len()
        ),
        entries,
        truncated,
        Some(MechanisticDecisionV1 {
            confidence,
            selected_paths,
            selected_recipes,
            reason_codes: vec!["bounded_lexical_ranking".into()],
            cache_status: MechanisticCacheStatusV1::NotApplicable,
            source_receipt_digest: None,
            isolation_backend: None,
            dependency_selection: None,
            failure_diagnosis: None,
        }),
    ))
}

fn summarize_changed_area(repo: &Path, max_entries: u16) -> Result<CompositeQueryTuple> {
    validate_entry_limit(max_entries)?;
    let (status_summary, mut status, status_truncated) = git_status_summary(repo, max_entries)?;
    let remaining = max_entries.saturating_sub(u16::try_from(status.len()).unwrap_or(max_entries));
    let (diff_summary, mut diff, diff_truncated) = if remaining == 0 {
        (String::new(), Vec::new(), true)
    } else {
        git_diff_summary(repo, remaining)?
    };
    let mut selected = BTreeSet::new();
    for entry in status.iter().chain(diff.iter()) {
        if let Some(path) = entry.split_whitespace().last() {
            if validate_relative_path(path).is_ok() && require_safe_query_path(path).is_ok() {
                selected.insert(path.to_string());
            }
        }
    }
    status.append(&mut diff);
    let selected_paths = selected.into_iter().collect::<Vec<_>>();
    Ok((
        format!("{status_summary}; {diff_summary}"),
        status,
        status_truncated || diff_truncated,
        Some(MechanisticDecisionV1 {
            confidence: if selected_paths.is_empty() {
                MechanisticConfidenceV1::Unknown
            } else {
                MechanisticConfidenceV1::High
            },
            selected_paths,
            selected_recipes: Vec::new(),
            reason_codes: vec!["git_status_and_diff".into()],
            cache_status: MechanisticCacheStatusV1::NotApplicable,
            source_receipt_digest: None,
            isolation_backend: None,
            dependency_selection: None,
            failure_diagnosis: None,
        }),
    ))
}

fn find_relevant_tests(
    repo: &Path,
    seed_paths: &[String],
    max_entries: u16,
) -> Result<CompositeQueryTuple> {
    validate_entry_limit(max_entries)?;
    let mut seed_terms = BTreeSet::new();
    for seed in seed_paths {
        let seed = validate_relative_path(seed)?;
        require_safe_query_path(&seed)?;
        let path = Path::new(&seed);
        if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            let folded = stem
                .trim_end_matches("Tests")
                .trim_end_matches("_test")
                .trim_end_matches("_tests")
                .to_ascii_lowercase();
            if !folded.is_empty() {
                seed_terms.insert(folded);
            }
        }
        if let Some(parent) = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|p| p.to_str())
        {
            let folded = parent.to_ascii_lowercase();
            if !matches!(folded.as_str(), "src" | "sources" | "lib") {
                seed_terms.insert(folded);
            }
        }
    }
    let mut ranked = tracked_files(repo)?
        .into_iter()
        .filter(|path| is_test_path(path))
        .filter_map(|path| {
            let folded = path.to_ascii_lowercase();
            let score = seed_terms
                .iter()
                .filter(|term| folded.contains(term.as_str()))
                .count();
            (score > 0).then_some((score, path))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    let truncated = ranked.len() > usize::from(max_entries);
    ranked.truncate(usize::from(max_entries));
    let selected_paths = ranked
        .iter()
        .map(|(_, path)| path.clone())
        .collect::<Vec<_>>();
    let selected_recipes = test_recipes_for_paths(&selected_paths);
    Ok((
        format!(
            "found {} structurally relevant tracked test file(s)",
            ranked.len()
        ),
        ranked
            .iter()
            .map(|(score, path)| format!("score={score} {path}"))
            .collect(),
        truncated,
        Some(MechanisticDecisionV1 {
            confidence: if selected_paths.is_empty() {
                MechanisticConfidenceV1::Low
            } else {
                MechanisticConfidenceV1::Medium
            },
            selected_paths,
            selected_recipes,
            reason_codes: vec!["path_and_test_naming_affinity".into()],
            cache_status: MechanisticCacheStatusV1::NotApplicable,
            source_receipt_digest: None,
            isolation_backend: None,
            dependency_selection: None,
            failure_diagnosis: None,
        }),
    ))
}

fn diagnose_failed_recipe(repo: &Path, job_id: &str) -> Result<CompositeQueryTuple> {
    let store = JobStore::new(repo);
    let record = store.load_sync(job_id)?;
    if record.kind != JobKind::DelegationEvidence {
        bail!("failure diagnosis accepts delegation evidence jobs only");
    }
    if matches!(record.status, JobState::Running | JobState::Completed) {
        bail!("failure diagnosis requires a terminal unsuccessful evidence job");
    }
    if repository_revision(&crate::paths::Paths::new(repo)?)? != record.expected_revision {
        bail!("failed evidence job is stale for the pinned repository revision");
    }
    let mut entries = Vec::new();
    if let Some(error) = record.error.as_deref() {
        entries.push(format!("job_error={}", redact_entry(error)));
    }
    let receipt = record.mechanistic_receipt.as_ref();
    if let Some(receipt) = receipt {
        if let Some(results) = receipt
            .get("action_results")
            .and_then(serde_json::Value::as_array)
        {
            for result in results.iter().take(8) {
                let status = result
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");
                let summary = result
                    .get("summary")
                    .and_then(serde_json::Value::as_str)
                    .map(redact_entry)
                    .unwrap_or_default();
                entries.push(format!("action_status={status} summary={summary}"));
            }
        }
    }
    let selected_paths = receipt
        .and_then(|value| value.get("action_results"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|result| result.get("decision"))
        .filter_map(|decision| decision.get("selected_paths"))
        .filter_map(serde_json::Value::as_array)
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .take(64)
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let selected_recipes = receipt
        .and_then(|value| value.get("action_results"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|result| result.get("decision"))
        .filter_map(|decision| decision.get("selected_recipes"))
        .filter_map(serde_json::Value::as_array)
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .take(8)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let dependency = receipt
        .and_then(|value| value.get("action_results"))
        .and_then(serde_json::Value::as_array)
        .and_then(|results| results.iter().find_map(|result| result.get("decision")))
        .and_then(|decision| decision.get("dependency_selection"));
    let package = dependency
        .and_then(|selection| selection.get("owning_packages"))
        .and_then(serde_json::Value::as_array)
        .and_then(|values| values.first())
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let target = dependency
        .and_then(|selection| selection.get("selected_targets"))
        .and_then(serde_json::Value::as_array)
        .and_then(|values| values.first())
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let current_failure_fingerprint = failure_fingerprint(record.error.as_deref(), receipt);
    let previously_failed = store
        .records()?
        .into_iter()
        .filter(|candidate| candidate.job_id != record.job_id)
        .filter(|candidate| candidate.kind == JobKind::DelegationEvidence)
        .filter(|candidate| candidate.status == JobState::Failed)
        .filter(|candidate| candidate.updated_at < record.started_at)
        .any(|candidate| {
            failure_fingerprint(
                candidate.error.as_deref(),
                candidate.mechanistic_receipt.as_ref(),
            ) == current_failure_fingerprint
        });
    let diagnostic_text = format!(
        "{} {}",
        record.error.as_deref().unwrap_or_default(),
        receipt
            .and_then(|value| value.get("action_results"))
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|result| result.get("summary").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    )
    .to_ascii_lowercase();
    let (cause, confidence, classification_reason) = if matches!(
        record.status,
        JobState::TimedOut | JobState::Cancelled | JobState::Interrupted
    ) || [
        "environment",
        "isolation",
        "toolchain",
        "unavailable",
        "no space left",
        "permission denied",
    ]
    .iter()
    .any(|needle| diagnostic_text.contains(needle))
    {
        (
            FailureCauseV1::Environmental,
            MechanisticConfidenceV1::High,
            "environmental_failure_signal",
        )
    } else if previously_failed {
        (
            FailureCauseV1::LikelyPreExisting,
            MechanisticConfidenceV1::High,
            "matching_prior_failure_fingerprint",
        )
    } else if !selected_paths.is_empty() && record.status == JobState::Failed {
        (
            FailureCauseV1::LikelyChangeCaused,
            MechanisticConfidenceV1::Medium,
            "failed_recipe_overlaps_changed_paths",
        )
    } else {
        (
            FailureCauseV1::Indeterminate,
            MechanisticConfidenceV1::Low,
            "insufficient_failure_evidence",
        )
    };
    let failed_recipe = selected_recipes.first().cloned();
    let recommended_rerun = failed_recipe.as_ref().map(|recipe| RecommendedRerunV1 {
        recipe: recipe.clone(),
        cwd: None,
        recipe_digest: format!("{:x}", Sha256::digest(recipe.as_bytes())),
    });
    let reason = format!("evidence_job_{}", job_state_name(record.status));
    let reason_codes = vec![reason.clone(), classification_reason.to_string()];
    entries.push(format!("failure_cause={}", failure_cause_name(cause)));
    if let Some(package) = &package {
        entries.push(format!("package={}", redact_entry(package)));
    }
    if let Some(target) = &target {
        entries.push(format!("target={}", redact_entry(target)));
    }
    if let Some(recipe) = &failed_recipe {
        entries.push(format!("recommended_rerun={}", redact_entry(recipe)));
    }
    let source_receipt_digest = receipt
        .and_then(|value| serde_json::to_vec(value).ok())
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)));
    Ok((
        format!("classified terminal evidence job `{job_id}` without rerunning it"),
        entries,
        false,
        Some(MechanisticDecisionV1 {
            confidence,
            selected_paths: selected_paths.clone(),
            selected_recipes: selected_recipes.clone(),
            reason_codes: reason_codes.clone(),
            cache_status: MechanisticCacheStatusV1::NotApplicable,
            source_receipt_digest,
            isolation_backend: None,
            dependency_selection: None,
            failure_diagnosis: Some(MechanisticFailureDiagnosisV1 {
                cause,
                confidence,
                package,
                target,
                likely_responsible_paths: selected_paths,
                failed_recipe,
                recommended_rerun,
                reason_codes,
            }),
        }),
    ))
}

fn failure_fingerprint(error: Option<&str>, receipt: Option<&serde_json::Value>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"codex-team-failure-fingerprint-v1\0");
    if let Some(error) = error {
        hasher.update(redact_entry(error));
    }
    if let Some(results) = receipt
        .and_then(|value| value.get("action_results"))
        .and_then(serde_json::Value::as_array)
    {
        for result in results.iter().take(8) {
            if let Some(status) = result.get("status").and_then(serde_json::Value::as_str) {
                hasher.update(status.as_bytes());
            }
            if let Some(summary) = result.get("summary").and_then(serde_json::Value::as_str) {
                hasher.update(redact_entry(summary));
            }
            if let Some(recipes) = result
                .get("decision")
                .and_then(|value| value.get("selected_recipes"))
                .and_then(serde_json::Value::as_array)
            {
                for recipe in recipes.iter().filter_map(serde_json::Value::as_str) {
                    hasher.update(recipe.as_bytes());
                }
            }
        }
    }
    format!("{:x}", hasher.finalize())
}

fn failure_cause_name(cause: FailureCauseV1) -> &'static str {
    match cause {
        FailureCauseV1::Environmental => "environmental",
        FailureCauseV1::LikelyPreExisting => "likely_pre_existing",
        FailureCauseV1::LikelyChangeCaused => "likely_change_caused",
        FailureCauseV1::Indeterminate => "indeterminate",
    }
}

fn job_state_name(state: JobState) -> &'static str {
    match state {
        JobState::Running => "running",
        JobState::Completed => "completed",
        JobState::Failed => "failed",
        JobState::Cancelled => "cancelled",
        JobState::TimedOut => "timed_out",
        JobState::Interrupted => "interrupted",
    }
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.starts_with("tests/")
        || lower.contains("/tests/")
        || lower.contains("/test/")
        || lower.ends_with("_test.rs")
        || lower.ends_with("_tests.rs")
        || lower.ends_with(".test.js")
        || lower.ends_with(".test.ts")
        || lower.ends_with("_test.py")
        || lower.ends_with("tests.swift")
}

fn test_recipes_for_paths(paths: &[String]) -> Vec<String> {
    let mut recipes = BTreeSet::new();
    for path in paths {
        if path.ends_with(".rs") {
            recipes.insert("cargo_test".to_string());
        } else if path.ends_with(".swift") {
            recipes.insert("swift_test".to_string());
        } else if path.ends_with(".py") {
            recipes.insert("pytest".to_string());
        } else if path.ends_with(".js") || path.ends_with(".ts") {
            recipes.insert("package_test".to_string());
        }
    }
    recipes.into_iter().collect()
}

fn scoped_tracked_files(repo: &Path, requested_paths: &[String]) -> Result<Vec<String>> {
    let tracked = tracked_files(repo)?;
    if requested_paths.is_empty() {
        return Ok(tracked);
    }
    if requested_paths.len() > usize::from(MAX_QUERY_ENTRIES) {
        bail!("query path list exceeds the query entry limit");
    }
    let mut candidates = BTreeSet::new();
    for requested in requested_paths {
        let scope = validate_relative_path(requested)?;
        require_safe_query_path(&scope)?;
        let mut matched_scope = false;
        for relative in &tracked {
            if relative == &scope
                || relative
                    .strip_prefix(&scope)
                    .is_some_and(|suffix| suffix.starts_with('/'))
            {
                matched_scope = true;
                candidates.insert(relative.clone());
            }
        }
        if !matched_scope {
            bail!("query scope does not match a tracked file or directory prefix: {scope}");
        }
    }
    Ok(candidates.into_iter().collect())
}

fn list_tracked_files(
    repo: &Path,
    prefix: Option<&str>,
    max_entries: u16,
) -> Result<(String, Vec<String>, bool)> {
    validate_entry_limit(max_entries)?;
    let prefix = prefix.map(validate_relative_path).transpose()?;
    let mut matching = tracked_files(repo)?
        .into_iter()
        .filter(|path| {
            prefix
                .as_ref()
                .is_none_or(|prefix| path.starts_with(prefix))
        })
        .collect::<Vec<_>>();
    let truncated = matching.len() > usize::from(max_entries);
    matching.truncate(usize::from(max_entries));
    Ok((
        format!("listed {} tracked file(s)", matching.len()),
        matching,
        truncated,
    ))
}

fn list_tracked_directory(
    repo: &Path,
    prefix: Option<&str>,
    max_entries: u16,
) -> Result<(String, Vec<String>, bool)> {
    validate_entry_limit(max_entries)?;
    let prefix = prefix.map(validate_relative_path).transpose()?;
    if let Some(prefix) = prefix.as_deref() {
        require_safe_query_path(prefix)?;
    }
    let tracked = tracked_files(repo)?;
    let mut entries = BTreeSet::new();
    let mut matched = prefix.is_none();
    for relative in tracked {
        let remainder = match prefix.as_deref() {
            None => relative.as_str(),
            Some(prefix) => {
                let Some(remainder) = relative.strip_prefix(prefix) else {
                    continue;
                };
                let Some(remainder) = remainder.strip_prefix('/') else {
                    continue;
                };
                matched = true;
                remainder
            }
        };
        let Some(component) = remainder.split('/').next() else {
            continue;
        };
        let directory = remainder.contains('/');
        let entry = match prefix.as_deref() {
            Some(prefix) if directory => format!("{prefix}/{component}/"),
            Some(prefix) => format!("{prefix}/{component}"),
            None if directory => format!("{component}/"),
            None => component.to_string(),
        };
        entries.insert(entry);
    }
    if !matched {
        bail!(
            "tracked directory does not exist: {}",
            prefix.unwrap_or_default()
        );
    }
    let total = entries.len();
    let entries = entries
        .into_iter()
        .take(usize::from(max_entries))
        .collect::<Vec<_>>();
    Ok((
        format!("listed {} tracked directory entries", entries.len()),
        entries,
        total > usize::from(max_entries),
    ))
}

fn glob_tracked_files(
    repo: &Path,
    pattern: &str,
    prefix: Option<&str>,
    max_entries: u16,
) -> Result<(String, Vec<String>, bool)> {
    validate_entry_limit(max_entries)?;
    if pattern.is_empty()
        || pattern.len() > 512
        || pattern.chars().any(char::is_control)
        || pattern.starts_with('/')
        || pattern.contains('\\')
        || pattern.split('/').any(|component| component == "..")
    {
        bail!("glob must be a bounded repository-relative pattern without traversal");
    }
    let prefix = prefix.map(validate_relative_path).transpose()?;
    if let Some(prefix) = prefix.as_deref() {
        require_safe_query_path(prefix)?;
    }
    let matcher = GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .context("invalid tracked-file glob")?
        .compile_matcher();
    let mut entries = tracked_files(repo)?
        .into_iter()
        .filter(|relative| {
            let scoped = match prefix.as_deref() {
                Some(prefix) => relative
                    .strip_prefix(prefix)
                    .and_then(|remainder| remainder.strip_prefix('/')),
                None => Some(relative.as_str()),
            };
            scoped.is_some_and(|path| matcher.is_match(path))
        })
        .collect::<Vec<_>>();
    let truncated = entries.len() > usize::from(max_entries);
    entries.truncate(usize::from(max_entries));
    Ok((
        format!("matched {} tracked file(s)", entries.len()),
        entries,
        truncated,
    ))
}

fn git_status_summary(repo: &Path, max_entries: u16) -> Result<(String, Vec<String>, bool)> {
    validate_entry_limit(max_entries)?;
    let bytes = git_output(
        repo,
        &["status", "--porcelain=v1", "-z", "--untracked-files=no"],
    )?;
    let fields = bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let field = std::str::from_utf8(fields[index]).context("Git status path was not UTF-8")?;
        if field.len() < 4 {
            bail!("Git returned malformed status output");
        }
        let status = &field[..2];
        let path = validate_relative_path(&field[3..])?;
        require_safe_query_path(&path)?;
        entries.push(format!("{status} {path}"));
        if matches!(status.as_bytes().first(), Some(b'R') | Some(b'C'))
            || matches!(status.as_bytes().get(1), Some(b'R') | Some(b'C'))
        {
            index += 1;
            if index >= fields.len() {
                bail!("Git returned an incomplete rename status");
            }
            let original = std::str::from_utf8(fields[index])?;
            let original = validate_relative_path(original)?;
            require_safe_query_path(&original)?;
            entries.push(format!("from {original}"));
        }
        index += 1;
    }
    let truncated = entries.len() > usize::from(max_entries);
    entries.truncate(usize::from(max_entries));
    Ok((
        format!("summarized {} tracked status entries", entries.len()),
        entries,
        truncated,
    ))
}

fn git_diff_summary(repo: &Path, max_entries: u16) -> Result<(String, Vec<String>, bool)> {
    validate_entry_limit(max_entries)?;
    let mut entries = Vec::new();
    for (scope, args) in [
        ("worktree", &["diff", "--numstat", "--no-ext-diff"][..]),
        (
            "index",
            &["diff", "--numstat", "--no-ext-diff", "--cached"][..],
        ),
    ] {
        let output = git_output(repo, args)?;
        let text = std::str::from_utf8(&output).context("Git diff summary was not UTF-8")?;
        for line in text.lines() {
            let mut fields = line.splitn(3, '\t');
            let added = fields
                .next()
                .context("Git diff summary omitted additions")?;
            let removed = fields.next().context("Git diff summary omitted removals")?;
            let path = validate_relative_path(
                fields.next().context("Git diff summary omitted its path")?,
            )?;
            require_safe_query_path(&path)?;
            entries.push(format!("{scope} +{added} -{removed} {path}"));
        }
    }
    entries.sort();
    entries.dedup();
    let truncated = entries.len() > usize::from(max_entries);
    entries.truncate(usize::from(max_entries));
    Ok((
        format!("summarized {} tracked diff entries", entries.len()),
        entries,
        truncated,
    ))
}

fn git_history(repo: &Path, max_entries: u8) -> Result<(String, Vec<String>, bool)> {
    if max_entries == 0 || max_entries > 50 {
        bail!("history max_entries must be between 1 and 50");
    }
    let count = format!("--max-count={max_entries}");
    let output = git_output(
        repo,
        &["log", &count, "--format=%H%x09%ct%x09%s", "--no-decorate"],
    )?;
    let text = std::str::from_utf8(&output).context("Git history was not UTF-8")?;
    let entries = text
        .lines()
        .map(|line| line.replace(['\r', '\0'], " "))
        .collect::<Vec<_>>();
    Ok((
        format!("returned {} bounded history entries", entries.len()),
        entries,
        false,
    ))
}

pub(crate) fn tracked_files(repo: &Path) -> Result<Vec<String>> {
    let output = git_output(repo, &["ls-files", "-z", "--cached"])?;
    let mut files = BTreeSet::new();
    for field in output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
    {
        let relative = std::str::from_utf8(field).context("tracked path was not UTF-8")?;
        let relative = validate_relative_path(relative)?;
        if require_safe_query_path(&relative).is_ok() {
            files.insert(relative);
        }
    }
    Ok(files.into_iter().collect())
}

pub(crate) fn require_tracked_regular_file(repo: &Path, value: &str) -> Result<(String, PathBuf)> {
    let relative = validate_relative_path(value)?;
    require_safe_query_path(&relative)?;
    let tracked = tracked_files(repo)?;
    if tracked.binary_search(&relative).is_err() {
        bail!("repo_query may read only tracked files");
    }
    Ok((relative.clone(), require_regular_file(repo, &relative)?))
}

fn require_regular_file(repo: &Path, relative: &str) -> Result<PathBuf> {
    let path = repo.join(relative);
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("inspect tracked query file {relative}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("tracked query target must be a regular non-symlink file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            bail!("hard-linked tracked files are not readable through repo_query");
        }
    }
    let canonical = fs::canonicalize(&path)?;
    let canonical_repo = fs::canonicalize(repo)?;
    if canonical != canonical_repo.join(relative) || !canonical.starts_with(&canonical_repo) {
        bail!("tracked query target escapes the pinned repository");
    }
    Ok(canonical)
}

pub(crate) fn validate_relative_path(value: &str) -> Result<String> {
    if value.is_empty() || value.len() > 4096 || value.contains('\\') || value.starts_with('/') {
        bail!("query path must be a bounded repository-relative path");
    }
    let path = Path::new(value);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("query path traversal is not allowed");
    }
    Ok(value.to_string())
}

pub(crate) fn require_safe_query_path(relative: &str) -> Result<()> {
    let lower = relative.to_ascii_lowercase();
    if lower == ".git"
        || lower.starts_with(".git/")
        || lower == ".orchestrator"
        || lower.starts_with(".orchestrator/")
        || lower.split('/').any(secret_shaped_component)
    {
        bail!("query path is private or secret-shaped");
    }
    Ok(())
}

fn secret_shaped_component(component: &str) -> bool {
    let normalized = component.replace(['-', '.'], "_");
    component == ".env"
        || component.starts_with(".env.")
        || normalized.contains("credential")
        || normalized.contains("private_key")
        || normalized.contains("api_key")
        || normalized.contains("access_token")
        || normalized.contains("refresh_token")
        || matches!(component, "id_rsa" | "id_ed25519" | "known_hosts")
}

fn validate_entry_limit(limit: u16) -> Result<()> {
    if limit == 0 || limit > MAX_QUERY_ENTRIES {
        bail!("entry limit must be between 1 and {MAX_QUERY_ENTRIES}");
    }
    Ok(())
}

fn git_output(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = crate::git_metadata::query(repo, args)?
        .current_dir(repo)
        .output()
        .with_context(|| format!("run bounded Git query `{}`", args.join(" ")))?;
    if !output.status.success() {
        bail!("bounded Git query failed with status {}", output.status);
    }
    if output.stdout.len() > MAX_GIT_OUTPUT_BYTES || output.stderr.len() > 64 * 1024 {
        bail!("bounded Git query output exceeded its safety limit");
    }
    Ok(output.stdout)
}

pub(crate) fn redact_entry(value: &str) -> String {
    crate::self_improvement::sanitize_trace_text(value)
        .trim_end()
        .chars()
        .take(8_192)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture(name: &str) -> Result<PathBuf> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!("codex-team-query-{name}-{nonce}"));
        fs::create_dir_all(&root)?;
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.invalid"],
            vec!["config", "user.name", "Query Test"],
        ] {
            assert!(Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()?
                .success());
        }
        fs::write(
            root.join("tracked.txt"),
            "first\nAuthorization: Bearer canary\nthird\n",
        )?;
        fs::write(root.join("untracked.txt"), "untracked\n")?;
        assert!(Command::new("git")
            .args(["add", "tracked.txt"])
            .current_dir(&root)
            .status()?
            .success());
        assert!(Command::new("git")
            .args(["commit", "-qm", "baseline"])
            .current_dir(&root)
            .status()?
            .success());
        Ok(root)
    }

    #[test]
    fn tracked_reads_are_windowed_redacted_and_untracked_reads_fail() -> Result<()> {
        let repo = fixture("read")?;
        let result = execute_query_action(
            &repo,
            0,
            &MechanisticActionV1::ReadTrackedFile {
                path: "tracked.txt".to_string(),
                start_line: 1,
                max_lines: 3,
            },
            64 * 1024,
        )?;
        assert_eq!(result.entries.len(), 3);
        assert!(result.entries[1].contains("REDACTED"));
        assert!(execute_query_action(
            &repo,
            0,
            &MechanisticActionV1::ReadTrackedFile {
                path: "untracked.txt".to_string(),
                start_line: 1,
                max_lines: 3,
            },
            64 * 1024,
        )
        .is_err());
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn tracked_directory_and_glob_use_only_the_git_index() -> Result<()> {
        let repo = fixture("directory-glob")?;
        fs::create_dir_all(repo.join("src/nested"))?;
        fs::write(repo.join("src/lib.rs"), "pub fn routed() {}\n")?;
        fs::write(repo.join("src/nested/test.rs"), "#[test] fn routed() {}\n")?;
        fs::write(repo.join("src/untracked.rs"), "secret\n")?;
        assert!(Command::new("git")
            .args(["add", "src/lib.rs", "src/nested/test.rs"])
            .current_dir(&repo)
            .status()?
            .success());

        let root = execute_query_action(
            &repo,
            0,
            &MechanisticActionV1::ListTrackedDirectory {
                path: None,
                max_entries: 20,
            },
            64 * 1024,
        )?;
        assert!(root.entries.contains(&"src/".to_string()));
        let directory = execute_query_action(
            &repo,
            0,
            &MechanisticActionV1::ListTrackedDirectory {
                path: Some("src".into()),
                max_entries: 20,
            },
            64 * 1024,
        )?;
        assert_eq!(directory.entries, ["src/lib.rs", "src/nested/"]);

        let globbed = execute_query_action(
            &repo,
            0,
            &MechanisticActionV1::GlobTrackedFiles {
                pattern: "**/*.rs".into(),
                prefix: Some("src".into()),
                max_entries: 20,
            },
            64 * 1024,
        )?;
        assert_eq!(globbed.entries, ["src/lib.rs", "src/nested/test.rs"]);
        assert!(!globbed
            .entries
            .iter()
            .any(|path| path.contains("untracked")));
        assert!(execute_query_action(
            &repo,
            0,
            &MechanisticActionV1::GlobTrackedFiles {
                pattern: "../**".into(),
                prefix: None,
                max_entries: 20,
            },
            64 * 1024,
        )
        .is_err());
        Ok(())
    }

    #[test]
    fn query_families_are_bounded_and_leave_targets_unchanged() -> Result<()> {
        let repo = fixture("families")?;
        let before = fs::read(repo.join("tracked.txt"))?;

        let search = execute_query_action(
            &repo,
            0,
            &MechanisticActionV1::SearchText {
                pattern: "Authorization".to_string(),
                match_mode: SearchMatchModeV1::Literal,
                paths: vec!["tracked.txt".to_string()],
                max_matches: 10,
            },
            64 * 1024,
        )?;
        assert_eq!(search.entries.len(), 1);
        assert!(search.entries[0].contains("REDACTED"));

        let listed = execute_query_action(
            &repo,
            1,
            &MechanisticActionV1::ListTrackedFiles {
                prefix: None,
                max_entries: 10,
            },
            64 * 1024,
        )?;
        assert_eq!(listed.entries, ["tracked.txt"]);

        fs::write(repo.join("tracked.txt"), "changed\n")?;
        let status = execute_query_action(
            &repo,
            2,
            &MechanisticActionV1::GitStatusSummary { max_entries: 10 },
            64 * 1024,
        )?;
        assert!(status
            .entries
            .iter()
            .any(|entry| entry.contains("tracked.txt")));
        let diff = execute_query_action(
            &repo,
            3,
            &MechanisticActionV1::GitDiffSummary { max_entries: 10 },
            64 * 1024,
        )?;
        assert!(diff
            .entries
            .iter()
            .any(|entry| entry.contains("tracked.txt")));
        fs::write(repo.join("tracked.txt"), &before)?;

        let history = execute_query_action(
            &repo,
            4,
            &MechanisticActionV1::GitHistory { max_entries: 1 },
            64 * 1024,
        )?;
        assert_eq!(history.entries.len(), 1);
        assert_eq!(fs::read(repo.join("tracked.txt"))?, before);

        let capped = execute_query_action(
            &repo,
            5,
            &MechanisticActionV1::ReadTrackedFile {
                path: "tracked.txt".to_string(),
                start_line: 1,
                max_lines: 3,
            },
            8,
        )?;
        assert!(capped.entries.is_empty());
        assert!(capped.truncated);
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn search_supports_component_prefixes_deduplication_and_explicit_regex() -> Result<()> {
        let repo = fixture("search-scopes")?;
        fs::create_dir_all(repo.join("src/nested"))?;
        fs::write(repo.join("src/alpha.rs"), "orchestration_available\n")?;
        fs::write(repo.join("src/nested/beta.rs"), "delegate_mechanistic\n")?;
        fs::write(repo.join("src-other.rs"), "must not match src scope\n")?;
        assert!(Command::new("git")
            .args(["add", "src/alpha.rs", "src/nested/beta.rs", "src-other.rs"])
            .current_dir(&repo)
            .status()?
            .success());

        let literal = execute_query_action(
            &repo,
            0,
            &MechanisticActionV1::SearchText {
                pattern: "orchestration_available|delegate_mechanistic".into(),
                match_mode: SearchMatchModeV1::Literal,
                paths: vec!["src".into()],
                max_matches: 10,
            },
            64 * 1024,
        )?;
        assert!(literal.entries.is_empty());

        let regex = execute_query_action(
            &repo,
            0,
            &MechanisticActionV1::SearchText {
                pattern: "orchestration_available|delegate_mechanistic".into(),
                match_mode: SearchMatchModeV1::Regex,
                paths: vec!["src".into(), "src/nested/beta.rs".into()],
                max_matches: 10,
            },
            64 * 1024,
        )?;
        assert_eq!(regex.entries.len(), 2);
        assert_eq!(
            regex
                .entries
                .iter()
                .filter(|entry| entry.contains("beta.rs"))
                .count(),
            1
        );
        assert!(regex
            .entries
            .iter()
            .all(|entry| !entry.contains("src-other")));

        for paths in [vec!["missing".into()], vec!["../src".into()]] {
            assert!(execute_query_action(
                &repo,
                0,
                &MechanisticActionV1::SearchText {
                    pattern: "anything".into(),
                    match_mode: SearchMatchModeV1::Literal,
                    paths,
                    max_matches: 10,
                },
                64 * 1024,
            )
            .is_err());
        }
        assert!(execute_query_action(
            &repo,
            0,
            &MechanisticActionV1::SearchText {
                pattern: "(".into(),
                match_mode: SearchMatchModeV1::Regex,
                paths: vec!["src".into()],
                max_matches: 10,
            },
            64 * 1024,
        )
        .is_err());

        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn composite_queries_rank_symbols_features_changes_and_tests() -> Result<()> {
        let repo = fixture("composites")?;
        fs::create_dir_all(repo.join("src"))?;
        fs::create_dir_all(repo.join("tests"))?;
        fs::write(
            repo.join("src/input.rs"),
            "pub fn move_left_arrow(cursor: usize) -> usize { cursor.saturating_sub(1) }\n",
        )?;
        fs::write(
            repo.join("tests/input_tests.rs"),
            "#[test]\nfn left_arrow_moves_cursor() {}\n",
        )?;
        assert!(Command::new("git")
            .args(["add", "src/input.rs", "tests/input_tests.rs"])
            .current_dir(&repo)
            .status()?
            .success());

        let symbol = execute_query_action(
            &repo,
            0,
            &MechanisticActionV1::InspectSymbol {
                symbol: "move_left_arrow".into(),
                paths: vec!["src".into()],
                max_matches: 10,
                context_lines: 1,
            },
            64 * 1024,
        )?;
        assert!(symbol
            .entries
            .iter()
            .any(|entry| entry.contains("input.rs")));
        assert_eq!(
            symbol.decision.as_ref().unwrap().confidence,
            MechanisticConfidenceV1::High
        );

        let feature = execute_query_action(
            &repo,
            1,
            &MechanisticActionV1::InspectFeature {
                terms: vec!["left".into(), "arrow".into()],
                paths: Vec::new(),
                max_files: 8,
            },
            64 * 1024,
        )?;
        assert!(feature.entries.iter().any(|entry| entry.contains("input")));

        let tests = execute_query_action(
            &repo,
            2,
            &MechanisticActionV1::FindRelevantTests {
                paths: vec!["src/input.rs".into()],
                max_entries: 8,
            },
            64 * 1024,
        )?;
        assert_eq!(
            tests.decision.as_ref().unwrap().selected_paths,
            ["tests/input_tests.rs"]
        );

        fs::write(repo.join("src/input.rs"), "pub fn move_left_arrow() {}\n")?;
        let changed = execute_query_action(
            &repo,
            3,
            &MechanisticActionV1::SummarizeChangedArea { max_entries: 8 },
            64 * 1024,
        )?;
        assert!(changed
            .decision
            .as_ref()
            .unwrap()
            .selected_paths
            .iter()
            .any(|path| path == "src/input.rs"));
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn private_secret_binary_and_oversized_inputs_fail_closed() -> Result<()> {
        let repo = fixture("unsafe-inputs")?;
        fs::create_dir_all(repo.join(".orchestrator"))?;
        fs::write(repo.join(".env"), "TOKEN=canary\n")?;
        fs::write(repo.join(".orchestrator/state.json"), "{}\n")?;
        fs::write(repo.join("binary.dat"), [0_u8, 1, 2, 3])?;
        fs::write(
            repo.join("oversized.txt"),
            vec![b'x'; usize::try_from(MAX_FILE_INPUT_BYTES)? + 1],
        )?;
        assert!(Command::new("git")
            .args([
                "add",
                ".env",
                ".orchestrator/state.json",
                "binary.dat",
                "oversized.txt",
            ])
            .current_dir(&repo)
            .status()?
            .success());

        for path in [
            ".env",
            ".orchestrator/state.json",
            "binary.dat",
            "oversized.txt",
        ] {
            assert!(execute_query_action(
                &repo,
                0,
                &MechanisticActionV1::ReadTrackedFile {
                    path: path.to_string(),
                    start_line: 1,
                    max_lines: 1,
                },
                64 * 1024,
            )
            .is_err());
        }
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[test]
    fn failed_recipe_diagnosis_is_typed_and_does_not_rerun() -> Result<()> {
        let repo = fixture("diagnosis")?;
        fs::write(repo.join(".gitignore"), ".orchestrator/\n")?;
        assert!(Command::new("git")
            .args(["add", ".gitignore"])
            .current_dir(&repo)
            .status()?
            .success());
        let revision = repository_revision(&crate::paths::Paths::new(&repo)?)?;
        let store = JobStore::new(&repo);
        store.save_atomic(&crate::mcp::jobs::JobRecord {
            schema: crate::mcp::jobs::JOB_SCHEMA.to_string(),
            job_id: "evidence-failed".to_string(),
            kind: JobKind::DelegationEvidence,
            status: JobState::Failed,
            expected_revision: revision,
            started_at: "2026-08-27T00:00:00Z".to_string(),
            updated_at: "2026-08-27T00:00:01Z".to_string(),
            outer_process_group: None,
            launch_nonce: None,
            process_provenance: None,
            active_child_process_group: None,
            progress: None,
            cycle_receipt: None,
            mechanistic_receipt: Some(serde_json::json!({
                "action_results": [{
                    "status": "failed",
                    "summary": "command failed: src/input.rs: cursor assertion failed",
                    "decision": {
                        "selected_paths": ["src/input.rs"],
                        "selected_recipes": ["cargo test -p ca --locked --offline input"],
                        "dependency_selection": {
                            "owning_packages": ["ca"],
                            "selected_targets": ["ca:input"]
                        }
                    }
                }]
            })),
            delegation_feedback: None,
            delegation_manifest_digest: None,
            error: Some("verification failed".to_string()),
            repository_may_be_partial: false,
        })?;

        let result = execute_query_action(
            &repo,
            0,
            &MechanisticActionV1::DiagnoseFailedRecipe {
                job_id: "evidence-failed".to_string(),
            },
            64 * 1024,
        )?;
        let diagnosis = result
            .decision
            .as_ref()
            .and_then(|decision| decision.failure_diagnosis.as_ref())
            .expect("typed diagnosis");
        assert_eq!(diagnosis.cause, FailureCauseV1::LikelyChangeCaused);
        assert_eq!(diagnosis.package.as_deref(), Some("ca"));
        assert_eq!(diagnosis.target.as_deref(), Some("ca:input"));
        assert_eq!(diagnosis.likely_responsible_paths, ["src/input.rs"]);
        assert!(diagnosis.recommended_rerun.is_some());
        assert_eq!(store.records()?.len(), 1, "diagnosis must not rerun work");
        fs::remove_dir_all(repo)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn traversal_symlink_and_hardlink_reads_fail_closed() -> Result<()> {
        let repo = fixture("links")?;
        assert!(require_tracked_regular_file(&repo, "../tracked.txt").is_err());
        let outside = repo.parent().unwrap().join("codex-team-query-outside");
        fs::write(&outside, "outside")?;
        std::os::unix::fs::symlink(&outside, repo.join("linked.txt"))?;
        assert!(Command::new("git")
            .args(["add", "linked.txt"])
            .current_dir(&repo)
            .status()?
            .success());
        assert!(require_tracked_regular_file(&repo, "linked.txt").is_err());
        fs::hard_link(repo.join("tracked.txt"), repo.join("hard.txt"))?;
        assert!(Command::new("git")
            .args(["add", "hard.txt"])
            .current_dir(&repo)
            .status()?
            .success());
        assert!(require_tracked_regular_file(&repo, "hard.txt").is_err());
        fs::remove_file(outside)?;
        fs::remove_dir_all(repo)?;
        Ok(())
    }
}
