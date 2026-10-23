//! Persistent, evidence-led questions about ca's behavior. Learning changes the
//! investigation queue, never user instructions, permissions, or task acceptance.
use crate::harness_questionnaire_catalog::{Question, Signal, QUESTIONS};
use crate::session_outcome::{ExecutionStatus, SessionOutcome, UserFeedback, VerificationStatus};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SCHEMA: &str = "ca.harness_questionnaire.v1";
const MAX_ENTRIES: usize = 4096;
const MAX_SCAN_BYTES: u64 = 64 * 1024 * 1024;
const MAX_OUTCOME_BYTES: u64 = 2 * 1024 * 1024;
const MAX_STATE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_OUTCOMES: usize = 1000;
const MAX_COHORTS: usize = 64;
const MAX_FOCUS_BYTES: usize = 8192;
const MAX_ANALYSIS_FOCUS_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerStatus {
    Unknown,
    ObservedSignal,
    Verified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub id: String,
    pub domain: String,
    pub question: String,
    pub evidence: String,
    pub improvement: String,
    pub signal: Signal,
    pub status: AnswerStatus,
    pub independent_roots: usize,
    pub signal_roots: usize,
    /// Question-specific host verification. Generic task acceptance does not
    /// establish these broad behavioral requirements, so this remains zero
    /// until a verifier supplies a contract explicitly bound to the question.
    pub verified_roots: usize,
    pub priority: u32,
    pub trend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohortReport {
    pub digest: String,
    pub workspace_hash: String,
    pub provider: String,
    pub model: String,
    pub build: String,
    pub route: String,
    pub harness_digest: String,
    pub latest_at: u64,
    pub independent_roots: usize,
    pub verified_outcome_roots: usize,
    pub outcome_count: usize,
    pub questions: Vec<Answer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BacklogItem {
    pub cohort_digest: String,
    pub question_id: String,
    pub domain: String,
    pub priority: u32,
    pub signal_roots: usize,
    pub improvement: String,
    pub evidence_needed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceFingerprint {
    pub task_id: String,
    pub root_id: String,
    pub outcome_digest: String,
    pub feedback_revision: u128,
    pub cohort_digest: String,
    pub started_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema: String,
    pub cycle: u64,
    pub changed: bool,
    pub generated_at: u64,
    pub evidence_digest: String,
    pub scanned_outcomes: usize,
    pub unreadable_outcomes: usize,
    pub excluded_outcomes: usize,
    pub independent_roots: usize,
    pub evidence: Vec<EvidenceFingerprint>,
    pub cohorts: Vec<CohortReport>,
    pub backlog: Vec<BacklogItem>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u32,
    catalog_digest: String,
    report: Report,
}

struct Scan {
    outcomes: Vec<SessionOutcome>,
    fingerprints: Vec<(String, String)>,
    scanned: usize,
    unreadable: usize,
    excluded: usize,
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn digest(value: &impl Serialize) -> Result<String> {
    Ok(hash(&serde_json::to_vec(value)?))
}

fn directory(data_dir: &Path) -> PathBuf {
    data_dir.join("harness/questionnaire")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn hex_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn route_label(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

fn comparable(outcome: &SessionOutcome) -> bool {
    outcome.version >= 2
        && outcome.evidence_complete
        && identifier(&outcome.task_id)
        && identifier(outcome.independent_task_id())
        && outcome.execution != ExecutionStatus::Running
        && outcome.harness_epochs.len() <= 1
        && outcome
            .harness_epochs
            .first()
            .is_none_or(|epoch| epoch == &outcome.harness)
        && outcome.harness.build.as_deref() == Some(env!("CA_BUILD_DIGEST"))
        && outcome
            .harness
            .model_route
            .as_deref()
            .is_some_and(hex_digest)
        && hex_digest(&outcome.workspace_hash)
        && outcome.provider.as_deref().is_some_and(route_label)
        && outcome.model.as_deref().is_some_and(route_label)
        && outcome.events.len() <= 256
        && outcome.tools.len() <= 256
}

fn checked_directory(path: &Path, private: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || (private && metadata.permissions().mode() & 0o077 != 0)
    {
        bail!(
            "questionnaire storage must be an owned directory{}",
            if private {
                " with private permissions"
            } else {
                ""
            }
        );
    }
    Ok(())
}

fn make_directory(path: &Path, private: bool) -> Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    checked_directory(path, private)
}

fn prepare_directory(data_dir: &Path) -> Result<PathBuf> {
    if let Err(error) = fs::symlink_metadata(data_dir) {
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(error.into());
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(data_dir)?;
    }
    checked_directory(data_dir, false)?;
    make_directory(&data_dir.join("harness"), false)?;
    let path = directory(data_dir);
    make_directory(&path, true)?;
    Ok(path)
}

fn bounded_file(path: &Path, maximum: u64, private: bool) -> Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.len() > maximum
        || (private
            && (metadata.uid() != unsafe { libc::geteuid() }
                || metadata.permissions().mode() & 0o077 != 0
                || metadata.nlink() != 1))
    {
        bail!(
            "questionnaire input must be a bounded regular file{}",
            if private {
                " with private ownership"
            } else {
                ""
            }
        );
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        bail!("questionnaire input changed beyond its size bound");
    }
    Ok(bytes)
}

fn acquire_lock(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path.join(".lock"))?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.nlink() != 1
    {
        bail!("questionnaire lock is not a private owned regular file");
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn load_state(data_dir: &Path) -> Result<Option<State>> {
    let root = directory(data_dir);
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    checked_directory(&data_dir.join("harness"), false)?;
    checked_directory(&root, true)?;
    let path = root.join("state.json");
    let bytes = match bounded_file(&path, MAX_STATE_BYTES, true) {
        Ok(bytes) => bytes,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let state: State = serde_json::from_slice(&bytes).context("invalid questionnaire state")?;
    if state.version != 1
        || state.catalog_digest != digest(&QUESTIONS)?
        || state.report.schema != SCHEMA
        || !hex_digest(&state.report.evidence_digest)
        || state.report.cohorts.len() > MAX_COHORTS
        || state.report.evidence.len() > MAX_OUTCOMES
        || state.report.evidence.iter().any(|evidence| {
            !identifier(&evidence.task_id)
                || !identifier(&evidence.root_id)
                || !hex_digest(&evidence.outcome_digest)
                || !hex_digest(&evidence.cohort_digest)
        })
        || state.report.backlog.len() > 64
        || state.report.cohorts.iter().any(|cohort| {
            !hex_digest(&cohort.digest)
                || !hex_digest(&cohort.workspace_hash)
                || !hex_digest(&cohort.build)
                || !hex_digest(&cohort.route)
                || !hex_digest(&cohort.harness_digest)
                || cohort.outcome_count > MAX_OUTCOMES
                || cohort.questions.len() != QUESTIONS.len()
                || cohort
                    .questions
                    .iter()
                    .zip(QUESTIONS)
                    .any(|(answer, question)| {
                        answer.id != question.id
                            || answer.signal != question.signal
                            || answer.signal_roots > answer.independent_roots
                            || answer.verified_roots > answer.independent_roots
                    })
        })
    {
        bail!("questionnaire state does not match the current bounded catalog");
    }
    Ok(Some(state))
}

fn save_state(path: &Path, state: &State) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(state)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        bail!("questionnaire state exceeds its byte bound");
    }
    let destination = path.join("state.json");
    if fs::symlink_metadata(&destination)
        .is_ok_and(|metadata| !metadata.is_file() || metadata.file_type().is_symlink())
    {
        bail!("questionnaire state destination is not a regular file");
    }
    let mut staged = tempfile::NamedTempFile::new_in(path)?;
    staged
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    staged.write_all(&bytes)?;
    staged.as_file().sync_all()?;
    staged.persist(destination).map_err(|error| error.error)?;
    File::open(path)?.sync_all()?;
    Ok(())
}

fn scan(data_dir: &Path, limit: usize) -> Result<Scan> {
    let root = data_dir.join("outcomes");
    let mut scan = Scan {
        outcomes: Vec::new(),
        fingerprints: Vec::new(),
        scanned: 0,
        unreadable: 0,
        excluded: 0,
    };
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(scan),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            bail!("questionnaire outcomes must be a real directory");
        }
        Ok(_) => {}
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(&root)? {
        if entries.len() >= MAX_ENTRIES {
            bail!("questionnaire scan exceeds {MAX_ENTRIES} entries");
        }
        entries.push(entry?);
    }
    entries.sort_by_key(|entry| entry.file_name());
    let mut remaining = MAX_SCAN_BYTES;
    let mut loaded = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        if remaining == 0 {
            bail!("questionnaire scan exceeds its 64 MiB byte bound");
        }
        let bytes = match bounded_file(&path, remaining.min(MAX_OUTCOME_BYTES), false) {
            Ok(bytes) => bytes,
            Err(_) => {
                scan.unreadable += 1;
                continue;
            }
        };
        remaining -= bytes.len() as u64;
        let outcome = match serde_json::from_slice::<SessionOutcome>(&bytes) {
            Ok(outcome)
                if path.file_stem().and_then(|stem| stem.to_str())
                    == Some(outcome.task_id.as_str()) =>
            {
                outcome
            }
            _ => {
                scan.unreadable += 1;
                continue;
            }
        };
        scan.scanned += 1;
        if comparable(&outcome) {
            loaded.push((outcome, hash(&bytes)));
        } else {
            scan.excluded += 1;
        }
    }
    loaded.sort_by(|left, right| {
        right
            .0
            .started_at
            .cmp(&left.0.started_at)
            .then_with(|| left.0.task_id.cmp(&right.0.task_id))
    });
    scan.excluded += loaded.len().saturating_sub(limit);
    loaded.truncate(limit);
    for (outcome, fingerprint) in loaded {
        scan.fingerprints
            .push((outcome.task_id.clone(), fingerprint));
        scan.outcomes.push(outcome);
    }
    scan.fingerprints.sort();
    Ok(scan)
}

fn cohort_key(outcome: &SessionOutcome) -> Result<String> {
    digest(&(
        &outcome.workspace_hash,
        &outcome.provider,
        &outcome.model,
        &outcome.harness,
    ))
}

fn signal_seen(signal: Signal, outcome: &SessionOutcome) -> bool {
    match signal {
        Signal::ToolFailures => outcome.tools.values().any(|tool| tool.failed > 0),
        Signal::VerificationFailure => matches!(
            outcome.verification,
            VerificationStatus::Failed | VerificationStatus::Stale
        ),
        Signal::RejectedFeedback => outcome.feedback == UserFeedback::Rejected,
        Signal::Corrections => outcome.signals.correction_count > 0,
        Signal::Undo => outcome.signals.undo_count > 0,
        Signal::LoopGuard => outcome.signals.loop_guard_trips > 0,
        Signal::Denied => {
            outcome.signals.denial_count > 0 || outcome.tools.values().any(|tool| tool.denied > 0)
        }
        Signal::Interrupted => {
            outcome.execution == ExecutionStatus::Interrupted || outcome.signals.interrupt_count > 0
        }
        Signal::Recovery => outcome.events.iter().any(|event| {
            matches!(
                event.kind.as_str(),
                "provider_failure" | "recovery_required" | "recovery_failed"
            ) || (event.kind.contains("recover") && event.succeeded == Some(false))
        }),
        Signal::Unknown => false,
    }
}

fn answers(outcomes: &[&SessionOutcome], previous: Option<&CohortReport>) -> Vec<Answer> {
    let roots: BTreeSet<_> = outcomes
        .iter()
        .map(|outcome| outcome.independent_task_id())
        .collect();
    QUESTIONS
        .iter()
        .map(|question| {
            let signal_roots: BTreeSet<_> = outcomes
                .iter()
                .filter(|outcome| signal_seen(question.signal, outcome))
                .map(|outcome| outcome.independent_task_id())
                .collect();
            let status = if !signal_roots.is_empty() {
                AnswerStatus::ObservedSignal
            } else {
                AnswerStatus::Unknown
            };
            let priority = question
                .weight
                .saturating_mul(1 + 5 * signal_roots.len().min(20) as u32);
            let trend = previous
                .and_then(|cohort| {
                    cohort
                        .questions
                        .iter()
                        .find(|answer| answer.id == question.id)
                })
                .filter(|old| old.independent_roots >= 2 && roots.len() >= 2)
                .map_or("insufficient_evidence", |old| {
                    let before = old.signal_roots * roots.len();
                    let after = signal_roots.len() * old.independent_roots;
                    match after.cmp(&before) {
                        std::cmp::Ordering::Less => "fewer_signals",
                        std::cmp::Ordering::Equal => "unchanged",
                        std::cmp::Ordering::Greater => "more_signals",
                    }
                });
            Answer {
                id: question.id.into(),
                domain: question.domain.into(),
                question: question.question.into(),
                evidence: question.evidence.into(),
                improvement: question.improvement.into(),
                signal: question.signal,
                status,
                independent_roots: roots.len(),
                signal_roots: signal_roots.len(),
                verified_roots: 0,
                priority,
                trend: trend.into(),
            }
        })
        .collect()
}

fn build_report(scan: &Scan, previous: Option<&Report>) -> Result<Report> {
    let fingerprints: BTreeMap<_, _> = scan
        .fingerprints
        .iter()
        .map(|(id, fingerprint)| (id.as_str(), fingerprint))
        .collect();
    let evidence: Vec<_> = scan
        .outcomes
        .iter()
        .map(|outcome| {
            Ok(EvidenceFingerprint {
                task_id: outcome.task_id.clone(),
                root_id: outcome.independent_task_id().to_owned(),
                outcome_digest: fingerprints
                    .get(outcome.task_id.as_str())
                    .context("outcome fingerprint missing")?
                    .to_string(),
                feedback_revision: outcome.feedback_revision,
                cohort_digest: cohort_key(outcome)?,
                started_at: outcome.started_at,
            })
        })
        .collect::<Result<_>>()?;
    let mut groups: BTreeMap<String, Vec<&SessionOutcome>> = BTreeMap::new();
    for outcome in &scan.outcomes {
        groups
            .entry(cohort_key(outcome)?)
            .or_default()
            .push(outcome);
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by(|left, right| {
        right
            .1
            .iter()
            .map(|outcome| outcome.started_at)
            .max()
            .cmp(&left.1.iter().map(|outcome| outcome.started_at).max())
            .then_with(|| left.0.cmp(&right.0))
    });
    let omitted: usize = groups
        .iter()
        .skip(MAX_COHORTS)
        .map(|(_, outcomes)| outcomes.len())
        .sum();
    groups.truncate(MAX_COHORTS);
    let mut cohorts = Vec::new();
    let mut root_keys = BTreeSet::new();
    for (key, outcomes) in groups {
        let latest = outcomes
            .iter()
            .max_by_key(|outcome| outcome.started_at)
            .unwrap();
        let old =
            previous.and_then(|report| report.cohorts.iter().find(|cohort| cohort.digest == key));
        let questions = answers(&outcomes, old);
        let verified_outcome_roots: BTreeSet<_> = outcomes
            .iter()
            .filter(|outcome| outcome.verified_positive())
            .map(|outcome| outcome.independent_task_id())
            .collect();
        root_keys.extend(
            outcomes
                .iter()
                .map(|outcome| outcome.independent_task_id().to_owned()),
        );
        cohorts.push(CohortReport {
            digest: key,
            workspace_hash: latest.workspace_hash.clone(),
            provider: latest.provider.clone().unwrap(),
            model: latest.model.clone().unwrap(),
            build: latest.harness.build.clone().unwrap(),
            route: latest.harness.model_route.clone().unwrap(),
            harness_digest: digest(&latest.harness)?,
            latest_at: latest.started_at,
            independent_roots: questions
                .first()
                .map_or(0, |answer| answer.independent_roots),
            verified_outcome_roots: verified_outcome_roots.len(),
            outcome_count: outcomes.len(),
            questions,
        });
    }
    let mut backlog: Vec<_> = cohorts
        .iter()
        .flat_map(|cohort| {
            cohort
                .questions
                .iter()
                .filter(|answer| answer.signal_roots > 0)
                .map(|answer| BacklogItem {
                    cohort_digest: cohort.digest.clone(),
                    question_id: answer.id.clone(),
                    domain: answer.domain.clone(),
                    priority: answer.priority,
                    signal_roots: answer.signal_roots,
                    improvement: answer.improvement.clone(),
                    evidence_needed: answer.evidence.clone(),
                })
        })
        .collect();
    backlog.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.question_id.cmp(&right.question_id))
            .then_with(|| left.cohort_digest.cmp(&right.cohort_digest))
    });
    backlog.truncate(64);
    Ok(Report {
        schema: SCHEMA.into(),
        cycle: previous.map_or(1, |report| report.cycle.saturating_add(1)),
        changed: true,
        generated_at: now(),
        evidence_digest: digest(&(
            &scan.fingerprints,
            scan.scanned,
            scan.unreadable,
            scan.excluded,
            digest(&QUESTIONS)?,
        ))?,
        scanned_outcomes: scan.scanned,
        unreadable_outcomes: scan.unreadable,
        excluded_outcomes: scan.excluded + omitted,
        independent_roots: root_keys.len(),
        evidence,
        cohorts,
        backlog,
    })
}

/// Recompute the current evidence window. Repeated cycles cannot multiply
/// confidence; revised feedback replaces its old contribution on the next scan.
pub fn run_cycle(data_dir: &Path, limit: usize) -> Result<Report> {
    if !(1..=MAX_OUTCOMES).contains(&limit) {
        bail!("questionnaire outcome limit must be between 1 and {MAX_OUTCOMES}");
    }
    let path = prepare_directory(data_dir)?;
    let _lock = acquire_lock(&path)?;
    let previous = load_state(data_dir)?;
    let scan = scan(data_dir, limit)?;
    let report = build_report(&scan, previous.as_ref().map(|state| &state.report))?;
    if let Some(previous) = &previous {
        if previous.report.evidence_digest == report.evidence_digest {
            let mut report = previous.report.clone();
            report.changed = false;
            return Ok(report);
        }
    }
    let state = State {
        version: 1,
        catalog_digest: digest(&QUESTIONS)?,
        report: report.clone(),
    };
    save_state(&path, &state)?;
    Ok(report)
}

/// Read-only inspection neither creates storage nor advances a learning cycle.
pub fn status(data_dir: &Path) -> Result<Option<Report>> {
    Ok(load_state(data_dir)?.map(|state| {
        let mut report = state.report;
        report.changed = false;
        report
    }))
}

pub fn format_report(report: &Report) -> String {
    let verified_outcomes: usize = report
        .cohorts
        .iter()
        .map(|cohort| cohort.verified_outcome_roots)
        .sum();
    let unknown = report
        .cohorts
        .iter()
        .flat_map(|cohort| &cohort.questions)
        .filter(|answer| answer.status == AnswerStatus::Unknown)
        .count();
    let observed = report
        .cohorts
        .iter()
        .flat_map(|cohort| &cohort.questions)
        .filter(|answer| answer.status == AnswerStatus::ObservedSignal)
        .count();
    let verified = report
        .cohorts
        .iter()
        .flat_map(|cohort| &cohort.questions)
        .filter(|answer| answer.status == AnswerStatus::Verified)
        .count();
    let mut text = format!(
        "Behavior questionnaire cycle {}{}\n{} outcomes inspected; {} independent roots in {} comparable cohorts.\n{} observed signals; {} verified question answers; {} unknown answers.\n{} task outcomes have acceptance evidence; that does not verify every behavioral question.\n{} excluded outcomes; {} unreadable.\n",
        report.cycle, if report.changed { " (new evidence)" } else { " (unchanged evidence)" },
        report.scanned_outcomes, report.independent_roots, report.cohorts.len(),
        observed, verified, unknown, verified_outcomes, report.excluded_outcomes, report.unreadable_outcomes,
    );
    if report.cohorts.is_empty() {
        text.push_str(
            "All 48 questions remain open until complete current-build evidence is available.\n",
        );
    }
    for item in report.backlog.iter().take(8) {
        text.push_str(&format!(
            "\n{} [{}] — {} roots; investigation priority {}\n  {}\n  Evidence needed: {}\n",
            item.question_id,
            item.domain,
            item.signal_roots,
            item.priority,
            item.improvement,
            item.evidence_needed
        ));
    }
    text.push_str("\nSignals prioritize investigations; they do not prove causes or create permanent rules.\n");
    text
}

fn append_bounded(output: &mut String, line: &str) {
    if output.len() + line.len() <= MAX_FOCUS_BYTES {
        output.push_str(line);
    }
}

fn focus_line(question: &Question, signal_roots: usize) -> String {
    format!(
        "{} [{}]: {} Observed signal roots: {}. Required evidence: {} Improvement to test: {}\n",
        question.id,
        question.domain,
        question.question,
        signal_roots,
        question.evidence,
        question.improvement
    )
}

/// AI Analyze sees every domain, with locally observed priorities first. The
/// prompt only asks questions; acceptance and authority stay with existing hosts.
pub fn analysis_focus(outcomes: &[SessionOutcome]) -> String {
    let comparable: Vec<_> = outcomes
        .iter()
        .filter(|outcome| comparable(outcome))
        .take(MAX_OUTCOMES)
        .collect();
    let mut cohorts: BTreeMap<String, Vec<&SessionOutcome>> = BTreeMap::new();
    for outcome in comparable {
        if let Ok(key) = cohort_key(outcome) {
            cohorts.entry(key).or_default().push(outcome);
        }
    }
    let mut output = String::from("CA permanent-behavior questionnaire. Investigate these questions using the supplied evidence. Signals are hypotheses, not proven causes; missing evidence means unknown. Do not invent user rules or infer permission changes.\n");
    output.push_str("Signal counts below use the largest single comparable cohort; different workspaces, routes and harnesses are never pooled. Generic task acceptance does not verify a behavioral question.\n");
    let domains: BTreeSet<_> = QUESTIONS.iter().map(|question| question.domain).collect();
    for domain in domains {
        let mut ranked: Vec<_> = QUESTIONS
            .iter()
            .filter(|question| question.domain == domain)
            .map(|question| {
                let roots = cohorts
                    .values()
                    .map(|outcomes| {
                        outcomes
                            .iter()
                            .filter(|outcome| signal_seen(question.signal, outcome))
                            .map(|outcome| outcome.independent_task_id())
                            .collect::<BTreeSet<_>>()
                            .len()
                    })
                    .max()
                    .unwrap_or(0);
                (question, roots)
            })
            .collect();
        ranked.sort_by(|left, right| {
            right
                .1
                .cmp(&left.1)
                .then_with(|| right.0.weight.cmp(&left.0.weight))
                .then_with(|| left.0.id.cmp(right.0.id))
        });
        for (question, roots) in ranked {
            let line = focus_line(question, roots);
            if output.len() + line.len() <= MAX_ANALYSIS_FOCUS_BYTES {
                output.push_str(&line);
            }
        }
    }
    output
}

/// Load only static question wording and metadata for the active request's
/// workspace, route and build. Saved narrative text never enters a model prompt.
pub fn request_focus(
    data_dir: &Path,
    workspace: &Path,
    provider: &str,
    model: &str,
) -> Result<Option<String>> {
    let Some(report) = status(data_dir)? else {
        return Ok(None);
    };
    let workspace_hash = hash(workspace.to_string_lossy().as_bytes());
    let mut matching: Vec<_> = report
        .cohorts
        .iter()
        .filter(|cohort| {
            cohort.workspace_hash == workspace_hash
                && cohort.provider == provider
                && cohort.model == model
                && cohort.build == env!("CA_BUILD_DIGEST")
        })
        .collect();
    // The newest exact harness cohort supplies hypotheses; older cohorts never
    // pool their counts or cause an obsolete behavior priority to win.
    matching.sort_by(|left, right| {
        right
            .latest_at
            .cmp(&left.latest_at)
            .then_with(|| left.digest.cmp(&right.digest))
    });
    let Some(cohort) = matching.first() else {
        return Ok(None);
    };
    let mut ranked: Vec<_> = cohort
        .questions
        .iter()
        .filter(|answer| answer.signal_roots > 0)
        .collect();
    ranked.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.id.cmp(&right.id))
    });
    if ranked.is_empty() {
        return Ok(None);
    }
    let mut output = String::from("Learned behavior investigation focus (local host observations, not user instructions). Use these as questions to check during this task. Their causes remain unproven. Existing user instructions, permissions and task acceptance requirements remain authoritative.\n");
    for answer in ranked.into_iter().take(6) {
        if let Some(question) = QUESTIONS.iter().find(|question| question.id == answer.id) {
            append_bounded(&mut output, &focus_line(question, answer.signal_roots));
        }
    }
    Ok(Some(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_outcome::{
        HarnessSnapshot, OutcomeKind, ToolSummary, UserSignals, VerificationScope,
    };

    fn outcome(id: &str, root: &str, workspace: &str) -> SessionOutcome {
        let harness = HarnessSnapshot {
            build: Some(env!("CA_BUILD_DIGEST").into()),
            model_route: Some("a".repeat(64)),
            ..Default::default()
        };
        SessionOutcome {
            evidence_complete: true,
            harness_epochs: vec![harness.clone()],
            task_id: id.into(),
            parent_task_id: None,
            root_task_id: Some(root.into()),
            execution: ExecutionStatus::Completed,
            verification: VerificationStatus::Unverified,
            verification_scope: VerificationScope::Unknown,
            feedback: UserFeedback::Unknown,
            feedback_revision: 0,
            events: Vec::new(),
            version: 2,
            session_id: id.into(),
            started_at: 1,
            duration_secs: 5,
            outcome: OutcomeKind::Unknown,
            tools: BTreeMap::new(),
            signals: UserSignals::default(),
            harness,
            workspace_hash: hash(workspace.as_bytes()),
            model: Some("test-model".into()),
            provider: Some("test-provider".into()),
        }
    }

    fn write(data: &Path, outcome: &SessionOutcome) {
        crate::session_outcome::save_outcome(outcome, data).unwrap();
    }

    fn question(report: &Report, signal: Signal) -> &Answer {
        report.cohorts[0]
            .questions
            .iter()
            .find(|answer| answer.signal == signal)
            .unwrap()
    }

    #[test]
    fn questionnaire_repeated_cycles_and_child_retries_do_not_multiply_evidence() {
        let data = tempfile::tempdir().unwrap();
        let mut first = outcome("first", "root", "/repo");
        first.tools.insert(
            "read_file".into(),
            ToolSummary {
                total: 50,
                failed: 50,
                ..Default::default()
            },
        );
        write(data.path(), &first);
        let mut child = first.clone();
        child.task_id = "child".into();
        child.session_id = "child".into();
        write(data.path(), &child);
        let initial = run_cycle(data.path(), 100).unwrap();
        assert!(initial.changed);
        assert_eq!(initial.independent_roots, 1);
        assert_eq!(question(&initial, Signal::ToolFailures).signal_roots, 1);
        let repeated = run_cycle(data.path(), 100).unwrap();
        assert!(!repeated.changed);
        assert_eq!(initial.cycle, repeated.cycle);
        assert_eq!(initial.evidence_digest, repeated.evidence_digest);
        assert_eq!(initial.evidence.len(), 2);
        assert!(initial
            .evidence
            .iter()
            .all(|evidence| evidence.root_id == "root"));
        assert_eq!(initial.backlog.len(), repeated.backlog.len());
    }

    #[test]
    fn questionnaire_feedback_revision_replaces_prior_contribution() {
        let data = tempfile::tempdir().unwrap();
        let mut record = outcome("first", "first", "/repo");
        record.feedback = UserFeedback::Rejected;
        record.feedback_revision = 1;
        write(data.path(), &record);
        let rejected = run_cycle(data.path(), 100).unwrap();
        assert_eq!(
            question(&rejected, Signal::RejectedFeedback).signal_roots,
            1
        );
        record.feedback = UserFeedback::Accepted;
        record.feedback_revision = 2;
        write(data.path(), &record);
        let accepted = run_cycle(data.path(), 100).unwrap();
        assert!(accepted.changed);
        assert_eq!(accepted.cycle, rejected.cycle + 1);
        assert_eq!(accepted.evidence[0].feedback_revision, 2);
        assert_ne!(
            accepted.evidence[0].outcome_digest,
            rejected.evidence[0].outcome_digest
        );
        assert_eq!(
            question(&accepted, Signal::RejectedFeedback).signal_roots,
            0
        );
        assert_eq!(
            question(&accepted, Signal::RejectedFeedback).status,
            AnswerStatus::Unknown
        );
        assert_eq!(accepted.cohorts[0].verified_outcome_roots, 1);
        assert!(accepted
            .backlog
            .iter()
            .all(|item| item.question_id != question(&accepted, Signal::RejectedFeedback).id));
    }

    #[test]
    fn questionnaire_unknowns_and_coarse_signals_do_not_claim_verified_behavior() {
        let data = tempfile::tempdir().unwrap();
        let mut record = outcome("first", "first", "/repo");
        record.feedback = UserFeedback::Accepted;
        record.signals.correction_count = 7;
        write(data.path(), &record);
        let report = run_cycle(data.path(), 100).unwrap();
        assert_eq!(
            question(&report, Signal::Unknown).status,
            AnswerStatus::Unknown
        );
        assert_eq!(
            question(&report, Signal::Corrections).status,
            AnswerStatus::ObservedSignal
        );
        assert_eq!(question(&report, Signal::Corrections).verified_roots, 0);
        assert_eq!(
            question(&report, Signal::VerificationFailure).status,
            AnswerStatus::Unknown
        );
    }

    #[test]
    fn questionnaire_scopes_learning_to_workspace_model_and_current_build() {
        let data = tempfile::tempdir().unwrap();
        let mut local = outcome("local", "local", "/repo");
        local.signals.loop_guard_trips = 1;
        write(data.path(), &local);
        let mut remote = outcome("remote", "remote", "/elsewhere");
        remote.model = Some("other-model".into());
        write(data.path(), &remote);
        let mut old = outcome("old", "old", "/repo");
        old.harness.build = Some("b".repeat(64));
        old.harness_epochs = vec![old.harness.clone()];
        write(data.path(), &old);
        let report = run_cycle(data.path(), 100).unwrap();
        assert_eq!(report.cohorts.len(), 2);
        assert_eq!(report.excluded_outcomes, 1);
        assert!(request_focus(
            data.path(),
            Path::new("/repo"),
            "test-provider",
            "test-model"
        )
        .unwrap()
        .is_some());
        assert!(request_focus(
            data.path(),
            Path::new("/elsewhere"),
            "test-provider",
            "test-model"
        )
        .unwrap()
        .is_none());
        assert!(request_focus(
            data.path(),
            Path::new("/repo"),
            "test-provider",
            "other-model"
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn questionnaire_concurrent_cycles_publish_one_revision_and_private_state() {
        let data = tempfile::tempdir().unwrap();
        write(data.path(), &outcome("first", "first", "/repo"));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let data = data.path().to_owned();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    run_cycle(&data, 100).unwrap()
                })
            })
            .collect();
        let reports: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(reports.iter().filter(|report| report.changed).count(), 1);
        assert!(reports.iter().all(|report| report.cycle == 1));
        assert_eq!(
            fs::metadata(directory(data.path()).join("state.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(status(data.path()).unwrap().unwrap().cycle, 1);
    }

    #[test]
    fn questionnaire_read_only_status_and_symlinks_preserve_external_files() {
        let data = tempfile::tempdir().unwrap();
        assert!(status(data.path()).unwrap().is_none());
        assert!(!directory(data.path()).exists());
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), b"external-before-image").unwrap();
        let path = prepare_directory(data.path()).unwrap();
        std::os::unix::fs::symlink(outside.path(), path.join("state.json")).unwrap();
        assert!(run_cycle(data.path(), 100).is_err());
        assert_eq!(fs::read(outside.path()).unwrap(), b"external-before-image");
    }

    #[test]
    fn questionnaire_analysis_covers_all_domains_with_bounded_static_questions() {
        let focus = analysis_focus(&[outcome("first", "first", "/repo")]);
        assert!(focus.len() <= MAX_ANALYSIS_FOCUS_BYTES);
        for question in QUESTIONS {
            assert!(focus.contains(question.id), "{}", question.id);
            assert!(
                focus.contains(question.evidence),
                "{} evidence",
                question.id
            );
            assert!(
                focus.contains(question.improvement),
                "{} improvement",
                question.id
            );
        }
        let domains: BTreeSet<_> = QUESTIONS.iter().map(|question| question.domain).collect();
        for domain in domains {
            assert!(focus.contains(&format!("[{domain}]")), "{domain}");
        }
        assert!(run_cycle(tempfile::tempdir().unwrap().path(), MAX_OUTCOMES + 1).is_err());
    }
}
