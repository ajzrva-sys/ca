use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const MAX_MEMORY_RECORDS: usize = 512;
pub const MAX_RECORD_BYTES: usize = 4 * 1024;
pub const MAX_STORE_BYTES: usize = 256 * 1024;
pub const MAX_INDEX_LINES: usize = 200;
pub const MAX_INDEX_BYTES: usize = 25 * 1024;
const MAX_ALWAYS_RECALL_RECORDS: usize = 8;
const MAX_ALWAYS_RECALL_BYTES: usize = 4 * 1024;
const MEMORY_BM25_K1: f64 = 1.2;
const MEMORY_BM25_LENGTH_NORMALIZATION: f64 = 0.75;
const MAX_MEMORY_QUERY_TERMS: usize = 128;
const MEMORY_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    User,
    Workspace,
}

impl MemoryScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Workspace => "workspace",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    UserPreference,
    UserWorkflow,
    WorkspaceConvention,
    WorkspaceDecision,
    ExternalFact,
}

impl MemoryKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::UserPreference => "preference",
            Self::UserWorkflow => "workflow",
            Self::WorkspaceConvention => "workspace convention",
            Self::WorkspaceDecision => "workspace decision",
            Self::ExternalFact => "external fact",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryProvenance {
    Manual,
    ApprovedProposal,
    Import,
    Dream,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecord {
    pub id: Uuid,
    pub scope: MemoryScope,
    pub kind: MemoryKind,
    pub text: String,
    pub provenance: MemoryProvenance,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintenance_run_id: Option<Uuid>,
}

impl std::fmt::Debug for MemoryRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MemoryRecord")
            .field("id", &self.id)
            .field("scope", &self.scope)
            .field("kind", &self.kind)
            .field("provenance", &self.provenance)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("maintenance_run_id", &self.maintenance_run_id)
            .field("text", &"[BOUNDED MEMORY TEXT]")
            .finish()
    }
}

impl MemoryRecord {
    pub fn new(
        scope: MemoryScope,
        kind: MemoryKind,
        text: impl Into<String>,
        provenance: MemoryProvenance,
    ) -> Result<Self, MemoryError> {
        let text = validate_text(scope, kind, &text.into())?;
        let now = Utc::now();
        Ok(Self {
            id: Uuid::new_v4(),
            scope,
            kind,
            text,
            provenance,
            created_at: now,
            updated_at: now,
            maintenance_run_id: None,
        })
    }

    pub fn dream(
        id: Uuid,
        scope: MemoryScope,
        kind: MemoryKind,
        text: impl Into<String>,
        run_id: Uuid,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, MemoryError> {
        Ok(Self {
            id,
            scope,
            kind,
            text: validate_text(scope, kind, &text.into())?,
            provenance: MemoryProvenance::Dream,
            created_at,
            updated_at,
            maintenance_run_id: Some(run_id),
        })
    }

    pub fn digest(&self) -> Result<String, MemoryError> {
        let bytes = serde_json::to_vec(self).map_err(|_| MemoryError::InvalidStore)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MemoryProposal {
    pub id: Uuid,
    pub scope: MemoryScope,
    pub kind: MemoryKind,
    pub text: String,
}

impl std::fmt::Debug for MemoryProposal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MemoryProposal")
            .field("id", &self.id)
            .field("scope", &self.scope)
            .field("kind", &self.kind)
            .field("text", &"[BOUNDED PROPOSAL]")
            .finish()
    }
}

impl MemoryProposal {
    pub fn new(
        scope: MemoryScope,
        kind: MemoryKind,
        text: impl Into<String>,
    ) -> Result<Self, MemoryError> {
        Ok(Self {
            id: Uuid::new_v4(),
            scope,
            kind,
            text: validate_text(scope, kind, &text.into())?,
        })
    }

    pub fn approve(self) -> MemoryRecord {
        let now = Utc::now();
        MemoryRecord {
            id: self.id,
            scope: self.scope,
            kind: self.kind,
            text: self.text,
            provenance: MemoryProvenance::ApprovedProposal,
            created_at: now,
            updated_at: now,
            maintenance_run_id: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryFile {
    version: u8,
    records: Vec<MemoryRecord>,
}

#[derive(Clone)]
pub struct MemoryStore {
    root: PathBuf,
    path: PathBuf,
    scope: MemoryScope,
    writes_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStoreSnapshot {
    pub records: Vec<MemoryRecord>,
    pub digest: String,
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MemoryStore")
            .field("scope", &self.scope)
            .field("writes_enabled", &self.writes_enabled)
            .finish_non_exhaustive()
    }
}

impl MemoryStore {
    pub fn user(
        paths: &crate::runtime::RuntimePaths,
        profile: &crate::runtime::ActiveProfile,
        writes_enabled: bool,
    ) -> Result<Self, MemoryError> {
        Self::new(
            paths.profile_data_dir(profile),
            paths.user_memory_file(profile),
            MemoryScope::User,
            writes_enabled,
        )
    }

    pub fn workspace(
        paths: &crate::runtime::RuntimePaths,
        profile: &crate::runtime::ActiveProfile,
        canonical_workspace: &Path,
        writes_enabled: bool,
    ) -> Result<Self, MemoryError> {
        Self::new(
            paths.profile_data_dir(profile),
            paths.workspace_memory_file(profile, canonical_workspace),
            MemoryScope::Workspace,
            writes_enabled,
        )
    }

    pub fn new(
        root: impl Into<PathBuf>,
        path: impl Into<PathBuf>,
        scope: MemoryScope,
        writes_enabled: bool,
    ) -> Result<Self, MemoryError> {
        let root = root.into();
        let path = path.into();
        if !path.starts_with(&root) || path == root {
            return Err(MemoryError::UnsafePath);
        }
        Ok(Self {
            root,
            path,
            scope,
            writes_enabled,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Vec<MemoryRecord>, MemoryError> {
        self.validate_existing_components(false)?;
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(MemoryError::Io(error)),
        };
        if !metadata.file_type().is_file() || metadata.len() > MAX_STORE_BYTES as u64 {
            return Err(MemoryError::InvalidStore);
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(&self.path)?
            .take((MAX_STORE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err(MemoryError::InvalidStore);
        }
        let file: MemoryFile =
            serde_json::from_slice(&bytes).map_err(|_| MemoryError::InvalidStore)?;
        if file.version != MEMORY_VERSION || file.records.len() > MAX_MEMORY_RECORDS {
            return Err(MemoryError::InvalidStore);
        }
        for record in &file.records {
            if record.scope != self.scope
                || validate_text(record.scope, record.kind, &record.text).is_err()
                || (record.provenance == MemoryProvenance::Dream
                    && record.maintenance_run_id.is_none())
                || (record.provenance != MemoryProvenance::Dream
                    && record.maintenance_run_id.is_some())
            {
                return Err(MemoryError::InvalidStore);
            }
        }
        Ok(file.records)
    }

    pub fn snapshot(&self) -> Result<MemoryStoreSnapshot, MemoryError> {
        let records = self.load()?;
        let digest = memory_records_digest(&records)?;
        Ok(MemoryStoreSnapshot { records, digest })
    }

    pub fn upsert(&self, record: MemoryRecord) -> Result<(), MemoryError> {
        if !self.writes_enabled {
            return Err(MemoryError::ReadOnly);
        }
        if record.scope != self.scope {
            return Err(MemoryError::WrongScope);
        }
        validate_text(record.scope, record.kind, &record.text)?;
        let mut records = self.load()?;
        if let Some(existing) = records.iter_mut().find(|item| item.id == record.id) {
            *existing = record;
        } else {
            if records.len() >= MAX_MEMORY_RECORDS {
                return Err(MemoryError::TooManyRecords);
            }
            records.push(record);
        }
        self.write_records(&records)
    }

    pub fn delete(&self, id: Uuid) -> Result<bool, MemoryError> {
        if !self.writes_enabled {
            return Err(MemoryError::ReadOnly);
        }
        let mut records = self.load()?;
        let before = records.len();
        records.retain(|record| record.id != id);
        if records.len() == before {
            return Ok(false);
        }
        self.write_records(&records)?;
        Ok(true)
    }

    pub fn replace_all_cas(
        &self,
        expected_digest: &str,
        records: &[MemoryRecord],
    ) -> Result<String, MemoryError> {
        if !self.writes_enabled {
            return Err(MemoryError::ReadOnly);
        }
        let current = self.snapshot()?;
        if current.digest != expected_digest {
            return Err(MemoryError::Conflict);
        }
        if records.len() > MAX_MEMORY_RECORDS {
            return Err(MemoryError::TooManyRecords);
        }
        for record in records {
            if record.scope != self.scope
                || validate_text(record.scope, record.kind, &record.text).is_err()
                || (record.provenance == MemoryProvenance::Dream
                    && record.maintenance_run_id.is_none())
                || (record.provenance != MemoryProvenance::Dream
                    && record.maintenance_run_id.is_some())
            {
                return Err(MemoryError::InvalidStore);
            }
        }
        let next_digest = memory_records_digest(records)?;
        self.write_records(records)?;
        Ok(next_digest)
    }

    fn write_records(&self, records: &[MemoryRecord]) -> Result<(), MemoryError> {
        if records.len() > MAX_MEMORY_RECORDS {
            return Err(MemoryError::TooManyRecords);
        }
        self.validate_existing_components(true)?;
        let parent = self.path.parent().ok_or(MemoryError::UnsafePath)?;
        fs::create_dir_all(parent)?;
        self.validate_existing_components(true)?;
        let encoded = serde_json::to_vec_pretty(&MemoryFile {
            version: MEMORY_VERSION,
            records: records.to_vec(),
        })
        .map_err(|_| MemoryError::InvalidStore)?;
        if encoded.len() > MAX_STORE_BYTES {
            return Err(MemoryError::StoreTooLarge);
        }
        let temporary = parent.join(format!(".memory-{}.tmp", Uuid::new_v4()));
        let result = (|| -> Result<(), MemoryError> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(&encoded)?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn validate_existing_components(&self, include_parent: bool) -> Result<(), MemoryError> {
        if fs::symlink_metadata(&self.root).is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(MemoryError::UnsafePath);
        }
        let target = if include_parent {
            self.path.parent().ok_or(MemoryError::UnsafePath)?
        } else {
            &self.path
        };
        let relative = target
            .strip_prefix(&self.root)
            .map_err(|_| MemoryError::UnsafePath)?;
        let mut current = self.root.clone();
        for component in relative.components() {
            current.push(component);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(MemoryError::UnsafePath)
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(MemoryError::Io(error)),
            }
        }
        Ok(())
    }
}

pub fn memory_records_digest(records: &[MemoryRecord]) -> Result<String, MemoryError> {
    let bytes = serde_json::to_vec(records).map_err(|_| MemoryError::InvalidStore)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn render_recall_index(scope: MemoryScope, records: &[MemoryRecord]) -> String {
    let mut output = format!(
        "{} memory (may be stale; reverify paths, versions, and facts before use):\n",
        match scope {
            MemoryScope::User => "User",
            MemoryScope::Workspace => "Workspace",
        }
    );
    let mut lines = 1usize;
    for record in records.iter().filter(|record| record.scope == scope) {
        for (index, line) in record.text.lines().enumerate() {
            if lines >= MAX_INDEX_LINES {
                break;
            }
            let prefix = if index == 0 {
                format!("- [{}] ", record.kind.label())
            } else {
                "  ".to_string()
            };
            let candidate = format!("{prefix}{line}\n");
            if output.len() + candidate.len() > MAX_INDEX_BYTES {
                break;
            }
            output.push_str(&candidate);
            lines += 1;
        }
        if lines >= MAX_INDEX_LINES || output.len() >= MAX_INDEX_BYTES {
            break;
        }
    }
    output
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRecallReason {
    Always,
    Query,
    HybridQuery,
}

#[derive(Debug, Clone)]
pub struct MemoryRecallMatch<'a> {
    pub record: &'a MemoryRecord,
    pub score: f64,
    pub matched_terms: usize,
    pub reason: MemoryRecallReason,
}

pub fn render_recall_index_for_query(
    scope: MemoryScope,
    records: &[MemoryRecord],
    query: &str,
) -> String {
    render_selected_recall(
        scope,
        select_memory_for_query(scope, records, query)
            .into_iter()
            .map(|candidate| candidate.record)
            .collect(),
    )
}

pub fn select_memory_for_query<'a>(
    scope: MemoryScope,
    records: &'a [MemoryRecord],
    query: &str,
) -> Vec<MemoryRecallMatch<'a>> {
    let scoped = records
        .iter()
        .filter(|record| record.scope == scope)
        .collect::<Vec<_>>();
    if scoped.is_empty() {
        return Vec::new();
    }

    let mut broad = scoped
        .iter()
        .copied()
        .filter(|record| broadly_applicable(record))
        .collect::<Vec<_>>();
    broad.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut selected = Vec::<MemoryRecallMatch>::new();
    let mut selected_ids = HashSet::new();
    let mut broad_bytes = 0usize;
    for record in broad.into_iter().take(MAX_ALWAYS_RECALL_RECORDS) {
        let rendered = render_memory_record(record);
        if broad_bytes.saturating_add(rendered.len()) > MAX_ALWAYS_RECALL_BYTES {
            continue;
        }
        broad_bytes = broad_bytes.saturating_add(rendered.len());
        selected_ids.insert(record.id);
        selected.push(MemoryRecallMatch {
            record,
            score: 0.0,
            matched_terms: 0,
            reason: MemoryRecallReason::Always,
        });
    }

    let query_terms = unique_memory_terms(query);
    if !query_terms.is_empty() {
        let documents = scoped
            .iter()
            .copied()
            .filter(|record| !selected_ids.contains(&record.id))
            .map(|record| (record, weighted_memory_terms(record)))
            .collect::<Vec<_>>();
        let average_length = if documents.is_empty() {
            1.0
        } else {
            documents
                .iter()
                .map(|(_, terms)| terms.len())
                .sum::<usize>() as f64
                / documents.len() as f64
        };
        let mut document_frequency = HashMap::<String, usize>::new();
        for (_, document) in &documents {
            for term in document.iter().collect::<HashSet<_>>() {
                *document_frequency.entry(term.clone()).or_default() += 1;
            }
        }
        let document_count = documents.len() as f64;
        let mut ranked = documents
            .into_iter()
            .filter_map(|(record, document)| {
                let mut score = 0.0_f64;
                let mut matched = 0usize;
                for term in &query_terms {
                    let frequency = document
                        .iter()
                        .filter(|candidate| *candidate == term)
                        .count();
                    if frequency == 0 {
                        continue;
                    }
                    matched += 1;
                    let frequency = frequency as f64;
                    let document_frequency =
                        document_frequency.get(term).copied().unwrap_or(0) as f64;
                    let inverse_document_frequency = (1.0
                        + (document_count - document_frequency + 0.5) / (document_frequency + 0.5))
                        .ln();
                    let length_adjustment = 1.0 - MEMORY_BM25_LENGTH_NORMALIZATION
                        + MEMORY_BM25_LENGTH_NORMALIZATION
                            * (document.len() as f64 / average_length.max(1.0));
                    score += inverse_document_frequency * (frequency * (MEMORY_BM25_K1 + 1.0))
                        / (frequency + MEMORY_BM25_K1 * length_adjustment);
                }
                (score > 0.0).then_some((record, score, matched))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| right.0.updated_at.cmp(&left.0.updated_at))
                .then_with(|| left.0.id.cmp(&right.0.id))
        });
        selected.extend(ranked.into_iter().map(|(record, score, matched_terms)| {
            MemoryRecallMatch {
                record,
                score,
                matched_terms,
                reason: MemoryRecallReason::Query,
            }
        }));
    }

    let mut bytes = recall_header(scope).len();
    let mut lines = 1usize;
    selected.retain(|candidate| {
        let rendered = render_memory_record(candidate.record);
        let record_lines = rendered.lines().count();
        if lines.saturating_add(record_lines) > MAX_INDEX_LINES
            || bytes.saturating_add(rendered.len()) > MAX_INDEX_BYTES
        {
            return false;
        }
        bytes += rendered.len();
        lines += record_lines;
        true
    });
    selected
}

fn broadly_applicable(record: &MemoryRecord) -> bool {
    matches!(
        (record.scope, record.kind),
        (
            MemoryScope::User,
            MemoryKind::UserPreference | MemoryKind::UserWorkflow
        ) | (
            MemoryScope::Workspace,
            MemoryKind::WorkspaceConvention | MemoryKind::WorkspaceDecision
        )
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecallBenchmarkDataset {
    pub name: String,
    pub scope: MemoryScope,
    pub records: Vec<RecallBenchmarkRecord>,
    pub cases: Vec<RecallBenchmarkCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecallBenchmarkRecord {
    pub label: String,
    pub scope: MemoryScope,
    pub kind: MemoryKind,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecallBenchmarkCase {
    pub query: String,
    pub relevant: Vec<String>,
    #[serde(default)]
    pub identifier: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RecallBenchmarkReport {
    pub dataset: String,
    pub k: usize,
    pub positive_cases: usize,
    pub negative_cases: usize,
    pub precision_at_k: f64,
    pub recall_at_k: f64,
    pub mrr: f64,
    pub false_positive_rate: f64,
    pub cases: Vec<RecallBenchmarkCaseReport>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RecallBenchmarkCaseReport {
    pub query: String,
    pub query_results: Vec<String>,
    pub always_recalled: Vec<String>,
    pub selected_bytes: usize,
    pub precision_at_k: f64,
    pub recall_at_k: f64,
    pub reciprocal_rank: f64,
}

/// Offline evaluation over an explicit synthetic or caller-supplied fixture.
/// Always-recalled preferences are separated from query-triggered results, so
/// a negative query is not incorrectly charged for an intentional preference.
pub fn evaluate_recall_benchmark(
    dataset: &RecallBenchmarkDataset,
    k: usize,
) -> Result<RecallBenchmarkReport, String> {
    evaluate_recall_benchmark_ranked(dataset, k, None)
}

/// A reproducible semantic score fixture evaluates fusion and negative-query
/// calibration independently of a particular model or embedding runtime.
pub fn evaluate_recall_benchmark_with_semantics(
    dataset: &RecallBenchmarkDataset,
    k: usize,
    minimum_similarity: f64,
    samples: &[Vec<(String, f64)>],
) -> Result<RecallBenchmarkReport, String> {
    validate_benchmark_semantics(dataset, minimum_similarity, samples)?;
    evaluate_recall_benchmark_ranked(dataset, k, Some((minimum_similarity, samples)))
}

fn validate_benchmark_semantics(
    dataset: &RecallBenchmarkDataset,
    minimum_similarity: f64,
    samples: &[SemanticRecallScoreRow],
) -> Result<(), String> {
    if samples.len() != dataset.cases.len()
        || !minimum_similarity.is_finite()
        || !(0.0..=1.0).contains(&minimum_similarity)
        || samples.iter().any(|scores| {
            scores.len() > MAX_MEMORY_RECORDS
                || scores
                    .iter()
                    .any(|(_, score)| !score.is_finite() || !(-1.0..=1.0).contains(score))
        })
    {
        return Err("recall semantic score fixture exceeds its bounds".into());
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct RecallLatencySummary {
    pub samples: usize,
    pub median_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    pub total_ms: f64,
}

impl RecallLatencySummary {
    fn measured(values: &[f64]) -> Self {
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        let len = sorted.len();
        let median_ms = if len == 0 {
            0.0
        } else if len.is_multiple_of(2) {
            (sorted[len / 2 - 1] + sorted[len / 2]) / 2.0
        } else {
            sorted[len / 2]
        };
        let p95_index = len.saturating_mul(95).div_ceil(100).saturating_sub(1);
        Self {
            samples: len,
            median_ms,
            p95_ms: sorted.get(p95_index).copied().unwrap_or_default(),
            max_ms: sorted.last().copied().unwrap_or_default(),
            total_ms: sorted.iter().sum(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RecallRankingCaseTiming {
    pub query: String,
    pub ranking_ms: f64,
    pub selected_context_bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct RecallRankingLatency {
    pub preprocessing_ms: f64,
    pub ranking: RecallLatencySummary,
    pub cases: Vec<RecallRankingCaseTiming>,
}

#[derive(Debug, Serialize)]
pub struct MeasuredRecallBenchmarkReport {
    #[serde(flatten)]
    pub quality: RecallBenchmarkReport,
    pub latency: RecallRankingLatency,
}

/// The CLI attaches measured ranking and context-selection time without changing
/// deterministic scoring APIs or pretending synthetic scores measure embeddings.
pub(crate) fn evaluate_recall_benchmark_measured(
    dataset: &RecallBenchmarkDataset,
    k: usize,
    semantics: Option<BenchmarkSemanticScores<'_>>,
) -> Result<MeasuredRecallBenchmarkReport, String> {
    if let Some((minimum, samples)) = semantics {
        validate_benchmark_semantics(dataset, minimum, samples)?;
    }
    let start = Instant::now();
    let mut timings = Vec::new();
    let quality =
        evaluate_recall_benchmark_ranked_with_timings(dataset, k, semantics, Some(&mut timings))?;
    let ranking = RecallLatencySummary::measured(&timings);
    let preprocessing_ms = (elapsed_ms(start) - ranking.total_ms).max(0.0);
    let cases = quality
        .cases
        .iter()
        .zip(timings)
        .map(|(case, ranking_ms)| RecallRankingCaseTiming {
            query: case.query.clone(),
            ranking_ms,
            selected_context_bytes: case.selected_bytes,
        })
        .collect();
    Ok(MeasuredRecallBenchmarkReport {
        quality,
        latency: RecallRankingLatency {
            preprocessing_ms,
            ranking,
            cases,
        },
    })
}

#[derive(Debug, Serialize)]
pub struct HybridRecallAcceptance {
    pub enough_samples: bool,
    pub recall_uplift_percentage_points: f64,
    pub identifier_recall_nonregression: bool,
    pub negative_query_gate: bool,
    pub passed: bool,
}

#[derive(Debug, Serialize)]
pub struct HybridRecallCalibration {
    pub minimum_similarity_percent: u8,
    pub recall_at_k: f64,
    pub false_positive_rate: f64,
    pub acceptance: HybridRecallAcceptance,
}

#[derive(Debug, Serialize)]
pub struct LocalRecallBenchmarkReport {
    pub runtime_id: String,
    pub model_id: String,
    pub model_sha256: String,
    pub minimum_similarity_percent: u8,
    pub lexical: RecallBenchmarkReport,
    pub hybrid: RecallBenchmarkReport,
    pub acceptance: HybridRecallAcceptance,
    pub calibration: Vec<HybridRecallCalibration>,
    pub latency: LocalRecallBenchmarkLatency,
}

const WARM_RECALL_LIMIT_MS: u64 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallBenchmarkQueryOutcome {
    Semantic,
    Timeout,
    EmbeddingError,
    SelectionTimeout,
}

#[derive(Debug, Serialize)]
pub struct LocalRecallCaseTiming {
    pub query: String,
    pub lexical_ms: f64,
    pub embedding_ms: f64,
    pub selection_ms: f64,
    /// Embedding plus similarity, fusion, ranking, and context selection. The
    /// separately reported lexical baseline is not subtracted from this bound.
    pub warm_added_ms: f64,
    pub outcome: RecallBenchmarkQueryOutcome,
    pub lexical_context_bytes: usize,
    pub selected_context_bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct LocalRecallColdTiming {
    /// None only for callers supplying an already running synthetic test server.
    pub server_startup_ms: Option<f64>,
    pub indexing_ms: f64,
    pub total_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct LocalRecallBenchmarkLatency {
    pub cold_preparation: LocalRecallColdTiming,
    pub lexical: RecallLatencySummary,
    pub lexical_preprocessing_ms: f64,
    pub warm_attempts: RecallLatencySummary,
    pub warm_semantic: RecallLatencySummary,
    pub warm_limit_ms: u64,
    pub fallback_cases: usize,
    pub warm_gate_passed: bool,
    pub cases: Vec<LocalRecallCaseTiming>,
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

struct BenchmarkEmbeddingAttempt {
    vector: Option<Vec<f32>>,
    outcome: RecallBenchmarkQueryOutcome,
    embedding_ms: f64,
}

fn finish_benchmark_query_timing(timing: &mut LocalRecallCaseTiming, ranking_ms: f64) {
    timing.selection_ms += ranking_ms;
    timing.warm_added_ms = timing.embedding_ms + timing.selection_ms;
    if timing.outcome == RecallBenchmarkQueryOutcome::Semantic
        && timing.warm_added_ms > WARM_RECALL_LIMIT_MS as f64
    {
        timing.outcome = RecallBenchmarkQueryOutcome::SelectionTimeout;
    }
}

async fn timed_benchmark_embedding<F, E>(
    request: F,
    expected_dimensions: Option<usize>,
) -> BenchmarkEmbeddingAttempt
where
    F: std::future::Future<Output = Result<Vec<Vec<f32>>, E>>,
{
    let start = Instant::now();
    let (vector, outcome) =
        match tokio::time::timeout(Duration::from_millis(WARM_RECALL_LIMIT_MS), request).await {
            Ok(Ok(mut vectors))
                if vectors.len() == 1 && expected_dimensions == Some(vectors[0].len()) =>
            {
                (vectors.pop(), RecallBenchmarkQueryOutcome::Semantic)
            }
            Ok(_) => (None, RecallBenchmarkQueryOutcome::EmbeddingError),
            Err(_) => (None, RecallBenchmarkQueryOutcome::Timeout),
        };
    BenchmarkEmbeddingAttempt {
        vector,
        outcome,
        embedding_ms: elapsed_ms(start),
    }
}

fn hybrid_acceptance(
    dataset: &RecallBenchmarkDataset,
    lexical: &RecallBenchmarkReport,
    hybrid: &RecallBenchmarkReport,
) -> HybridRecallAcceptance {
    let enough_samples = lexical.positive_cases >= 6
        && lexical.negative_cases >= 3
        && dataset
            .cases
            .iter()
            .any(|case| case.identifier && !case.relevant.is_empty());
    let recall_uplift_percentage_points = (hybrid.recall_at_k - lexical.recall_at_k) * 100.0;
    let identifier_recall_nonregression = dataset
        .cases
        .iter()
        .zip(&lexical.cases)
        .zip(&hybrid.cases)
        .filter(|((case, _), _)| case.identifier)
        .all(|((_, before), after)| after.recall_at_k >= before.recall_at_k);
    let negative_query_gate = hybrid.false_positive_rate <= 0.05
        && hybrid.false_positive_rate <= lexical.false_positive_rate;
    HybridRecallAcceptance {
        enough_samples,
        recall_uplift_percentage_points,
        identifier_recall_nonregression,
        negative_query_gate,
        passed: enough_samples
            && recall_uplift_percentage_points + f64::EPSILON >= 10.0
            && identifier_recall_nonregression
            && negative_query_gate,
    }
}

fn apply_benchmark_lexical_fallbacks(
    dataset: &RecallBenchmarkDataset,
    lexical: &RecallBenchmarkReport,
    hybrid: &mut RecallBenchmarkReport,
    timings: &[LocalRecallCaseTiming],
) {
    for (index, timing) in timings.iter().enumerate() {
        if timing.outcome != RecallBenchmarkQueryOutcome::Semantic {
            hybrid.cases[index] = lexical.cases[index].clone();
        }
    }
    let positives = hybrid.positive_cases.max(1) as f64;
    let mut false_positives = 0usize;
    hybrid.precision_at_k = 0.0;
    hybrid.recall_at_k = 0.0;
    hybrid.mrr = 0.0;
    for (case, outcome) in dataset.cases.iter().zip(&hybrid.cases) {
        if case.relevant.is_empty() {
            false_positives += usize::from(!outcome.query_results.is_empty());
        } else {
            hybrid.precision_at_k += outcome.precision_at_k;
            hybrid.recall_at_k += outcome.recall_at_k;
            hybrid.mrr += outcome.reciprocal_rank;
        }
    }
    hybrid.precision_at_k /= positives;
    hybrid.recall_at_k /= positives;
    hybrid.mrr /= positives;
    hybrid.false_positive_rate = false_positives as f64 / hybrid.negative_cases.max(1) as f64;
}

/// Explicit offline fixture evaluation against an already registered local
/// runtime and model. Never selects or downloads an artifact, writes a cache,
/// changes settings, or enables retrieval. The report is bound to the verified
/// model hash and includes negative-query calibration; a failed gate is data.
pub async fn evaluate_local_recall_benchmark(
    dataset: &RecallBenchmarkDataset,
    k: usize,
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    config: &crate::config::LocalHybridMemoryConfig,
) -> Result<LocalRecallBenchmarkReport, String> {
    let lexical = evaluate_recall_benchmark(dataset, k)?;
    if matches!(profile, crate::runtime::ActiveProfile::Bare)
        || config.minimum_similarity_percent > 100
    {
        return Err("Local recall benchmark requires an explicit registered model profile and similarity 0-100".into());
    }
    // The entire process-owning future is bounded. Dropping it also drops the
    // supervised server, so timeout cannot leave an embedding worker behind.
    tokio::time::timeout(std::time::Duration::from_secs(180), async {
        let startup_start = Instant::now();
        let server = crate::local_models::start_managed_embedding_server(
            paths,
            profile,
            &config.runtime_id,
            &config.model_id,
            config.context_size,
            std::time::Duration::from_secs(config.startup_timeout_secs),
        )
        .await
        .map_err(|error| error.to_string())?;
        let startup_ms = elapsed_ms(startup_start);
        let mut report =
            evaluate_local_recall_with_server(dataset, k, config, lexical, server).await?;
        report.latency.cold_preparation.server_startup_ms = Some(startup_ms);
        report.latency.cold_preparation.total_ms += startup_ms;
        Ok(report)
    })
    .await
    .map_err(|_| "Local recall benchmark exceeded its 180-second bound".to_string())?
}

async fn evaluate_local_recall_with_server(
    dataset: &RecallBenchmarkDataset,
    k: usize,
    config: &crate::config::LocalHybridMemoryConfig,
    _validated_lexical: RecallBenchmarkReport,
    server: crate::local_models::ManagedEmbeddingServer,
) -> Result<LocalRecallBenchmarkReport, String> {
    let measured_lexical = evaluate_recall_benchmark_measured(dataset, k, None)?;
    let lexical = measured_lexical.quality;
    let lexical_latency = measured_lexical.latency;
    let indexing_start = Instant::now();
    let mut records = Vec::new();
    let mut dimensions = None;
    let mut vector_bytes = 0usize;
    for record in dataset
        .records
        .iter()
        .filter(|record| record.scope == dataset.scope)
    {
        if crate::learning::sanitized_recall_text(&record.text, MAX_RECORD_BYTES).is_none() {
            return Err("Local recall benchmark cannot embed private record content".into());
        }
        let chunks = memory_embedding_chunks(&record.text);
        let mut vectors = Vec::new();
        for batch in chunks.chunks(16) {
            for vector in server
                .embed(batch)
                .await
                .map_err(|error| error.to_string())?
            {
                if dimensions.is_some_and(|dimension| dimension != vector.len()) {
                    return Err("Local benchmark embedding dimensions changed".into());
                }
                dimensions = Some(vector.len());
                vector_bytes = vector_bytes.saturating_add(vector.len() * 4);
                if vector_bytes > MAX_HYBRID_VECTOR_BYTES {
                    return Err("Local benchmark vector cache exceeds 32 MiB".into());
                }
                vectors.push(vector);
            }
        }
        records.push((record.label.clone(), vectors));
    }
    let indexing_ms = elapsed_ms(indexing_start);
    let mut samples = Vec::new();
    let mut query_timings = Vec::new();
    for (case_index, case) in dataset.cases.iter().enumerate() {
        if case.query.is_empty()
            || crate::learning::sanitized_recall_text(&case.query, MAX_RECORD_BYTES).is_none()
        {
            return Err("Local recall benchmark requires nonempty nonprivate query content".into());
        }
        let attempt =
            timed_benchmark_embedding(server.embed(std::slice::from_ref(&case.query)), dimensions)
                .await;
        let similarity_start = Instant::now();
        samples.push(attempt.vector.as_ref().map_or_else(Vec::new, |query| {
            records
                .iter()
                .filter_map(|(label, chunks)| {
                    maximum_vector_similarity(chunks, query).map(|score| (label.clone(), score))
                })
                .collect::<Vec<_>>()
        }));
        query_timings.push(LocalRecallCaseTiming {
            query: case.query.clone(),
            lexical_ms: lexical_latency.cases[case_index].ranking_ms,
            embedding_ms: attempt.embedding_ms,
            selection_ms: elapsed_ms(similarity_start),
            warm_added_ms: 0.0,
            outcome: attempt.outcome,
            lexical_context_bytes: lexical.cases[case_index].selected_bytes,
            selected_context_bytes: 0,
        });
    }
    let minimum_similarity = f64::from(config.minimum_similarity_percent) / 100.0;
    let mut selection_timings = Vec::new();
    let mut hybrid = evaluate_recall_benchmark_ranked_with_timings(
        dataset,
        k,
        Some((minimum_similarity, &samples)),
        Some(&mut selection_timings),
    )?;
    for (case_index, (timing, selection_ms)) in
        query_timings.iter_mut().zip(selection_timings).enumerate()
    {
        finish_benchmark_query_timing(timing, selection_ms);
        if timing.outcome == RecallBenchmarkQueryOutcome::SelectionTimeout {
            samples[case_index].clear();
        }
    }
    // Preserve the exact lexical context, including its wider candidate limit.
    // A missed deadline cannot retain unattainable semantic quality improvement.
    apply_benchmark_lexical_fallbacks(dataset, &lexical, &mut hybrid, &query_timings);
    for (timing, case) in query_timings.iter_mut().zip(&hybrid.cases) {
        timing.selected_context_bytes = case.selected_bytes;
    }
    let fallback_cases = query_timings
        .iter()
        .filter(|case| case.outcome != RecallBenchmarkQueryOutcome::Semantic)
        .count();
    let warm_gate_passed = !query_timings.is_empty()
        && fallback_cases == 0
        && query_timings
            .iter()
            .all(|case| case.warm_added_ms <= WARM_RECALL_LIMIT_MS as f64);
    let mut acceptance = hybrid_acceptance(dataset, &lexical, &hybrid);
    acceptance.passed &= warm_gate_passed;
    let mut thresholds = (5..=95).step_by(5).collect::<Vec<u8>>();
    thresholds.push(config.minimum_similarity_percent);
    thresholds.sort_unstable();
    thresholds.dedup();
    let calibration = thresholds
        .into_iter()
        .map(|minimum_similarity_percent| {
            let mut report = evaluate_recall_benchmark_with_semantics(
                dataset,
                k,
                f64::from(minimum_similarity_percent) / 100.0,
                &samples,
            )?;
            apply_benchmark_lexical_fallbacks(dataset, &lexical, &mut report, &query_timings);
            let mut acceptance = hybrid_acceptance(dataset, &lexical, &report);
            acceptance.passed &= warm_gate_passed;
            Ok(HybridRecallCalibration {
                minimum_similarity_percent,
                recall_at_k: report.recall_at_k,
                false_positive_rate: report.false_positive_rate,
                acceptance,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let warm_attempts = RecallLatencySummary::measured(
        &query_timings
            .iter()
            .map(|case| case.warm_added_ms)
            .collect::<Vec<_>>(),
    );
    let warm_semantic = RecallLatencySummary::measured(
        &query_timings
            .iter()
            .filter(|case| case.outcome == RecallBenchmarkQueryOutcome::Semantic)
            .map(|case| case.warm_added_ms)
            .collect::<Vec<_>>(),
    );
    Ok(LocalRecallBenchmarkReport {
        runtime_id: config.runtime_id.clone(),
        model_id: config.model_id.clone(),
        model_sha256: server.model_digest().into(),
        minimum_similarity_percent: config.minimum_similarity_percent,
        lexical,
        hybrid,
        acceptance,
        calibration,
        latency: LocalRecallBenchmarkLatency {
            cold_preparation: LocalRecallColdTiming {
                server_startup_ms: None,
                indexing_ms,
                total_ms: indexing_ms,
            },
            lexical: lexical_latency.ranking,
            lexical_preprocessing_ms: lexical_latency.preprocessing_ms,
            warm_attempts,
            warm_semantic,
            warm_limit_ms: WARM_RECALL_LIMIT_MS,
            fallback_cases,
            warm_gate_passed,
            cases: query_timings,
        },
    })
}

pub(crate) type SemanticRecallScoreRow = Vec<(String, f64)>;
pub(crate) type BenchmarkSemanticScores<'a> = (f64, &'a [SemanticRecallScoreRow]);

fn evaluate_recall_benchmark_ranked(
    dataset: &RecallBenchmarkDataset,
    k: usize,
    semantics: Option<BenchmarkSemanticScores<'_>>,
) -> Result<RecallBenchmarkReport, String> {
    evaluate_recall_benchmark_ranked_with_timings(dataset, k, semantics, None)
}

fn evaluate_recall_benchmark_ranked_with_timings(
    dataset: &RecallBenchmarkDataset,
    k: usize,
    semantics: Option<BenchmarkSemanticScores<'_>>,
    mut timings: Option<&mut Vec<f64>>,
) -> Result<RecallBenchmarkReport, String> {
    if !(1..=20).contains(&k)
        || dataset.records.len() > MAX_MEMORY_RECORDS
        || dataset.cases.len() > 512
        || dataset.name.is_empty()
        || dataset.name.len() > 128
    {
        return Err("recall benchmark exceeds its fixture bounds".into());
    }
    let mut records = Vec::new();
    let mut labels = HashMap::new();
    let mut known = HashSet::new();
    let timestamp = DateTime::from_timestamp(0, 0).expect("epoch is valid");
    for (index, fixture) in dataset.records.iter().enumerate() {
        if fixture.label.is_empty()
            || fixture.label.len() > 128
            || fixture.label.chars().any(char::is_control)
            || !known.insert(fixture.label.clone())
        {
            return Err("recall benchmark labels must be unique bounded text".into());
        }
        let mut record = MemoryRecord::new(
            fixture.scope,
            fixture.kind,
            &fixture.text,
            MemoryProvenance::Manual,
        )
        .map_err(|_| "recall benchmark contains an invalid memory record".to_string())?;
        record.id = Uuid::from_u128(index as u128 + 1);
        record.created_at = timestamp;
        record.updated_at = timestamp;
        labels.insert(record.id, fixture.label.clone());
        records.push(record);
    }
    let mut report = RecallBenchmarkReport {
        dataset: dataset.name.clone(),
        k,
        positive_cases: 0,
        negative_cases: 0,
        precision_at_k: 0.0,
        recall_at_k: 0.0,
        mrr: 0.0,
        false_positive_rate: 0.0,
        cases: Vec::new(),
    };
    let mut false_positives = 0usize;
    for (case_index, case) in dataset.cases.iter().enumerate() {
        let case_start = Instant::now();
        if case.query.len() > 1_024 || case.relevant.iter().any(|label| !known.contains(label)) {
            return Err("recall benchmark contains an invalid query or relevance label".into());
        }
        let selected = if let Some((minimum, samples)) = semantics {
            let mut semantic = Vec::new();
            for (label, score) in &samples[case_index] {
                let id = labels
                    .iter()
                    .find_map(|(id, known)| (known == label).then_some(*id))
                    .ok_or("recall semantic fixture refers to an unknown record")?;
                if *score >= minimum {
                    semantic.push((id, *score));
                }
            }
            semantic.sort_by(|left, right| {
                right
                    .1
                    .total_cmp(&left.1)
                    .then_with(|| left.0.cmp(&right.0))
            });
            select_hybrid_memory(dataset.scope, &records, &case.query, &semantic)
        } else {
            select_memory_for_query(dataset.scope, &records, &case.query)
        };
        let query_results = selected
            .iter()
            .filter(|candidate| candidate.reason != MemoryRecallReason::Always)
            .take(k)
            .map(|candidate| labels[&candidate.record.id].clone())
            .collect::<Vec<_>>();
        let always_recalled = selected
            .iter()
            .filter(|candidate| candidate.reason == MemoryRecallReason::Always)
            .map(|candidate| labels[&candidate.record.id].clone())
            .collect();
        let relevant = case.relevant.iter().collect::<HashSet<_>>();
        let hits = query_results
            .iter()
            .filter(|label| relevant.contains(label))
            .count();
        let precision = hits as f64 / k as f64;
        let recall = hits as f64 / relevant.len().max(1) as f64;
        let reciprocal_rank = query_results
            .iter()
            .position(|label| relevant.contains(label))
            .map_or(0.0, |index| 1.0 / (index + 1) as f64);
        if relevant.is_empty() {
            report.negative_cases += 1;
            false_positives += usize::from(!query_results.is_empty());
        } else {
            report.positive_cases += 1;
            report.precision_at_k += precision;
            report.recall_at_k += recall;
            report.mrr += reciprocal_rank;
        }
        let selected_bytes = render_selected_recall(
            dataset.scope,
            selected
                .into_iter()
                .map(|candidate| candidate.record)
                .collect(),
        )
        .len();
        report.cases.push(RecallBenchmarkCaseReport {
            query: case.query.clone(),
            query_results,
            always_recalled,
            selected_bytes,
            precision_at_k: precision,
            recall_at_k: recall,
            reciprocal_rank,
        });
        if let Some(timings) = &mut timings {
            timings.push(elapsed_ms(case_start));
        }
    }
    let positives = report.positive_cases.max(1) as f64;
    report.precision_at_k /= positives;
    report.recall_at_k /= positives;
    report.mrr /= positives;
    report.false_positive_rate = false_positives as f64 / report.negative_cases.max(1) as f64;
    Ok(report)
}

fn unique_memory_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    for term in memory_terms(value) {
        if seen.insert(term.clone()) {
            terms.push(term);
            if terms.len() >= MAX_MEMORY_QUERY_TERMS {
                break;
            }
        }
    }
    terms
}

fn memory_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            if current.len() < 64 {
                current.push(character);
            }
        } else if !current.is_empty() {
            let term = std::mem::take(&mut current);
            if !memory_stopword(&term) {
                terms.push(term);
            }
        }
    }
    if !current.is_empty() && !memory_stopword(&current) {
        terms.push(current);
    }
    terms
}

fn memory_stopword(term: &str) -> bool {
    matches!(
        term,
        "a" | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "by"
            | "for"
            | "from"
            | "in"
            | "is"
            | "it"
            | "of"
            | "on"
            | "or"
            | "that"
            | "the"
            | "this"
            | "to"
            | "was"
            | "were"
            | "with"
    )
}

fn weighted_memory_terms(record: &MemoryRecord) -> Vec<String> {
    let mut terms = memory_terms(record.kind.label());
    let kind_terms = terms.clone();
    terms.extend(kind_terms);
    terms.extend(memory_terms(&record.text));
    terms
}

fn render_memory_record(record: &MemoryRecord) -> String {
    let mut output = String::new();
    for (index, line) in record.text.lines().enumerate() {
        if index == 0 {
            output.push_str(&format!("- [{}] {line}\n", record.kind.label()));
        } else {
            output.push_str("  ");
            output.push_str(line);
            output.push('\n');
        }
    }
    output
}

fn recall_header(scope: MemoryScope) -> String {
    format!(
        "{} memory (selected locally; may be stale; reverify paths, versions, and facts before use):\n",
        match scope {
            MemoryScope::User => "User",
            MemoryScope::Workspace => "Workspace",
        }
    )
}

fn render_selected_recall(scope: MemoryScope, records: Vec<&MemoryRecord>) -> String {
    if records.is_empty() {
        return String::new();
    }
    let mut output = recall_header(scope);
    let mut lines = 1usize;
    for record in records {
        let rendered = render_memory_record(record);
        let record_lines = rendered.lines().count();
        if lines.saturating_add(record_lines) > MAX_INDEX_LINES
            || output.len().saturating_add(rendered.len()) > MAX_INDEX_BYTES
        {
            continue;
        }
        output.push_str(&rendered);
        lines = lines.saturating_add(record_lines);
    }
    if lines == 1 {
        String::new()
    } else {
        output
    }
}

const MAX_HYBRID_VECTOR_BYTES: usize = 32 * 1024 * 1024;
const HYBRID_CHUNK_BYTES: usize = 1024;
const HYBRID_CHUNK_OVERLAP: usize = 128;

#[derive(Clone)]
struct LocalHybridRecall {
    paths: crate::runtime::RuntimePaths,
    profile: crate::runtime::ActiveProfile,
    config: crate::config::LocalHybridMemoryConfig,
    engine: std::sync::Arc<tokio::sync::Mutex<Option<LocalHybridEngine>>>,
}
struct LocalHybridEngine {
    server: crate::local_models::ManagedEmbeddingServer,
    model_digest: String,
    dimensions: Option<usize>,
    entries: HashMap<Uuid, (String, Vec<Vec<f32>>)>,
    vector_bytes: usize,
    ready: bool,
}

fn maximum_vector_similarity(chunks: &[Vec<f32>], query: &[f32]) -> Option<f64> {
    chunks
        .iter()
        .filter(|vector| vector.len() == query.len())
        .map(|vector| {
            vector
                .iter()
                .zip(query)
                .map(|(left, right)| f64::from(*left) * f64::from(*right))
                .sum::<f64>()
                .clamp(-1.0, 1.0)
        })
        .max_by(f64::total_cmp)
}

fn memory_embedding_chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + HYBRID_CHUNK_BYTES).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        chunks.push(text[start..end].to_string());
        if end == text.len() {
            break;
        }
        start = end.saturating_sub(HYBRID_CHUNK_OVERLAP).max(start + 1);
        while !text.is_char_boundary(start) {
            start += 1;
        }
    }
    chunks
}

/// Deterministic reciprocal-rank fusion over native records only. Broad user
/// preferences remain first and keep their existing separate prompt budget.
fn select_hybrid_memory<'a>(
    scope: MemoryScope,
    records: &'a [MemoryRecord],
    query: &str,
    semantic: &[(Uuid, f64)],
) -> Vec<MemoryRecallMatch<'a>> {
    let lexical = select_memory_for_query(scope, records, query);
    let mut broad = lexical
        .iter()
        .filter(|candidate| candidate.reason == MemoryRecallReason::Always)
        .cloned()
        .collect::<Vec<_>>();
    let broad_ids = broad
        .iter()
        .map(|candidate| candidate.record.id)
        .collect::<HashSet<_>>();
    let mut scores = HashMap::<Uuid, (f64, usize)>::new();
    for (rank, candidate) in lexical
        .iter()
        .filter(|candidate| candidate.reason != MemoryRecallReason::Always)
        .take(20)
        .enumerate()
    {
        scores.insert(
            candidate.record.id,
            (1.0 / (60.0 + (rank + 1) as f64), candidate.matched_terms),
        );
    }
    let eligible = records
        .iter()
        .filter(|record| record.scope == scope)
        .map(|record| record.id)
        .collect::<HashSet<_>>();
    for (rank, (id, _)) in semantic
        .iter()
        .filter(|(id, _)| eligible.contains(id))
        .take(20)
        .enumerate()
    {
        if broad_ids.contains(id) {
            continue;
        }
        scores.entry(*id).or_default().0 += 1.0 / (60.0 + (rank + 1) as f64);
    }
    let mut ranked = records
        .iter()
        .filter(|record| record.scope == scope)
        .filter_map(|record| {
            scores
                .get(&record.id)
                .map(|(score, matched_terms)| MemoryRecallMatch {
                    record,
                    score: *score,
                    matched_terms: *matched_terms,
                    reason: MemoryRecallReason::HybridQuery,
                })
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| right.matched_terms.cmp(&left.matched_terms))
            .then_with(|| right.record.updated_at.cmp(&left.record.updated_at))
            .then_with(|| left.record.id.cmp(&right.record.id))
    });
    broad.extend(ranked.into_iter().take(20));
    broad
}

#[derive(Clone, Default)]
pub struct MemoryRecall {
    user_enabled: bool,
    workspace_enabled: bool,
    user_records: Vec<MemoryRecord>,
    workspace_records: Vec<MemoryRecord>,
    foreign: crate::foreign_knowledge::ForeignKnowledgeIndex,
    local_hybrid: Option<LocalHybridRecall>,
}

impl std::fmt::Debug for MemoryRecall {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MemoryRecall")
            .field("user_enabled", &self.user_enabled)
            .field("workspace_enabled", &self.workspace_enabled)
            .field("user_record_count", &self.user_records.len())
            .field("workspace_record_count", &self.workspace_records.len())
            .field("foreign_record_count", &self.foreign.records().len())
            .finish()
    }
}

impl MemoryRecall {
    /// Attaches optional local retrieval without starting a process or reading a
    /// model. Call asynchronous preparation from a worker, never the TUI loop.
    pub fn with_local_hybrid(
        mut self,
        paths: &crate::runtime::RuntimePaths,
        profile: &crate::runtime::ActiveProfile,
        config: Option<&crate::config::LocalHybridMemoryConfig>,
    ) -> Self {
        self.local_hybrid = config
            .filter(|_| !matches!(profile, crate::runtime::ActiveProfile::Bare))
            .map(|config| LocalHybridRecall {
                paths: paths.clone(),
                profile: profile.clone(),
                config: config.clone(),
                engine: Default::default(),
            });
        self
    }

    pub async fn prepare_local_hybrid(&self) -> Result<(), String> {
        let Some(hybrid) = &self.local_hybrid else {
            return Ok(());
        };
        let config = &hybrid.config;
        if config.minimum_similarity_percent > 100 {
            return Err("Local recall similarity must be 0-100 percent".into());
        }
        let mut owned = hybrid.engine.lock().await;
        if owned.is_none() {
            let server = crate::local_models::start_managed_embedding_server(
                &hybrid.paths,
                &hybrid.profile,
                &config.runtime_id,
                &config.model_id,
                config.context_size,
                std::time::Duration::from_secs(config.startup_timeout_secs),
            )
            .await
            .map_err(|error| error.to_string())?;
            let digest = server.model_digest().to_string();
            *owned = Some(LocalHybridEngine {
                server,
                model_digest: digest,
                dimensions: None,
                entries: HashMap::new(),
                vector_bytes: 0,
                ready: false,
            });
        }
        let engine = owned.as_mut().expect("initialized local recall owner");
        // Publication is atomic at the preparation boundary. Errors or a
        // cancelled preparation may retain cached work but never expose it.
        engine.ready = false;
        let records = self
            .user_records
            .iter()
            .filter(|_| self.user_enabled)
            .chain(
                self.workspace_records
                    .iter()
                    .filter(|_| self.workspace_enabled),
            )
            .collect::<Vec<_>>();
        let live = records
            .iter()
            .map(|record| record.id)
            .collect::<HashSet<_>>();
        engine.entries.retain(|id, _| live.contains(id));
        engine.vector_bytes = engine
            .entries
            .values()
            .flat_map(|(_, vectors)| vectors)
            .map(|vector| vector.len() * 4)
            .sum();
        for record in records {
            if crate::learning::sanitized_recall_text(&record.text, MAX_RECORD_BYTES).is_none() {
                continue;
            }
            let key = format!(
                "{}:{}:{}",
                engine.model_digest,
                record.scope.label(),
                record.digest().map_err(|error| error.to_string())?
            );
            if engine
                .entries
                .get(&record.id)
                .is_some_and(|(previous, _)| previous == &key)
            {
                continue;
            }
            if let Some((_, vectors)) = engine.entries.remove(&record.id) {
                engine.vector_bytes = engine
                    .vector_bytes
                    .saturating_sub(vectors.iter().map(|vector| vector.len() * 4).sum::<usize>());
            }
            let chunks = memory_embedding_chunks(&record.text);
            let mut vectors = Vec::new();
            for batch in chunks.chunks(16) {
                let response = engine
                    .server
                    .embed(batch)
                    .await
                    .map_err(|error| error.to_string())?;
                for vector in response {
                    if engine
                        .dimensions
                        .is_some_and(|dimensions| dimensions != vector.len())
                    {
                        return Err("Local embedding dimensions changed".into());
                    }
                    engine.dimensions = Some(vector.len());
                    vectors.push(vector);
                }
            }
            let bytes = vectors.iter().map(|vector| vector.len() * 4).sum::<usize>();
            if engine.vector_bytes.saturating_add(bytes) > MAX_HYBRID_VECTOR_BYTES {
                return Err("Local recall vector cache reached its 32 MiB bound".into());
            }
            engine.vector_bytes += bytes;
            engine.entries.insert(record.id, (key, vectors));
        }
        engine.ready = true;
        Ok(())
    }

    /// Returns a frozen prompt snapshot for one logical request. Any cold,
    /// unavailable, malformed, private, or slow local path falls back to lexical.
    pub async fn render_for_query_async(&self, query: &str) -> String {
        let lexical = self.render_for_query(query);
        let Some(hybrid) = &self.local_hybrid else {
            return lexical;
        };
        if query.is_empty()
            || query.len() > 4096
            || crate::learning::sanitized_recall_text(query, MAX_RECORD_BYTES).is_none()
        {
            return lexical;
        }
        let selected = tokio::time::timeout(std::time::Duration::from_millis(200), async {
            let owned = hybrid.engine.lock().await;
            let engine = owned.as_ref()?;
            if !engine.ready {
                return None;
            }
            let vectors = engine.server.embed(&[query.to_string()]).await.ok()?;
            let query_vector = vectors.first()?;
            if engine.dimensions != Some(query_vector.len()) {
                return None;
            }
            let minimum = f64::from(hybrid.config.minimum_similarity_percent) / 100.0;
            let current_keys = self
                .user_records
                .iter()
                .filter(|_| self.user_enabled)
                .chain(
                    self.workspace_records
                        .iter()
                        .filter(|_| self.workspace_enabled),
                )
                .filter(|record| {
                    crate::learning::sanitized_recall_text(&record.text, MAX_RECORD_BYTES).is_some()
                })
                .filter_map(|record| {
                    record.digest().ok().map(|digest| {
                        (
                            record.id,
                            format!(
                                "{}:{}:{}",
                                engine.model_digest,
                                record.scope.label(),
                                digest
                            ),
                        )
                    })
                })
                .collect::<HashMap<_, _>>();
            let mut semantic = engine
                .entries
                .iter()
                .filter_map(|(id, (key, chunks))| {
                    if current_keys.get(id) != Some(key) {
                        return None;
                    }
                    let score = maximum_vector_similarity(chunks, query_vector)?;
                    (score >= minimum).then_some((*id, score))
                })
                .collect::<Vec<_>>();
            semantic.sort_by(|left, right| {
                right
                    .1
                    .total_cmp(&left.1)
                    .then_with(|| left.0.cmp(&right.0))
            });
            let user = if self.user_enabled {
                render_selected_recall(
                    MemoryScope::User,
                    select_hybrid_memory(MemoryScope::User, &self.user_records, query, &semantic)
                        .into_iter()
                        .map(|candidate| candidate.record)
                        .collect(),
                )
            } else {
                String::new()
            };
            let workspace = if self.workspace_enabled {
                render_selected_recall(
                    MemoryScope::Workspace,
                    select_hybrid_memory(
                        MemoryScope::Workspace,
                        &self.workspace_records,
                        query,
                        &semantic,
                    )
                    .into_iter()
                    .map(|candidate| candidate.record)
                    .collect(),
                )
            } else {
                String::new()
            };
            let foreign =
                self.foreign
                    .render_for_query(query, self.user_enabled, self.workspace_enabled);
            Some(
                if user.is_empty() && workspace.is_empty() && foreign.is_empty() {
                    String::new()
                } else {
                    format!("\n\n## Recalled Memory\n{user}{workspace}{foreign}")
                },
            )
        })
        .await;
        selected.ok().flatten().unwrap_or(lexical)
    }

    pub fn load(
        paths: &crate::runtime::RuntimePaths,
        profile: &crate::runtime::ActiveProfile,
        canonical_workspace: &Path,
        user_enabled: bool,
        workspace_enabled: bool,
    ) -> Result<Self, MemoryError> {
        let user_records = if user_enabled {
            let store = MemoryStore::user(paths, profile, false)?;
            store.load()?
        } else {
            Vec::new()
        };
        let workspace_records = if workspace_enabled {
            let store = MemoryStore::workspace(paths, profile, canonical_workspace, false)?;
            store.load()?
        } else {
            Vec::new()
        };
        let foreign = crate::foreign_knowledge::ForeignKnowledgeIndex::discover(
            &paths.home,
            canonical_workspace,
        );
        Ok(Self {
            user_enabled,
            workspace_enabled,
            user_records,
            workspace_records,
            foreign,
            local_hybrid: None,
        })
    }

    pub fn render(&self) -> String {
        if (!self.user_enabled && !self.workspace_enabled)
            || (self.user_records.is_empty()
                && self.workspace_records.is_empty()
                && self.foreign.is_empty())
        {
            return String::new();
        }
        let mut output = String::from("\n\n## Recalled Memory\n");
        if self.user_enabled && !self.user_records.is_empty() {
            output.push_str(&render_recall_index(MemoryScope::User, &self.user_records));
        }
        if self.workspace_enabled && !self.workspace_records.is_empty() {
            output.push_str(&render_recall_index(
                MemoryScope::Workspace,
                &self.workspace_records,
            ));
        }
        output.push_str(&self.foreign.render_for_query(
            "",
            self.user_enabled,
            self.workspace_enabled,
        ));
        output
    }

    pub fn render_for_query(&self, query: &str) -> String {
        let user = if self.user_enabled {
            render_recall_index_for_query(MemoryScope::User, &self.user_records, query)
        } else {
            String::new()
        };
        let workspace = if self.workspace_enabled {
            render_recall_index_for_query(MemoryScope::Workspace, &self.workspace_records, query)
        } else {
            String::new()
        };
        let foreign =
            self.foreign
                .render_for_query(query, self.user_enabled, self.workspace_enabled);
        if user.is_empty() && workspace.is_empty() && foreign.is_empty() {
            return String::new();
        }
        format!("\n\n## Recalled Memory\n{user}{workspace}{foreign}")
    }

    pub fn from_records(
        user_enabled: bool,
        workspace_enabled: bool,
        user_records: &[MemoryRecord],
        workspace_records: &[MemoryRecord],
    ) -> Self {
        Self {
            user_enabled,
            workspace_enabled,
            user_records: if user_enabled {
                user_records.to_vec()
            } else {
                Vec::new()
            },
            workspace_records: if workspace_enabled {
                workspace_records.to_vec()
            } else {
                Vec::new()
            },
            foreign: crate::foreign_knowledge::ForeignKnowledgeIndex::default(),
            local_hybrid: None,
        }
    }

    pub(crate) fn with_foreign(
        mut self,
        foreign: crate::foreign_knowledge::ForeignKnowledgeIndex,
    ) -> Self {
        self.foreign = foreign;
        self
    }

    pub(crate) fn replace_native_records(
        &mut self,
        user_enabled: bool,
        workspace_enabled: bool,
        user_records: &[MemoryRecord],
        workspace_records: &[MemoryRecord],
    ) {
        self.user_enabled = user_enabled;
        self.workspace_enabled = workspace_enabled;
        self.user_records = if user_enabled {
            user_records.to_vec()
        } else {
            Vec::new()
        };
        self.workspace_records = if workspace_enabled {
            workspace_records.to_vec()
        } else {
            Vec::new()
        };
    }
}

pub fn export_markdown(
    path: &Path,
    user_records: &[MemoryRecord],
    workspace_records: &[MemoryRecord],
) -> Result<(), MemoryError> {
    if path.as_os_str().is_empty() || path.exists() {
        return Err(MemoryError::ExportExists);
    }
    let parent = path.parent().ok_or(MemoryError::UnsafePath)?;
    fs::create_dir_all(parent)?;
    let mut output = String::from("# ca Memory Export\n\n");
    for (title, records) in [
        ("User memory", user_records),
        ("Workspace memory", workspace_records),
    ] {
        output.push_str(&format!("## {title}\n\n"));
        if records.is_empty() {
            output.push_str("(none)\n\n");
        } else {
            for record in records {
                output.push_str(&format!(
                    "- `{}` `{}` — {}\n",
                    record.id,
                    record.kind.label(),
                    record.text.replace('\n', " ")
                ));
            }
            output.push('\n');
        }
    }
    if output.len() > 2 * MAX_STORE_BYTES {
        return Err(MemoryError::StoreTooLarge);
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(output.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

fn validate_text(scope: MemoryScope, kind: MemoryKind, text: &str) -> Result<String, MemoryError> {
    let normalized = text.replace("\r\n", "\n").trim().to_string();
    if normalized.is_empty() || normalized.len() > MAX_RECORD_BYTES {
        return Err(MemoryError::InvalidText);
    }
    if normalized
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        return Err(MemoryError::InvalidText);
    }
    if scope == MemoryScope::User
        && matches!(
            kind,
            MemoryKind::WorkspaceConvention | MemoryKind::WorkspaceDecision
        )
    {
        return Err(MemoryError::WrongScope);
    }
    if scope == MemoryScope::Workspace
        && matches!(kind, MemoryKind::UserPreference | MemoryKind::UserWorkflow)
    {
        return Err(MemoryError::WrongScope);
    }
    let lower = normalized.to_ascii_lowercase();
    const SECRET_MARKERS: &[&str] = &[
        "api_key=",
        "api-key:",
        "authorization: bearer",
        "password=",
        "password:",
        "secret=",
        "private key",
    ];
    let looks_like_secret_token = normalized
        .split(|character: char| character.is_whitespace() || matches!(character, ',' | ';'))
        .any(|token| token.starts_with("sk-") && token.len() >= 16);
    if SECRET_MARKERS.iter().any(|marker| lower.contains(marker)) || looks_like_secret_token {
        return Err(MemoryError::SensitiveContent);
    }
    if scope == MemoryScope::User
        && matches!(kind, MemoryKind::ExternalFact)
        && ["this project", "the project", "repository", "workspace"]
            .iter()
            .any(|marker| lower.contains(marker))
    {
        return Err(MemoryError::WrongScope);
    }
    const ACTIVE_STATE_MARKERS: &[&str] = &[
        "todo:",
        "current task",
        "working on:",
        "next step:",
        "assistant:",
        "tool result:",
    ];
    if ACTIVE_STATE_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return Err(MemoryError::ActiveState);
    }
    const REREADABLE_MARKERS: &[&str] = &[
        "src/",
        "cargo.toml",
        ".rs:",
        "git commit ",
        "current branch ",
        "package version ",
    ];
    if REREADABLE_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return Err(MemoryError::RereadableRepositoryFact);
    }
    Ok(normalized)
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("memory path is unsafe")]
    UnsafePath,
    #[error("memory store is invalid or exceeds its bounds")]
    InvalidStore,
    #[error("memory store exceeds its size limit")]
    StoreTooLarge,
    #[error("memory record limit reached")]
    TooManyRecords,
    #[error("memory is read-only for this run")]
    ReadOnly,
    #[error("memory entry belongs to the wrong scope")]
    WrongScope,
    #[error("memory changed since the maintenance snapshot")]
    Conflict,
    #[error("memory entry is empty, too long, or contains unsafe controls")]
    InvalidText,
    #[error("memory entry may contain sensitive material")]
    SensitiveContent,
    #[error("active task or transcript state cannot be stored as memory")]
    ActiveState,
    #[error("repository facts that can be reread cannot be stored as memory")]
    RereadableRepositoryFact,
    #[error("memory export target already exists")]
    ExportExists,
    #[error("memory storage failed")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hybrid_chunks_are_utf8_bounded_and_fusion_keeps_scope_and_preferences() {
        let text = "编译器🦀diagnostic ".repeat(160);
        let chunks = memory_embedding_chunks(&text);
        assert!(chunks.len() > 1);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.len() <= HYBRID_CHUNK_BYTES && !chunk.is_empty()));
        assert!(chunks.first().unwrap().starts_with("编译器🦀"));
        assert!(chunks.last().unwrap().ends_with("diagnostic "));
        let records = vec![
            MemoryRecord::new(
                MemoryScope::User,
                MemoryKind::UserPreference,
                "Give the outcome first.",
                MemoryProvenance::Manual,
            )
            .unwrap(),
            MemoryRecord::new(
                MemoryScope::User,
                MemoryKind::ExternalFact,
                "Release starts from signed tags.",
                MemoryProvenance::Manual,
            )
            .unwrap(),
            MemoryRecord::new(
                MemoryScope::Workspace,
                MemoryKind::ExternalFact,
                "Publishing begins from signed tags.",
                MemoryProvenance::Manual,
            )
            .unwrap(),
        ];
        let selected = select_hybrid_memory(
            MemoryScope::User,
            &records,
            "what makes publishing begin",
            &[(records[1].id, 0.9), (records[2].id, 1.0)],
        );
        assert_eq!(selected[0].reason, MemoryRecallReason::Always);
        assert_eq!(selected[1].record.id, records[1].id);
        assert_eq!(selected.len(), 2);
    }

    #[tokio::test]
    async fn disabled_or_cold_hybrid_preserves_lexical_prompt_without_contacting_runtime() {
        let record = MemoryRecord::new(
            MemoryScope::User,
            MemoryKind::ExternalFact,
            "Saffron compiler diagnostics.",
            MemoryProvenance::Manual,
        )
        .unwrap();
        let recall = MemoryRecall::from_records(true, false, &[record], &[]);
        assert_eq!(
            recall.render_for_query_async("saffron").await,
            recall.render_for_query("saffron")
        );
        let temporary = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(temporary.path());
        let config = crate::config::LocalHybridMemoryConfig {
            runtime_id: "unavailable".into(),
            model_id: "unavailable".into(),
            context_size: 2048,
            startup_timeout_secs: 1,
            minimum_similarity_percent: 70,
        };
        let recall = recall.with_local_hybrid(
            &paths,
            &crate::runtime::ActiveProfile::Default,
            Some(&config),
        );
        assert_eq!(
            recall.render_for_query_async("saffron").await,
            recall.render_for_query("saffron")
        );
        assert!(recall.prepare_local_hybrid().await.is_err());
        assert_eq!(
            recall.render_for_query_async("saffron").await,
            recall.render_for_query("saffron")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_partial_embedding_preparation_keeps_the_lexical_snapshot() {
        let temporary = tempfile::tempdir().unwrap();
        let records = ["Saffron compiler diagnostics.", "fixture-missing-vector"].map(|text| {
            MemoryRecord::new(
                MemoryScope::User,
                MemoryKind::ExternalFact,
                text,
                MemoryProvenance::Manual,
            )
            .unwrap()
        });
        let mut recall = MemoryRecall::from_records(true, false, &records, &[]);
        recall.local_hybrid = Some(LocalHybridRecall {
            paths: crate::runtime::RuntimePaths::from_home(temporary.path()),
            profile: crate::runtime::ActiveProfile::Default,
            config: crate::config::LocalHybridMemoryConfig {
                runtime_id: "fixture".into(),
                model_id: "embedding".into(),
                context_size: 2048,
                startup_timeout_secs: 5,
                minimum_similarity_percent: 70,
            },
            engine: std::sync::Arc::new(tokio::sync::Mutex::new(Some(LocalHybridEngine {
                server: crate::local_models::ManagedEmbeddingServer::fixture(temporary.path())
                    .await,
                model_digest: "fixture".into(),
                dimensions: None,
                entries: HashMap::new(),
                vector_bytes: 0,
                ready: false,
            }))),
        });
        assert!(recall.prepare_local_hybrid().await.is_err());
        let query = "unrelated publishing question";
        assert_eq!(
            recall.render_for_query_async(query).await,
            recall.render_for_query(query)
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn slow_embedding_http_attempt_is_timed_out_before_semantic_quality_can_count() {
        use tokio::io::AsyncReadExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (started, request_observed) = tokio::sync::oneshot::channel();
        let slow_provider = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let bytes = socket.read(&mut request).await.unwrap();
            started.send(request[..bytes].to_vec()).unwrap();
            // A real connected HTTP provider accepts the request but withholds
            // its response. Cancellation must drop the pending semantic future.
            std::future::pending::<()>().await;
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let attempt = timed_benchmark_embedding(
            async {
                client
                    .post(format!("http://{address}/embeddings"))
                    .json(&serde_json::json!({"input": ["what makes publishing begin"]}))
                    .send()
                    .await?
                    .json::<Vec<Vec<f32>>>()
                    .await
            },
            Some(2),
        )
        .await;
        let request = tokio::time::timeout(Duration::from_secs(2), request_observed).await;
        slow_provider.abort();
        assert!(slow_provider.await.unwrap_err().is_cancelled());
        let request = request.unwrap().unwrap();
        assert!(String::from_utf8_lossy(&request).starts_with("POST /embeddings "));
        assert_eq!(attempt.outcome, RecallBenchmarkQueryOutcome::Timeout);
        assert!(attempt.vector.is_none());
        assert!(attempt.embedding_ms >= WARM_RECALL_LIMIT_MS as f64);

        let dataset: RecallBenchmarkDataset =
            serde_json::from_str(include_str!("../tests/fixtures/memory_recall/ranking.json"))
                .unwrap();
        let lexical = evaluate_recall_benchmark(&dataset, 5).unwrap();
        let scores: Vec<SemanticRecallScoreRow> = serde_json::from_str(include_str!(
            "../tests/fixtures/memory_recall/semantic_scores.json"
        ))
        .unwrap();
        let mut hybrid =
            evaluate_recall_benchmark_with_semantics(&dataset, 5, 0.7, &scores).unwrap();
        assert!(hybrid_acceptance(&dataset, &lexical, &hybrid).passed);
        let timings = dataset
            .cases
            .iter()
            .map(|case| LocalRecallCaseTiming {
                query: case.query.clone(),
                lexical_ms: 0.0,
                embedding_ms: attempt.embedding_ms,
                selection_ms: 0.0,
                warm_added_ms: attempt.embedding_ms,
                outcome: attempt.outcome,
                lexical_context_bytes: 0,
                selected_context_bytes: 0,
            })
            .collect::<Vec<_>>();
        apply_benchmark_lexical_fallbacks(&dataset, &lexical, &mut hybrid, &timings);
        assert_eq!(hybrid, lexical);
        assert!(!hybrid_acceptance(&dataset, &lexical, &hybrid).passed);
    }

    #[test]
    fn warm_gate_counts_embedding_similarity_and_selection_at_the_200_ms_boundary() {
        let mut timing = LocalRecallCaseTiming {
            query: "fixture query".into(),
            lexical_ms: 1.0,
            embedding_ms: 100.0,
            selection_ms: 80.0,
            warm_added_ms: 0.0,
            outcome: RecallBenchmarkQueryOutcome::Semantic,
            lexical_context_bytes: 1,
            selected_context_bytes: 2,
        };
        finish_benchmark_query_timing(&mut timing, 20.0);
        assert_eq!(timing.warm_added_ms, 200.0);
        assert_eq!(timing.outcome, RecallBenchmarkQueryOutcome::Semantic);
        finish_benchmark_query_timing(&mut timing, 1.0);
        assert_eq!(timing.warm_added_ms, 201.0);
        assert_eq!(
            timing.outcome,
            RecallBenchmarkQueryOutcome::SelectionTimeout
        );
    }

    #[test]
    fn latency_summary_uses_nearest_rank_p95_and_measured_context_sizes() {
        let summary = RecallLatencySummary::measured(&[4.0, 1.0, 3.0, 2.0]);
        assert_eq!(summary.samples, 4);
        assert_eq!(summary.median_ms, 2.5);
        assert_eq!(summary.p95_ms, 4.0);
        assert_eq!(summary.max_ms, 4.0);
        assert_eq!(summary.total_ms, 10.0);
        assert_eq!(RecallLatencySummary::measured(&[]).samples, 0);
        let dataset: RecallBenchmarkDataset =
            serde_json::from_str(include_str!("../tests/fixtures/memory_recall/ranking.json"))
                .unwrap();
        let pure = evaluate_recall_benchmark(&dataset, 5).unwrap();
        let measured = evaluate_recall_benchmark_measured(&dataset, 5, None).unwrap();
        assert_eq!(measured.quality, pure);
        for (quality, timing) in measured.quality.cases.iter().zip(&measured.latency.cases) {
            assert_eq!(timing.selected_context_bytes, quality.selected_bytes);
            assert!(timing.ranking_ms.is_finite() && timing.ranking_ms >= 0.0);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_benchmark_reports_lexical_cold_and_warm_query_timings() {
        let dataset: RecallBenchmarkDataset =
            serde_json::from_str(include_str!("../tests/fixtures/memory_recall/ranking.json"))
                .unwrap();
        let config = crate::config::LocalHybridMemoryConfig {
            runtime_id: "fixture".into(),
            model_id: "fixture".into(),
            context_size: 2048,
            startup_timeout_secs: 5,
            minimum_similarity_percent: 70,
        };
        let temporary = tempfile::tempdir().unwrap();
        let server = crate::local_models::ManagedEmbeddingServer::fixture_with_calibration(
            temporary.path(),
            true,
        )
        .await;
        let lexical = evaluate_recall_benchmark(&dataset, 5).unwrap();
        let report = evaluate_local_recall_with_server(&dataset, 5, &config, lexical, server)
            .await
            .unwrap();
        let value = serde_json::to_value(report).unwrap();
        let latency = &value["latency"];
        assert!(
            latency.is_object(),
            "the local benchmark must report latency"
        );
        assert!(latency["cold_preparation"]["indexing_ms"].is_number());
        assert_eq!(latency["lexical"]["samples"], dataset.cases.len());
        assert_eq!(latency["warm_semantic"]["samples"], dataset.cases.len());
        assert_eq!(
            latency["cases"].as_array().unwrap().len(),
            dataset.cases.len()
        );
        for case in latency["cases"].as_array().unwrap() {
            assert!(case["lexical_ms"].is_number());
            assert!(case["embedding_ms"].is_number());
            assert!(case["selection_ms"].is_number());
            assert!(case["warm_added_ms"].is_number());
            assert_eq!(case["outcome"], "semantic");
        }
        assert_eq!(latency["warm_limit_ms"], 200);
        assert!(latency["warm_gate_passed"].is_boolean());
        assert_eq!(latency["fallback_cases"], 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_benchmark_records_invalid_embedding_as_lexical_fallback_and_fails_gate() {
        let mut dataset: RecallBenchmarkDataset =
            serde_json::from_str(include_str!("../tests/fixtures/memory_recall/ranking.json"))
                .unwrap();
        dataset.cases[2].query = "fixture-missing-vector".into();
        let config = crate::config::LocalHybridMemoryConfig {
            runtime_id: "fixture".into(),
            model_id: "fixture".into(),
            context_size: 2048,
            startup_timeout_secs: 5,
            minimum_similarity_percent: 70,
        };
        let temporary = tempfile::tempdir().unwrap();
        let server = crate::local_models::ManagedEmbeddingServer::fixture_with_calibration(
            temporary.path(),
            true,
        )
        .await;
        let lexical = evaluate_recall_benchmark(&dataset, 5).unwrap();
        let report = evaluate_local_recall_with_server(&dataset, 5, &config, lexical, server)
            .await
            .expect("query failure must remain explicit benchmark evidence");
        let value = serde_json::to_value(report).unwrap();
        assert_eq!(value["latency"]["cases"][2]["outcome"], "embedding_error");
        assert_eq!(value["latency"]["fallback_cases"], 1);
        assert_eq!(value["latency"]["warm_gate_passed"], false);
        assert_eq!(value["acceptance"]["passed"], false);
        assert_eq!(value["hybrid"]["cases"][2], value["lexical"]["cases"][2]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn supervised_fixture_embedding_pipeline_checks_positive_and_negative_calibration() {
        let dataset: RecallBenchmarkDataset =
            serde_json::from_str(include_str!("../tests/fixtures/memory_recall/ranking.json"))
                .unwrap();
        let config = crate::config::LocalHybridMemoryConfig {
            runtime_id: "fixture".into(),
            model_id: "fixture".into(),
            context_size: 2048,
            startup_timeout_secs: 5,
            minimum_similarity_percent: 70,
        };
        let temporary = tempfile::tempdir().unwrap();
        let server = crate::local_models::ManagedEmbeddingServer::fixture_with_calibration(
            temporary.path(),
            true,
        )
        .await;
        let lexical = evaluate_recall_benchmark(&dataset, 5).unwrap();
        let calibrated = evaluate_local_recall_with_server(&dataset, 5, &config, lexical, server)
            .await
            .unwrap();
        assert!(calibrated.acceptance.passed);
        assert_eq!(calibrated.hybrid.false_positive_rate, 0.0);
        assert!(
            calibrated
                .calibration
                .iter()
                .any(|row| row.minimum_similarity_percent == 10
                    && !row.acceptance.negative_query_gate)
        );
        assert!(calibrated
            .calibration
            .iter()
            .any(|row| row.minimum_similarity_percent == 70 && row.acceptance.passed));
        assert_eq!(calibrated.model_sha256, "fixture");
        let temporary = tempfile::tempdir().unwrap();
        let server = crate::local_models::ManagedEmbeddingServer::fixture(temporary.path()).await;
        let lexical = evaluate_recall_benchmark(&dataset, 5).unwrap();
        let unselective = evaluate_local_recall_with_server(&dataset, 5, &config, lexical, server)
            .await
            .unwrap();
        assert_eq!(unselective.hybrid.false_positive_rate, 1.0);
        assert!(!unselective.acceptance.passed);
        assert!(unselective
            .calibration
            .iter()
            .all(|row| !row.acceptance.passed));
        eprintln!(
            "RECALL_RECEIPT {}",
            serde_json::json!({
                "evidence": "supervised synthetic HTTP embedding fixture; not real model quality",
                "fixture_sha256": format!("{:x}", Sha256::digest(include_bytes!("../tests/fixtures/memory_recall/ranking.json"))),
                "lexical": calibrated.lexical,
                "selective_fixture": {"hybrid": calibrated.hybrid, "acceptance": calibrated.acceptance, "calibration": calibrated.calibration},
                "unselective_fixture": {"hybrid": unselective.hybrid, "acceptance": unselective.acceptance, "calibration": unselective.calibration},
                "actual_registered_model_gate": {"measured": false, "status": "pending explicitly selected registered runtime and model IDs"}
            })
        );
    }

    #[tokio::test]
    async fn local_benchmark_requires_an_explicit_available_registered_binding() {
        let dataset: RecallBenchmarkDataset =
            serde_json::from_str(include_str!("../tests/fixtures/memory_recall/ranking.json"))
                .unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(temporary.path());
        let config = crate::config::LocalHybridMemoryConfig {
            runtime_id: "unavailable".into(),
            model_id: "unavailable".into(),
            context_size: 2048,
            startup_timeout_secs: 1,
            minimum_similarity_percent: 70,
        };
        assert!(evaluate_local_recall_benchmark(
            &dataset,
            5,
            &paths,
            &crate::runtime::ActiveProfile::Bare,
            &config
        )
        .await
        .is_err());
        assert!(evaluate_local_recall_benchmark(
            &dataset,
            5,
            &paths,
            &crate::runtime::ActiveProfile::Default,
            &config
        )
        .await
        .is_err());
        assert!(!paths
            .local_models_dir(&crate::runtime::ActiveProfile::Default)
            .exists());
    }

    #[test]
    fn recall_benchmark_is_deterministic_and_separates_always_recalled_preferences() {
        let dataset: RecallBenchmarkDataset =
            serde_json::from_str(include_str!("../tests/fixtures/memory_recall/ranking.json"))
                .unwrap();
        let first = evaluate_recall_benchmark(&dataset, 5).unwrap();
        assert_eq!(first, evaluate_recall_benchmark(&dataset, 5).unwrap());
        assert_eq!(first.positive_cases, 6);
        assert_eq!(first.negative_cases, 3);
        assert_eq!(first.false_positive_rate, 0.0);
        assert!(first
            .cases
            .iter()
            .all(|case| case.always_recalled == ["preference"]));
        assert!(first
            .cases
            .iter()
            .all(|case| case.selected_bytes <= MAX_INDEX_BYTES));
        assert_eq!(first.cases[0].query_results, ["release"]);
        assert!(
            first.cases[2].query_results.is_empty(),
            "fixture must expose the paraphrase gap"
        );
        assert!(
            first.cases[5].query_results.is_empty(),
            "fixture must expose the segmentation gap"
        );
        assert!(
            first.cases[6].query_results.is_empty(),
            "foreign scope must stay excluded"
        );
    }
    #[test]
    fn semantic_fixture_proves_fusion_uplift_and_negative_threshold_calibration() {
        let dataset: RecallBenchmarkDataset =
            serde_json::from_str(include_str!("../tests/fixtures/memory_recall/ranking.json"))
                .unwrap();
        let scores: Vec<Vec<(String, f64)>> = serde_json::from_str(include_str!(
            "../tests/fixtures/memory_recall/semantic_scores.json"
        ))
        .unwrap();
        let lexical = evaluate_recall_benchmark(&dataset, 5).unwrap();
        let calibrated =
            evaluate_recall_benchmark_with_semantics(&dataset, 5, 0.7, &scores).unwrap();
        assert!(calibrated.recall_at_k >= lexical.recall_at_k + 0.10);
        assert!(
            calibrated.false_positive_rate <= 0.05
                && calibrated.false_positive_rate <= lexical.false_positive_rate
        );
        assert!(calibrated.cases[0].recall_at_k >= lexical.cases[0].recall_at_k);
        assert!(calibrated.cases[1].recall_at_k >= lexical.cases[1].recall_at_k);
        assert!(calibrated.cases[6].query_results.is_empty());
        assert_eq!(
            calibrated,
            evaluate_recall_benchmark_with_semantics(&dataset, 5, 0.7, &scores).unwrap()
        );
        let uncalibrated =
            evaluate_recall_benchmark_with_semantics(&dataset, 5, 0.1, &scores).unwrap();
        assert!(uncalibrated.false_positive_rate > 0.05);
        assert!(!serde_json::to_string(&calibrated)
            .unwrap()
            .contains("Release automation starts"));
    }

    #[test]
    fn recall_selection_reports_scores_without_changing_rendered_results() {
        let record = MemoryRecord::new(
            MemoryScope::User,
            MemoryKind::ExternalFact,
            "Saffron compiler diagnostics preserve the exact error identifier.",
            MemoryProvenance::Manual,
        )
        .unwrap();
        let records = vec![record];
        let selected = select_memory_for_query(MemoryScope::User, &records, "saffron compiler");
        assert_eq!(selected.len(), 1);
        assert!(selected[0].score > 0.0);
        assert_eq!(selected[0].matched_terms, 2);
        assert_eq!(selected[0].reason, MemoryRecallReason::Query);
        assert_eq!(
            render_recall_index_for_query(MemoryScope::User, &records, "saffron compiler"),
            render_selected_recall(MemoryScope::User, vec![&records[0]])
        );
    }

    fn store(temp: &tempfile::TempDir, scope: MemoryScope, writable: bool) -> MemoryStore {
        let root = temp.path().join("data");
        let name = match scope {
            MemoryScope::User => "user.json",
            MemoryScope::Workspace => "workspace.json",
        };
        MemoryStore::new(
            root.clone(),
            root.join("memory").join(name),
            scope,
            writable,
        )
        .unwrap()
    }

    #[test]
    fn missing_store_is_empty_and_does_not_create_files() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp, MemoryScope::User, true);
        assert!(store.load().unwrap().is_empty());
        assert!(!store.path().exists());
    }

    #[test]
    fn upsert_load_delete_are_exact_and_atomic() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp, MemoryScope::User, true);
        let record = MemoryRecord::new(
            MemoryScope::User,
            MemoryKind::UserPreference,
            "Prefer concise completion notes.",
            MemoryProvenance::Manual,
        )
        .unwrap();
        let id = record.id;
        store.upsert(record).unwrap();
        assert_eq!(store.load().unwrap()[0].id, id);
        assert!(store.delete(id).unwrap());
        assert!(store.load().unwrap().is_empty());
        assert!(!store.delete(id).unwrap());
    }

    #[test]
    fn scopes_and_read_only_runs_are_enforced() {
        let temp = tempfile::tempdir().unwrap();
        let read_only = store(&temp, MemoryScope::User, false);
        let record = MemoryRecord::new(
            MemoryScope::User,
            MemoryKind::UserWorkflow,
            "Run focused checks before broad gates.",
            MemoryProvenance::Manual,
        )
        .unwrap();
        assert!(matches!(
            read_only.upsert(record),
            Err(MemoryError::ReadOnly)
        ));
        assert!(matches!(
            MemoryRecord::new(
                MemoryScope::User,
                MemoryKind::WorkspaceConvention,
                "Use the local fixture.",
                MemoryProvenance::Manual,
            ),
            Err(MemoryError::WrongScope)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_store_components_are_rejected() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        fs::create_dir_all(&root).unwrap();
        symlink(outside.path(), root.join("memory")).unwrap();
        let store = MemoryStore::new(
            root.clone(),
            root.join("memory/user.json"),
            MemoryScope::User,
            true,
        )
        .unwrap();
        assert!(matches!(store.load(), Err(MemoryError::UnsafePath)));
    }

    #[test]
    fn validator_rejects_secrets_active_state_and_rereadable_facts_without_echo() {
        for (text, expected) in [
            ("api_key=super-secret", "sensitive"),
            ("Current task: repair the provider", "active task"),
            ("The package version is 1.2.3", "repository facts"),
        ] {
            let error = MemoryRecord::new(
                MemoryScope::Workspace,
                MemoryKind::WorkspaceDecision,
                text,
                MemoryProvenance::Manual,
            )
            .unwrap_err();
            assert!(error.to_string().contains(expected));
            assert!(!error.to_string().contains("super-secret"));
        }
    }

    #[test]
    fn proposals_are_process_only_until_explicit_approval() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp, MemoryScope::Workspace, true);
        let proposal = MemoryProposal::new(
            MemoryScope::Workspace,
            MemoryKind::WorkspaceConvention,
            "Use hermetic loopback fixtures for provider checks.",
        )
        .unwrap();
        assert!(!store.path().exists());
        let record = proposal.approve();
        store.upsert(record).unwrap();
        assert_eq!(store.load().unwrap().len(), 1);
    }

    #[test]
    fn recall_index_is_stale_labelled_and_independently_bounded() {
        let records = (0..300)
            .map(|index| {
                MemoryRecord::new(
                    MemoryScope::User,
                    MemoryKind::UserPreference,
                    format!("Preference {index}: keep output deterministic."),
                    MemoryProvenance::Manual,
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let rendered = render_recall_index(MemoryScope::User, &records);
        assert!(rendered.contains("may be stale; reverify"));
        assert!(rendered.lines().count() <= MAX_INDEX_LINES);
        assert!(rendered.len() <= MAX_INDEX_BYTES);
    }

    #[test]
    fn query_recall_selects_late_relevance_before_unrelated_insertion_order() {
        let mut records = (0..400)
            .map(|index| {
                MemoryRecord::new(
                    MemoryScope::Workspace,
                    MemoryKind::ExternalFact,
                    format!(
                        "Gardening note {index}: rotate seasonal flowers and water the raised bed."
                    ),
                    MemoryProvenance::Manual,
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        records.push(
            MemoryRecord::new(
                MemoryScope::Workspace,
                MemoryKind::ExternalFact,
                "Use hermetic loopback transport for provider continuation fixtures.",
                MemoryProvenance::Manual,
            )
            .unwrap(),
        );

        let rendered = render_recall_index_for_query(
            MemoryScope::Workspace,
            &records,
            "repair the provider loopback continuation transport",
        );
        assert!(rendered.contains("hermetic loopback transport"));
        assert!(!rendered.contains("Gardening note 0"));
        assert!(rendered.lines().count() <= MAX_INDEX_LINES);
        assert!(rendered.len() <= MAX_INDEX_BYTES);
    }

    #[test]
    fn broad_memory_has_a_small_reserve_but_unmatched_external_facts_do_not() {
        let preference = MemoryRecord::new(
            MemoryScope::User,
            MemoryKind::UserPreference,
            "Keep final answers concise and outcome-first.",
            MemoryProvenance::Manual,
        )
        .unwrap();
        let unrelated = MemoryRecord::new(
            MemoryScope::User,
            MemoryKind::ExternalFact,
            "The favorite garden flower is a yellow tulip.",
            MemoryProvenance::Manual,
        )
        .unwrap();
        let recall = MemoryRecall::from_records(true, false, &[unrelated, preference], &[]);
        let rendered = recall.render_for_query("debug the provider retry state machine");

        assert!(rendered.contains("outcome-first"));
        assert!(!rendered.contains("yellow tulip"));
        assert!(!format!("{recall:?}").contains("outcome-first"));
        assert!(!format!("{recall:?}").contains("provider retry"));
    }

    #[test]
    fn debug_redacts_record_and_proposal_text() {
        let record = MemoryRecord::new(
            MemoryScope::User,
            MemoryKind::UserPreference,
            "private preference",
            MemoryProvenance::Manual,
        )
        .unwrap();
        let proposal = MemoryProposal::new(
            MemoryScope::User,
            MemoryKind::UserPreference,
            "another private preference",
        )
        .unwrap();
        assert!(!format!("{record:?}").contains("private preference"));
        assert!(!format!("{proposal:?}").contains("another private preference"));
    }

    #[test]
    fn profiles_and_workspaces_cannot_observe_each_others_records() {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(temp.path());
        let default =
            MemoryStore::user(&paths, &crate::runtime::ActiveProfile::Default, true).unwrap();
        let named = MemoryStore::user(
            &paths,
            &crate::runtime::ActiveProfile::Named("work".into()),
            true,
        )
        .unwrap();
        default
            .upsert(
                MemoryRecord::new(
                    MemoryScope::User,
                    MemoryKind::UserPreference,
                    "Prefer short summaries.",
                    MemoryProvenance::Manual,
                )
                .unwrap(),
            )
            .unwrap();
        assert!(named.load().unwrap().is_empty());

        let first_workspace = MemoryStore::workspace(
            &paths,
            &crate::runtime::ActiveProfile::Default,
            Path::new("/workspace/one"),
            true,
        )
        .unwrap();
        let second_workspace = MemoryStore::workspace(
            &paths,
            &crate::runtime::ActiveProfile::Default,
            Path::new("/workspace/two"),
            true,
        )
        .unwrap();
        first_workspace
            .upsert(
                MemoryRecord::new(
                    MemoryScope::Workspace,
                    MemoryKind::WorkspaceConvention,
                    "Use hermetic fixtures.",
                    MemoryProvenance::Manual,
                )
                .unwrap(),
            )
            .unwrap();
        assert!(second_workspace.load().unwrap().is_empty());
    }

    #[test]
    fn project_facts_cannot_enter_user_memory_and_normal_risk_text_is_allowed() {
        assert!(matches!(
            MemoryRecord::new(
                MemoryScope::User,
                MemoryKind::ExternalFact,
                "This project uses a custom provider.",
                MemoryProvenance::Manual,
            ),
            Err(MemoryError::WrongScope)
        ));
        assert!(MemoryRecord::new(
            MemoryScope::User,
            MemoryKind::UserPreference,
            "Prefer risk-based validation.",
            MemoryProvenance::Manual,
        )
        .is_ok());
    }

    #[test]
    fn disabled_recall_is_byte_empty_and_does_not_create_paths() {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(temp.path());
        let recall = MemoryRecall::load(
            &paths,
            &crate::runtime::ActiveProfile::Default,
            temp.path(),
            false,
            false,
        )
        .unwrap();
        assert_eq!(recall.render(), "");
        assert!(!paths.data_dir.exists());
    }
}
