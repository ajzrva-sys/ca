//! Typed dry-run, correction, approval, and execution for remote automation workflows.
//!
//! The wire carries closed adapter actions and safe names only. This module
//! owns policy/state persistence and composes the existing fixed remote adapter;
//! it never accepts shell text or model-selected executables.

use anyhow::{bail, Context, Result};
use ca_automation_protocol::{
    classify_workflow_correction, validate_workflow_draft, validate_workflow_request,
    RemoteRpcOperationV1, WorkflowActionExecutionV1, WorkflowActionPlanV1, WorkflowActionV1,
    WorkflowAdapterV1, WorkflowAssertionV1, WorkflowCorrectionChangeV1,
    WorkflowCorrectionClassificationV1, WorkflowCorrectionReceiptV1, WorkflowCorrectionScopeV1,
    WorkflowDraftV1, WorkflowDryRunDepthV1, WorkflowExecutionReceiptV1, WorkflowExecutionStatusV1,
    WorkflowLimitsV1, WorkflowOperationV1, WorkflowPlanReceiptV1, WorkflowPlanStatusV1,
    WorkflowRepairCauseV1, WorkflowRequestV1, WorkflowResponseV1, WorkflowSummaryV1,
    WorkflowVerificationV1, MAX_MECHANISTIC_OUTPUT_BYTES, WORKFLOW_CORRECTION_RECEIPT_SCHEMA,
    WORKFLOW_EXECUTION_RECEIPT_SCHEMA, WORKFLOW_PLAN_RECEIPT_SCHEMA,
};
use chrono::Utc;
use futures::stream::{self, StreamExt};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const APPROVED_SCHEMA: &str = "codex-team.approved_workflow.v1";
const POLICY_SCHEMA: &str = "codex-team.workflow_policy_record.v1";
const PREVIEW_GRANT_SCHEMA: &str = "codex-team.workflow_preview_grant.v1";
const FEEDBACK_SCHEMA: &str = "codex-team.workflow_feedback.v1";
const CHECKPOINT_SCHEMA: &str = "codex-team.workflow_checkpoint.v1";
const WORKFLOW_ENGINE_PROTOCOL_VERSION: u32 = 1;
const MAX_STATE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovedWorkflow {
    schema: String,
    workflow_id: String,
    workflow_digest: String,
    policy_digest: String,
    resource_binding_digest: String,
    #[serde(default)]
    engine_version: String,
    #[serde(default)]
    engine_protocol_version: u32,
    #[serde(default)]
    remote_identity_digest: String,
    first_run_plan_digest: String,
    enabled: bool,
    approved_at: String,
    draft: WorkflowDraftV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewGrant {
    schema: String,
    run_plan_digest: String,
    workflow_id: String,
    workflow_digest: String,
    policy_digest: String,
    resource_binding_digest: String,
    correction_round: u8,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowFeedbackRecord {
    schema: String,
    workflow_digest: String,
    receipt_digest: String,
    category: ca_automation_protocol::WorkflowFeedbackCategoryV1,
    disabled: bool,
    created_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum WorkflowCheckpointPhase {
    Running,
    Terminal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowCheckpoint {
    schema: String,
    workflow_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run_id: Option<String>,
    workflow_digest: String,
    policy_digest: String,
    resource_binding_digest: String,
    run_plan_digest: String,
    phase: WorkflowCheckpointPhase,
    status: Option<WorkflowExecutionStatusV1>,
    predicted_items: u32,
    predicted_bytes: u64,
    effect_hashes: Vec<String>,
    effect_hashes_truncated: bool,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyRecord {
    schema: String,
    correction_digest: String,
    classification: WorkflowCorrectionClassificationV1,
    source: PolicySource,
    scope: WorkflowCorrectionScopeV1,
    changes: Vec<WorkflowCorrectionChangeV1>,
    created_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
enum PolicySource {
    Ca,
    Human,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActionRuntime {
    summary: String,
    items: u32,
    bytes: u64,
    value: Option<Value>,
    plan_digest: Option<String>,
    uncertainties: Vec<String>,
}

#[derive(Debug, Clone)]
struct PreviewState {
    receipt: WorkflowPlanReceiptV1,
    draft: WorkflowDraftV1,
    runtimes: BTreeMap<String, ActionRuntime>,
}

fn approved_matches(
    approved: &ApprovedWorkflow,
    workflow_digest: &str,
    policy_digest: &str,
    resource_binding_digest: &str,
    remote_identity_digest: &str,
) -> bool {
    approved.enabled
        && approved.workflow_digest == workflow_digest
        && approved.policy_digest == policy_digest
        && approved.resource_binding_digest == resource_binding_digest
        && approved.engine_version == env!("CARGO_PKG_VERSION")
        && approved.engine_protocol_version == WORKFLOW_ENGINE_PROTOCOL_VERSION
        && approved.remote_identity_digest == remote_identity_digest
}

pub async fn execute(
    config_path: &Path,
    state_root: &Path,
    request: WorkflowRequestV1,
) -> Result<WorkflowResponseV1> {
    execute_with_run_id(config_path, state_root, request, None).await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityWorkflowInvocation {
    pub context: crate::runtime::ExecutionContext,
    pub config_path: PathBuf,
    pub request: WorkflowRequestV1,
}

/// Retained admission for one already identified workflow occurrence. The ca
/// host passes these same resources to its worker; this guard never schedules
/// another occurrence or changes the conservative physical ownership keys.
pub struct WorkflowAdmission {
    _executor: Option<crate::runtime::ResourceLease>,
    pub resources: Vec<crate::runtime::ResourceLease>,
    deadline: Instant,
}

impl WorkflowAdmission {
    pub fn remaining_runtime(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
}

/// Wait only for contention before starting effects, within the supplied run's
/// runtime and cancellation budget. A duplicate observes its terminal receipt
/// instead of acquiring authority to execute a second time.
pub async fn admit_workflow_run(
    store: &crate::runtime::RunStore,
    claim_executor: bool,
) -> Result<Option<WorkflowAdmission>> {
    let deadline = Instant::now() + Duration::from_secs(store.context().runtime_seconds);
    loop {
        if store.status()?.state.terminal() {
            return Ok(None);
        }
        if store.cancel_requested()? {
            let receipt = store.reconcile()?;
            if receipt.state.terminal() {
                return Ok(None);
            }
            bail!("workflow admission was cancelled; its active owner retains recovery authority");
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "workflow runtime budget exhausted while awaiting resource admission"
        );
        let admission = (|| -> Result<WorkflowAdmission> {
            let executor = if claim_executor {
                Some(store.execution_lease()?)
            } else {
                None
            };
            let resources = store.acquire_resources()?;
            Ok(WorkflowAdmission {
                _executor: executor,
                resources,
                deadline,
            })
        })();
        match admission {
            Ok(admission) => {
                // A previous owner can finish or receive cancellation while
                // this occurrence was waiting for its exact resource claims.
                if store.status()?.state.terminal() {
                    return Ok(None);
                }
                if store.cancel_requested()? {
                    drop(admission);
                    continue;
                }
                return Ok(Some(admission));
            }
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock) => {}
            Err(error) => return Err(error),
        }
        tokio::time::sleep(
            Duration::from_millis(20).min(deadline.saturating_duration_since(Instant::now())),
        )
        .await;
    }
}

/// Rebind the legacy request without changing its V1 identity or storage root.
pub fn compatibility_invocation(
    config_path: &Path,
    state_root: &Path,
    request: WorkflowRequestV1,
    run_id: Option<String>,
) -> Result<CompatibilityWorkflowInvocation> {
    validate_workflow_request(&request).map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        matches!(&request.operation, WorkflowOperationV1::Run { .. }),
        "compatibility dispatch only accepts a workflow occurrence"
    );
    let storage = WorkflowStore::open(state_root)?;
    let config = config_path.canonicalize()?;
    let request_bytes = serde_json::to_vec(&request)?;
    let definition_digest = format!("{:x}", Sha256::digest(&request_bytes));
    let binding = crate::remote::workflow_config_summary(&config)?.binding_digest;
    let run_id = run_id.unwrap_or_else(|| {
        format!(
            "compatibility_{}_{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        )
    });
    let context = crate::runtime::ExecutionContext {
        schema: crate::runtime::EXECUTION_SCHEMA.into(),
        run_id,
        profile: "compatibility".into(),
        state_root: storage.root.canonicalize()?,
        lease_root: crate::runtime::machine_lease_root()?,
        definition_digest,
        resource_binding_digest: binding,
        policy_digest: format!("{:x}", Sha256::digest(&request_bytes)),
        resources: vec![
            format!("workflow:{}", config.display()),
            "typed-workflow-mutation".into(),
        ],
        runtime_seconds: 86_400,
        output_bytes: 8 * 1024 * 1024,
    };
    context.validate().map_err(anyhow::Error::msg)?;
    Ok(CompatibilityWorkflowInvocation {
        context,
        config_path: config,
        request,
    })
}

#[cfg(not(test))]
async fn dispatch_compatibility(
    invocation: CompatibilityWorkflowInvocation,
) -> Result<crate::runtime::ExecutionReceipt> {
    use std::process::Stdio;
    let configured = std::env::var_os("CA_AUTOMATION_HOST").map(PathBuf::from);
    let host = match configured {
        Some(path) => path,
        None => std::env::current_exe()?
            .parent()
            .context("legacy executable has no parent")?
            .join("ca"),
    };
    let host = host.canonicalize().context(
        "ca-owned workflow dispatch is unavailable; install the matching ca host or configure CA_AUTOMATION_HOST"
    )?;
    let metadata = fs::metadata(&host)?;
    anyhow::ensure!(
        metadata.is_file(),
        "ca automation host must be a regular executable"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(
            (metadata.uid() == unsafe { libc::geteuid() } || metadata.uid() == 0)
                && metadata.mode() & 0o022 == 0
                && metadata.mode() & 0o111 != 0,
            "ca automation host must be owner-controlled and executable"
        );
    }
    let mut command = tokio::process::Command::new(host);
    command
        .args(["automation-engine", "__compatibility-workflow"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    // The host owns the occurrence. Ending this observation must not cancel it.
    use tokio::io::AsyncWriteExt;
    let mut child = command
        .spawn()
        .context("start ca-owned workflow dispatch")?;
    let mut stdin = child.stdin.take().context("ca host omitted input")?;
    let stdout = child.stdout.take().context("ca host omitted output")?;
    let stderr = child.stderr.take().context("ca host omitted diagnostics")?;
    let bytes = serde_json::to_vec(&invocation)?;
    let limit = invocation.context.output_bytes as usize + 256 * 1024;
    let total = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let stdout_total = total.clone();
    let output = tokio::time::timeout(
        Duration::from_secs(invocation.context.runtime_seconds + 5),
        async {
            let (status, stdout, stderr, ()) = tokio::try_join!(
                async { child.wait().await.map_err(anyhow::Error::from) },
                crate::remote::read_process_output(stdout, stdout_total, limit),
                crate::remote::read_process_output(stderr, total, limit),
                async move {
                    stdin.write_all(&bytes).await?;
                    stdin.shutdown().await?;
                    drop(stdin);
                    Ok::<_, anyhow::Error>(())
                }
            )?;
            Ok::<_, anyhow::Error>(std::process::Output {
                status,
                stdout,
                stderr,
            })
        },
    )
    .await
    .context("ca workflow observation timed out; inspect or cancel its durable run")??;
    anyhow::ensure!(
        output.status.success(),
        "ca-owned workflow dispatch failed; inspect its durable run"
    );
    let receipt: crate::runtime::ExecutionReceipt = serde_json::from_slice(&output.stdout)
        .context("ca-owned workflow dispatch returned an invalid receipt")?;
    let persisted = crate::runtime::RunStore::open(&invocation.context)?.status()?;
    anyhow::ensure!(
        receipt == persisted,
        "ca workflow response differs from its durable receipt"
    );
    Ok(receipt)
}

#[cfg(test)]
async fn dispatch_compatibility(
    invocation: CompatibilityWorkflowInvocation,
) -> Result<crate::runtime::ExecutionReceipt> {
    // Library unit tests exercise typed adapters without requiring an installed
    // host. Built CLI/MCP integration exercises the actual supervised boundary.
    Box::pin(crate::native::AutomationEngine::start(
        &invocation.context,
        crate::native::EngineOperation::Workflow {
            config_path: invocation.config_path,
            request: invocation.request,
        },
    ))
    .await
}

/// The compatibility adapter supplies one occurrence identity while retaining
/// its V1 response. Native callers already own the execution context.
pub async fn execute_with_run_id(
    config_path: &Path,
    state_root: &Path,
    request: WorkflowRequestV1,
    run_id: Option<String>,
) -> Result<WorkflowResponseV1> {
    validate_workflow_request(&request).map_err(anyhow::Error::msg)?;
    if crate::runtime::current_run_store().is_none()
        && matches!(&request.operation, WorkflowOperationV1::Run { .. })
    {
        let receipt = dispatch_compatibility(compatibility_invocation(
            config_path,
            state_root,
            request,
            run_id,
        )?)
        .await?;
        let result = receipt.result.with_context(|| {
            format!(
                "workflow run {} requires attention: {}",
                receipt.run_id,
                receipt.reason.as_deref().unwrap_or("unconfirmed effects")
            )
        })?;
        return Ok(serde_json::from_value(result)?);
    }
    let _legacy_resource_lease = if crate::runtime::current_run_store().is_none()
        && matches!(&request.operation, WorkflowOperationV1::Run { .. })
    {
        let config = std::fs::canonicalize(config_path)?;
        Some(crate::runtime::ResourceLease::acquire(
            &crate::runtime::machine_lease_root()?,
            &format!("workflow:{}", config.display()),
        )?)
    } else {
        None
    };
    // Conservative global ownership also covers separate profile manifests that
    // bind the same physical endpoint or overlapping destination paths.
    let _mutation_lease = if crate::runtime::current_run_store().is_none()
        && matches!(&request.operation, WorkflowOperationV1::Run { .. })
    {
        Some(crate::runtime::ResourceLease::acquire(
            &crate::runtime::machine_lease_root()?,
            "typed-workflow-mutation",
        )?)
    } else {
        None
    };
    let store = WorkflowStore::open(state_root)?;
    match request.operation {
        WorkflowOperationV1::List { workflow_id } => {
            let workflows = store.list(workflow_id.as_deref())?;
            Ok(WorkflowResponseV1::List { workflows })
        }
        WorkflowOperationV1::Preview {
            draft,
            depth,
            correction_round,
        } => {
            let preview = preview(config_path, &store, draft, depth, correction_round).await?;
            Ok(WorkflowResponseV1::Plan {
                receipt: preview.receipt,
            })
        }
        WorkflowOperationV1::Correct {
            draft,
            correction,
            accepted_correction_digest,
        } => {
            let response = correct(
                config_path,
                &store,
                draft,
                correction,
                accepted_correction_digest.as_deref(),
            )
            .await?;
            Ok(response)
        }
        WorkflowOperationV1::Run {
            workflow_id,
            draft,
            accepted_plan_digest,
            expected_workflow_digest,
            expected_resource_binding_digest,
        } => {
            let receipt = run(
                config_path,
                &store,
                &workflow_id,
                draft,
                accepted_plan_digest.as_deref(),
                expected_workflow_digest.as_deref(),
                expected_resource_binding_digest.as_deref(),
            )
            .await?;
            Ok(WorkflowResponseV1::Execution { receipt })
        }
        WorkflowOperationV1::Disable {
            workflow_id,
            accepted_workflow_digest,
        } => {
            let workflow_digest = store.disable(&workflow_id, &accepted_workflow_digest)?;
            Ok(WorkflowResponseV1::Disabled {
                workflow_id,
                workflow_digest,
            })
        }
        WorkflowOperationV1::Feedback {
            workflow_digest,
            receipt_digest,
            category,
        } => {
            let disabled = store.record_feedback(&workflow_digest, &receipt_digest, category)?;
            Ok(WorkflowResponseV1::FeedbackRecorded {
                workflow_digest,
                disabled,
            })
        }
    }
}

async fn preview(
    config_path: &Path,
    store: &WorkflowStore,
    mut draft: WorkflowDraftV1,
    depth: WorkflowDryRunDepthV1,
    correction_round: u8,
) -> Result<PreviewState> {
    validate_workflow_draft(&draft).map_err(anyhow::Error::msg)?;
    let remote = crate::remote::workflow_config_summary(config_path)?;
    let (policy_digest, policy_reasons) = store.apply_policies(&mut draft, &remote)?;
    let policy_violations = store.policy_violations(&draft, &remote)?;
    validate_workflow_draft(&draft).map_err(anyhow::Error::msg)?;
    validate_resources(&draft, &remote)?;
    let workflow_digest = digest_json(&draft)?;
    let mut action_plans = Vec::new();
    let mut runtimes = BTreeMap::new();
    for batch in topological_action_batches(&draft)? {
        let outcomes = stream::iter(batch.iter().map(|node| {
            preview_action(
                config_path,
                &remote,
                node,
                depth,
                draft.limits.conflict_policy,
                &draft.limits,
                &runtimes,
            )
        }))
        .buffered(4)
        .collect::<Vec<_>>()
        .await;
        for (node, outcome) in batch.into_iter().zip(outcomes) {
            let runtime = outcome?;
            let (target_hashes, torrent_control) = match &node.action {
                WorkflowActionV1::ControlTorrents { control, .. } => (
                    runtime
                        .value
                        .as_ref()
                        .map(selected_hashes)
                        .transpose()?
                        .unwrap_or_default(),
                    Some(control.clone()),
                ),
                _ => (Vec::new(), None),
            };
            action_plans.push(WorkflowActionPlanV1 {
                action_id: node.id.clone(),
                kind: node.action.kind_name().to_string(),
                summary: bounded_text(&runtime.summary, 1_024),
                predicted_items: runtime.items,
                predicted_bytes: runtime.bytes,
                uncertainties: runtime
                    .uncertainties
                    .iter()
                    .map(|value| bounded_text(value, 256))
                    .collect(),
                target_hashes,
                torrent_control,
            });
            runtimes.insert(node.id.clone(), runtime);
        }
    }
    let (remote_identity_digest, probe_fingerprint) = preview_binding_fingerprints(&runtimes)?;
    let mutation_present = draft.actions.iter().any(|node| node.action.is_mutating());
    let (predicted_items, predicted_bytes) = predicted_effect_totals(&draft, &runtimes);
    let mut reason_codes = policy_reasons;
    let mut status = if policy_violations.is_empty() {
        WorkflowPlanStatusV1::Previewed
    } else {
        reason_codes.extend(policy_violations);
        WorkflowPlanStatusV1::Blocked
    };
    if runtimes.values().any(|runtime| {
        runtime
            .uncertainties
            .iter()
            .any(|reason| reason == "destination_conflict")
    }) {
        reason_codes.push("destination_conflict".into());
        status = WorkflowPlanStatusV1::Blocked;
    }
    enforce_limits(
        &draft.limits,
        (predicted_items, predicted_bytes),
        mutation_present,
        &draft,
        &remote,
        &mut reason_codes,
        &mut status,
    );
    let run_plan_digest = crate::json_identity::digest_value(&serde_json::json!({
        "schema": "codex-team.workflow_run_plan.v1",
        "workflow_digest": workflow_digest,
        "policy_digest": policy_digest,
        "resource_binding_digest": remote.binding_digest,
        "engine_version": env!("CARGO_PKG_VERSION"),
        "protocol_version": WORKFLOW_ENGINE_PROTOCOL_VERSION,
        "remote_identity_digest": remote_identity_digest,
        "probe_fingerprint": probe_fingerprint,
        "depth": depth,
        "actions": action_plans,
        "predicted_items": predicted_items,
        "predicted_bytes": predicted_bytes,
    }))?;
    let approved = store.read_approved(&draft.workflow_id)?;
    let reusable = approved.as_ref().is_some_and(|approved| {
        approved_matches(
            approved,
            &workflow_digest,
            &policy_digest,
            &remote.binding_digest,
            &remote_identity_digest,
        )
    });
    let approval_required = mutation_present && !reusable;
    if status != WorkflowPlanStatusV1::Blocked {
        status = if approval_required {
            WorkflowPlanStatusV1::ApprovalRequired
        } else if mutation_present && depth == WorkflowDryRunDepthV1::MutationPreview {
            WorkflowPlanStatusV1::Ready
        } else {
            WorkflowPlanStatusV1::Previewed
        };
    }
    let mut receipt = WorkflowPlanReceiptV1 {
        schema: WORKFLOW_PLAN_RECEIPT_SCHEMA.to_string(),
        workflow_id: draft.workflow_id.clone(),
        depth,
        status,
        workflow_digest,
        policy_digest,
        resource_binding_digest: remote.binding_digest,
        run_plan_digest,
        action_plans,
        predicted_items,
        predicted_bytes,
        mutation_present,
        limits_authorize_mutation: draft.limits.authorizes_mutation(),
        approval_required,
        rollback: draft.rollback.clone(),
        correction_round,
        reason_codes,
        truncated: false,
    };
    bound_plan_receipt(&mut receipt)?;
    store.record_preview(&receipt)?;
    Ok(PreviewState {
        receipt,
        draft,
        runtimes,
    })
}

async fn preview_action(
    config_path: &Path,
    _remote: &crate::remote::WorkflowRemoteSummary,
    node: &ca_automation_protocol::WorkflowNodeV1,
    depth: WorkflowDryRunDepthV1,
    conflict_policy: ca_automation_protocol::WorkflowConflictPolicyV1,
    limits: &WorkflowLimitsV1,
    prior: &BTreeMap<String, ActionRuntime>,
) -> Result<ActionRuntime> {
    let offline = depth == WorkflowDryRunDepthV1::Offline;
    let mutation_preview = depth == WorkflowDryRunDepthV1::MutationPreview;
    match &node.action {
        WorkflowActionV1::ProbeSsh { host } => {
            if offline {
                return Ok(offline_runtime(format!(
                    "validated configured SSH host `{host}`"
                )));
            }
            let probe = crate::remote::workflow_probe_ssh(config_path, host).await?;
            Ok(ActionRuntime {
                summary: format!("{}; identity digest {}", probe.summary, probe.identity_digest),
                items: 0,
                bytes: 0,
                value: Some(serde_json::json!({
                    "identity_digest": probe.identity_digest,
                })),
                plan_digest: None,
                uncertainties: Vec::new(),
            })
        }
        WorkflowActionV1::QueryTransmission {
            endpoint,
            operation,
        } => {
            if offline {
                return Ok(offline_runtime(format!(
                    "validated configured Transmission {:?} query for `{endpoint}`",
                    operation
                )));
            }
            let value = crate::remote::workflow_query_transmission(config_path, endpoint, *operation)
                .await?;
            let items = value
                .get("torrents")
                .and_then(Value::as_array)
                .map_or(0, |values| values.len().min(u32::MAX as usize) as u32);
            Ok(ActionRuntime {
                summary: format!("read-only Transmission {:?} query completed", operation),
                items,
                bytes: 0,
                value: Some(value),
                plan_digest: None,
                uncertainties: Vec::new(),
            })
        }
        WorkflowActionV1::QueryTorrents { endpoint } => {
            if offline {
                return Ok(offline_runtime(format!(
                    "validated configured torrent inventory query for `{endpoint}`"
                )));
            }
            let inventory = crate::remote::workflow_query_torrents(config_path, endpoint).await?;
            let items = inventory.torrents.len().min(u32::MAX as usize) as u32;
            Ok(ActionRuntime {
                summary: format!("read-only normalized torrent query returned {items} items"),
                items,
                bytes: 0,
                value: Some(serde_json::to_value(inventory)?),
                plan_digest: None,
                uncertainties: Vec::new(),
            })
        }
        WorkflowActionV1::SelectTorrents {
            source_action,
            completed,
            label,
            info_hashes,
            states,
            maximum_items,
        } => {
            if offline {
                return Ok(offline_runtime("validated bounded torrent selector".into()));
            }
            let source = prior
                .get(source_action)
                .and_then(|runtime| runtime.value.as_ref())
                .context("torrent selector source did not produce RPC data")?;
            let selected = select_torrents(
                source,
                *completed,
                label.as_deref(),
                info_hashes,
                states,
                *maximum_items,
            )?;
            let bytes = selected
                .iter()
                .filter_map(|item| {
                    item.get("total_bytes")
                        .or_else(|| item.get("totalSize"))
                        .and_then(Value::as_u64)
                })
                .sum();
            Ok(ActionRuntime {
                summary: format!("selected {} bounded torrent entries", selected.len()),
                items: selected.len() as u32,
                bytes,
                value: Some(Value::Array(selected)),
                plan_digest: None,
                uncertainties: Vec::new(),
            })
        }
        WorkflowActionV1::ControlTorrents {
            endpoint,
            selection_action,
            control,
        } => {
            if offline {
                return Ok(ActionRuntime {
                    uncertainties: vec![
                        "exact torrent targets require a read-only or mutation preview".into(),
                    ],
                    ..offline_runtime(format!(
                        "validated governed {:?} control for `{endpoint}`",
                        control
                    ))
                });
            }
            let selected = prior
                .get(selection_action)
                .and_then(|runtime| runtime.value.as_ref())
                .and_then(Value::as_array)
                .context("torrent control selection is unavailable")?;
            Ok(ActionRuntime {
                summary: format!(
                    "would apply {:?} to {} selected torrent(s) on `{endpoint}`",
                    control,
                    selected.len()
                ),
                items: selected.len().min(u32::MAX as usize) as u32,
                bytes: 0,
                value: Some(Value::Array(selected.clone())),
                plan_digest: None,
                uncertainties: if mutation_preview {
                    Vec::new()
                } else {
                    vec!["torrent control remains disabled until an exact mutation preview".into()]
                },
            })
        }
        WorkflowActionV1::CheckFreeSpace {
            endpoint,
            minimum_bytes,
        } => {
            if offline {
                return Ok(offline_runtime(format!(
                    "validated free-space precondition for `{endpoint}`"
                )));
            }
            let value = crate::remote::workflow_query_transmission(
                config_path,
                endpoint,
                RemoteRpcOperationV1::FreeSpace,
            )
            .await?;
            let available = value
                .get("size-bytes")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if available < *minimum_bytes {
                bail!("insufficient remote free space")
            }
            Ok(ActionRuntime {
                summary: format!("free-space precondition passed with {available} bytes"),
                items: 0,
                bytes: available,
                value: Some(value),
                plan_digest: None,
                uncertainties: Vec::new(),
            })
        }
        WorkflowActionV1::CompareInventory { profile }
        | WorkflowActionV1::PlanSync { profile, .. } => {
            if !mutation_preview {
                return Ok(ActionRuntime {
                    uncertainties: vec![
                        "exact itemized effects require mutation-preview depth".into(),
                    ],
                    ..offline_runtime(format!("validated configured sync profile `{profile}`"))
                });
            }
            let keep_both = conflict_policy
                == ca_automation_protocol::WorkflowConflictPolicyV1::KeepBoth;
            let selection = match &node.action {
                WorkflowActionV1::PlanSync {
                    selection_action: Some(selection_action),
                    ..
                } => prior
                    .get(selection_action)
                    .and_then(|runtime| runtime.value.as_ref()),
                _ => None,
            };
            let plan = crate::remote::workflow_plan_sync(
                config_path,
                profile,
                keep_both,
                selection,
                limits.max_files.map(|value| value as usize),
            )
            .await?;
            let uncertainties = if plan.has_conflicts && !keep_both {
                vec!["destination_conflict".into()]
            } else {
                Vec::new()
            };
            Ok(ActionRuntime {
                summary: format!("predicted {} itemized sync changes", plan.entries.len()),
                items: plan.entries.len() as u32,
                bytes: plan.predicted_bytes,
                value: Some(Value::Array(
                    plan.entries.into_iter().map(Value::String).collect(),
                )),
                plan_digest: Some(plan.digest),
                uncertainties,
            })
        }
        WorkflowActionV1::ApplySync {
            profile,
            plan_action,
        } => {
            let plan = prior.get(plan_action);
            Ok(ActionRuntime {
                summary: format!("would apply exact plan for sync profile `{profile}`"),
                items: plan.map_or(0, |runtime| runtime.items),
                bytes: plan.map_or(0, |runtime| runtime.bytes),
                value: None,
                plan_digest: plan.and_then(|runtime| runtime.plan_digest.clone()),
                uncertainties: if mutation_preview {
                    Vec::new()
                } else {
                    vec!["apply remains disabled until an exact mutation preview".into()]
                },
            })
        }
        WorkflowActionV1::VerifySha256 {
            profile,
            maximum_files,
        } => Ok(ActionRuntime {
            summary: format!(
                "would compare SHA-256 inventory for `{profile}` across at most {maximum_files} files"
            ),
            items: 0,
            bytes: 0,
            value: None,
            plan_digest: None,
            uncertainties: vec!["verification runs after the approved mutation".into()],
        }),
        WorkflowActionV1::Assert {
            source_action,
            condition,
        } => {
            if offline {
                return Ok(offline_runtime("validated typed outcome assertion".into()));
            }
            let source = prior
                .get(source_action)
                .context("assertion source did not produce a result")?;
            assert_runtime(source, condition)?;
            Ok(ActionRuntime {
                summary: "typed outcome assertion passed".into(),
                items: source.items,
                bytes: source.bytes,
                value: None,
                plan_digest: None,
                uncertainties: Vec::new(),
            })
        }
    }
}

fn offline_runtime(summary: String) -> ActionRuntime {
    ActionRuntime {
        summary,
        items: 0,
        bytes: 0,
        value: None,
        plan_digest: None,
        uncertainties: Vec::new(),
    }
}

async fn correct(
    config_path: &Path,
    store: &WorkflowStore,
    draft: WorkflowDraftV1,
    correction: ca_automation_protocol::WorkflowCorrectionV1,
    accepted_digest: Option<&str>,
) -> Result<WorkflowResponseV1> {
    let remote = crate::remote::workflow_config_summary(config_path)?;
    let mut draft = draft;
    let (policy_digest, _) = store.apply_policies(&mut draft, &remote)?;
    validate_workflow_draft(&draft).map_err(anyhow::Error::msg)?;
    validate_resources(&draft, &remote)?;
    let workflow_digest = digest_json(&draft)?;
    store.validate_preview_source(
        &correction.source_plan_digest,
        &draft.workflow_id,
        &workflow_digest,
        &policy_digest,
        &remote.binding_digest,
        correction.correction_round.saturating_sub(1),
    )?;
    match &correction.scope {
        WorkflowCorrectionScopeV1::Workflow { workflow_id }
            if workflow_id != &draft.workflow_id =>
        {
            bail!("workflow correction scope does not match the previewed workflow")
        }
        WorkflowCorrectionScopeV1::Resource {
            resource_binding_digest,
        } if resource_binding_digest != &remote.binding_digest => {
            bail!("workflow correction resource scope is stale")
        }
        _ => {}
    }
    let classification = classify_correction_against_draft(&draft, &correction);
    let correction_digest = crate::json_identity::digest_value(&serde_json::json!({
        "schema": "codex-team.workflow_correction_plan.v1",
        "draft": draft,
        "correction": correction,
        "resource_binding_digest": remote.binding_digest,
        "classification": classification,
    }))?;
    let approval_required = classification == WorkflowCorrectionClassificationV1::Widening;
    let may_persist = classification == WorkflowCorrectionClassificationV1::Tightening
        || (approval_required && accepted_digest == Some(correction_digest.as_str()));
    if accepted_digest.is_some() && !may_persist {
        bail!("workflow correction digest is stale or the correction conflicts")
    }
    apply_correction(&mut draft, &correction, classification)?;
    validate_workflow_draft(&draft).map_err(anyhow::Error::msg)?;
    let mut persisted = false;
    if may_persist {
        let record = PolicyRecord {
            schema: POLICY_SCHEMA.into(),
            correction_digest: correction_digest.clone(),
            classification,
            source: if correction.reason_code
                == ca_automation_protocol::WorkflowCorrectionReasonV1::HumanCorrection
            {
                PolicySource::Human
            } else {
                PolicySource::Ca
            },
            scope: stored_policy_scope(&correction.scope)?,
            changes: correction.changes.clone(),
            created_at: Utc::now().to_rfc3339(),
        };
        store.write_policy(&record)?;
        persisted = true;
    }
    let policy_digest = store.policy_digest(&draft, &remote)?;
    let summary = match classification {
        WorkflowCorrectionClassificationV1::Tightening if persisted => {
            "restriction-tightening correction persisted machine-wide"
        }
        WorkflowCorrectionClassificationV1::Widening if persisted => {
            "approved authority-widening correction persisted"
        }
        WorkflowCorrectionClassificationV1::Widening => {
            "authority-widening correction requires exact approval"
        }
        WorkflowCorrectionClassificationV1::Conflicting => {
            "correction conflicts with the current workflow and was not persisted"
        }
        _ => "correction was not persisted",
    };
    Ok(WorkflowResponseV1::Correction {
        receipt: WorkflowCorrectionReceiptV1 {
            schema: WORKFLOW_CORRECTION_RECEIPT_SCHEMA.into(),
            workflow_id: draft.workflow_id.clone(),
            classification,
            correction_digest,
            policy_digest,
            persisted,
            approval_required: approval_required && !persisted,
            correction_round: correction.correction_round,
            summary: summary.into(),
        },
        corrected_draft: draft,
    })
}

async fn run(
    config_path: &Path,
    store: &WorkflowStore,
    workflow_id: &str,
    draft: Option<WorkflowDraftV1>,
    accepted_plan_digest: Option<&str>,
    expected_workflow_digest: Option<&str>,
    expected_resource_binding_digest: Option<&str>,
) -> Result<WorkflowExecutionReceiptV1> {
    let existing = store.read_approved(workflow_id)?;
    let draft = match (draft, existing.as_ref()) {
        (Some(draft), _) => draft,
        (None, Some(approved)) if approved.enabled => approved.draft.clone(),
        (None, Some(_)) => bail!("workflow is disabled"),
        (None, None) => bail!("workflow has not been approved"),
    };
    let preview = preview(
        config_path,
        store,
        draft,
        WorkflowDryRunDepthV1::MutationPreview,
        0,
    )
    .await?;
    if preview.receipt.status == WorkflowPlanStatusV1::Blocked {
        return Ok(blocked_execution(
            &preview.receipt,
            "workflow preview is blocked",
        ));
    }
    if expected_workflow_digest.is_some_and(|expected| expected != preview.receipt.workflow_digest)
    {
        return Ok(stale_execution(
            &preview.receipt,
            "scheduled workflow revision is stale",
        ));
    }
    if expected_resource_binding_digest
        .is_some_and(|expected| expected != preview.receipt.resource_binding_digest)
    {
        return Ok(stale_execution(
            &preview.receipt,
            "scheduled workflow resource binding is stale",
        ));
    }
    let remote_identity_digest = preview_binding_fingerprints(&preview.runtimes)?.0;
    let reusable = existing.as_ref().is_some_and(|approved| {
        approved_matches(
            approved,
            &preview.receipt.workflow_digest,
            &preview.receipt.policy_digest,
            &preview.receipt.resource_binding_digest,
            &remote_identity_digest,
        )
    });
    if !reusable {
        if accepted_plan_digest != Some(preview.receipt.run_plan_digest.as_str()) {
            return Ok(blocked_execution(
                &preview.receipt,
                "exact first-run plan approval is required",
            ));
        }
        if !preview.draft.limits.authorizes_mutation() {
            return Ok(blocked_execution(
                &preview.receipt,
                "workflow lacks bounded mutation limits",
            ));
        }
        store.write_approved(&ApprovedWorkflow {
            schema: APPROVED_SCHEMA.into(),
            workflow_id: workflow_id.into(),
            workflow_digest: preview.receipt.workflow_digest.clone(),
            policy_digest: preview.receipt.policy_digest.clone(),
            resource_binding_digest: preview.receipt.resource_binding_digest.clone(),
            engine_version: env!("CARGO_PKG_VERSION").into(),
            engine_protocol_version: WORKFLOW_ENGINE_PROTOCOL_VERSION,
            remote_identity_digest,
            first_run_plan_digest: preview.receipt.run_plan_digest.clone(),
            enabled: true,
            approved_at: Utc::now().to_rfc3339(),
            draft: preview.draft.clone(),
        })?;
    }
    let current_effects = preview_effect_hashes(&preview);
    let recovering = if let Some(checkpoint) = store.read_checkpoint(workflow_id)? {
        if checkpoint.phase == WorkflowCheckpointPhase::Running {
            let native_run = crate::runtime::current_run_store()
                .map(|store| store.status().map(|receipt| receipt.run_id))
                .transpose()?;
            if native_run.is_none() || checkpoint.run_id != native_run {
                return Ok(blocked_execution(
                    &preview.receipt,
                    "interrupted workflow effects require reconciliation before another run",
                ));
            }
            if checkpoint.workflow_digest != preview.receipt.workflow_digest
                || checkpoint.policy_digest != preview.receipt.policy_digest
                || checkpoint.resource_binding_digest != preview.receipt.resource_binding_digest
            {
                return Ok(stale_execution(
                    &preview.receipt,
                    "interrupted workflow bindings changed before recovery",
                ));
            }
            let exact_plan = checkpoint.run_plan_digest == preview.receipt.run_plan_digest;
            let bounded_resume = !preview.draft.limits.deletion_allowed
                && !checkpoint.effect_hashes_truncated
                && !current_effects.1
                && current_effects
                    .0
                    .iter()
                    .all(|effect| checkpoint.effect_hashes.binary_search(effect).is_ok());
            if !exact_plan && !bounded_resume {
                return Ok(stale_execution(
                    &preview.receipt,
                    "interrupted workflow effects drifted before recovery",
                ));
            }
            true
        } else {
            false
        }
    } else {
        false
    };
    store.write_checkpoint(&preview, WorkflowCheckpointPhase::Running, None)?;
    let aggregate_timeout = preview.draft.limits.max_runtime_seconds;
    let plan = preview.receipt.clone();
    let checkpoint_preview = preview.clone();
    let mut receipt = match tokio::time::timeout(
        Duration::from_secs(aggregate_timeout),
        execute_previewed(config_path, preview),
    )
    .await
    {
        Ok(receipt) => receipt?,
        Err(_) => failed_execution(&plan, "workflow aggregate runtime limit was exceeded"),
    };
    if recovering {
        receipt
            .reason_codes
            .push("interrupted_workflow_resumed_from_checkpoint".into());
        receipt
            .deviations
            .push("execution resumed from a bounded interrupted checkpoint".into());
    }
    store.write_checkpoint(
        &checkpoint_preview,
        WorkflowCheckpointPhase::Terminal,
        Some(receipt.status),
    )?;
    Ok(receipt)
}

async fn execute_previewed(
    config_path: &Path,
    preview: PreviewState,
) -> Result<WorkflowExecutionReceiptV1> {
    let started_at = Utc::now();
    let started = Instant::now();
    let mut results = Vec::new();
    let mut deterministic_repairs = 0_u8;
    let mut runtimes = preview.runtimes.clone();
    let mut applied_profiles = BTreeSet::new();
    for batch in topological_action_batches(&preview.draft)? {
        let outcomes = stream::iter(batch.iter().map(|node| {
            execute_action_with_retries(
                config_path,
                node,
                preview.draft.limits.conflict_policy,
                &preview.draft,
                &runtimes,
            )
        }))
        .buffered(4)
        .collect::<Vec<_>>()
        .await;
        let mut batch_failure = None;
        for (node, outcome) in batch.into_iter().zip(outcomes) {
            let (outcome, attempts, repairs, duration_millis) = outcome;
            deterministic_repairs = deterministic_repairs.saturating_add(repairs);
            match outcome {
                Ok(runtime) => {
                    if let WorkflowActionV1::ApplySync { profile, .. } = &node.action {
                        applied_profiles.insert(profile.clone());
                    }
                    let summary = bounded_text(&runtime.summary, 1_024);
                    let torrent_mutation = runtime.value.as_ref().and_then(|value| {
                        serde_json::from_value::<ca_automation_protocol::TorrentMutationReceiptV1>(
                            value.clone(),
                        )
                        .ok()
                    });
                    runtimes.insert(node.id.clone(), runtime);
                    results.push(WorkflowActionExecutionV1 {
                        action_id: node.id.clone(),
                        status: WorkflowExecutionStatusV1::Completed,
                        summary,
                        attempts,
                        repair_cause: None,
                        torrent_mutation,
                        duration_millis,
                    });
                }
                Err(error) => {
                    let cause = classify_failure(&error);
                    results.push(WorkflowActionExecutionV1 {
                        action_id: node.id.clone(),
                        status: WorkflowExecutionStatusV1::Failed,
                        summary: bounded_text(&format!("{error:#}"), 1_024),
                        attempts,
                        repair_cause: Some(cause),
                        torrent_mutation: None,
                        duration_millis,
                    });
                    batch_failure.get_or_insert(cause);
                }
            }
        }
        if let Some(cause) = batch_failure {
            return finish_execution(
                &preview,
                WorkflowExecutionStatusV1::Failed,
                results,
                deterministic_repairs,
                false,
                vec![format!("failure_{cause:?}").to_lowercase()],
                started_at,
                started,
            );
        }
    }
    let mut verification = verify_outcome(
        config_path,
        &preview.draft.limits,
        &preview.draft,
        &runtimes,
        &applied_profiles,
    )
    .await;
    let verification_cause = verification.as_ref().err().map(classify_failure);
    let repair_allowed = matches!(
        verification_cause,
        Some(
            WorkflowRepairCauseV1::InterruptedTransfer
                | WorkflowRepairCauseV1::TransientTransport
                | WorkflowRepairCauseV1::ChecksumMismatch
        )
    );
    if repair_allowed && !applied_profiles.is_empty() {
        let repair_started = Instant::now();
        let mut repaired = true;
        for profile in &applied_profiles {
            let keep_both = preview.draft.limits.conflict_policy
                == ca_automation_protocol::WorkflowConflictPolicyV1::KeepBoth;
            let selection = selection_for_profile(&preview.draft, &preview.runtimes, profile);
            let plan = crate::remote::workflow_plan_sync(
                config_path,
                profile,
                keep_both,
                selection,
                preview.draft.limits.max_files.map(|value| value as usize),
            )
            .await?;
            if !plan.entries.is_empty() {
                if preview
                    .draft
                    .limits
                    .max_items
                    .is_some_and(|limit| plan.entries.len() as u64 > u64::from(limit))
                    || preview
                        .draft
                        .limits
                        .max_transfer_bytes
                        .is_some_and(|limit| plan.predicted_bytes > limit)
                {
                    bail!("verification repair exceeds the approved mutation limits");
                }
                let journal = crate::runtime::current_run_store();
                let action_id = format!("verification_repair_{profile}");
                if let Some(store) = &journal {
                    store.prepare_action(&action_id)?;
                    if !store.begin_action(&action_id)? {
                        bail!("confirmed verification repair cannot be repeated; review the changed outcome");
                    }
                }
                if crate::remote::workflow_apply_sync(
                    config_path,
                    profile,
                    &plan.digest,
                    keep_both,
                    selection,
                    preview.draft.limits.max_files.map(|value| value as usize),
                )
                .await
                .is_err()
                {
                    repaired = false;
                    break;
                }
                if let Some(store) = &journal {
                    store.confirm_action(&action_id,
                    serde_json::json!({"profile":profile,"plan_digest":plan.digest,"completed":true}))?;
                }
            }
        }
        if repaired {
            deterministic_repairs = deterministic_repairs.saturating_add(1);
            verification = verify_outcome(
                config_path,
                &preview.draft.limits,
                &preview.draft,
                &runtimes,
                &applied_profiles,
            )
            .await;
            results.push(WorkflowActionExecutionV1 {
                action_id: "verification_repair".into(),
                status: if verification.is_ok() {
                    WorkflowExecutionStatusV1::Completed
                } else {
                    WorkflowExecutionStatusV1::Failed
                },
                summary: "bounded verification repair and recheck completed".into(),
                attempts: 1,
                repair_cause: verification_cause,
                torrent_mutation: None,
                duration_millis: repair_started.elapsed().as_millis() as u64,
            });
        }
    }
    let (status, verification_passed, reasons) = match verification {
        Ok(()) => (WorkflowExecutionStatusV1::Completed, true, Vec::new()),
        Err(error) => (
            WorkflowExecutionStatusV1::Failed,
            false,
            vec![bounded_text(
                &format!("verification_failed: {error:#}"),
                1_024,
            )],
        ),
    };
    finish_execution(
        &preview,
        status,
        results,
        deterministic_repairs,
        verification_passed,
        reasons,
        started_at,
        started,
    )
}

async fn execute_action(
    config_path: &Path,
    node: &ca_automation_protocol::WorkflowNodeV1,
    conflict_policy: ca_automation_protocol::WorkflowConflictPolicyV1,
    draft: &WorkflowDraftV1,
    prior: &BTreeMap<String, ActionRuntime>,
) -> Result<ActionRuntime> {
    match &node.action {
        WorkflowActionV1::ApplySync {
            profile,
            plan_action,
        } => {
            let runtime = prior
                .get(plan_action)
                .context("apply action has no bounded sync plan")?;
            if draft
                .limits
                .max_items
                .is_some_and(|limit| runtime.items > limit)
                || draft
                    .limits
                    .max_transfer_bytes
                    .is_some_and(|limit| runtime.bytes > limit)
            {
                bail!("current sync plan exceeds approved mutation limits");
            }
            let digest = prior
                .get(plan_action)
                .and_then(|runtime| runtime.plan_digest.as_deref())
                .context("apply action has no exact sync plan digest")?;
            let selection = draft
                .actions
                .iter()
                .find(|candidate| candidate.id == *plan_action)
                .and_then(|candidate| match &candidate.action {
                    WorkflowActionV1::PlanSync {
                        selection_action: Some(selection_action),
                        ..
                    } => prior
                        .get(selection_action)
                        .and_then(|runtime| runtime.value.as_ref()),
                    _ => None,
                });
            crate::remote::workflow_apply_sync(
                config_path,
                profile,
                digest,
                conflict_policy == ca_automation_protocol::WorkflowConflictPolicyV1::KeepBoth,
                selection,
                draft.limits.max_files.map(|value| value as usize),
            )
            .await?;
            Ok(offline_runtime(format!(
                "applied exact sync plan for `{profile}`"
            )))
        }
        WorkflowActionV1::VerifySha256 {
            profile,
            maximum_files,
        } => {
            let count = crate::remote::workflow_verify_sha256(
                config_path,
                profile,
                *maximum_files as usize,
            )
            .await?;
            Ok(ActionRuntime {
                summary: format!("verified {count} SHA-256 file pairs"),
                items: count as u32,
                ..offline_runtime(String::new())
            })
        }
        WorkflowActionV1::PlanSync { profile, .. }
        | WorkflowActionV1::CompareInventory { profile } => {
            let keep_both =
                conflict_policy == ca_automation_protocol::WorkflowConflictPolicyV1::KeepBoth;
            let selection = match &node.action {
                WorkflowActionV1::PlanSync {
                    selection_action: Some(selection_action),
                    ..
                } => prior
                    .get(selection_action)
                    .and_then(|runtime| runtime.value.as_ref()),
                _ => None,
            };
            let new = crate::remote::workflow_plan_sync(
                config_path,
                profile,
                keep_both,
                selection,
                draft.limits.max_files.map(|value| value as usize),
            )
            .await?;
            if new.has_conflicts && !keep_both {
                bail!("sync preview found a destination conflict")
            }
            if prior
                .get(&node.id)
                .and_then(|runtime| runtime.plan_digest.as_deref())
                .is_some_and(|expected| expected != new.digest)
            {
                bail!("remote sync plan changed after preview")
            }
            Ok(ActionRuntime {
                summary: format!("confirmed {} itemized sync changes", new.entries.len()),
                items: new.entries.len() as u32,
                bytes: new.predicted_bytes,
                value: Some(Value::Array(
                    new.entries.into_iter().map(Value::String).collect(),
                )),
                plan_digest: Some(new.digest),
                uncertainties: Vec::new(),
            })
        }
        WorkflowActionV1::ControlTorrents {
            endpoint,
            selection_action,
            control,
        } => {
            let selection = prior
                .get(selection_action)
                .and_then(|runtime| runtime.value.as_ref())
                .context("torrent control selection is unavailable")?;
            let hashes = selected_hashes(selection)?;
            let receipt = crate::remote::workflow_control_torrents(
                config_path,
                endpoint,
                hashes,
                selection,
                control.clone(),
            )
            .await?;
            Ok(ActionRuntime {
                summary: format!(
                    "applied {:?} to {} selected torrent(s)",
                    control,
                    receipt.target_hashes.len()
                ),
                items: receipt.target_hashes.len().min(u32::MAX as usize) as u32,
                bytes: 0,
                value: Some(serde_json::to_value(receipt)?),
                plan_digest: None,
                uncertainties: Vec::new(),
            })
        }
        _ => {
            let remote = crate::remote::workflow_config_summary(config_path)?;
            let next = preview_action(
                config_path,
                &remote,
                node,
                WorkflowDryRunDepthV1::MutationPreview,
                conflict_policy,
                &draft.limits,
                prior,
            )
            .await?;
            if matches!(node.action, WorkflowActionV1::ProbeSsh { .. })
                && prior
                    .get(&node.id)
                    .is_some_and(|before| before.value != next.value)
            {
                bail!("remote SSH identity changed after preview")
            }
            if matches!(
                node.action,
                WorkflowActionV1::QueryTransmission { .. } | WorkflowActionV1::QueryTorrents { .. }
            ) && prior
                .get(&node.id)
                .is_some_and(|before| before.value != next.value)
            {
                bail!("torrent inventory, client identity, or protocol changed after preview")
            }
            Ok(next)
        }
    }
}

async fn verify_outcome(
    config_path: &Path,
    limits: &WorkflowLimitsV1,
    draft: &WorkflowDraftV1,
    runtimes: &BTreeMap<String, ActionRuntime>,
    applied_profiles: &BTreeSet<String>,
) -> Result<()> {
    for verification in &limits.verification {
        match verification {
            WorkflowVerificationV1::EmptySyncPlan | WorkflowVerificationV1::Inventory => {
                for profile in applied_profiles {
                    let plan = crate::remote::workflow_plan_sync(
                        config_path,
                        profile,
                        limits.conflict_policy
                            == ca_automation_protocol::WorkflowConflictPolicyV1::KeepBoth,
                        selection_for_profile(draft, runtimes, profile),
                        limits.max_files.map(|value| value as usize),
                    )
                    .await?;
                    if !plan.entries.is_empty() {
                        bail!("post-execution sync plan is not empty")
                    }
                }
            }
            WorkflowVerificationV1::Sha256 => {
                for profile in applied_profiles {
                    let maximum = draft
                        .actions
                        .iter()
                        .find_map(|node| match &node.action {
                            WorkflowActionV1::VerifySha256 {
                                profile: candidate,
                                maximum_files,
                            } if candidate == profile => Some(*maximum_files as usize),
                            _ => None,
                        })
                        .or_else(|| limits.max_items.map(|value| value as usize))
                        .context("SHA-256 verification requires a bounded file count")?;
                    crate::remote::workflow_verify_sha256(config_path, profile, maximum).await?;
                }
            }
        }
    }
    Ok(())
}

fn select_torrents(
    value: &Value,
    completed: bool,
    label: Option<&str>,
    info_hashes: &[String],
    states: &[ca_automation_protocol::TorrentStateV1],
    maximum: u32,
) -> Result<Vec<Value>> {
    let torrents = value
        .get("torrents")
        .and_then(Value::as_array)
        .context("torrent query omitted torrents")?;
    let requested_hashes = info_hashes
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut selected = torrents
        .iter()
        .filter(|torrent| {
            let is_completed = torrent
                .get("completed")
                .and_then(Value::as_bool)
                .unwrap_or_else(|| {
                    torrent
                        .get("percentDone")
                        .and_then(Value::as_f64)
                        .is_some_and(|value| value >= 1.0)
                        || torrent.get("leftUntilDone").and_then(Value::as_u64) == Some(0)
                });
            let hash = torrent
                .get("info_hash")
                .or_else(|| torrent.get("hashString"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let state_matches = states.is_empty()
                || torrent
                    .get("state")
                    .and_then(Value::as_str)
                    .is_some_and(|actual| {
                        states.iter().any(|state| {
                            serde_json::to_value(state)
                                .ok()
                                .and_then(|value| value.as_str().map(str::to_string))
                                .as_deref()
                                == Some(actual)
                        })
                    });
            (!completed || is_completed)
                && (requested_hashes.is_empty() || requested_hashes.contains(hash))
                && state_matches
                && label.is_none_or(|expected| {
                    torrent
                        .get("labels")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .any(|actual| actual == expected)
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    selected.sort_by_key(|torrent| {
        torrent
            .get("hashString")
            .or_else(|| torrent.get("info_hash"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    });
    if selected.len() > maximum as usize {
        bail!("torrent selector exceeded its item bound")
    }
    Ok(selected)
}

fn selected_hashes(value: &Value) -> Result<Vec<String>> {
    let values = value
        .as_array()
        .context("torrent selection is not an array")?;
    let mut hashes = values
        .iter()
        .map(|torrent| {
            torrent
                .get("info_hash")
                .or_else(|| torrent.get("hashString"))
                .and_then(Value::as_str)
                .context("selected torrent omitted its info hash")
                .map(str::to_string)
        })
        .collect::<Result<Vec<_>>>()?;
    hashes.sort();
    hashes.dedup();
    if hashes.len() != values.len() {
        bail!("torrent selection contains duplicate info hashes")
    }
    Ok(hashes)
}

fn selection_for_profile<'a>(
    draft: &WorkflowDraftV1,
    runtimes: &'a BTreeMap<String, ActionRuntime>,
    profile: &str,
) -> Option<&'a Value> {
    draft.actions.iter().find_map(|node| match &node.action {
        WorkflowActionV1::PlanSync {
            profile: candidate,
            selection_action: Some(selection_action),
        } if candidate == profile => runtimes
            .get(selection_action)
            .and_then(|runtime| runtime.value.as_ref()),
        _ => None,
    })
}

fn assert_runtime(runtime: &ActionRuntime, condition: &WorkflowAssertionV1) -> Result<()> {
    let passed = match condition {
        WorkflowAssertionV1::Empty => runtime.items == 0,
        WorkflowAssertionV1::NonEmpty => runtime.items > 0,
        WorkflowAssertionV1::Completed => true,
        WorkflowAssertionV1::AtMostItems { maximum } => runtime.items <= *maximum,
    };
    if !passed {
        bail!("typed workflow assertion failed")
    }
    Ok(())
}

fn validate_resources(
    draft: &WorkflowDraftV1,
    remote: &crate::remote::WorkflowRemoteSummary,
) -> Result<()> {
    for node in &draft.actions {
        match &node.action {
            WorkflowActionV1::ProbeSsh { host } if !remote.ssh_hosts.contains(host) => {
                bail!("workflow references an unknown SSH host `{host}`")
            }
            WorkflowActionV1::QueryTransmission { endpoint, .. }
            | WorkflowActionV1::CheckFreeSpace { endpoint, .. }
                if !remote.transmission.contains(endpoint) =>
            {
                bail!("workflow references an unknown Transmission endpoint `{endpoint}`")
            }
            WorkflowActionV1::QueryTorrents { endpoint }
            | WorkflowActionV1::ControlTorrents { endpoint, .. }
                if !remote.transmission.contains(endpoint)
                    && !remote.rtorrent.contains(endpoint) =>
            {
                bail!("workflow references an unknown torrent endpoint `{endpoint}`")
            }
            WorkflowActionV1::CompareInventory { profile }
            | WorkflowActionV1::PlanSync { profile, .. }
            | WorkflowActionV1::ApplySync { profile, .. }
            | WorkflowActionV1::VerifySha256 { profile, .. }
                if !remote.syncs.contains_key(profile) =>
            {
                bail!("workflow references an unknown sync profile `{profile}`")
            }
            _ => {}
        }
    }
    let probes = draft
        .actions
        .iter()
        .filter_map(|node| match &node.action {
            WorkflowActionV1::ProbeSsh { host } => Some((host.as_str(), node.id.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    for node in &draft.actions {
        let WorkflowActionV1::QueryTorrents { endpoint } = &node.action else {
            continue;
        };
        let Some(required_host) = remote.rtorrent_ssh_hosts.get(endpoint) else {
            continue;
        };
        if !probes.iter().any(|(host, probe_id)| {
            *host == required_host.as_str() && action_transitively_depends_on(draft, node, probe_id)
        }) {
            bail!(
                "SSH-forwarded rTorrent endpoint `{endpoint}` requires an explicit dependent SSH identity probe"
            )
        }
    }
    for node in &draft.actions {
        let profile_name = match &node.action {
            WorkflowActionV1::CompareInventory { profile }
            | WorkflowActionV1::PlanSync { profile, .. }
            | WorkflowActionV1::ApplySync { profile, .. }
            | WorkflowActionV1::VerifySha256 { profile, .. } => profile,
            _ => continue,
        };
        let profile = &remote.syncs[profile_name];
        if profile.controller != "local"
            && !probes.iter().any(|(host, probe_id)| {
                *host == profile.controller.as_str()
                    && action_transitively_depends_on(draft, node, probe_id)
            })
        {
            bail!(
                "remote sync profile `{profile_name}` requires an explicit dependent SSH identity probe"
            )
        }
    }
    for node in &draft.actions {
        if let WorkflowActionV1::ControlTorrents {
            endpoint,
            selection_action,
            ..
        } = &node.action
        {
            let selected_endpoint = torrent_selection_endpoint(draft, selection_action)
                .context("torrent control could not resolve its selection endpoint")?;
            if selected_endpoint != endpoint {
                bail!("torrent control endpoint does not match its selection source")
            }
        }
        if let WorkflowActionV1::PlanSync {
            profile,
            selection_action: Some(selection_action),
        } = &node.action
        {
            let endpoint = torrent_selection_endpoint(draft, selection_action)
                .context("selection-aware sync could not resolve its torrent endpoint")?;
            if remote.syncs[profile].torrent_endpoint.as_deref() != Some(endpoint) {
                bail!("selection-aware sync profile is not bound to the selected torrent endpoint")
            }
        }
        if let WorkflowActionV1::ControlTorrents {
            endpoint,
            selection_action,
            control: ca_automation_protocol::TorrentControlActionV1::AdoptLocation { location },
            ..
        } = &node.action
        {
            if remote.torrent_locations.get(location) != Some(endpoint) {
                bail!("torrent location is not bound to the controlled endpoint")
            }
            let transfer = draft.actions.iter().find(|candidate| {
                let WorkflowActionV1::ApplySync {
                    profile,
                    plan_action,
                } = &candidate.action
                else {
                    return false;
                };
                let exact_selection = draft.actions.iter().any(|plan| {
                    plan.id == *plan_action
                        && matches!(&plan.action, WorkflowActionV1::PlanSync {
                            selection_action: Some(candidate_selection), ..
                        } if candidate_selection == selection_action)
                });
                remote.syncs[profile].torrent_endpoint.as_deref() == Some(endpoint.as_str())
                    && exact_selection
                    && action_transitively_depends_on(draft, node, &candidate.id)
            });
            let verification = draft.actions.iter().find(|candidate| {
                matches!(&candidate.action, WorkflowActionV1::VerifySha256 { profile, .. }
                    if remote.syncs[profile].torrent_endpoint.as_deref() == Some(endpoint.as_str()))
                    && action_transitively_depends_on(draft, node, &candidate.id)
            });
            if transfer.is_none() || verification.is_none() {
                bail!("torrent location adoption must depend on its approved transfer and SHA-256 verification")
            }
        }
    }
    Ok(())
}

fn torrent_selection_endpoint<'a>(
    draft: &'a WorkflowDraftV1,
    selection_action: &str,
) -> Option<&'a str> {
    let selection = draft
        .actions
        .iter()
        .find(|node| node.id == selection_action)?;
    let WorkflowActionV1::SelectTorrents { source_action, .. } = &selection.action else {
        return None;
    };
    let source = draft
        .actions
        .iter()
        .find(|node| node.id == *source_action)?;
    match &source.action {
        WorkflowActionV1::QueryTransmission { endpoint, .. }
        | WorkflowActionV1::QueryTorrents { endpoint } => Some(endpoint),
        _ => None,
    }
}

fn action_transitively_depends_on(
    draft: &WorkflowDraftV1,
    node: &ca_automation_protocol::WorkflowNodeV1,
    required: &str,
) -> bool {
    node.depends_on.iter().any(|dependency| {
        dependency == required
            || draft
                .actions
                .iter()
                .find(|candidate| candidate.id == *dependency)
                .is_some_and(|candidate| action_transitively_depends_on(draft, candidate, required))
    })
}

fn enforce_limits(
    limits: &WorkflowLimitsV1,
    predicted: (u32, u64),
    mutation_present: bool,
    draft: &WorkflowDraftV1,
    remote: &crate::remote::WorkflowRemoteSummary,
    reasons: &mut Vec<String>,
    status: &mut WorkflowPlanStatusV1,
) {
    let (items, bytes) = predicted;
    let exceeds_items = limits.max_items.is_some_and(|maximum| items > maximum);
    let exceeds_bytes = limits
        .max_transfer_bytes
        .is_some_and(|maximum| bytes > maximum);
    let referenced = referenced_sync_profiles(draft);
    let delete_conflict = !limits.deletion_allowed
        && remote
            .syncs
            .iter()
            .any(|(name, profile)| referenced.contains(name) && profile.delete);
    let missing_free_space_gate = mutation_present
        && limits.min_free_space_bytes > 0
        && !draft.actions.iter().any(|node| {
            matches!(
                &node.action,
                WorkflowActionV1::CheckFreeSpace { minimum_bytes, .. }
                    if *minimum_bytes >= limits.min_free_space_bytes
            )
        });
    let torrent_backed_mutation = draft.actions.iter().any(|node| {
        matches!(node.action, WorkflowActionV1::ControlTorrents { .. })
            || matches!(
                node.action,
                WorkflowActionV1::PlanSync {
                    selection_action: Some(_),
                    ..
                }
            )
    });
    let missing_torrent_file_limit = torrent_backed_mutation && limits.max_files.is_none();
    if exceeds_items
        || exceeds_bytes
        || delete_conflict
        || missing_free_space_gate
        || missing_torrent_file_limit
        || (mutation_present && !limits.authorizes_mutation())
    {
        *status = WorkflowPlanStatusV1::Blocked;
    }
    if exceeds_items {
        reasons.push("item_limit_exceeded".into());
    }
    if exceeds_bytes {
        reasons.push("transfer_limit_exceeded".into());
    }
    if delete_conflict {
        reasons.push("configured_deletion_denied".into());
    }
    if missing_free_space_gate {
        reasons.push("free_space_precondition_missing".into());
    }
    if missing_torrent_file_limit {
        reasons.push("torrent_file_limit_missing".into());
    }
    if mutation_present && !limits.authorizes_mutation() {
        reasons.push("mutation_limits_incomplete".into());
    }
}

fn classify_correction_against_draft(
    draft: &WorkflowDraftV1,
    correction: &ca_automation_protocol::WorkflowCorrectionV1,
) -> WorkflowCorrectionClassificationV1 {
    let base = classify_workflow_correction(correction);
    if base != WorkflowCorrectionClassificationV1::Tightening {
        return base;
    }
    for change in &correction.changes {
        match change {
            WorkflowCorrectionChangeV1::NarrowTorrentLabel { label } => {
                if draft.actions.iter().any(|node| {
                    matches!(&node.action, WorkflowActionV1::SelectTorrents { label: Some(existing), .. } if existing != label)
                }) {
                    return WorkflowCorrectionClassificationV1::Conflicting;
                }
            }
            WorkflowCorrectionChangeV1::LowerMaxItems { maximum }
                if draft.limits.max_items.is_some_and(|current| *maximum > current) =>
            {
                return WorkflowCorrectionClassificationV1::Conflicting;
            }
            WorkflowCorrectionChangeV1::LowerMaxTransferBytes { maximum }
                if draft
                    .limits
                    .max_transfer_bytes
                    .is_some_and(|current| *maximum > current) =>
            {
                return WorkflowCorrectionClassificationV1::Conflicting;
            }
            WorkflowCorrectionChangeV1::LowerMaxFiles { maximum }
                if draft.limits.max_files.is_some_and(|current| *maximum > current) =>
            {
                return WorkflowCorrectionClassificationV1::Conflicting;
            }
            _ => {}
        }
    }
    base
}

fn apply_correction(
    draft: &mut WorkflowDraftV1,
    correction: &ca_automation_protocol::WorkflowCorrectionV1,
    classification: WorkflowCorrectionClassificationV1,
) -> Result<()> {
    if classification == WorkflowCorrectionClassificationV1::Conflicting {
        return Ok(());
    }
    for change in &correction.changes {
        match change {
            WorkflowCorrectionChangeV1::DenyDeletion => draft.limits.deletion_allowed = false,
            WorkflowCorrectionChangeV1::PermitDeletion => draft.limits.deletion_allowed = true,
            WorkflowCorrectionChangeV1::NarrowTorrentLabel { label } => {
                for node in &mut draft.actions {
                    if let WorkflowActionV1::SelectTorrents { label: current, .. } =
                        &mut node.action
                    {
                        *current = Some(label.clone());
                    }
                }
            }
            WorkflowCorrectionChangeV1::LowerMaxItems { maximum }
            | WorkflowCorrectionChangeV1::RaiseMaxItems { maximum } => {
                draft.limits.max_items = Some(*maximum)
            }
            WorkflowCorrectionChangeV1::LowerMaxTransferBytes { maximum }
            | WorkflowCorrectionChangeV1::RaiseMaxTransferBytes { maximum } => {
                draft.limits.max_transfer_bytes = Some(*maximum)
            }
            WorkflowCorrectionChangeV1::LowerMaxFiles { maximum }
            | WorkflowCorrectionChangeV1::RaiseMaxFiles { maximum } => {
                draft.limits.max_files = Some(*maximum)
            }
            WorkflowCorrectionChangeV1::RequireVerification { verification } => {
                if !draft.limits.verification.contains(verification) {
                    draft.limits.verification.push(*verification);
                }
            }
            WorkflowCorrectionChangeV1::RemoveVerification { verification } => draft
                .limits
                .verification
                .retain(|item| item != verification),
            WorkflowCorrectionChangeV1::IncreaseMinFreeSpace { minimum_bytes } => {
                draft.limits.min_free_space_bytes =
                    draft.limits.min_free_space_bytes.max(*minimum_bytes)
            }
            WorkflowCorrectionChangeV1::AddPrecondition { action_id } => {
                if !draft.actions.iter().any(|node| &node.id == action_id) {
                    bail!("correction precondition action does not exist")
                }
                for node in &mut draft.actions {
                    if node.action.is_mutating()
                        && &node.id != action_id
                        && !node.depends_on.contains(action_id)
                    {
                        node.depends_on.push(action_id.clone());
                        node.depends_on.sort();
                    }
                }
            }
            WorkflowCorrectionChangeV1::NarrowDestinationPrefix { .. }
            | WorkflowCorrectionChangeV1::ChangeDestination { .. } => {
                // Destination constraints apply against private configured resources;
                // model wire data never rewrites those resources directly.
            }
        }
    }
    Ok(())
}

fn topological_action_batches(
    draft: &WorkflowDraftV1,
) -> Result<Vec<Vec<&ca_automation_protocol::WorkflowNodeV1>>> {
    let mut remaining = draft.actions.iter().collect::<Vec<_>>();
    let mut completed = BTreeSet::new();
    let mut batches = Vec::new();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                node.depends_on
                    .iter()
                    .all(|dependency| completed.contains(dependency))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let Some(first) = ready.first().copied() else {
            bail!("workflow dependency graph has no ready action")
        };
        let selected = if remaining[first].action.is_mutating() {
            vec![first]
        } else {
            ready
                .into_iter()
                .take_while(|index| !remaining[*index].action.is_mutating())
                .take(4)
                .collect::<Vec<_>>()
        };
        let mut batch = Vec::with_capacity(selected.len());
        for index in selected.into_iter().rev() {
            let node = remaining.remove(index);
            completed.insert(node.id.clone());
            batch.push(node);
        }
        batch.reverse();
        batches.push(batch);
    }
    Ok(batches)
}

async fn execute_action_with_retries(
    config_path: &Path,
    node: &ca_automation_protocol::WorkflowNodeV1,
    conflict_policy: ca_automation_protocol::WorkflowConflictPolicyV1,
    draft: &WorkflowDraftV1,
    prior: &BTreeMap<String, ActionRuntime>,
) -> (Result<ActionRuntime>, u8, u8, u64) {
    let started = Instant::now();
    let journal = crate::runtime::current_run_store();
    let action_id = format!("workflow_{}", node.id);
    if let Some(store) = &journal {
        let prepared = store
            .prepare_action(&action_id)
            .and_then(|()| store.begin_action(&action_id));
        match prepared {
            Ok(false) => {
                let replay = store.status().and_then(|receipt| {
                    let saved = receipt
                        .actions
                        .into_iter()
                        .find(|action| action.id == action_id)
                        .and_then(|action| action.result)
                        .context("confirmed workflow action has no receipt")?;
                    if let Some(error) = saved.get("error").and_then(Value::as_str) {
                        bail!("{error}");
                    }
                    Ok(serde_json::from_value::<ActionRuntime>(
                        saved["runtime"].clone(),
                    )?)
                });
                return (replay, 0, 0, 0);
            }
            Ok(true) => {}
            Err(error) => return (Err(error), 0, 0, 0),
        }
    }
    let mut attempts = 0_u8;
    let mut repairs = 0_u8;
    let retry_permitted = !node.action.is_mutating()
        && !matches!(
            node.action,
            WorkflowActionV1::ControlTorrents {
                control: ca_automation_protocol::TorrentControlActionV1::Verify
                    | ca_automation_protocol::TorrentControlActionV1::Reannounce,
                ..
            }
        );
    let outcome = loop {
        attempts = attempts.saturating_add(1);
        match execute_action(config_path, node, conflict_policy, draft, prior).await {
            Ok(runtime) => break Ok(runtime),
            Err(error)
                if retry_permitted && attempts <= node.max_retries && transient_error(&error) =>
            {
                repairs = repairs.saturating_add(1);
                tokio::time::sleep(Duration::from_millis(50 * u64::from(attempts))).await;
            }
            Err(error) => break Err(error),
        }
    };
    if let Some(store) = &journal {
        let saved = match &outcome {
            Ok(runtime) => {
                Some(serde_json::json!({"runtime":runtime,"attempts":attempts,"repairs":repairs}))
            }
            Err(error) if !node.action.is_mutating() => Some(
                serde_json::json!({"error":format!("{error:#}"),"attempts":attempts,"repairs":repairs}),
            ),
            Err(_) => None,
        };
        if let Some(saved) = saved {
            if let Err(error) = store.confirm_action(&action_id, saved) {
                return (
                    Err(error),
                    attempts,
                    repairs,
                    started.elapsed().as_millis() as u64,
                );
            }
        }
    }
    (
        outcome,
        attempts,
        repairs,
        started.elapsed().as_millis() as u64,
    )
}

fn transient_error(error: &anyhow::Error) -> bool {
    let text = error.to_string().to_ascii_lowercase();
    text.contains("timed out")
        || text.contains("transport failed")
        || text.contains("connection")
        || text.contains("interrupted")
}

fn classify_failure(error: &anyhow::Error) -> WorkflowRepairCauseV1 {
    let text = error.to_string().to_ascii_lowercase();
    if text.contains("post-execution sync plan is not empty")
        || text.contains("interrupted")
        || text.contains("partial transfer")
    {
        WorkflowRepairCauseV1::InterruptedTransfer
    } else if text.contains("space") {
        WorkflowRepairCauseV1::InsufficientSpace
    } else if text.contains("credential") || text.contains("401") || text.contains("403") {
        WorkflowRepairCauseV1::Authentication
    } else if text.contains("identity") {
        WorkflowRepairCauseV1::IdentityChanged
    } else if text.contains("checksum") || text.contains("sha-256") {
        WorkflowRepairCauseV1::ChecksumMismatch
    } else if transient_error(error) {
        WorkflowRepairCauseV1::TransientTransport
    } else if text.contains("policy") || text.contains("limit") {
        WorkflowRepairCauseV1::PolicyViolation
    } else if text.contains("conflict") {
        WorkflowRepairCauseV1::Conflict
    } else {
        WorkflowRepairCauseV1::Indeterminate
    }
}

fn blocked_execution(plan: &WorkflowPlanReceiptV1, reason: &str) -> WorkflowExecutionReceiptV1 {
    let now = Utc::now().to_rfc3339();
    WorkflowExecutionReceiptV1 {
        schema: WORKFLOW_EXECUTION_RECEIPT_SCHEMA.into(),
        workflow_id: plan.workflow_id.clone(),
        workflow_digest: plan.workflow_digest.clone(),
        policy_digest: plan.policy_digest.clone(),
        resource_binding_digest: plan.resource_binding_digest.clone(),
        run_plan_digest: plan.run_plan_digest.clone(),
        status: WorkflowExecutionStatusV1::Blocked,
        action_results: Vec::new(),
        deterministic_repairs: 0,
        verification_passed: false,
        rollback: Some(plan.rollback.clone()),
        deviations: Vec::new(),
        reason_codes: vec![reason.into()],
        started_at: now.clone(),
        finished_at: now,
        elapsed_millis: 0,
    }
}

fn stale_execution(plan: &WorkflowPlanReceiptV1, reason: &str) -> WorkflowExecutionReceiptV1 {
    let mut receipt = blocked_execution(plan, reason);
    receipt.status = WorkflowExecutionStatusV1::Stale;
    receipt
}

fn failed_execution(plan: &WorkflowPlanReceiptV1, reason: &str) -> WorkflowExecutionReceiptV1 {
    let mut receipt = blocked_execution(plan, reason);
    receipt.status = WorkflowExecutionStatusV1::Failed;
    receipt
}

#[allow(clippy::too_many_arguments)]
fn finish_execution(
    preview: &PreviewState,
    status: WorkflowExecutionStatusV1,
    action_results: Vec<WorkflowActionExecutionV1>,
    deterministic_repairs: u8,
    verification_passed: bool,
    reason_codes: Vec<String>,
    started_at: chrono::DateTime<Utc>,
    started: Instant,
) -> Result<WorkflowExecutionReceiptV1> {
    let mut receipt = WorkflowExecutionReceiptV1 {
        schema: WORKFLOW_EXECUTION_RECEIPT_SCHEMA.into(),
        workflow_id: preview.receipt.workflow_id.clone(),
        workflow_digest: preview.receipt.workflow_digest.clone(),
        policy_digest: preview.receipt.policy_digest.clone(),
        resource_binding_digest: preview.receipt.resource_binding_digest.clone(),
        run_plan_digest: preview.receipt.run_plan_digest.clone(),
        status,
        action_results,
        deterministic_repairs,
        verification_passed,
        rollback: Some(preview.draft.rollback.clone()),
        deviations: if deterministic_repairs == 0 {
            Vec::new()
        } else {
            vec![format!(
                "{deterministic_repairs} bounded deterministic repair attempt(s) were applied"
            )]
        },
        reason_codes,
        started_at: started_at.to_rfc3339(),
        finished_at: Utc::now().to_rfc3339(),
        elapsed_millis: started.elapsed().as_millis() as u64,
    };
    bound_execution_receipt(&mut receipt)?;
    Ok(receipt)
}

fn bound_execution_receipt(receipt: &mut WorkflowExecutionReceiptV1) -> Result<()> {
    while serde_json::to_vec(&*receipt)?.len() > MAX_MECHANISTIC_OUTPUT_BYTES {
        let mut reduced = false;
        for action in receipt.action_results.iter_mut().rev() {
            let Some(mutation) = action.torrent_mutation.as_mut() else {
                continue;
            };
            if !mutation.changed_hashes.is_empty() {
                mutation.changed_hashes.clear();
                mutation.truncated = true;
                reduced = true;
                break;
            }
            if !mutation.target_hashes.is_empty() {
                mutation.target_hashes.clear();
                mutation.truncated = true;
                reduced = true;
                break;
            }
        }
        if !reduced {
            if receipt.deviations.pop().is_some() || receipt.reason_codes.pop().is_some() {
                continue;
            }
            let Some(action) = receipt.action_results.pop() else {
                bail!("workflow execution receipt exceeds its public output bound")
            };
            receipt.deviations.push(format!(
                "execution detail for `{}` was truncated; authoritative run digest {} remains",
                action.action_id, receipt.run_plan_digest
            ));
        }
    }
    Ok(())
}

fn bound_plan_receipt(receipt: &mut WorkflowPlanReceiptV1) -> Result<()> {
    if serde_json::to_vec(&*receipt)?.len() > MAX_MECHANISTIC_OUTPUT_BYTES
        && receipt
            .action_plans
            .iter()
            .any(|action| action.torrent_control.is_some())
    {
        receipt.status = WorkflowPlanStatusV1::Blocked;
        receipt.approval_required = false;
        if !receipt
            .reason_codes
            .iter()
            .any(|reason| reason == "torrent_effects_exceed_public_preview_bound")
        {
            receipt
                .reason_codes
                .push("torrent_effects_exceed_public_preview_bound".into());
        }
        for action in &mut receipt.action_plans {
            action.target_hashes.clear();
        }
        receipt.truncated = true;
    }
    while serde_json::to_vec(receipt)?.len() > MAX_MECHANISTIC_OUTPUT_BYTES {
        let Some(action) = receipt.action_plans.last_mut() else {
            bail!("workflow plan receipt exceeds its public output bound")
        };
        if action.uncertainties.pop().is_none() && receipt.action_plans.pop().is_none() {
            bail!("workflow plan receipt exceeds its public output bound")
        }
        receipt.truncated = true;
    }
    Ok(())
}

struct WorkflowStore {
    root: PathBuf,
    policy_dir: PathBuf,
    workflow_dir: PathBuf,
    preview_dir: PathBuf,
    feedback_dir: PathBuf,
    checkpoint_dir: PathBuf,
}

impl WorkflowStore {
    fn open(root: &Path) -> Result<Self> {
        if !root.is_absolute() {
            bail!("workflow state root must be absolute")
        }
        let store = Self {
            root: root.to_path_buf(),
            policy_dir: root.join("workflow-policies/v1"),
            workflow_dir: root.join("workflows/v1"),
            preview_dir: root.join("workflow-previews/v1"),
            feedback_dir: root.join("workflow-feedback/v1"),
            checkpoint_dir: root.join("workflow-checkpoints/v1"),
        };
        store.ensure_private_dir(&store.root)?;
        store.ensure_private_dir(&store.policy_dir)?;
        store.ensure_private_dir(&store.workflow_dir)?;
        store.ensure_private_dir(&store.preview_dir)?;
        store.ensure_private_dir(&store.feedback_dir)?;
        store.ensure_private_dir(&store.checkpoint_dir)?;
        Ok(store)
    }

    fn ensure_private_dir(&self, path: &Path) -> Result<()> {
        let existed = match fs::symlink_metadata(path) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            bail!("workflow state directory is unsafe")
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if metadata.uid() != unsafe { libc::geteuid() } {
                bail!("workflow state directory has the wrong owner")
            }
            if existed && metadata.mode() & 0o077 != 0 {
                bail!("workflow state directory permissions are insecure")
            }
            if !existed {
                fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            }
        }
        Ok(())
    }

    fn list(&self, workflow_id: Option<&str>) -> Result<Vec<WorkflowSummaryV1>> {
        let mut paths = fs::read_dir(&self.workflow_dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        if paths.len() > 1_000 {
            bail!("workflow store exceeds its entry bound")
        }
        let mut summaries = Vec::new();
        for path in paths {
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let approved: ApprovedWorkflow = read_private_json(&path)?;
            if approved.schema != APPROVED_SCHEMA {
                bail!("unsupported approved workflow record")
            }
            if workflow_id.is_none_or(|expected| approved.workflow_id == expected) {
                let checkpoint = self.read_checkpoint(&approved.workflow_id)?;
                summaries.push(WorkflowSummaryV1 {
                    workflow_id: approved.workflow_id,
                    workflow_digest: approved.workflow_digest,
                    policy_digest: approved.policy_digest,
                    resource_binding_digest: approved.resource_binding_digest,
                    enabled: approved.enabled,
                    checkpoint_phase: checkpoint.as_ref().map(|checkpoint| {
                        match checkpoint.phase {
                            WorkflowCheckpointPhase::Running => "running".into(),
                            WorkflowCheckpointPhase::Terminal => "terminal".into(),
                        }
                    }),
                    checkpoint_status: checkpoint.and_then(|checkpoint| checkpoint.status),
                });
            }
        }
        Ok(summaries)
    }

    fn read_approved(&self, workflow_id: &str) -> Result<Option<ApprovedWorkflow>> {
        let path = self.workflow_path(workflow_id)?;
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                let record: ApprovedWorkflow = read_private_json(&path)?;
                if record.schema != APPROVED_SCHEMA || record.workflow_id != workflow_id {
                    bail!("approved workflow record is invalid")
                }
                Ok(Some(record))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn write_approved(&self, approved: &ApprovedWorkflow) -> Result<()> {
        write_private_json(&self.workflow_path(&approved.workflow_id)?, approved)
    }

    fn record_preview(&self, receipt: &WorkflowPlanReceiptV1) -> Result<()> {
        let grant = PreviewGrant {
            schema: PREVIEW_GRANT_SCHEMA.into(),
            run_plan_digest: receipt.run_plan_digest.clone(),
            workflow_id: receipt.workflow_id.clone(),
            workflow_digest: receipt.workflow_digest.clone(),
            policy_digest: receipt.policy_digest.clone(),
            resource_binding_digest: receipt.resource_binding_digest.clone(),
            correction_round: receipt.correction_round,
            created_at: Utc::now().to_rfc3339(),
        };
        write_private_json(
            &self
                .preview_dir
                .join(format!("{}.json", receipt.run_plan_digest)),
            &grant,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_preview_source(
        &self,
        digest: &str,
        workflow_id: &str,
        workflow_digest: &str,
        policy_digest: &str,
        resource_binding_digest: &str,
        correction_round: u8,
    ) -> Result<()> {
        let grant: PreviewGrant =
            read_private_json(&self.preview_dir.join(format!("{digest}.json")))
                .context("workflow correction source preview is missing or stale")?;
        let created_at = chrono::DateTime::parse_from_rfc3339(&grant.created_at)
            .context("workflow preview grant timestamp is malformed")?
            .with_timezone(&Utc);
        if grant.schema != PREVIEW_GRANT_SCHEMA
            || grant.run_plan_digest != digest
            || grant.workflow_id != workflow_id
            || grant.workflow_digest != workflow_digest
            || grant.policy_digest != policy_digest
            || grant.resource_binding_digest != resource_binding_digest
            || grant.correction_round != correction_round
            || Utc::now().signed_duration_since(created_at) > chrono::Duration::hours(24)
        {
            bail!("workflow correction source plan is stale")
        }
        Ok(())
    }

    fn disable(&self, workflow_id: &str, accepted_digest: &str) -> Result<String> {
        let mut approved = self
            .read_approved(workflow_id)?
            .context("workflow has not been approved")?;
        if approved.workflow_digest != accepted_digest {
            bail!("approved workflow digest is stale")
        }
        approved.enabled = false;
        self.write_approved(&approved)?;
        Ok(approved.workflow_digest)
    }

    fn record_feedback(
        &self,
        workflow_digest: &str,
        receipt_digest: &str,
        category: ca_automation_protocol::WorkflowFeedbackCategoryV1,
    ) -> Result<bool> {
        let negative = category != ca_automation_protocol::WorkflowFeedbackCategoryV1::Accepted;
        let mut disabled = false;
        if negative {
            let mut paths = fs::read_dir(&self.workflow_dir)?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<std::io::Result<Vec<_>>>()?;
            paths.sort();
            for path in paths.into_iter().take(1_000) {
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let mut approved: ApprovedWorkflow = read_private_json(&path)?;
                if approved.workflow_digest == workflow_digest && approved.enabled {
                    approved.enabled = false;
                    self.write_approved(&approved)?;
                    disabled = true;
                }
            }
        }
        let record = WorkflowFeedbackRecord {
            schema: FEEDBACK_SCHEMA.into(),
            workflow_digest: workflow_digest.into(),
            receipt_digest: receipt_digest.into(),
            category,
            disabled,
            created_at: Utc::now().to_rfc3339(),
        };
        write_private_json(
            &self.feedback_dir.join(format!(
                "{}-{}.json",
                &receipt_digest[..16],
                Utc::now().timestamp_micros()
            )),
            &record,
        )?;
        Ok(disabled)
    }

    fn read_checkpoint(&self, workflow_id: &str) -> Result<Option<WorkflowCheckpoint>> {
        let path = self.checkpoint_dir.join(format!("{workflow_id}.json"));
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                let checkpoint: WorkflowCheckpoint = read_private_json(&path)?;
                if checkpoint.schema != CHECKPOINT_SCHEMA || checkpoint.workflow_id != workflow_id {
                    bail!("workflow checkpoint is malformed")
                }
                Ok(Some(checkpoint))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn write_checkpoint(
        &self,
        preview: &PreviewState,
        phase: WorkflowCheckpointPhase,
        status: Option<WorkflowExecutionStatusV1>,
    ) -> Result<()> {
        let plan = &preview.receipt;
        let (effect_hashes, effect_hashes_truncated) = preview_effect_hashes(preview);
        write_private_json(
            &self
                .checkpoint_dir
                .join(format!("{}.json", plan.workflow_id)),
            &WorkflowCheckpoint {
                schema: CHECKPOINT_SCHEMA.into(),
                workflow_id: plan.workflow_id.clone(),
                run_id: crate::runtime::current_run_store()
                    .map(|store| store.status().map(|receipt| receipt.run_id))
                    .transpose()?,
                workflow_digest: plan.workflow_digest.clone(),
                policy_digest: plan.policy_digest.clone(),
                resource_binding_digest: plan.resource_binding_digest.clone(),
                run_plan_digest: plan.run_plan_digest.clone(),
                phase,
                status,
                predicted_items: plan.predicted_items,
                predicted_bytes: plan.predicted_bytes,
                effect_hashes,
                effect_hashes_truncated,
                updated_at: Utc::now().to_rfc3339(),
            },
        )
    }

    fn workflow_path(&self, workflow_id: &str) -> Result<PathBuf> {
        if workflow_id.is_empty()
            || !workflow_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            bail!("workflow ID is unsafe")
        }
        Ok(self.workflow_dir.join(format!("{workflow_id}.json")))
    }

    fn write_policy(&self, record: &PolicyRecord) -> Result<()> {
        write_private_json(
            &self
                .policy_dir
                .join(format!("{}.json", record.correction_digest)),
            record,
        )
    }

    fn policies(&self) -> Result<Vec<PolicyRecord>> {
        let mut paths = fs::read_dir(&self.policy_dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        if paths.len() > 1_000 {
            bail!("workflow policy store exceeds its entry bound")
        }
        let mut records = Vec::new();
        for path in paths {
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let record: PolicyRecord = read_private_json(&path)?;
            if record.schema != POLICY_SCHEMA {
                bail!("unsupported workflow policy record")
            }
            records.push(record);
        }
        records.sort_by_key(|record| (record.source, record.created_at.clone()));
        Ok(records)
    }

    fn applicable_policies(
        &self,
        draft: &WorkflowDraftV1,
        remote: &crate::remote::WorkflowRemoteSummary,
    ) -> Result<Vec<PolicyRecord>> {
        let workflow_identity_digest = digest_json(&draft.workflow_id)?;
        Ok(self
            .policies()?
            .into_iter()
            .filter(|record| match &record.scope {
                WorkflowCorrectionScopeV1::Workflow { workflow_id } => {
                    workflow_id == &workflow_identity_digest
                }
                WorkflowCorrectionScopeV1::Resource {
                    resource_binding_digest,
                } => resource_binding_digest == &remote.binding_digest,
                WorkflowCorrectionScopeV1::Adapter { adapter } => {
                    workflow_uses_adapter(draft, *adapter)
                }
            })
            .collect())
    }

    fn apply_policies(
        &self,
        draft: &mut WorkflowDraftV1,
        remote: &crate::remote::WorkflowRemoteSummary,
    ) -> Result<(String, Vec<String>)> {
        let policies = self.applicable_policies(draft, remote)?;
        let mut digests = Vec::new();
        let mut reasons = Vec::new();
        for policy in policies {
            let correction = ca_automation_protocol::WorkflowCorrectionV1 {
                schema: ca_automation_protocol::WORKFLOW_CORRECTION_SCHEMA.into(),
                source_plan_digest: "0".repeat(64),
                reason_code: ca_automation_protocol::WorkflowCorrectionReasonV1::HumanCorrection,
                scope: policy.scope,
                changes: policy.changes,
                evidence_refs: Vec::new(),
                correction_round: 0,
            };
            apply_correction(draft, &correction, policy.classification)?;
            digests.push(policy.correction_digest);
            reasons.push("machine_policy_applied".into());
        }
        digests.sort();
        Ok((digest_json(&digests)?, reasons))
    }

    fn policy_digest(
        &self,
        draft: &WorkflowDraftV1,
        remote: &crate::remote::WorkflowRemoteSummary,
    ) -> Result<String> {
        let mut digests = self
            .applicable_policies(draft, remote)?
            .into_iter()
            .map(|policy| policy.correction_digest)
            .collect::<Vec<_>>();
        digests.sort();
        digest_json(&digests)
    }

    fn policy_violations(
        &self,
        draft: &WorkflowDraftV1,
        remote: &crate::remote::WorkflowRemoteSummary,
    ) -> Result<Vec<String>> {
        let referenced = referenced_sync_profiles(draft);
        let mut violations = Vec::new();
        for policy in self.applicable_policies(draft, remote)? {
            for change in policy.changes {
                match change {
                    WorkflowCorrectionChangeV1::NarrowDestinationPrefix { prefix } => {
                        let prefix = Path::new(&prefix);
                        if remote.syncs.iter().any(|(name, profile)| {
                            referenced.contains(name)
                                && !sync_path(&profile.destination).starts_with(prefix)
                        }) {
                            violations.push("destination_prefix_policy_mismatch".into());
                        }
                    }
                    WorkflowCorrectionChangeV1::ChangeDestination { destination } => {
                        if remote.syncs.iter().any(|(name, profile)| {
                            referenced.contains(name) && profile.destination != destination
                        }) {
                            violations.push("approved_destination_policy_mismatch".into());
                        }
                    }
                    WorkflowCorrectionChangeV1::AddPrecondition { action_id }
                        if !draft.actions.iter().any(|node| node.id == action_id) =>
                    {
                        violations.push("required_precondition_missing".into());
                    }
                    _ => {}
                }
            }
        }
        violations.sort();
        violations.dedup();
        Ok(violations)
    }
}

fn stored_policy_scope(scope: &WorkflowCorrectionScopeV1) -> Result<WorkflowCorrectionScopeV1> {
    Ok(match scope {
        WorkflowCorrectionScopeV1::Workflow { workflow_id } => {
            WorkflowCorrectionScopeV1::Workflow {
                workflow_id: digest_json(workflow_id)?,
            }
        }
        other => other.clone(),
    })
}

fn referenced_sync_profiles(draft: &WorkflowDraftV1) -> BTreeSet<String> {
    draft
        .actions
        .iter()
        .filter_map(|node| match &node.action {
            WorkflowActionV1::CompareInventory { profile }
            | WorkflowActionV1::PlanSync { profile, .. }
            | WorkflowActionV1::ApplySync { profile, .. }
            | WorkflowActionV1::VerifySha256 { profile, .. } => Some(profile.clone()),
            _ => None,
        })
        .collect()
}

fn sync_path(value: &str) -> &Path {
    Path::new(value.rsplit_once(':').map_or(value, |(_, path)| path))
}

fn workflow_uses_adapter(draft: &WorkflowDraftV1, adapter: WorkflowAdapterV1) -> bool {
    draft.actions.iter().any(|node| match adapter {
        WorkflowAdapterV1::Ssh => matches!(node.action, WorkflowActionV1::ProbeSsh { .. }),
        WorkflowAdapterV1::Transmission => matches!(
            node.action,
            WorkflowActionV1::QueryTransmission { .. }
                | WorkflowActionV1::SelectTorrents { .. }
                | WorkflowActionV1::CheckFreeSpace { .. }
        ),
        WorkflowAdapterV1::Rtorrent => matches!(
            node.action,
            WorkflowActionV1::QueryTorrents { .. }
                | WorkflowActionV1::SelectTorrents { .. }
                | WorkflowActionV1::ControlTorrents { .. }
        ),
        WorkflowAdapterV1::Rsync => matches!(
            node.action,
            WorkflowActionV1::CompareInventory { .. }
                | WorkflowActionV1::PlanSync { .. }
                | WorkflowActionV1::ApplySync { .. }
                | WorkflowActionV1::VerifySha256 { .. }
        ),
    })
}

fn read_private_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_STATE_BYTES
    {
        bail!("workflow state must be one bounded regular non-symlink file")
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.nlink() != 1
            || metadata.mode() & 0o077 != 0
        {
            bail!("workflow state must be private with one hard link")
        }
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_STATE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        bail!("workflow state exceeds its size bound")
    }
    serde_json::from_slice(&bytes).context("parse workflow state")
}

fn write_private_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        bail!("workflow state exceeds its size bound")
    }
    let parent = path.parent().context("workflow state has no parent")?;
    let metadata = fs::symlink_metadata(parent)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!("workflow state parent is unsafe")
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            bail!("workflow state parent permissions or owner are unsafe")
        }
    }
    let existing = match fs::symlink_metadata(path) {
        Ok(existing) => Some(existing),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(existing) = existing {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if !existing.is_file()
                || existing.file_type().is_symlink()
                || existing.uid() != unsafe { libc::geteuid() }
                || existing.nlink() != 1
                || existing.mode() & 0o077 != 0
            {
                bail!("existing workflow state target is unsafe")
            }
        }
        #[cfg(not(unix))]
        if !existing.is_file() || existing.file_type().is_symlink() {
            bail!("existing workflow state target is unsafe")
        }
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = parent.join(format!(".workflow-{}-{nonce}.tmp", std::process::id()));
    let write = (|| -> Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if write.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write
}

fn digest_json(value: &impl Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

fn preview_binding_fingerprints(
    runtimes: &BTreeMap<String, ActionRuntime>,
) -> Result<(String, String)> {
    let identities = runtimes
        .iter()
        .filter_map(|(action_id, runtime)| {
            let value = runtime.value.as_ref()?;
            value
                .get("identity_digest")
                .or_else(|| value.get("identity_fingerprint"))
                .or_else(|| {
                    value
                        .get("identity")
                        .and_then(|identity| identity.get("fingerprint"))
                })
                .and_then(Value::as_str)
                .map(|digest| (action_id, digest))
        })
        .collect::<Vec<_>>();
    let probes = runtimes
        .iter()
        .map(|(action_id, runtime)| {
            Ok(serde_json::json!({
                "action_id": action_id,
                "items": runtime.items,
                "bytes": runtime.bytes,
                "value_digest": runtime
                    .value
                    .as_ref()
                    .map(crate::json_identity::digest_value)
                    .transpose()?,
                "plan_digest": runtime.plan_digest.as_deref(),
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((
        digest_json(&identities)?,
        crate::json_identity::digest_value(&serde_json::Value::Array(probes))?,
    ))
}

fn predicted_effect_totals(
    draft: &WorkflowDraftV1,
    runtimes: &BTreeMap<String, ActionRuntime>,
) -> (u32, u64) {
    let mut items = 0_u32;
    let mut bytes = 0_u64;
    let mut mutations = 0_u8;
    for node in &draft.actions {
        if !node.action.is_mutating() {
            continue;
        }
        mutations = mutations.saturating_add(1);
        if let Some(runtime) = runtimes.get(&node.id) {
            items = items.saturating_add(runtime.items);
            bytes = bytes.saturating_add(runtime.bytes);
        }
    }
    if mutations == 0 {
        (
            runtimes.values().map(|item| item.items).max().unwrap_or(0),
            runtimes.values().map(|item| item.bytes).max().unwrap_or(0),
        )
    } else {
        (items, bytes)
    }
}

fn preview_effect_hashes(preview: &PreviewState) -> (Vec<String>, bool) {
    let mut hashes = preview
        .runtimes
        .values()
        .filter_map(|runtime| runtime.value.as_ref().and_then(Value::as_array))
        .flatten()
        .filter_map(|entry| crate::json_identity::digest_value(entry).ok())
        .collect::<Vec<_>>();
    hashes.sort();
    hashes.dedup();
    let truncated = hashes.len() > 2_048;
    hashes.truncate(2_048);
    (hashes, truncated)
}

fn bounded_text(value: &str, maximum: usize) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let mut chars = sanitized.trim().chars();
    let result = chars.by_ref().take(maximum).collect::<String>();
    if chars.next().is_some() {
        format!("{result}…")
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ca_automation_protocol::{
        WorkflowConflictPolicyV1, WorkflowCorrectionReasonV1, WorkflowNodeV1,
        WORKFLOW_CORRECTION_SCHEMA, WORKFLOW_DRAFT_SCHEMA, WORKFLOW_REQUEST_SCHEMA,
    };
    use tempfile::TempDir;

    #[tokio::test]
    async fn confirmed_native_action_replays_receipt_without_touching_adapter() -> Result<()> {
        use crate::runtime::{ExecutionContext, RunStore, EXECUTION_SCHEMA, EXECUTION_STORE};
        let root = TempDir::new()?;
        let root = root.path().canonicalize()?;
        let context = ExecutionContext {
            schema: EXECUTION_SCHEMA.into(),
            run_id: "workflow-replay".into(),
            profile: "fixture".into(),
            state_root: root.join("state"),
            lease_root: root.join("leases"),
            definition_digest: "a".repeat(64),
            resource_binding_digest: "b".repeat(64),
            policy_digest: "c".repeat(64),
            resources: vec![],
            runtime_seconds: 30,
            output_bytes: 1024 * 1024,
        };
        let store = RunStore::open(&context)?;
        store.start()?;
        store.prepare_action("workflow_saved")?;
        assert!(store.begin_action("workflow_saved")?);
        let cached = ActionRuntime {
            summary: "confirmed result".into(),
            items: 3,
            bytes: 42,
            value: Some(serde_json::json!(["cached"])),
            plan_digest: None,
            uncertainties: vec![],
        };
        store.confirm_action(
            "workflow_saved",
            serde_json::json!({"runtime":cached, "attempts":1,"repairs":0}),
        )?;
        let node = WorkflowNodeV1 {
            id: "saved".into(),
            depends_on: vec![],
            max_retries: 1,
            action: WorkflowActionV1::ProbeSsh {
                host: "adapter-must-not-run".into(),
            },
        };
        let (result, attempts, _, _) = EXECUTION_STORE
            .scope(
                store,
                execute_action_with_retries(
                    Path::new("/missing-fixture-config"),
                    &node,
                    WorkflowConflictPolicyV1::Block,
                    &draft(),
                    &BTreeMap::new(),
                ),
            )
            .await;
        assert_eq!(result?.summary, "confirmed result");
        assert_eq!(attempts, 0);
        Ok(())
    }

    #[tokio::test]
    async fn native_action_with_unknown_effect_is_not_repeated() -> Result<()> {
        use crate::runtime::{ExecutionContext, RunStore, EXECUTION_SCHEMA, EXECUTION_STORE};
        let root = TempDir::new()?;
        let root = root.path().canonicalize()?;
        let store = RunStore::open(&ExecutionContext {
            schema: EXECUTION_SCHEMA.into(),
            run_id: "workflow-unknown".into(),
            profile: "fixture".into(),
            state_root: root.join("state"),
            lease_root: root.join("leases"),
            definition_digest: "a".repeat(64),
            resource_binding_digest: "b".repeat(64),
            policy_digest: "c".repeat(64),
            resources: vec![],
            runtime_seconds: 30,
            output_bytes: 1024 * 1024,
        })?;
        store.start()?;
        store.prepare_action("workflow_saved")?;
        assert!(store.begin_action("workflow_saved")?);
        let node = WorkflowNodeV1 {
            id: "saved".into(),
            depends_on: vec![],
            max_retries: 1,
            action: WorkflowActionV1::ProbeSsh {
                host: "adapter-must-not-run".into(),
            },
        };
        let (result, attempts, _, _) = EXECUTION_STORE
            .scope(
                store,
                execute_action_with_retries(
                    Path::new("/missing-fixture-config"),
                    &node,
                    WorkflowConflictPolicyV1::Block,
                    &draft(),
                    &BTreeMap::new(),
                ),
            )
            .await;
        assert!(format!("{:#}", result.unwrap_err()).contains("unconfirmed effect"));
        assert_eq!(attempts, 0);
        Ok(())
    }

    fn private_config(root: &TempDir, delete: bool) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir_all(&source)?;
        fs::create_dir_all(&destination)?;
        fs::write(source.join("movie.bin"), b"typed workflow payload")?;
        let config = root.path().join("remote.toml");
        fs::write(
            &config,
            format!(
                "schema = 'codex-team.remote_config.v1'\n[syncs.local_sync]\ncontroller = 'local'\nsource = '{}/'\ndestination = '{}/'\ndelete = {delete}\ntimeout_seconds = 30\nmax_changes = 100\n",
                source.display(), destination.display()
            ),
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&config, fs::Permissions::from_mode(0o600))?;
        }
        Ok((config, source, destination))
    }

    fn draft() -> WorkflowDraftV1 {
        WorkflowDraftV1 {
            schema: WORKFLOW_DRAFT_SCHEMA.into(),
            workflow_id: "local_archive".into(),
            title: "Synchronize a local archive fixture".into(),
            actions: vec![
                WorkflowNodeV1 {
                    id: "plan".into(),
                    depends_on: Vec::new(),
                    max_retries: 0,
                    action: WorkflowActionV1::PlanSync {
                        profile: "local_sync".into(),
                        selection_action: None,
                    },
                },
                WorkflowNodeV1 {
                    id: "apply".into(),
                    depends_on: vec!["plan".into()],
                    max_retries: 1,
                    action: WorkflowActionV1::ApplySync {
                        profile: "local_sync".into(),
                        plan_action: "plan".into(),
                    },
                },
                WorkflowNodeV1 {
                    id: "sha".into(),
                    depends_on: vec!["apply".into()],
                    max_retries: 0,
                    action: WorkflowActionV1::VerifySha256 {
                        profile: "local_sync".into(),
                        maximum_files: 100,
                    },
                },
            ],
            limits: WorkflowLimitsV1 {
                max_items: Some(100),
                max_transfer_bytes: Some(1024 * 1024),
                max_files: None,
                max_runtime_seconds: 60,
                min_free_space_bytes: 0,
                deletion_allowed: false,
                conflict_policy: WorkflowConflictPolicyV1::KeepBoth,
                verification: vec![
                    WorkflowVerificationV1::EmptySyncPlan,
                    WorkflowVerificationV1::Sha256,
                ],
            },
            rollback: ca_automation_protocol::WorkflowRollbackV1::ResumeOnly {
                limitation:
                    "Fixture transfers can resume; prior destination bytes are not restored".into(),
            },
        }
    }

    #[test]
    fn independent_reads_are_four_way_and_mutations_are_serial_barriers() -> Result<()> {
        let mut value = draft();
        for index in 0..4 {
            value.actions.insert(
                1 + index,
                WorkflowNodeV1 {
                    id: format!("read_{index}"),
                    depends_on: Vec::new(),
                    max_retries: 0,
                    action: WorkflowActionV1::PlanSync {
                        profile: "local_sync".into(),
                        selection_action: None,
                    },
                },
            );
        }
        let batches = topological_action_batches(&value)?;
        assert_eq!(
            batches.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![4, 1, 1, 1]
        );
        assert!(batches
            .iter()
            .filter(|batch| batch.iter().any(|node| node.action.is_mutating()))
            .all(|batch| batch.len() == 1));
        Ok(())
    }

    #[test]
    fn remote_identity_and_engine_drift_require_fresh_approval() {
        let mut approved = ApprovedWorkflow {
            schema: APPROVED_SCHEMA.into(),
            workflow_id: "local_archive".into(),
            workflow_digest: "a".repeat(64),
            policy_digest: "b".repeat(64),
            resource_binding_digest: "c".repeat(64),
            engine_version: env!("CARGO_PKG_VERSION").into(),
            engine_protocol_version: WORKFLOW_ENGINE_PROTOCOL_VERSION,
            remote_identity_digest: "d".repeat(64),
            first_run_plan_digest: "e".repeat(64),
            enabled: true,
            approved_at: "2026-08-27T00:00:00Z".into(),
            draft: draft(),
        };
        assert!(approved_matches(
            &approved,
            &"a".repeat(64),
            &"b".repeat(64),
            &"c".repeat(64),
            &"d".repeat(64)
        ));
        assert!(!approved_matches(
            &approved,
            &"a".repeat(64),
            &"b".repeat(64),
            &"c".repeat(64),
            &"f".repeat(64)
        ));
        approved.engine_version = "different-engine".into();
        assert!(!approved_matches(
            &approved,
            &"a".repeat(64),
            &"b".repeat(64),
            &"c".repeat(64),
            &"d".repeat(64)
        ));
    }

    #[test]
    fn ssh_forwarded_rtorrent_workflow_requires_host_identity_probe() {
        let mut value = draft();
        value.actions = vec![WorkflowNodeV1 {
            id: "query".into(),
            depends_on: Vec::new(),
            max_retries: 0,
            action: WorkflowActionV1::QueryTorrents {
                endpoint: "rtorrent".into(),
            },
        }];
        let remote = crate::remote::WorkflowRemoteSummary {
            binding_digest: "a".repeat(64),
            ssh_hosts: ["seedbox".to_string()].into_iter().collect(),
            transmission: BTreeSet::new(),
            rtorrent: ["rtorrent".to_string()].into_iter().collect(),
            rtorrent_ssh_hosts: [("rtorrent".to_string(), "seedbox".to_string())]
                .into_iter()
                .collect(),
            torrent_locations: BTreeMap::new(),
            syncs: BTreeMap::new(),
        };
        assert!(validate_resources(&value, &remote).is_err());
        value.actions.insert(
            0,
            WorkflowNodeV1 {
                id: "identity".into(),
                depends_on: Vec::new(),
                max_retries: 0,
                action: WorkflowActionV1::ProbeSsh {
                    host: "seedbox".into(),
                },
            },
        );
        value.actions[1].depends_on.push("identity".into());
        assert!(validate_resources(&value, &remote).is_ok());
    }

    #[tokio::test]
    async fn offline_torrent_control_preview_never_resolves_targets_or_connects() {
        let node = WorkflowNodeV1 {
            id: "start".into(),
            depends_on: vec!["select".into()],
            max_retries: 0,
            action: WorkflowActionV1::ControlTorrents {
                endpoint: "unreachable".into(),
                selection_action: "select".into(),
                control: ca_automation_protocol::TorrentControlActionV1::Start,
            },
        };
        let remote = crate::remote::WorkflowRemoteSummary {
            binding_digest: "a".repeat(64),
            ssh_hosts: BTreeSet::new(),
            transmission: BTreeSet::new(),
            rtorrent: ["unreachable".to_string()].into_iter().collect(),
            rtorrent_ssh_hosts: BTreeMap::new(),
            torrent_locations: BTreeMap::new(),
            syncs: BTreeMap::new(),
        };
        let runtime = preview_action(
            Path::new("/path/that/does/not/exist"),
            &remote,
            &node,
            WorkflowDryRunDepthV1::Offline,
            WorkflowConflictPolicyV1::Block,
            &draft().limits,
            &BTreeMap::new(),
        )
        .await
        .unwrap();
        assert!(runtime.value.is_none());
        assert!(runtime
            .uncertainties
            .iter()
            .any(|value| value.contains("exact torrent targets")));
    }

    #[test]
    fn oversized_torrent_effect_preview_blocks_instead_of_hiding_approved_targets() {
        let hashes = (0..512)
            .map(|index| format!("{index:040x}"))
            .collect::<Vec<_>>();
        let mut receipt = WorkflowPlanReceiptV1 {
            schema: WORKFLOW_PLAN_RECEIPT_SCHEMA.into(),
            workflow_id: "many_controls".into(),
            depth: WorkflowDryRunDepthV1::MutationPreview,
            status: WorkflowPlanStatusV1::ApprovalRequired,
            workflow_digest: "a".repeat(64),
            policy_digest: "b".repeat(64),
            resource_binding_digest: "c".repeat(64),
            run_plan_digest: "d".repeat(64),
            action_plans: (0..4)
                .map(|index| WorkflowActionPlanV1 {
                    action_id: format!("control_{index}"),
                    kind: "control_torrents".into(),
                    summary: "exact typed control".into(),
                    predicted_items: 512,
                    predicted_bytes: 0,
                    uncertainties: Vec::new(),
                    target_hashes: hashes.clone(),
                    torrent_control: Some(ca_automation_protocol::TorrentControlActionV1::Start),
                })
                .collect(),
            predicted_items: 2_048,
            predicted_bytes: 0,
            mutation_present: true,
            limits_authorize_mutation: true,
            approval_required: true,
            rollback: ca_automation_protocol::WorkflowRollbackV1::Unavailable {
                limitation: "client state can be restored only by another approved action".into(),
            },
            correction_round: 0,
            reason_codes: Vec::new(),
            truncated: false,
        };
        bound_plan_receipt(&mut receipt).unwrap();
        assert_eq!(receipt.status, WorkflowPlanStatusV1::Blocked);
        assert!(!receipt.approval_required);
        assert!(receipt.truncated);
        assert!(receipt
            .reason_codes
            .contains(&"torrent_effects_exceed_public_preview_bound".to_string()));
        assert!(serde_json::to_vec(&receipt).unwrap().len() <= MAX_MECHANISTIC_OUTPUT_BYTES);
    }

    #[test]
    fn execution_receipt_preserves_torrent_digests_when_hash_details_are_truncated() {
        let hashes = (0..512)
            .map(|index| format!("{index:040x}"))
            .collect::<Vec<_>>();
        let mutation = ca_automation_protocol::TorrentMutationReceiptV1 {
            endpoint: "seedbox".into(),
            identity: ca_automation_protocol::TorrentClientIdentityV1 {
                kind: ca_automation_protocol::TorrentClientKindV1::Transmission,
                protocol: ca_automation_protocol::TorrentProtocolV1::TransmissionJsonRpc2,
                version: Some("4.1.0".into()),
                fingerprint: "a".repeat(64),
            },
            action: ca_automation_protocol::TorrentControlActionV1::Start,
            target_hashes: hashes.clone(),
            changed_hashes: hashes,
            target_hashes_digest: Some("b".repeat(64)),
            changed_hashes_digest: Some("c".repeat(64)),
            post_state_confirmed: true,
            ambiguous_transport_resolved: false,
            truncated: false,
        };
        let mut receipt = WorkflowExecutionReceiptV1 {
            schema: WORKFLOW_EXECUTION_RECEIPT_SCHEMA.into(),
            workflow_id: "many_controls".into(),
            workflow_digest: "d".repeat(64),
            policy_digest: "e".repeat(64),
            resource_binding_digest: "f".repeat(64),
            run_plan_digest: "1".repeat(64),
            status: WorkflowExecutionStatusV1::Completed,
            action_results: (0..4)
                .map(|index| WorkflowActionExecutionV1 {
                    action_id: format!("control_{index}"),
                    status: WorkflowExecutionStatusV1::Completed,
                    summary: "state-confirmed torrent control".into(),
                    attempts: 1,
                    repair_cause: None,
                    torrent_mutation: Some(mutation.clone()),
                    duration_millis: 1,
                })
                .collect(),
            deterministic_repairs: 0,
            verification_passed: true,
            rollback: None,
            deviations: Vec::new(),
            reason_codes: Vec::new(),
            started_at: "2026-08-27T00:00:00Z".into(),
            finished_at: "2026-08-27T00:00:01Z".into(),
            elapsed_millis: 1,
        };
        bound_execution_receipt(&mut receipt).unwrap();
        assert!(serde_json::to_vec(&receipt).unwrap().len() <= MAX_MECHANISTIC_OUTPUT_BYTES);
        assert!(receipt.action_results.iter().any(|action| {
            action.torrent_mutation.as_ref().is_some_and(|mutation| {
                mutation.truncated
                    && mutation
                        .target_hashes_digest
                        .as_deref()
                        .is_some_and(|value| value == "b".repeat(64))
                    && mutation
                        .changed_hashes_digest
                        .as_deref()
                        .is_some_and(|value| value == "c".repeat(64))
            })
        }));
    }

    #[test]
    fn workflow_scoped_policy_storage_hashes_its_target_identity() -> Result<()> {
        let stored = stored_policy_scope(&WorkflowCorrectionScopeV1::Workflow {
            workflow_id: "local_archive".into(),
        })?;
        let WorkflowCorrectionScopeV1::Workflow { workflow_id } = stored else {
            panic!("expected workflow scope")
        };
        assert_ne!(workflow_id, "local_archive");
        assert_eq!(workflow_id.len(), 64);
        Ok(())
    }

    #[test]
    fn remote_sync_actions_require_a_transitive_identity_probe() {
        let mut value = draft();
        let remote = crate::remote::WorkflowRemoteSummary {
            binding_digest: "a".repeat(64),
            ssh_hosts: ["seedbox".to_string()].into_iter().collect(),
            transmission: BTreeSet::new(),
            rtorrent: BTreeSet::new(),
            rtorrent_ssh_hosts: BTreeMap::new(),
            torrent_locations: BTreeMap::new(),
            syncs: [(
                "local_sync".to_string(),
                crate::remote::WorkflowSyncProfileSummary {
                    controller: "seedbox".into(),
                    destination: "/media/archive".into(),
                    delete: false,
                    torrent_endpoint: None,
                },
            )]
            .into_iter()
            .collect(),
        };
        assert!(validate_resources(&value, &remote).is_err());
        value.actions.insert(
            0,
            WorkflowNodeV1 {
                id: "identity".into(),
                depends_on: Vec::new(),
                max_retries: 0,
                action: WorkflowActionV1::ProbeSsh {
                    host: "seedbox".into(),
                },
            },
        );
        let plan = value
            .actions
            .iter_mut()
            .find(|node| node.id == "plan")
            .expect("plan action");
        plan.depends_on.push("identity".into());
        assert!(validate_resources(&value, &remote).is_ok());
    }

    #[test]
    fn torrent_location_adoption_requires_bound_selection_transfer_and_verification() {
        let mut value = draft();
        value.limits.max_files = Some(100);
        value.actions.insert(
            0,
            WorkflowNodeV1 {
                id: "query".into(),
                depends_on: Vec::new(),
                max_retries: 0,
                action: WorkflowActionV1::QueryTorrents {
                    endpoint: "seedbox".into(),
                },
            },
        );
        value.actions.insert(
            1,
            WorkflowNodeV1 {
                id: "select".into(),
                depends_on: vec!["query".into()],
                max_retries: 0,
                action: WorkflowActionV1::SelectTorrents {
                    source_action: "query".into(),
                    completed: true,
                    label: Some("archive".into()),
                    info_hashes: Vec::new(),
                    states: Vec::new(),
                    maximum_items: 10,
                },
            },
        );
        let plan = value
            .actions
            .iter_mut()
            .find(|node| node.id == "plan")
            .unwrap();
        plan.depends_on = vec!["select".into()];
        plan.action = WorkflowActionV1::PlanSync {
            profile: "local_sync".into(),
            selection_action: Some("select".into()),
        };
        value.actions.push(WorkflowNodeV1 {
            id: "adopt".into(),
            depends_on: vec!["select".into(), "apply".into(), "sha".into()],
            max_retries: 0,
            action: WorkflowActionV1::ControlTorrents {
                endpoint: "seedbox".into(),
                selection_action: "select".into(),
                control: ca_automation_protocol::TorrentControlActionV1::AdoptLocation {
                    location: "home".into(),
                },
            },
        });
        validate_workflow_draft(&value).unwrap();
        let remote = crate::remote::WorkflowRemoteSummary {
            binding_digest: "a".repeat(64),
            ssh_hosts: BTreeSet::new(),
            transmission: ["seedbox".to_string()].into_iter().collect(),
            rtorrent: BTreeSet::new(),
            rtorrent_ssh_hosts: BTreeMap::new(),
            torrent_locations: [("home".to_string(), "seedbox".to_string())]
                .into_iter()
                .collect(),
            syncs: [(
                "local_sync".to_string(),
                crate::remote::WorkflowSyncProfileSummary {
                    controller: "local".into(),
                    destination: "/media/archive".into(),
                    delete: false,
                    torrent_endpoint: Some("seedbox".into()),
                },
            )]
            .into_iter()
            .collect(),
        };
        assert!(validate_resources(&value, &remote).is_ok());
        let mut cross_endpoint_control = value.clone();
        let control = cross_endpoint_control
            .actions
            .iter_mut()
            .find(|node| node.id == "adopt")
            .unwrap();
        control.action = WorkflowActionV1::ControlTorrents {
            endpoint: "other".into(),
            selection_action: "select".into(),
            control: ca_automation_protocol::TorrentControlActionV1::Start,
        };
        let mut cross_endpoint_remote = remote.clone();
        cross_endpoint_remote.rtorrent.insert("other".into());
        assert!(validate_resources(&cross_endpoint_control, &cross_endpoint_remote).is_err());
        let mut unrelated_transfer = value.clone();
        unrelated_transfer
            .actions
            .iter_mut()
            .find(|node| node.id == "plan")
            .unwrap()
            .action = WorkflowActionV1::PlanSync {
            profile: "local_sync".into(),
            selection_action: None,
        };
        assert!(validate_resources(&unrelated_transfer, &remote).is_err());
        value
            .actions
            .iter_mut()
            .find(|node| node.id == "adopt")
            .unwrap()
            .depends_on
            .retain(|dependency| dependency != "sha");
        assert!(validate_resources(&value, &remote).is_err());
    }

    #[tokio::test]
    async fn offline_preview_is_process_free_and_exact_run_is_reusable() -> Result<()> {
        let root = TempDir::new()?;
        let (config, source, destination) = private_config(&root, false)?;
        let state = root.path().join("state");
        let offline = execute(
            &config,
            &state,
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Preview {
                    draft: draft(),
                    depth: WorkflowDryRunDepthV1::Offline,
                    correction_round: 0,
                },
            },
        )
        .await?;
        let WorkflowResponseV1::Plan { receipt } = offline else {
            panic!("expected plan")
        };
        assert_eq!(receipt.depth, WorkflowDryRunDepthV1::Offline);
        assert!(!destination.join("movie.bin").exists());

        let preview = execute(
            &config,
            &state,
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Preview {
                    draft: draft(),
                    depth: WorkflowDryRunDepthV1::MutationPreview,
                    correction_round: 0,
                },
            },
        )
        .await?;
        let WorkflowResponseV1::Plan { receipt } = preview else {
            panic!("expected plan")
        };
        assert!(receipt.predicted_items > 0);
        assert!(!destination.join("movie.bin").exists());
        let accepted = receipt.run_plan_digest.clone();
        let stale = execute(
            &config,
            &state,
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Run {
                    workflow_id: "local_archive".into(),
                    draft: Some(draft()),
                    accepted_plan_digest: Some(accepted.clone()),
                    expected_workflow_digest: Some("f".repeat(64)),
                    expected_resource_binding_digest: Some(receipt.resource_binding_digest.clone()),
                },
            },
        )
        .await?;
        let WorkflowResponseV1::Execution { receipt: stale } = stale else {
            panic!("expected stale execution")
        };
        assert_eq!(stale.status, WorkflowExecutionStatusV1::Stale);
        assert!(!destination.join("movie.bin").exists());
        let run = execute(
            &config,
            &state,
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Run {
                    workflow_id: "local_archive".into(),
                    draft: Some(draft()),
                    accepted_plan_digest: Some(accepted),
                    expected_workflow_digest: None,
                    expected_resource_binding_digest: None,
                },
            },
        )
        .await?;
        let WorkflowResponseV1::Execution { receipt } = run else {
            panic!("expected execution")
        };
        assert_eq!(receipt.status, WorkflowExecutionStatusV1::Completed);
        assert!(receipt.verification_passed);
        assert!(destination.join("movie.bin").exists());

        fs::write(source.join("later.bin"), b"later bounded inventory")?;

        let repeated = execute(
            &config,
            &state,
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Run {
                    workflow_id: "local_archive".into(),
                    draft: None,
                    accepted_plan_digest: None,
                    expected_workflow_digest: None,
                    expected_resource_binding_digest: None,
                },
            },
        )
        .await?;
        let WorkflowResponseV1::Execution { receipt } = repeated else {
            panic!("expected repeated execution")
        };
        assert_eq!(receipt.status, WorkflowExecutionStatusV1::Completed);
        assert!(destination.join("later.bin").exists());
        let workflow_digest = receipt.workflow_digest;
        let accepted_feedback = execute(
            &config,
            &state,
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Feedback {
                    workflow_digest: workflow_digest.clone(),
                    receipt_digest: "c".repeat(64),
                    category: ca_automation_protocol::WorkflowFeedbackCategoryV1::Accepted,
                },
            },
        )
        .await?;
        assert!(matches!(
            accepted_feedback,
            WorkflowResponseV1::FeedbackRecorded {
                disabled: false,
                ..
            }
        ));
        let unsafe_feedback = execute(
            &config,
            &state,
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Feedback {
                    workflow_digest,
                    receipt_digest: "d".repeat(64),
                    category: ca_automation_protocol::WorkflowFeedbackCategoryV1::Unsafe,
                },
            },
        )
        .await?;
        assert!(matches!(
            unsafe_feedback,
            WorkflowResponseV1::FeedbackRecorded { disabled: true, .. }
        ));
        Ok(())
    }

    #[tokio::test]
    async fn keep_both_preserves_conflicting_destination_and_still_verifies() -> Result<()> {
        let root = TempDir::new()?;
        let (config, source, destination) = private_config(&root, false)?;
        fs::write(destination.join("movie.bin"), b"previous destination bytes")?;
        let state = root.path().join("state");
        let preview = execute(
            &config,
            &state,
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Preview {
                    draft: draft(),
                    depth: WorkflowDryRunDepthV1::MutationPreview,
                    correction_round: 0,
                },
            },
        )
        .await?;
        let WorkflowResponseV1::Plan { receipt } = preview else {
            panic!("expected plan")
        };
        assert_eq!(receipt.status, WorkflowPlanStatusV1::ApprovalRequired);
        let run = execute(
            &config,
            &state,
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Run {
                    workflow_id: "local_archive".into(),
                    draft: Some(draft()),
                    accepted_plan_digest: Some(receipt.run_plan_digest),
                    expected_workflow_digest: None,
                    expected_resource_binding_digest: None,
                },
            },
        )
        .await?;
        let WorkflowResponseV1::Execution { receipt } = run else {
            panic!("expected execution")
        };
        assert_eq!(receipt.status, WorkflowExecutionStatusV1::Completed);
        assert_eq!(
            fs::read(destination.join("movie.bin"))?,
            fs::read(source.join("movie.bin"))?
        );
        let conflict_root = destination.join(".codex-team-conflicts");
        let preserved = fs::read_dir(conflict_root)?
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path().join("movie.bin"))
            .find(|path| path.is_file())
            .context("keep-both backup was not retained")?;
        assert_eq!(fs::read(preserved)?, b"previous destination bytes");
        Ok(())
    }

    #[tokio::test]
    async fn block_conflict_policy_returns_a_failed_preview_without_mutation() -> Result<()> {
        let root = TempDir::new()?;
        let (config, _source, destination) = private_config(&root, false)?;
        fs::write(destination.join("movie.bin"), b"existing")?;
        let mut blocked = draft();
        blocked.limits.conflict_policy = WorkflowConflictPolicyV1::Block;
        let response = execute(
            &config,
            &root.path().join("state"),
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Preview {
                    draft: blocked,
                    depth: WorkflowDryRunDepthV1::MutationPreview,
                    correction_round: 0,
                },
            },
        )
        .await?;
        let WorkflowResponseV1::Plan { receipt } = response else {
            panic!("expected plan")
        };
        assert_eq!(receipt.status, WorkflowPlanStatusV1::Blocked);
        assert!(receipt
            .reason_codes
            .iter()
            .any(|reason| reason == "destination_conflict"));
        assert_eq!(fs::read(destination.join("movie.bin"))?, b"existing");
        Ok(())
    }

    #[tokio::test]
    async fn interrupted_legacy_checkpoint_requires_readback_before_effects_resume() -> Result<()> {
        let root = TempDir::new()?;
        let (config, _source, destination) = private_config(&root, false)?;
        let state = root.path().join("state");
        let store = WorkflowStore::open(&state)?;
        let preview = preview(
            &config,
            &store,
            draft(),
            WorkflowDryRunDepthV1::MutationPreview,
            0,
        )
        .await?;
        store.write_checkpoint(&preview, WorkflowCheckpointPhase::Running, None)?;
        let accepted = preview.receipt.run_plan_digest;
        let run = execute(
            &config,
            &state,
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Run {
                    workflow_id: "local_archive".into(),
                    draft: Some(draft()),
                    accepted_plan_digest: Some(accepted),
                    expected_workflow_digest: None,
                    expected_resource_binding_digest: None,
                },
            },
        )
        .await?;
        let WorkflowResponseV1::Execution { receipt } = run else {
            panic!("expected execution")
        };
        assert_eq!(receipt.status, WorkflowExecutionStatusV1::Blocked);
        assert!(receipt
            .reason_codes
            .iter()
            .any(|reason| reason.contains("reconciliation")));
        assert!(!destination.join("movie.bin").is_file());
        let checkpoint = store.read_checkpoint("local_archive")?.unwrap();
        assert_eq!(checkpoint.phase, WorkflowCheckpointPhase::Running);
        Ok(())
    }

    #[tokio::test]
    async fn compatibility_dispatch_reuses_one_receipt_and_new_occurrences_remain_independent(
    ) -> Result<()> {
        let root = TempDir::new()?;
        let (config, _, destination) = private_config(&root, false)?;
        let state = root.path().join("state");
        let preview = execute(
            &config,
            &state,
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Preview {
                    draft: draft(),
                    depth: WorkflowDryRunDepthV1::MutationPreview,
                    correction_round: 0,
                },
            },
        )
        .await?;
        let WorkflowResponseV1::Plan { receipt } = preview else {
            panic!("expected plan");
        };
        let request = WorkflowRequestV1 {
            schema: WORKFLOW_REQUEST_SCHEMA.into(),
            operation: WorkflowOperationV1::Run {
                workflow_id: "local_archive".into(),
                draft: Some(draft()),
                accepted_plan_digest: Some(receipt.run_plan_digest),
                expected_workflow_digest: None,
                expected_resource_binding_digest: None,
            },
        };
        let (first, concurrent_duplicate) = tokio::join!(
            execute_with_run_id(
                &config,
                &state,
                request.clone(),
                Some("compatibility_first".into()),
            ),
            execute_with_run_id(
                &config,
                &state,
                request.clone(),
                Some("compatibility_first".into()),
            )
        );
        let first = first?;
        assert_eq!(
            serde_json::to_value(&first)?,
            serde_json::to_value(concurrent_duplicate?)?
        );
        assert!(destination.join("movie.bin").is_file());
        fs::remove_file(destination.join("movie.bin"))?;
        let duplicate = execute_with_run_id(
            &config,
            &state,
            request.clone(),
            Some("compatibility_first".into()),
        )
        .await?;
        assert_eq!(
            serde_json::to_value(first)?,
            serde_json::to_value(duplicate)?
        );
        assert!(
            !destination.join("movie.bin").exists(),
            "duplicate dispatch must not repeat the adapter"
        );
        execute_with_run_id(
            &config,
            &state,
            request,
            Some("compatibility_second".into()),
        )
        .await?;
        assert!(destination.join("movie.bin").is_file());
        Ok(())
    }

    #[tokio::test]
    async fn workflow_admission_waits_for_physical_ownership_and_cancels_before_effects(
    ) -> Result<()> {
        use crate::runtime::{ExecutionState, RunStore};
        let root = TempDir::new()?;
        let (config, _, destination) = private_config(&root, false)?;
        let invocation = compatibility_invocation(
            &config,
            &root.path().join("state"),
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Run {
                    workflow_id: "local_archive".into(),
                    draft: Some(draft()),
                    accepted_plan_digest: None,
                    expected_workflow_digest: None,
                    expected_resource_binding_digest: None,
                },
            },
            Some("admission_owner".into()),
        )?;
        let mut context = invocation.context;
        // Both profiles bind the same physical keys in a private fixture
        // namespace; production compatibility keeps its machine-wide root.
        context.lease_root = root.path().canonicalize()?.join("leases");
        context.runtime_seconds = 1;
        let owner = RunStore::open(&context)?;
        let admission = admit_workflow_run(&owner, true).await?.unwrap();
        context.run_id = "admission_waiter".into();
        context.profile = "second-profile".into();
        let waiter = RunStore::open(&context)?;
        let waiting = admit_workflow_run(&waiter, true);
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(Duration::from_millis(40), &mut waiting)
                .await
                .is_err()
        );
        assert_eq!(waiter.status()?.state, ExecutionState::Prepared);
        assert!(waiter.status()?.actions.is_empty());
        waiter.cancel()?;
        assert!(waiting.await?.is_none());
        assert_eq!(waiter.status()?.state, ExecutionState::Cancelled);
        assert!(!destination.join("movie.bin").exists());
        for (resource, lease) in context
            .resources
            .iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .zip(&admission.resources)
        {
            lease.validate_binding(&context.lease_root, resource)?;
        }
        drop(admission);
        context.run_id = "admission_next_occurrence".into();
        let next = RunStore::open(&context)?;
        assert!(admit_workflow_run(&next, true).await?.is_some());
        assert_eq!(next.status()?.state, ExecutionState::Prepared);
        Ok(())
    }

    #[tokio::test]
    async fn workflow_admission_cannot_extend_the_supplied_runtime_budget() -> Result<()> {
        use crate::runtime::{ExecutionContext, RunStore, EXECUTION_SCHEMA};
        let root = TempDir::new()?;
        let root = root.path().canonicalize()?;
        let context = ExecutionContext {
            schema: EXECUTION_SCHEMA.into(),
            run_id: "budget_waiter".into(),
            profile: "fixture".into(),
            state_root: root.join("state"),
            lease_root: root.join("leases"),
            definition_digest: "a".repeat(64),
            resource_binding_digest: "b".repeat(64),
            policy_digest: "c".repeat(64),
            resources: vec!["typed-workflow-mutation".into()],
            runtime_seconds: 1,
            output_bytes: 1024,
        };
        let held =
            crate::runtime::ResourceLease::acquire(&context.lease_root, "typed-workflow-mutation")?;
        let store = RunStore::open(&context)?;
        let result = admit_workflow_run(&store, true).await;
        assert!(
            matches!(result, Err(error) if error.to_string().contains("runtime budget exhausted"))
        );
        assert!(store.status()?.actions.is_empty());
        assert_eq!(store.context().runtime_seconds, 1);
        held.validate_binding(&context.lease_root, "typed-workflow-mutation")?;
        Ok(())
    }

    #[tokio::test]
    async fn tightening_persists_but_widening_needs_exact_approval() -> Result<()> {
        let root = TempDir::new()?;
        let (config, _, _) = private_config(&root, false)?;
        let state = root.path().join("state");
        let source = execute(
            &config,
            &state,
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Preview {
                    draft: draft(),
                    depth: WorkflowDryRunDepthV1::Offline,
                    correction_round: 0,
                },
            },
        )
        .await?;
        let WorkflowResponseV1::Plan { receipt } = source else {
            panic!("expected source plan")
        };
        let correction =
            |source_plan_digest, changes, reason| ca_automation_protocol::WorkflowCorrectionV1 {
                schema: WORKFLOW_CORRECTION_SCHEMA.into(),
                source_plan_digest,
                reason_code: reason,
                scope: WorkflowCorrectionScopeV1::Adapter {
                    adapter: WorkflowAdapterV1::Rsync,
                },
                changes,
                evidence_refs: Vec::new(),
                correction_round: 1,
            };
        let tightened = execute(
            &config,
            &state,
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Correct {
                    draft: draft(),
                    correction: correction(
                        receipt.run_plan_digest,
                        vec![WorkflowCorrectionChangeV1::LowerMaxItems { maximum: 50 }],
                        WorkflowCorrectionReasonV1::LimitTooBroad,
                    ),
                    accepted_correction_digest: None,
                },
            },
        )
        .await?;
        let WorkflowResponseV1::Correction { receipt, .. } = tightened else {
            panic!("expected correction")
        };
        assert!(receipt.persisted);
        assert!(!receipt.approval_required);

        let source = execute(
            &config,
            &state,
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Preview {
                    draft: draft(),
                    depth: WorkflowDryRunDepthV1::Offline,
                    correction_round: 0,
                },
            },
        )
        .await?;
        let WorkflowResponseV1::Plan { receipt: source } = source else {
            panic!("expected source plan")
        };
        let widening_request = |accepted_correction_digest| WorkflowRequestV1 {
            schema: WORKFLOW_REQUEST_SCHEMA.into(),
            operation: WorkflowOperationV1::Correct {
                draft: draft(),
                correction: correction(
                    source.run_plan_digest.clone(),
                    vec![WorkflowCorrectionChangeV1::RaiseMaxItems { maximum: 200 }],
                    WorkflowCorrectionReasonV1::HumanCorrection,
                ),
                accepted_correction_digest,
            },
        };
        let widening = execute(&config, &state, widening_request(None)).await?;
        let WorkflowResponseV1::Correction { receipt, .. } = widening else {
            panic!("expected widening preview")
        };
        assert!(!receipt.persisted);
        assert!(receipt.approval_required);
        let digest = receipt.correction_digest;
        let approved = execute(&config, &state, widening_request(Some(digest))).await?;
        let WorkflowResponseV1::Correction { receipt, .. } = approved else {
            panic!("expected approved correction")
        };
        assert!(receipt.persisted);
        Ok(())
    }

    #[tokio::test]
    async fn deletion_policy_blocks_before_target_mutation() -> Result<()> {
        let root = TempDir::new()?;
        let (config, _, destination) = private_config(&root, true)?;
        fs::write(destination.join("keep.txt"), b"keep")?;
        let response = execute(
            &config,
            &root.path().join("state"),
            WorkflowRequestV1 {
                schema: WORKFLOW_REQUEST_SCHEMA.into(),
                operation: WorkflowOperationV1::Preview {
                    draft: draft(),
                    depth: WorkflowDryRunDepthV1::MutationPreview,
                    correction_round: 0,
                },
            },
        )
        .await?;
        let WorkflowResponseV1::Plan { receipt } = response else {
            panic!("expected plan")
        };
        assert_eq!(receipt.status, WorkflowPlanStatusV1::Blocked);
        assert!(destination.join("keep.txt").exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn workflow_vault_rejects_insecure_directories_symlinks_and_hardlinks() -> Result<()> {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root = TempDir::new()?;
        let insecure = root.path().join("insecure");
        fs::create_dir(&insecure)?;
        fs::set_permissions(&insecure, fs::Permissions::from_mode(0o755))?;
        assert!(WorkflowStore::open(&insecure).is_err());

        let safe = root.path().join("safe");
        fs::create_dir(&safe)?;
        fs::set_permissions(&safe, fs::Permissions::from_mode(0o700))?;
        let store = WorkflowStore::open(&safe)?;
        let outside = root.path().join("outside.json");
        fs::write(&outside, b"outside")?;
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600))?;

        let linked = store.policy_dir.join("linked.json");
        symlink(&outside, &linked)?;
        assert!(write_private_json(&linked, &serde_json::json!({"safe":true})).is_err());
        assert_eq!(fs::read(&outside)?, b"outside");
        fs::remove_file(&linked)?;

        fs::hard_link(&outside, &linked)?;
        assert!(write_private_json(&linked, &serde_json::json!({"safe":true})).is_err());
        assert_eq!(fs::read(&outside)?, b"outside");
        Ok(())
    }
}
