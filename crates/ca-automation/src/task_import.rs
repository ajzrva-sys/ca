//! Digest-bound, single-task import contract for external project governors.
//!
//! Parsing, canonicalization, validation, preview, and apply preconditions live
//! here. CLI/app wiring owns state persistence and the repository lock; callers
//! must hold that lock across `prepare_apply`, durable transaction creation,
//! state save, revision recompute, and immutable receipt persistence.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use globset::Glob;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::domain::{AutomationLevel, ReviewTier, TaskSpec};
use crate::paths::Paths;
use crate::revision::{git_head, repository_revision, require_clean_outside_orchestrator};
use crate::verification::ca::parse_ca_verification_command;

pub(crate) const TASK_IMPORT_SCHEMA: &str = "codex-team.task_import.v1";
const MAX_INPUT_BYTES: usize = 1024 * 1024;
const MAX_ID_BYTES: usize = 128;
const MAX_TITLE_BYTES: usize = 512;
const MAX_KIND_BYTES: usize = 64;
const MAX_PROSE_BYTES: usize = 16 * 1024;
const MAX_LIST_ITEM_BYTES: usize = 4 * 1024;
const MAX_GENERAL_LIST_ITEMS: usize = 64;
const MAX_ALLOWED_FILES: usize = 24;
const MAX_READ_ONLY_FILES: usize = 64;
const MAX_ESTIMATED_MINUTES: u32 = 10_080;
static RECEIPT_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskImportRequest {
    pub(crate) schema: String,
    pub(crate) source: String,
    pub(crate) base_head: String,
    pub(crate) task: ImportedTask,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportedTask {
    pub(crate) task_id: String,
    pub(crate) title: String,
    pub(crate) kind: String,
    pub(crate) objective: String,
    pub(crate) why_unblocks_progress: String,
    pub(crate) allowed_files: Vec<String>,
    #[serde(default)]
    pub(crate) read_only_files: Vec<String>,
    pub(crate) forbidden_actions: Vec<String>,
    pub(crate) steps: Vec<String>,
    pub(crate) acceptance_criteria: Vec<String>,
    pub(crate) verification_commands: Vec<String>,
    pub(crate) stop_conditions: Vec<String>,
    pub(crate) discovery_budget: String,
    pub(crate) estimated_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ValidationIssue {
    pub(crate) code: String,
    pub(crate) field: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskImportSummary {
    pub(crate) source: String,
    pub(crate) base_head: String,
    pub(crate) task_id: String,
    pub(crate) title: String,
    pub(crate) kind: String,
    pub(crate) objective: String,
    pub(crate) why_unblocks_progress: String,
    pub(crate) allowed_files: Vec<String>,
    pub(crate) read_only_files: Vec<String>,
    pub(crate) forbidden_actions: Vec<String>,
    pub(crate) steps: Vec<String>,
    pub(crate) acceptance_criteria: Vec<String>,
    pub(crate) verification_commands: Vec<String>,
    pub(crate) stop_conditions: Vec<String>,
    pub(crate) discovery_budget: String,
    pub(crate) estimated_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskImportPreviewReceipt {
    pub(crate) schema: String,
    pub(crate) mutation_performed: bool,
    pub(crate) valid: bool,
    pub(crate) repository_revision: String,
    pub(crate) current_head: Option<String>,
    pub(crate) import_digest: String,
    pub(crate) task: TaskImportSummary,
    pub(crate) validation: Vec<ValidationIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportedTaskRecord {
    pub(crate) schema: String,
    pub(crate) task_id: String,
    pub(crate) import_digest: String,
    pub(crate) source: String,
    pub(crate) base_head: String,
    pub(crate) imported_at: String,
    pub(crate) receipt_path: String,
}

pub(crate) type ImportedTaskRegistry = BTreeMap<String, ImportedTaskRecord>;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskImportApplyReceipt {
    pub(crate) schema: String,
    pub(crate) outcome: String,
    pub(crate) task_id: String,
    pub(crate) import_digest: String,
    pub(crate) expected_revision: String,
    pub(crate) revision_before: String,
    pub(crate) revision_after: String,
    pub(crate) base_head: String,
    pub(crate) task: TaskImportSummary,
    pub(crate) receipt_path: String,
    pub(crate) applied_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct TaskImportApplyPlan {
    pub(crate) task: TaskSpec,
    pub(crate) record: ImportedTaskRecord,
    pub(crate) preview: TaskImportPreviewReceipt,
    pub(crate) already_applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct TaskImportApplyTransaction {
    schema: String,
    task_id: String,
    import_digest: String,
    expected_revision: String,
    source: String,
    base_head: String,
    task: TaskImportSummary,
    receipt_path: String,
    imported_at: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MutationBlockers {
    pub(crate) active_pause: bool,
    pub(crate) pending_commit: bool,
}

/// Strictly parse at most 1 MiB and produce a mutation-free canonical preview.
pub(crate) fn preview(paths: &Paths, input: &[u8]) -> Result<TaskImportPreviewReceipt> {
    let canonical = parse_canonical(input)?;
    let current_head = git_head(paths)?;
    let mut validation = validate_request(&canonical);
    if current_head.as_deref() != Some(canonical.base_head.as_str()) {
        validation.push(issue(
            "base_head_mismatch",
            "base_head",
            "base_head must equal the repository's current full Git HEAD",
        ));
    }
    validation.sort_by(|left, right| {
        left.field
            .cmp(&right.field)
            .then(left.code.cmp(&right.code))
    });
    let import_digest = canonical_digest(&canonical)?;
    Ok(TaskImportPreviewReceipt {
        schema: "codex-team.task_import_preview.v1".to_string(),
        mutation_performed: false,
        valid: validation.is_empty(),
        repository_revision: repository_revision(paths)?,
        current_head,
        import_digest,
        task: TaskImportSummary::from(&canonical),
        validation,
    })
}

/// Validate all digest/revision/clean-tree/state preconditions and return the
/// exact state mutation. This does not itself save state or write a receipt.
pub(crate) fn prepare_apply(
    paths: &Paths,
    input: &[u8],
    accept_digest: &str,
    expected_revision: &str,
    queue: &[TaskSpec],
    imports: &ImportedTaskRegistry,
    blockers: MutationBlockers,
) -> Result<TaskImportApplyPlan> {
    validate_sha256("accept digest", accept_digest)?;
    validate_sha256("expected revision", expected_revision)?;
    let preview = preview(paths, input)?;
    if preview.import_digest != accept_digest {
        bail!("task import digest is stale or does not match the accepted preview")
    }
    if preview.repository_revision != expected_revision {
        bail!("repository revision is stale; preview the task again")
    }
    let canonical = parse_canonical(input)?;
    let task_id = canonical.task.task_id.clone();
    let existing = imports.get(&task_id);
    let matching_existing = existing.is_some_and(|record| record.import_digest == accept_digest);
    let only_head_changed = !preview.validation.is_empty()
        && preview
            .validation
            .iter()
            .all(|issue| issue.code == "base_head_mismatch");
    if !preview.valid && !(matching_existing && only_head_changed) {
        let details = preview
            .validation
            .iter()
            .take(8)
            .map(|issue| format!("{}: {}", issue.field, issue.message))
            .collect::<Vec<_>>()
            .join("; ");
        bail!("task import validation failed: {details}")
    }
    require_clean_outside_orchestrator(paths)?;
    if blockers.active_pause {
        bail!("task import is blocked while a human disposition pause is active")
    }
    if blockers.pending_commit {
        bail!("task import is blocked while a manual commit is pending")
    }

    if let Some(existing) = existing {
        if existing.import_digest == accept_digest {
            let task = queue
                .iter()
                .find(|task| task.task_id == task_id)
                .cloned()
                .unwrap_or_else(|| canonical.to_task_spec());
            return Ok(TaskImportApplyPlan {
                task,
                record: existing.clone(),
                preview,
                already_applied: true,
            });
        }
        bail!("task ID {task_id} was already imported with different content")
    }
    if queue.iter().any(|task| task.task_id == task_id) {
        bail!("task ID {task_id} already exists in the orchestrator queue")
    }

    let receipt_file = receipt_filename(&task_id, accept_digest);
    let receipt_path = format!(".orchestrator/imports/{receipt_file}");
    let record = ImportedTaskRecord {
        schema: "codex-team.imported_task_record.v1".to_string(),
        task_id,
        import_digest: accept_digest.to_string(),
        source: canonical.source.clone(),
        base_head: canonical.base_head.clone(),
        imported_at: Utc::now().to_rfc3339(),
        receipt_path,
    };
    Ok(TaskImportApplyPlan {
        task: canonical.to_task_spec(),
        record,
        preview,
        already_applied: false,
    })
}

/// Durably record enough canonical data to repair a crash after state becomes
/// visible but before the immutable apply receipt is published. The caller
/// must invoke this before saving the queued task/import registry.
pub(crate) fn begin_apply_transaction(paths: &Paths, plan: &TaskImportApplyPlan) -> Result<()> {
    if plan.already_applied {
        return Ok(());
    }
    let transaction = TaskImportApplyTransaction {
        schema: "codex-team.task_import_transaction.v1".to_string(),
        task_id: plan.record.task_id.clone(),
        import_digest: plan.record.import_digest.clone(),
        expected_revision: plan.preview.repository_revision.clone(),
        source: plan.record.source.clone(),
        base_head: plan.record.base_head.clone(),
        task: plan.preview.task.clone(),
        receipt_path: plan.record.receipt_path.clone(),
        imported_at: plan.record.imported_at.clone(),
    };
    let path = transaction_path(paths, &plan.record)?;
    ensure_safe_import_receipt_path(paths, &path, true)?;
    if path.exists() {
        let existing: TaskImportApplyTransaction = serde_json::from_slice(&fs::read(&path)?)
            .with_context(|| {
                format!("parse task import recovery transaction {}", path.display())
            })?;
        validate_pre_state_transaction(&existing, plan)?;
        if existing.expected_revision == transaction.expected_revision
            && existing.imported_at == transaction.imported_at
        {
            return Ok(());
        }
        // No imported state exists when `already_applied` is false. A stale
        // pre-state journal can therefore be replaced by this newly approved,
        // revision-bound apply without discarding executable work.
        fs::remove_file(&path)
            .with_context(|| format!("replace stale task import transaction {}", path.display()))?;
    }
    validate_transaction(&transaction, plan, true)?;
    write_immutable_json(&path, &transaction, true)
        .context("persist task import recovery transaction before state mutation")
}

/// Persist the immutable receipt after the caller atomically saves the queued
/// task and import registry. Existing byte-identical receipts are idempotent;
/// differing content fails closed and is never overwritten.
pub(crate) fn persist_apply_receipt(
    paths: &Paths,
    plan: &TaskImportApplyPlan,
    revision_after: String,
) -> Result<TaskImportApplyReceipt> {
    validate_sha256("post-apply revision", &revision_after)?;
    validate_import_record(&plan.record)?;
    let path = paths.root.join(&plan.record.receipt_path);
    ensure_safe_import_receipt_path(paths, &path, true)?;
    if path.exists() {
        let existing = fs::read(&path).with_context(|| {
            format!("read existing immutable import receipt {}", path.display())
        })?;
        let mut receipt: TaskImportApplyReceipt = serde_json::from_slice(&existing)
            .with_context(|| format!("parse immutable import receipt {}", path.display()))?;
        validate_apply_receipt(&receipt, &plan.record, Some(&plan.preview.task), &path)?;
        if !plan.already_applied && receipt.revision_after != revision_after {
            bail!(
                "immutable import receipt {} already differs",
                path.display()
            )
        }
        if plan.already_applied {
            receipt.outcome = "already_applied".to_string();
        }
        remove_transaction_best_effort(paths, &plan.record);
        return Ok(receipt);
    }
    let transaction = read_transaction(paths, plan)?;
    validate_transaction(&transaction, plan, !plan.already_applied)?;
    let receipt = TaskImportApplyReceipt {
        schema: "codex-team.task_import_apply.v1".to_string(),
        // Immutable bytes always describe the original apply. A replay changes
        // only the returned DTO below, never the durable receipt.
        outcome: "applied".to_string(),
        task_id: transaction.task_id.clone(),
        import_digest: transaction.import_digest.clone(),
        expected_revision: transaction.expected_revision.clone(),
        revision_before: transaction.expected_revision.clone(),
        revision_after,
        base_head: transaction.base_head.clone(),
        task: transaction.task.clone(),
        receipt_path: plan.record.receipt_path.clone(),
        applied_at: plan.record.imported_at.clone(),
    };
    write_immutable_json(&path, &receipt, false)?;
    remove_transaction_best_effort(paths, &plan.record);
    if plan.already_applied {
        let mut replay = receipt;
        replay.outcome = "already_applied".to_string();
        Ok(replay)
    } else {
        Ok(receipt)
    }
}

/// Reject execution whenever imported state lacks its immutable audit receipt.
/// A matching recovery transaction makes the failure explicitly repairable by
/// replaying the exact digest-bound `task-import` apply command.
pub(crate) fn ensure_import_receipts_complete(
    paths: &Paths,
    queue: &[TaskSpec],
    imports: &ImportedTaskRegistry,
) -> Result<()> {
    for record in imports.values() {
        validate_import_record(record)?;
        let path = paths.root.join(&record.receipt_path);
        ensure_safe_import_receipt_path(paths, &path, false)?;
        match fs::read(&path) {
            Ok(bytes) => {
                let receipt: TaskImportApplyReceipt =
                    serde_json::from_slice(&bytes).with_context(|| {
                        format!("parse immutable import receipt {}", path.display())
                    })?;
                validate_apply_receipt(&receipt, record, None, &path)?;
                validate_imported_queue_binding(queue, record, &receipt.task)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let transaction = transaction_path(paths, record)?;
                match fs::symlink_metadata(&transaction) {
                    Ok(_) => {
                        ensure_safe_import_receipt_path(paths, &transaction, false)?;
                        let recovery: TaskImportApplyTransaction =
                            serde_json::from_slice(&fs::read(&transaction).with_context(|| {
                                format!(
                                    "read task import recovery transaction {}",
                                    transaction.display()
                                )
                            })?)
                            .with_context(|| {
                                format!(
                                    "parse task import recovery transaction {}",
                                    transaction.display()
                                )
                            })?;
                        validate_transaction_record(&recovery, record)?;
                        bail!(
                            "imported task {} has a recoverable incomplete receipt; replay the exact task-import apply before running work",
                            record.task_id
                        );
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!(
                                "inspect task import recovery transaction {}",
                                transaction.display()
                            )
                        })
                    }
                }
                bail!(
                    "imported task {} is missing its immutable receipt and recovery transaction; execution is blocked",
                    record.task_id
                );
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("read immutable import receipt {}", path.display()))
            }
        }
    }
    Ok(())
}

fn validate_imported_queue_binding(
    queue: &[TaskSpec],
    record: &ImportedTaskRecord,
    summary: &TaskImportSummary,
) -> Result<()> {
    let matching = queue
        .iter()
        .filter(|task| task.task_id == record.task_id)
        .collect::<Vec<_>>();
    if matching.len() > 1 {
        bail!(
            "imported task {} appears more than once in the queue",
            record.task_id
        )
    }
    let Some(task) = matching.first() else {
        // Completed imported tasks retain their immutable audit record after
        // their active queue entry is removed.
        return Ok(());
    };
    let expected = request_from_summary(summary).to_task_spec();
    if serde_json::to_value(task)? != serde_json::to_value(expected)? {
        bail!(
            "imported task {} queue entry does not match its immutable receipt",
            record.task_id
        )
    }
    Ok(())
}

fn validate_apply_receipt(
    receipt: &TaskImportApplyReceipt,
    record: &ImportedTaskRecord,
    expected_task: Option<&TaskImportSummary>,
    path: &Path,
) -> Result<()> {
    let receipt_digest = canonical_digest_from_summary(&receipt.task);
    if receipt.schema != "codex-team.task_import_apply.v1"
        || receipt.outcome != "applied"
        || receipt.task_id != record.task_id
        || receipt.import_digest != record.import_digest
        || !receipt_digest
            .as_ref()
            .is_ok_and(|digest| digest == &record.import_digest)
        || receipt.expected_revision != receipt.revision_before
        || validate_sha256(
            "immutable receipt expected revision",
            &receipt.expected_revision,
        )
        .is_err()
        || validate_sha256(
            "immutable receipt post-apply revision",
            &receipt.revision_after,
        )
        .is_err()
        || receipt.base_head != record.base_head
        || expected_task.is_some_and(|task| &receipt.task != task)
        || receipt.receipt_path != record.receipt_path
        || receipt.applied_at != record.imported_at
    {
        bail!(
            "immutable import receipt {} does not match state",
            path.display()
        )
    }
    Ok(())
}

fn read_transaction(
    paths: &Paths,
    plan: &TaskImportApplyPlan,
) -> Result<TaskImportApplyTransaction> {
    let path = transaction_path(paths, &plan.record)?;
    ensure_safe_import_receipt_path(paths, &path, false)?;
    let bytes = fs::read(&path).with_context(|| {
        format!(
            "read task import recovery transaction {}; state was not authorized without this record",
            path.display()
        )
    })?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("parse task import recovery transaction {}", path.display()))
}

fn validate_transaction(
    transaction: &TaskImportApplyTransaction,
    plan: &TaskImportApplyPlan,
    require_current_preview: bool,
) -> Result<()> {
    if validate_transaction_record(transaction, &plan.record).is_err()
        || transaction.task != plan.preview.task
        || (require_current_preview
            && transaction.expected_revision != plan.preview.repository_revision)
    {
        bail!("task import recovery transaction does not match the approved import")
    }
    Ok(())
}

/// A transaction can outlive a crash before state is saved. In that case the
/// retried plan necessarily has a new timestamp, but no executable imported
/// state exists yet. Validate every authority-bearing field except the
/// timestamp before replacing the stale pre-state journal with the new,
/// revision-bound transaction.
fn validate_pre_state_transaction(
    transaction: &TaskImportApplyTransaction,
    plan: &TaskImportApplyPlan,
) -> Result<()> {
    if validate_transaction_record_fields(transaction, &plan.record, false).is_err()
        || transaction.task != plan.preview.task
    {
        bail!("task import recovery transaction does not match the approved import")
    }
    Ok(())
}

fn validate_transaction_record(
    transaction: &TaskImportApplyTransaction,
    record: &ImportedTaskRecord,
) -> Result<()> {
    validate_transaction_record_fields(transaction, record, true)
}

fn validate_transaction_record_fields(
    transaction: &TaskImportApplyTransaction,
    record: &ImportedTaskRecord,
    bind_imported_at: bool,
) -> Result<()> {
    let transaction_digest = canonical_digest_from_summary(&transaction.task);
    if transaction.schema != "codex-team.task_import_transaction.v1"
        || transaction.task_id != record.task_id
        || transaction.import_digest != record.import_digest
        || !transaction_digest
            .as_ref()
            .is_ok_and(|digest| digest == &record.import_digest)
        || transaction.source != record.source
        || transaction.base_head != record.base_head
        || transaction.task.task_id != record.task_id
        || transaction.task.source != record.source
        || transaction.task.base_head != record.base_head
        || transaction.receipt_path != record.receipt_path
        || (bind_imported_at && transaction.imported_at != record.imported_at)
        || validate_sha256(
            "transaction expected revision",
            &transaction.expected_revision,
        )
        .is_err()
    {
        bail!("task import recovery transaction does not match state")
    }
    Ok(())
}

fn validate_import_record(record: &ImportedTaskRecord) -> Result<()> {
    if record.schema != "codex-team.imported_task_record.v1" {
        bail!("imported task record has an unsupported schema")
    }
    let mut issues = Vec::new();
    validate_safe_id(
        &record.task_id,
        "imported task ID",
        MAX_ID_BYTES,
        &mut issues,
    );
    if !issues.is_empty() {
        bail!("imported task record has an invalid task ID")
    }
    validate_sha256("imported task digest", &record.import_digest)?;
    if record.source != "ca" || !is_full_object_id(&record.base_head) {
        bail!("imported task record has invalid source or base HEAD")
    }
    if chrono::DateTime::parse_from_rfc3339(&record.imported_at).is_err() {
        bail!("imported task record has an invalid import timestamp")
    }
    let expected = format!(
        ".orchestrator/imports/{}",
        receipt_filename(&record.task_id, &record.import_digest)
    );
    if record.receipt_path != expected {
        bail!("imported task record has a noncanonical receipt path")
    }
    Ok(())
}

fn transaction_path(paths: &Paths, record: &ImportedTaskRecord) -> Result<std::path::PathBuf> {
    validate_import_record(record)?;
    Ok(paths.orch.join("imports").join(format!(
        ".{}-{}.transaction.json",
        record.task_id,
        &record.import_digest[..16]
    )))
}

fn remove_transaction_best_effort(paths: &Paths, record: &ImportedTaskRecord) {
    if let Ok(path) = transaction_path(paths, record) {
        let _ = fs::remove_file(path);
    }
}

fn parse_canonical(input: &[u8]) -> Result<TaskImportRequest> {
    if input.len() > MAX_INPUT_BYTES {
        bail!("task import input exceeds {MAX_INPUT_BYTES} bytes")
    }
    let mut parsed: TaskImportRequest =
        serde_json::from_slice(input).context("parse strict task import JSON")?;
    canonicalize_request(&mut parsed);
    Ok(parsed)
}

fn canonicalize_request(request: &mut TaskImportRequest) {
    request.schema = request.schema.trim().to_string();
    request.source = request.source.trim().to_string();
    request.base_head = request.base_head.trim().to_ascii_lowercase();
    let task = &mut request.task;
    trim(&mut task.task_id);
    trim(&mut task.title);
    trim(&mut task.kind);
    trim(&mut task.objective);
    trim(&mut task.why_unblocks_progress);
    trim_all(&mut task.allowed_files);
    trim_all(&mut task.read_only_files);
    trim_all(&mut task.forbidden_actions);
    trim_all(&mut task.steps);
    trim_all(&mut task.acceptance_criteria);
    trim_all(&mut task.stop_conditions);
    trim(&mut task.discovery_budget);
    for command in &mut task.verification_commands {
        trim(command);
        if let Some(parsed) = parse_ca_verification_command(command, "task.import") {
            *command = parsed.display();
        }
    }
}

fn validate_request(request: &TaskImportRequest) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    if request.schema != TASK_IMPORT_SCHEMA {
        issues.push(issue(
            "invalid_schema",
            "schema",
            "schema must be codex-team.task_import.v1",
        ));
    }
    if request.source != "ca" {
        issues.push(issue(
            "unsupported_source",
            "source",
            "source must be ca in v1",
        ));
    }
    if !is_full_object_id(&request.base_head) {
        issues.push(issue(
            "invalid_object_id",
            "base_head",
            "base_head must be a 40- or 64-character full hexadecimal Git object ID",
        ));
    }

    let task = &request.task;
    validate_safe_id(&task.task_id, "task.task_id", MAX_ID_BYTES, &mut issues);
    validate_required_bounded(&task.title, "task.title", MAX_TITLE_BYTES, &mut issues);
    validate_safe_id(&task.kind, "task.kind", MAX_KIND_BYTES, &mut issues);
    validate_required_bounded(
        &task.objective,
        "task.objective",
        MAX_PROSE_BYTES,
        &mut issues,
    );
    validate_required_bounded(
        &task.why_unblocks_progress,
        "task.why_unblocks_progress",
        MAX_PROSE_BYTES,
        &mut issues,
    );
    validate_required_bounded(
        &task.discovery_budget,
        "task.discovery_budget",
        MAX_PROSE_BYTES,
        &mut issues,
    );
    validate_path_list(
        &task.allowed_files,
        "task.allowed_files",
        MAX_ALLOWED_FILES,
        true,
        &mut issues,
    );
    validate_path_list(
        &task.read_only_files,
        "task.read_only_files",
        MAX_READ_ONLY_FILES,
        false,
        &mut issues,
    );
    validate_nonempty_list(
        &task.forbidden_actions,
        "task.forbidden_actions",
        &mut issues,
    );
    validate_nonempty_list(&task.steps, "task.steps", &mut issues);
    validate_nonempty_list(
        &task.acceptance_criteria,
        "task.acceptance_criteria",
        &mut issues,
    );
    validate_nonempty_list(&task.stop_conditions, "task.stop_conditions", &mut issues);
    validate_nonempty_list(
        &task.verification_commands,
        "task.verification_commands",
        &mut issues,
    );
    for (index, command) in task.verification_commands.iter().enumerate() {
        if parse_ca_verification_command(command, "task.import").is_none() {
            issues.push(issue(
                "unsupported_verification",
                &format!("task.verification_commands[{index}]"),
                "verification command is not an approved shell-free ca recipe",
            ));
        }
    }
    if task.estimated_minutes == 0 || task.estimated_minutes > MAX_ESTIMATED_MINUTES {
        issues.push(issue(
            "out_of_range",
            "task.estimated_minutes",
            "estimated_minutes must be between 1 and 10080",
        ));
    }
    issues
}

fn validate_path_list(
    values: &[String],
    field: &str,
    limit: usize,
    writable: bool,
    issues: &mut Vec<ValidationIssue>,
) {
    if writable && values.is_empty() {
        issues.push(issue("required", field, "at least one path is required"));
    }
    if values.len() > limit {
        issues.push(issue(
            "too_many_items",
            field,
            &format!("at most {limit} paths are allowed"),
        ));
    }
    for (index, value) in values.iter().enumerate() {
        let item_field = format!("{field}[{index}]");
        if value.is_empty() || value.len() > 1024 {
            issues.push(issue(
                "invalid_path",
                &item_field,
                "path pattern must be nonempty and at most 1024 bytes",
            ));
            continue;
        }
        if !safe_relative_pattern(value) {
            issues.push(issue(
                "unsafe_path",
                &item_field,
                "path must be relative and may not contain escapes, .orchestrator, or invalid glob syntax",
            ));
            continue;
        }
        if writable && pattern_reaches_protected_tree(value) {
            issues.push(issue(
                "protected_tree",
                &item_field,
                "writable paths may not select oracles/** or reference/**",
            ));
        }
        if writable && pattern_reaches_target_queue(value) {
            issues.push(issue(
                "target_queue_owned",
                &item_field,
                "imported tasks may not write NEXT_STEPS.md or NEXT_STEPS_non-ai.md",
            ));
        }
    }
}

fn validate_nonempty_list(values: &[String], field: &str, issues: &mut Vec<ValidationIssue>) {
    if values.is_empty() {
        issues.push(issue("required", field, "at least one item is required"));
        return;
    }
    if values.len() > MAX_GENERAL_LIST_ITEMS {
        issues.push(issue(
            "too_many_items",
            field,
            &format!("at most {MAX_GENERAL_LIST_ITEMS} items are allowed"),
        ));
    }
    for (index, value) in values.iter().enumerate() {
        validate_required_bounded(
            value,
            &format!("{field}[{index}]"),
            MAX_LIST_ITEM_BYTES,
            issues,
        );
    }
}

fn validate_safe_id(value: &str, field: &str, limit: usize, issues: &mut Vec<ValidationIssue>) {
    if value.is_empty()
        || value.len() > limit
        || !value.is_ascii()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        issues.push(issue(
            "invalid_identifier",
            field,
            &format!("identifier must use 1-{limit} safe ASCII bytes"),
        ));
    }
}

fn validate_required_bounded(
    value: &str,
    field: &str,
    limit: usize,
    issues: &mut Vec<ValidationIssue>,
) {
    if value.is_empty() {
        issues.push(issue("required", field, "value must not be empty"));
    } else if value.len() > limit {
        issues.push(issue(
            "too_large",
            field,
            &format!("value exceeds {limit} bytes"),
        ));
    }
}

fn safe_relative_pattern(value: &str) -> bool {
    let path = Path::new(value);
    if path.is_absolute()
        || value.starts_with('/')
        || value.starts_with('~')
        || value.as_bytes().get(0..3).is_some_and(|prefix| {
            prefix[0].is_ascii_alphabetic() && prefix[1] == b':' && prefix[2] == b'/'
        })
        || value.contains('\\')
        || value.contains('\0')
        || value.bytes().any(|byte| byte.is_ascii_control())
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return false;
    }
    let first = value.split('/').next().unwrap_or_default();
    if first.eq_ignore_ascii_case(".git")
        || first.eq_ignore_ascii_case(".orchestrator")
        || value.split('/').any(|part| part == "..")
    {
        return false;
    }
    let Ok(glob) = Glob::new(value) else {
        return false;
    };
    let matcher = glob.compile_matcher();
    !matcher.is_match(".orchestrator/state.json")
        && !matcher.is_match(".orchestrator/imports/receipt.json")
}

fn pattern_reaches_protected_tree(value: &str) -> bool {
    let Ok(glob) = Glob::new(value) else {
        return true;
    };
    let matcher = glob.compile_matcher();
    let first = value.split('/').next().unwrap_or_default();
    let lowercase = value.to_ascii_lowercase();
    let wildcard_root = value.contains('/')
        && first
            .bytes()
            .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']' | b'{' | b'}'));
    wildcard_root
        || lowercase == "oracles"
        || lowercase.starts_with("oracles/")
        || lowercase == "reference"
        || lowercase.starts_with("reference/")
        || matcher.is_match("oracles/PROTECTED")
        || matcher.is_match("reference/PROTECTED")
}

fn pattern_reaches_target_queue(value: &str) -> bool {
    let Ok(glob) = Glob::new(value) else {
        return true;
    };
    let matcher = glob.compile_matcher();
    matcher.is_match("NEXT_STEPS.md") || matcher.is_match("NEXT_STEPS_non-ai.md")
}

fn canonical_digest(request: &TaskImportRequest) -> Result<String> {
    let canonical = serde_json::to_vec(request).context("serialize canonical task import")?;
    let mut hasher = Sha256::new();
    hasher.update(b"codex-team-task-import-v1\0");
    hasher.update((canonical.len() as u64).to_be_bytes());
    hasher.update(canonical);
    Ok(hex_digest(&hasher.finalize()))
}

fn canonical_digest_from_summary(summary: &TaskImportSummary) -> Result<String> {
    canonical_digest(&request_from_summary(summary))
}

fn request_from_summary(summary: &TaskImportSummary) -> TaskImportRequest {
    TaskImportRequest {
        schema: TASK_IMPORT_SCHEMA.to_string(),
        source: summary.source.clone(),
        base_head: summary.base_head.clone(),
        task: ImportedTask {
            task_id: summary.task_id.clone(),
            title: summary.title.clone(),
            kind: summary.kind.clone(),
            objective: summary.objective.clone(),
            why_unblocks_progress: summary.why_unblocks_progress.clone(),
            allowed_files: summary.allowed_files.clone(),
            read_only_files: summary.read_only_files.clone(),
            forbidden_actions: summary.forbidden_actions.clone(),
            steps: summary.steps.clone(),
            acceptance_criteria: summary.acceptance_criteria.clone(),
            verification_commands: summary.verification_commands.clone(),
            stop_conditions: summary.stop_conditions.clone(),
            discovery_budget: summary.discovery_budget.clone(),
            estimated_minutes: summary.estimated_minutes,
        },
    }
}

fn validate_sha256(label: &str, value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{label} must be a 64-character SHA-256 digest")
    }
    if value != value.to_ascii_lowercase() {
        bail!("{label} must use lowercase hexadecimal")
    }
    Ok(())
}

fn is_full_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn receipt_filename(task_id: &str, digest: &str) -> String {
    format!("{task_id}-{}.json", &digest[..16])
}

fn write_immutable_json<T: Serialize>(
    path: &Path,
    value: &T,
    require_directory_sync: bool,
) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    if let Ok(existing) = fs::read(path) {
        if existing == bytes || existing == [bytes.as_slice(), b"\n"].concat() {
            if require_directory_sync {
                sync_directory(
                    path.parent()
                        .ok_or_else(|| anyhow::anyhow!("import receipt path has no parent"))?,
                )?;
            }
            return Ok(());
        }
        bail!(
            "immutable import receipt {} already differs",
            path.display()
        )
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("import receipt path has no parent"))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create import receipt dir {}", parent.display()))?;
    let sequence = RECEIPT_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("receipt.json");
    let temp = parent.join(format!(".{name}.tmp-{}-{sequence}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .with_context(|| format!("create import receipt temp file {}", temp.display()))?;
    let write_result = (|| -> Result<()> {
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        fs::hard_link(&temp, path)
            .with_context(|| format!("publish immutable import receipt {}", path.display()))?;
        // The pre-state recovery transaction requires a directory sync before
        // callers may expose executable state. For the final receipt, link
        // publication is the commit point: a later directory-sync failure must
        // not make app.rs roll state back behind visible `applied` evidence.
        if require_directory_sync {
            sync_directory(parent)?;
        } else {
            let _ = sync_directory(parent);
        }
        Ok(())
    })();
    let _ = fs::remove_file(&temp);
    write_result
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?
        .sync_all()
        .with_context(|| format!("sync import receipt directory {}", path.display()))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<()> {
    Ok(())
}

fn ensure_safe_import_receipt_path(paths: &Paths, path: &Path, create_parent: bool) -> Result<()> {
    let expected_parent = paths.orch.join("imports");
    if path.parent() != Some(expected_parent.as_path())
        || !path
            .file_name()
            .is_some_and(|name| Path::new(name).components().count() == 1)
    {
        bail!("import receipt path must be a direct child of .orchestrator/imports")
    }
    let orch_metadata = fs::symlink_metadata(&paths.orch)
        .with_context(|| format!("inspect orchestrator directory {}", paths.orch.display()))?;
    if orch_metadata.file_type().is_symlink() || !orch_metadata.is_dir() {
        bail!(
            "orchestrator runtime root {} must be a real directory",
            paths.orch.display()
        )
    }
    let parent = expected_parent.as_path();
    if create_parent {
        fs::create_dir_all(parent)
            .with_context(|| format!("create import receipt dir {}", parent.display()))?;
    }
    let parent_metadata = match fs::symlink_metadata(parent) {
        Ok(metadata) => metadata,
        Err(error) if !create_parent && error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(())
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect import receipt dir {}", parent.display()))
        }
    };
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        bail!(
            "import receipt directory {} must be a real directory",
            parent.display()
        )
    }
    let canonical_orch = fs::canonicalize(&paths.orch)?;
    let canonical_parent = fs::canonicalize(parent)?;
    if canonical_parent != canonical_orch.join("imports") {
        bail!("import receipt directory escapes .orchestrator/imports")
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            bail!(
                "import receipt target {} is not a regular file",
                path.display()
            )
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("inspect import receipt {}", path.display()))
        }
    }
    Ok(())
}

fn issue(code: &str, field: &str, message: &str) -> ValidationIssue {
    ValidationIssue {
        code: code.to_string(),
        field: field.to_string(),
        message: message.to_string(),
    }
}

fn trim(value: &mut String) {
    *value = value.trim().to_string();
}

fn trim_all(values: &mut [String]) {
    for value in values {
        trim(value);
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

impl TaskImportRequest {
    fn to_task_spec(&self) -> TaskSpec {
        TaskSpec {
            task_id: self.task.task_id.clone(),
            parent_task: None,
            status: "approved".to_string(),
            priority: "normal".to_string(),
            review_tier: ReviewTier::AlwaysAiReview,
            automation_level: AutomationLevel::AiWorkerAiReview,
            dependencies: Vec::new(),
            title: self.task.title.clone(),
            kind: self.task.kind.clone(),
            objective: self.task.objective.clone(),
            why_unblocks_progress: self.task.why_unblocks_progress.clone(),
            allowed_files: self.task.allowed_files.clone(),
            read_only_files: self.task.read_only_files.clone(),
            forbidden_actions: self.task.forbidden_actions.clone(),
            exact_steps: self.task.steps.clone(),
            success_criteria: self.task.acceptance_criteria.clone(),
            tests_to_run: self.task.verification_commands.clone(),
            stop_conditions: self.task.stop_conditions.clone(),
            discovery_budget: self.task.discovery_budget.clone(),
            estimated_minutes: self.task.estimated_minutes,
            requires_network: false,
            prefer_worktree: None,
        }
    }
}

impl From<&TaskImportRequest> for TaskImportSummary {
    fn from(request: &TaskImportRequest) -> Self {
        Self {
            source: request.source.clone(),
            base_head: request.base_head.clone(),
            task_id: request.task.task_id.clone(),
            title: request.task.title.clone(),
            kind: request.task.kind.clone(),
            objective: request.task.objective.clone(),
            why_unblocks_progress: request.task.why_unblocks_progress.clone(),
            allowed_files: request.task.allowed_files.clone(),
            read_only_files: request.task.read_only_files.clone(),
            forbidden_actions: request.task.forbidden_actions.clone(),
            steps: request.task.steps.clone(),
            acceptance_criteria: request.task.acceptance_criteria.clone(),
            verification_commands: request.task.verification_commands.clone(),
            stop_conditions: request.task.stop_conditions.clone(),
            discovery_budget: request.task.discovery_budget.clone(),
            estimated_minutes: request.task.estimated_minutes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use serde_json::json;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn fixture() -> Result<(std::path::PathBuf, Paths, Vec<u8>)> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "codex-team-import-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        git(&root, &["init", "-q"])?;
        git(&root, &["config", "user.email", "test@example.invalid"])?;
        git(&root, &["config", "user.name", "Test"])?;
        fs::write(root.join("README.md"), "test\n")?;
        git(&root, &["add", "README.md"])?;
        git(&root, &["commit", "-qm", "initial"])?;
        let paths = Paths::new(&root)?;
        fs::create_dir_all(&paths.orch)?;
        let head = git_head(&paths)?.unwrap();
        let value = json!({
            "schema": TASK_IMPORT_SCHEMA,
            "source": "ca",
            "base_head": head,
            "task": {
                "task_id": "INT-148",
                "title": "Scoped role-run contract",
                "kind": "protocol",
                "objective": "Observable outcome",
                "why_unblocks_progress": "Safe orchestration",
                "allowed_files": ["src/role_run.rs", "src/main.rs"],
                "read_only_files": ["src/headless.rs"],
                "forbidden_actions": ["No runtime configuration mutation"],
                "steps": ["Characterize", "Implement", "Validate"],
                "acceptance_criteria": ["Binary observable result"],
                "verification_commands": ["cargo test role_run --no-fail-fast"],
                "stop_conditions": ["Stop on workspace escape"],
                "discovery_budget": "Read only named files unless a stop condition applies",
                "estimated_minutes": 180
            }
        });
        Ok((root, paths, serde_json::to_vec(&value)?))
    }

    fn git(root: &Path, args: &[&str]) -> Result<()> {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            bail!("git {:?} failed", args)
        }
        Ok(())
    }

    #[test]
    fn preview_is_canonical_digest_bound_and_mutation_free() -> Result<()> {
        let (root, paths, input) = fixture()?;
        let before = fs::read_dir(&paths.orch)?.count();
        let first = preview(&paths, &input)?;
        let second = preview(&paths, &input)?;
        assert!(first.valid);
        assert_eq!(first.import_digest, second.import_digest);
        assert_eq!(first.repository_revision, second.repository_revision);
        assert!(!first.mutation_performed);
        assert_eq!(fs::read_dir(&paths.orch)?.count(), before);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn strict_schema_and_scope_rejections_are_reported() -> Result<()> {
        let (root, paths, input) = fixture()?;
        let mut value: serde_json::Value = serde_json::from_slice(&input)?;
        value["task"]["allowed_files"] = json!(["**", "../escape", "oracles/**", "NEXT_STEPS.md"]);
        value["task"]["verification_commands"] = json!(["cargo test && touch bad"]);
        let receipt = preview(&paths, &serde_json::to_vec(&value)?)?;
        assert!(!receipt.valid);
        assert!(receipt
            .validation
            .iter()
            .any(|issue| issue.code == "unsafe_path"));
        assert!(receipt
            .validation
            .iter()
            .any(|issue| issue.code == "protected_tree"));
        assert!(receipt
            .validation
            .iter()
            .any(|issue| issue.code == "target_queue_owned"));
        assert!(receipt
            .validation
            .iter()
            .any(|issue| issue.code == "unsupported_verification"));

        value["task"]["status"] = json!("approved");
        let error = preview(&paths, &serde_json::to_vec(&value)?).unwrap_err();
        assert!(error.to_string().contains("strict task import JSON"));
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn writable_protected_tree_aliases_are_rejected_case_insensitively() -> Result<()> {
        let (root, paths, input) = fixture()?;
        let original: serde_json::Value = serde_json::from_slice(&input)?;

        for pattern in [
            "Oracles/**",
            "ORACLES/nested/file.rs",
            "Reference/**",
            "REFERENCE/nested/file.rs",
        ] {
            let mut value = original.clone();
            value["task"]["allowed_files"] = json!([pattern]);
            let receipt = preview(&paths, &serde_json::to_vec(&value)?)?;
            assert!(!receipt.valid, "{pattern:?} unexpectedly validated");
            assert!(
                receipt
                    .validation
                    .iter()
                    .any(|issue| issue.code == "protected_tree"),
                "{pattern:?}: {:?}",
                receipt.validation
            );
        }

        for pattern in [".Orchestrator/state.json", ".GIT/config"] {
            let mut value = original.clone();
            value["task"]["allowed_files"] = json!([pattern]);
            let receipt = preview(&paths, &serde_json::to_vec(&value)?)?;
            assert!(!receipt.valid, "{pattern:?} unexpectedly validated");
            assert!(
                receipt
                    .validation
                    .iter()
                    .any(|issue| issue.code == "unsafe_path"),
                "{pattern:?}: {:?}",
                receipt.validation
            );
        }

        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn prepare_apply_enforces_digest_revision_cleanliness_and_idempotency() -> Result<()> {
        let (root, paths, input) = fixture()?;
        let receipt = preview(&paths, &input)?;
        let plan = prepare_apply(
            &paths,
            &input,
            &receipt.import_digest,
            &receipt.repository_revision,
            &[],
            &ImportedTaskRegistry::new(),
            MutationBlockers::default(),
        )?;
        assert_eq!(plan.task.task_id, "INT-148");
        assert_eq!(plan.task.status, "approved");
        assert_eq!(plan.task.review_tier, ReviewTier::AlwaysAiReview);
        assert!(!plan.already_applied);
        assert!(prepare_apply(
            &paths,
            &input,
            &"0".repeat(64),
            &receipt.repository_revision,
            &[],
            &ImportedTaskRegistry::new(),
            MutationBlockers::default(),
        )
        .unwrap_err()
        .to_string()
        .contains("digest is stale"));
        assert!(prepare_apply(
            &paths,
            &input,
            &receipt.import_digest,
            &"0".repeat(64),
            &[],
            &ImportedTaskRegistry::new(),
            MutationBlockers::default(),
        )
        .unwrap_err()
        .to_string()
        .contains("revision is stale"));
        assert!(prepare_apply(
            &paths,
            &input,
            &receipt.import_digest,
            &receipt.repository_revision,
            &[],
            &ImportedTaskRegistry::new(),
            MutationBlockers {
                active_pause: true,
                pending_commit: false,
            },
        )
        .unwrap_err()
        .to_string()
        .contains("pause is active"));

        let mut imports = ImportedTaskRegistry::new();
        imports.insert(plan.record.task_id.clone(), plan.record.clone());
        let again = prepare_apply(
            &paths,
            &input,
            &receipt.import_digest,
            &receipt.repository_revision,
            std::slice::from_ref(&plan.task),
            &imports,
            MutationBlockers::default(),
        )?;
        assert!(again.already_applied);

        fs::write(root.join("dirty.txt"), "dirty")?;
        let dirty_preview = preview(&paths, &input)?;
        let dirty_error = prepare_apply(
            &paths,
            &input,
            &dirty_preview.import_digest,
            &dirty_preview.repository_revision,
            &[],
            &ImportedTaskRegistry::new(),
            MutationBlockers::default(),
        )
        .unwrap_err();
        assert!(dirty_error.to_string().contains("must be clean"));
        fs::remove_file(root.join("dirty.txt"))?;

        fs::write(root.join("advanced.txt"), "new head\n")?;
        git(&root, &["add", "advanced.txt"])?;
        git(&root, &["commit", "-qm", "advance head"])?;
        let later_preview = preview(&paths, &input)?;
        assert!(!later_preview.valid);
        assert!(later_preview
            .validation
            .iter()
            .all(|issue| issue.code == "base_head_mismatch"));
        let later = prepare_apply(
            &paths,
            &input,
            &later_preview.import_digest,
            &later_preview.repository_revision,
            std::slice::from_ref(&plan.task),
            &imports,
            MutationBlockers::default(),
        )?;
        assert!(later.already_applied);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn immutable_receipt_is_idempotent_but_never_overwritten() -> Result<()> {
        let (root, paths, input) = fixture()?;
        let initial_preview = preview(&paths, &input)?;
        let plan = prepare_apply(
            &paths,
            &input,
            &initial_preview.import_digest,
            &initial_preview.repository_revision,
            &[],
            &ImportedTaskRegistry::new(),
            MutationBlockers::default(),
        )?;
        begin_apply_transaction(&paths, &plan)?;
        let receipt = persist_apply_receipt(&paths, &plan, "a".repeat(64))?;
        assert!(paths.root.join(&receipt.receipt_path).is_file());
        persist_apply_receipt(&paths, &plan, "a".repeat(64))?;
        assert!(persist_apply_receipt(&paths, &plan, "b".repeat(64)).is_err());

        let mut repeated = plan.clone();
        repeated.already_applied = true;
        let existing = persist_apply_receipt(&paths, &repeated, "c".repeat(64))?;
        assert_eq!(existing.outcome, "already_applied");
        assert_eq!(existing.revision_after, "a".repeat(64));

        let receipt_path = paths.root.join(&receipt.receipt_path);
        let mut forged_receipt = receipt.clone();
        forged_receipt.task.objective = "forged objective".to_string();
        fs::write(&receipt_path, serde_json::to_vec_pretty(&forged_receipt)?)?;
        let error = persist_apply_receipt(&paths, &repeated, "c".repeat(64)).unwrap_err();
        assert!(error.to_string().contains("does not match state"));

        fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
        let mut tampered: serde_json::Value = serde_json::from_slice(&fs::read(&receipt_path)?)?;
        tampered["unreviewed_extension"] = json!(true);
        fs::write(&receipt_path, serde_json::to_vec_pretty(&tampered)?)?;
        let error = persist_apply_receipt(&paths, &repeated, "c".repeat(64)).unwrap_err();
        assert!(error.to_string().contains("parse immutable import receipt"));
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn recovery_transaction_binds_import_timestamp_and_canonical_task() -> Result<()> {
        let (root, paths, input) = fixture()?;
        let initial_preview = preview(&paths, &input)?;
        let plan = prepare_apply(
            &paths,
            &input,
            &initial_preview.import_digest,
            &initial_preview.repository_revision,
            &[],
            &ImportedTaskRegistry::new(),
            MutationBlockers::default(),
        )?;
        begin_apply_transaction(&paths, &plan)?;
        let path = transaction_path(&paths, &plan.record)?;
        let original = fs::read(&path)?;
        let mut transaction: TaskImportApplyTransaction = serde_json::from_slice(&original)?;
        transaction.imported_at = Utc::now()
            .checked_add_signed(chrono::Duration::seconds(1))
            .unwrap()
            .to_rfc3339();
        fs::write(&path, serde_json::to_vec_pretty(&transaction)?)?;
        assert!(persist_apply_receipt(&paths, &plan, "a".repeat(64)).is_err());

        fs::write(&path, &original)?;
        let mut transaction: TaskImportApplyTransaction = serde_json::from_slice(&original)?;
        transaction.task.objective = "forged objective".to_string();
        fs::write(&path, serde_json::to_vec_pretty(&transaction)?)?;
        assert!(persist_apply_receipt(&paths, &plan, "a".repeat(64)).is_err());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn immutable_receipt_binds_the_active_imported_queue_entry() -> Result<()> {
        let (root, paths, input) = fixture()?;
        let initial_preview = preview(&paths, &input)?;
        let plan = prepare_apply(
            &paths,
            &input,
            &initial_preview.import_digest,
            &initial_preview.repository_revision,
            &[],
            &ImportedTaskRegistry::new(),
            MutationBlockers::default(),
        )?;
        begin_apply_transaction(&paths, &plan)?;
        persist_apply_receipt(&paths, &plan, "a".repeat(64))?;
        let mut imports = ImportedTaskRegistry::new();
        imports.insert(plan.record.task_id.clone(), plan.record.clone());

        ensure_import_receipts_complete(&paths, std::slice::from_ref(&plan.task), &imports)?;
        let mut widened = plan.task.clone();
        widened.allowed_files.push("src/unreviewed.rs".to_string());
        assert!(
            ensure_import_receipts_complete(&paths, &[widened], &imports)
                .unwrap_err()
                .to_string()
                .contains("does not match its immutable receipt")
        );
        let mut changed_verification = plan.task.clone();
        changed_verification.tests_to_run = vec!["cargo test --no-fail-fast".to_string()];
        assert!(
            ensure_import_receipts_complete(&paths, &[changed_verification], &imports)
                .unwrap_err()
                .to_string()
                .contains("does not match its immutable receipt")
        );
        assert!(ensure_import_receipts_complete(
            &paths,
            &[plan.task.clone(), plan.task.clone()],
            &imports,
        )
        .unwrap_err()
        .to_string()
        .contains("more than once"));
        ensure_import_receipts_complete(&paths, &[], &imports)?;
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn pre_state_crash_journal_is_safely_replaced_on_exact_retry() -> Result<()> {
        let (root, paths, input) = fixture()?;
        let initial_preview = preview(&paths, &input)?;
        let first = prepare_apply(
            &paths,
            &input,
            &initial_preview.import_digest,
            &initial_preview.repository_revision,
            &[],
            &ImportedTaskRegistry::new(),
            MutationBlockers::default(),
        )?;
        begin_apply_transaction(&paths, &first)?;
        let transaction_path = transaction_path(&paths, &first.record)?;

        // Simulate a durable pre-state journal from an earlier process. There
        // is deliberately no queued task or import registry entry.
        let mut earlier: TaskImportApplyTransaction =
            serde_json::from_slice(&fs::read(&transaction_path)?)?;
        earlier.imported_at = "2000-01-01T00:00:00+00:00".to_string();
        fs::write(&transaction_path, serde_json::to_vec_pretty(&earlier)?)?;

        let retry_preview = preview(&paths, &input)?;
        let retry = prepare_apply(
            &paths,
            &input,
            &retry_preview.import_digest,
            &retry_preview.repository_revision,
            &[],
            &ImportedTaskRegistry::new(),
            MutationBlockers::default(),
        )?;
        begin_apply_transaction(&paths, &retry)?;
        let replaced: TaskImportApplyTransaction =
            serde_json::from_slice(&fs::read(&transaction_path)?)?;
        assert_eq!(replaced.imported_at, retry.record.imported_at);
        assert_eq!(replaced.task, retry.preview.task);
        persist_apply_receipt(&paths, &retry, "a".repeat(64))?;
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn incomplete_state_is_fail_closed_and_exact_replay_repairs_receipt() -> Result<()> {
        let (root, paths, input) = fixture()?;
        let initial_preview = preview(&paths, &input)?;
        let plan = prepare_apply(
            &paths,
            &input,
            &initial_preview.import_digest,
            &initial_preview.repository_revision,
            &[],
            &ImportedTaskRegistry::new(),
            MutationBlockers::default(),
        )?;

        // This is the durable ordering used by app.rs: recovery transaction,
        // then state. Simulate a process crash immediately after state became
        // visible by omitting the final receipt publication.
        begin_apply_transaction(&paths, &plan)?;
        begin_apply_transaction(&paths, &plan)?;
        let mut imports = ImportedTaskRegistry::new();
        imports.insert(plan.record.task_id.clone(), plan.record.clone());
        let blocked =
            ensure_import_receipts_complete(&paths, std::slice::from_ref(&plan.task), &imports)
                .unwrap_err()
                .to_string();
        assert!(blocked.contains("recoverable incomplete receipt"));

        // State bytes change the approval revision, exactly as they do in the
        // real crash window. A fresh preview/apply replay still discovers the
        // original transaction and preserves its original expected revision.
        fs::write(
            &paths.state,
            serde_json::to_vec_pretty(&json!({"imported_task":"INT-148"}))?,
        )?;
        let replay_preview = preview(&paths, &input)?;
        assert_ne!(
            replay_preview.repository_revision,
            initial_preview.repository_revision
        );
        let replay = prepare_apply(
            &paths,
            &input,
            &replay_preview.import_digest,
            &replay_preview.repository_revision,
            std::slice::from_ref(&plan.task),
            &imports,
            MutationBlockers::default(),
        )?;
        assert!(replay.already_applied);
        let recovered =
            persist_apply_receipt(&paths, &replay, replay_preview.repository_revision.clone())?;
        assert_eq!(recovered.outcome, "already_applied");
        assert_eq!(
            recovered.expected_revision,
            initial_preview.repository_revision
        );
        assert_eq!(recovered.revision_after, replay_preview.repository_revision);
        ensure_import_receipts_complete(&paths, std::slice::from_ref(&plan.task), &imports)?;

        // Later retries are read-only and return the already-applied outcome.
        let receipt_path = paths.root.join(&recovered.receipt_path);
        let before = fs::read(&receipt_path)?;
        let repeated = persist_apply_receipt(&paths, &replay, "c".repeat(64))?;
        assert_eq!(repeated.outcome, "already_applied");
        assert_eq!(fs::read(receipt_path)?, before);
        assert!(!transaction_path(&paths, &plan.record)?.exists());

        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn missing_receipt_without_transaction_blocks_execution() -> Result<()> {
        let (root, paths, input) = fixture()?;
        let preview = preview(&paths, &input)?;
        let plan = prepare_apply(
            &paths,
            &input,
            &preview.import_digest,
            &preview.repository_revision,
            &[],
            &ImportedTaskRegistry::new(),
            MutationBlockers::default(),
        )?;
        let mut imports = ImportedTaskRegistry::new();
        imports.insert(plan.record.task_id.clone(), plan.record.clone());

        let error = ensure_import_receipts_complete(&paths, &[], &imports)
            .unwrap_err()
            .to_string();
        assert!(error.contains("missing its immutable receipt and recovery transaction"));
        assert!(!paths.orch.join("imports").exists());

        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn immutable_receipt_rejects_symlinked_import_directory() -> Result<()> {
        let (root, paths, input) = fixture()?;
        let preview = preview(&paths, &input)?;
        let plan = prepare_apply(
            &paths,
            &input,
            &preview.import_digest,
            &preview.repository_revision,
            &[],
            &ImportedTaskRegistry::new(),
            MutationBlockers::default(),
        )?;
        let outside = root.join("outside-imports");
        fs::create_dir_all(&outside)?;
        std::os::unix::fs::symlink(&outside, paths.orch.join("imports"))?;
        let error = persist_apply_receipt(&paths, &plan, "a".repeat(64)).unwrap_err();
        assert!(error.to_string().contains("must be a real directory"));
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
